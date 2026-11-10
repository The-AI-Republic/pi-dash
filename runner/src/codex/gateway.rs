//! Model-gateway refusal detection shared by the chat and run lanes.
//!
//! The bundled engine files a gateway 402/401 under "stream disconnected"
//! and re-sends the whole request. It announces each retry as an `error`
//! notification with `willRetry: true`, `codexErrorInfo.*.httpStatusCode`,
//! and `additionalDetails` carrying the raw
//! `unexpected status <code> ...` string. A retry cannot change the outcome,
//! so lanes stop the turn on the first such notice instead of forwarding it.
//!
//! Chat's `ChatFailed.code` mapping lives in `crate::ipc::chat`; this module
//! only knows the wire shape and the gateway's text.

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
