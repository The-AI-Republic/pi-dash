//! Model-gateway refusal detection shared by the chat and run lanes.
//!
//! The bundled engine files a gateway 402/401 under "stream disconnected"
//! and re-sends the whole request. It announces each retry as an `error`
//! notification with `willRetry: true`, `codexErrorInfo.*.httpStatusCode`,
//! and `additionalDetails` carrying the raw
//! `unexpected status <code> ...` string. A retry cannot change the outcome,
//! so lanes stop the turn on the first such notice instead of forwarding it.
//!
//! The local-chat `ChatFailed.code` mapping lives here too, so the runner's
//! per-session lane (`crate::ipc::chat`) and the shared engine server
//! (`crate::pidash_agent::chat_server`) classify gateway answers identically.

/// Split the engine's `unexpected status 402 Payment Required: <text>, url:
/// <endpoint>, request id: <id>` into the status and the gateway's text.
pub fn gateway_status_text(raw: &str) -> Option<(u64, String)> {
    let rest = raw.split_once("unexpected status ")?.1;
    let status = rest.get(..3)?.parse().ok()?;
    let text = rest.split_once(": ").map_or("", |(_, text)| text);
    let text = text.rsplit_once(", url: ").map_or(text, |(text, _)| text);
    Some((status, text.trim().to_string()))
}

/// True for a gateway status that repeating the request cannot change.
pub fn is_gateway_refusal_status(status: u64) -> bool {
    matches!(status, 401 | 402)
}

/// Recognise the engine's "I will retry" notice for any gateway status, as
/// `(http status, gateway text)`. Callers that also classify the body (the
/// free-quota classifier) start from here; [`gateway_refusal`] is the 401/402
/// subset whose answer no retry can change.
pub fn gateway_will_retry_notice(
    method: &str,
    params: &serde_json::Value,
) -> Option<(u64, String)> {
    if method != "error" || params.get("willRetry").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let error = params.get("error")?;
    let status = error
        .get("codexErrorInfo")?
        .as_object()?
        .values()
        .find_map(|info| info.get("httpStatusCode")?.as_u64())?;
    let text = error
        .get("additionalDetails")
        .and_then(|v| v.as_str())
        .and_then(gateway_status_text)
        .map(|(_, text)| text)
        .unwrap_or_default();
    Some((status, text))
}

/// Recognise the engine's "I will retry" notice for a gateway answer that no
/// retry can change, as `(http status, gateway text)`.
///
/// The engine reports the HTTP status under `codexErrorInfo` (for example
/// `{"responseStreamDisconnected": {"httpStatusCode": 402}}`) and the body in
/// `additionalDetails`; `message` is only `Reconnecting... 1/5`.
pub fn gateway_refusal(method: &str, params: &serde_json::Value) -> Option<(u64, String)> {
    let (status, text) = gateway_will_retry_notice(method, params)?;
    if !is_gateway_refusal_status(status) {
        return None;
    }
    Some((status, text))
}

/// `ChatFailed.code` for a model gateway that answered 402: the account has
/// no credit left. The detail is the gateway's own sentence.
pub const CHAT_MODEL_CREDIT_EXHAUSTED: &str = "model_credit_exhausted";
/// `ChatFailed.code` for a model gateway that answered 401: the credential
/// the engine holds is not accepted.
pub const CHAT_MODEL_AUTH_FAILED: &str = "model_auth_failed";
/// `ChatFailed.code` for an engine turn that failed on its own.
pub const CHAT_AGENT_FAILED: &str = "agent_failed";

/// The `ChatFailed.code` for a gateway status that repeating the request
/// cannot change, or `None` for one worth retrying.
pub fn gateway_refusal_code(status: u64) -> Option<&'static str> {
    match status {
        401 => Some(CHAT_MODEL_AUTH_FAILED),
        402 => Some(CHAT_MODEL_CREDIT_EXHAUSTED),
        _ => None,
    }
}

/// Recognise the engine's "I will retry" notice for a gateway answer that no
/// retry can change, as `(ChatFailed code, gateway text)`. Both chat turn
/// drivers use this so a 401/402 stops the turn before the engine retries.
pub fn gateway_chat_refusal(
    method: &str,
    params: &serde_json::Value,
) -> Option<(&'static str, String)> {
    let (status, text) = gateway_refusal(method, params)?;
    Some((gateway_refusal_code(status)?, text))
}

/// Whether a raw engine frame is an assistant text delta. `drive_turn` in
/// `crate::ipc::chat` and the shared server's driver both map these to the
/// `assistant_delta` chat event kind; everything else raw stays `raw`.
pub fn is_assistant_text_delta(method: &str, params: &serde_json::Value) -> bool {
    if method == "item/agentMessage/delta" {
        return assistant_delta_text(params).is_some();
    }
    method == "stream_event/content_block_delta"
        && params
            .get("delta")
            .and_then(|delta| delta.get("type"))
            .and_then(|value| value.as_str())
            == Some("text_delta")
        && params
            .get("delta")
            .and_then(|delta| delta.get("text"))
            .and_then(|value| value.as_str())
            .is_some()
}

fn assistant_delta_text(params: &serde_json::Value) -> Option<&str> {
    if let Some(delta) = params.get("delta") {
        if let Some(text) = delta.as_str() {
            return Some(text);
        }
        if let Some(text) = delta.get("text").and_then(|value| value.as_str()) {
            return Some(text);
        }
    }
    params.get("text").and_then(|value| value.as_str())
}

/// The human-readable assistant text carried by a completed turn's `done`
/// payload, if it carries one as text. Both turn drivers use this for the
/// terminal `ChatMessageCompleted.assistant_message`.
pub fn assistant_text_from_done_payload(payload: &serde_json::Value) -> Option<String> {
    if let Some(text) = payload.as_str() {
        return Some(text.to_string());
    }
    let obj = payload.as_object()?;
    for key in ["result", "message", "text", "output", "summary", "content"] {
        if let Some(text) = obj.get(key).and_then(|value| value.as_str())
            && !text.is_empty()
        {
            return Some(text.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn notice(status: u64, will_retry: bool) -> serde_json::Value {
        json!({
            "error": {
                "message": "Reconnecting... 1/5",
                "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": status}},
                "additionalDetails": format!("unexpected status {status} Whatever: body text, url: http://x/v1/responses"),
            },
            "willRetry": will_retry,
        })
    }

    #[test]
    fn only_402_and_401_will_retry_notices_are_refusals() {
        assert_eq!(
            gateway_refusal("error", &notice(402, true)),
            Some((402, "body text".to_string()))
        );
        assert_eq!(
            gateway_refusal("error", &notice(401, true)),
            Some((401, "body text".to_string()))
        );
        assert_eq!(gateway_refusal("error", &notice(503, true)), None);
        assert_eq!(gateway_refusal("error", &notice(429, true)), None);
        // The any-status extractor sees the retryable statuses the refusal
        // filter drops, so the free-quota classifier can grade them.
        assert_eq!(
            gateway_will_retry_notice("error", &notice(503, true)),
            Some((503, "body text".to_string()))
        );
        assert_eq!(
            gateway_will_retry_notice("error", &notice(429, true)),
            Some((429, "body text".to_string()))
        );
        assert_eq!(
            gateway_will_retry_notice("error", &notice(402, false)),
            None
        );
        assert_eq!(gateway_refusal("error", &notice(402, false)), None);
        assert_eq!(gateway_refusal("warning", &notice(402, true)), None);
        let dropped = json!({
            "error": {
                "message": "Reconnecting... 1/5",
                "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": null}},
                "additionalDetails": "stream disconnected before completion: Incomplete response returned, reason: max_output_tokens",
            },
            "willRetry": true,
        });
        assert_eq!(gateway_refusal("error", &dropped), None);
        assert_eq!(
            gateway_status_text(
                "unexpected status 402 Payment Required: add credit, url: https://g/v1/responses, request id: r1"
            ),
            Some((402, "add credit".to_string()))
        );
        assert_eq!(
            gateway_status_text("stream disconnected before completion"),
            None
        );
    }
}
