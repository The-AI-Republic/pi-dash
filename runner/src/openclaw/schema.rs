//! ACP (Agent Client Protocol) JSON-RPC message shapes emitted by
//! `acpx --format json openclaw exec`. `acpx` is a headless ACP client that
//! drives the OpenClaw coding agent and, with `--format json`, dumps the raw
//! ACP JSON-RPC traffic to stdout as NDJSON — one complete JSON-RPC 2.0
//! message per line, with no wrapper envelope.
//!
//! Modelled the same way as the Cursor / Claude bridges: capture the JSON-RPC
//! envelope (the discriminating fields we actually read) and retain the full
//! body as a `serde_json::Value` so the daemon can ship it to local history
//! verbatim without tracking every upstream schema revision.
//!
//! ACP is JSON-RPC 2.0 (<https://agentclientprotocol.com>). Three message
//! kinds appear on the wire:
//! - **Method** — a request or notification carrying a `method` (e.g.
//!   `session/new`, `session/prompt`, `session/update`,
//!   `session/request_permission`). Requests also carry an `id`; notifications
//!   don't. We only read the stream, so we don't distinguish the two.
//! - **Response** — a success reply carrying `result` (e.g. the
//!   `session/prompt` reply `{"result":{"stopReason":"end_turn"}}`).
//! - **Error** — a failure reply carrying `error` (`{code, message, data?}`).

use serde::{Deserialize, Deserializer};

/// A single ACP JSON-RPC message read off `acpx`'s NDJSON stream. Dispatch is
/// on which of `method` / `result` / `error` is present; anything that fits
/// none collapses to [`AcpMessage::Unknown`] so forward-compatible upstream
/// changes don't crash the bridge.
#[derive(Debug, Clone)]
pub enum AcpMessage {
    /// A request or notification carrying a `method` and `params`. Covers
    /// `session/update` (the streamed agent output), `session/prompt`,
    /// `session/new`, `session/request_permission`, `initialize`, etc.
    Method {
        method: String,
        params: serde_json::Value,
    },
    /// A successful JSON-RPC response (`result` present, no `method`). The
    /// `session/prompt` response carries `result.stopReason`, which is how we
    /// detect the end of a turn.
    Response { result: serde_json::Value },
    /// A JSON-RPC error response (`error` present).
    Error { error: serde_json::Value },
    /// Anything else: preserved verbatim in history.
    Unknown(serde_json::Value),
}

impl<'de> Deserialize<'de> for AcpMessage {
    // Dispatch manually rather than with `#[serde(untagged)]`: the JSON-RPC
    // envelope is structural (presence of `method` vs `result` vs `error`),
    // not tag-based, and an untagged enum would greedily misroute. A catch-all
    // keeps the bridge forward-compatible.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
            let params = v.get("params").cloned().unwrap_or(serde_json::Value::Null);
            return Ok(AcpMessage::Method {
                method: method.to_string(),
                params,
            });
        }
        if let Some(error) = v.get("error") {
            return Ok(AcpMessage::Error {
                error: error.clone(),
            });
        }
        if let Some(result) = v.get("result") {
            return Ok(AcpMessage::Response {
                result: result.clone(),
            });
        }
        Ok(AcpMessage::Unknown(v))
    }
}

impl AcpMessage {
    /// Best-effort extraction of the ACP `sessionId` carried by a frame. The
    /// session id appears on the `session/prompt` request, every
    /// `session/update` notification, and the `session/new` response — we use
    /// the first one we see as the run's thread id (see
    /// `bridge::Bridge::wait_for_session`). Checks both the flattened shape
    /// (`params.sessionId`) and the nested `result.sessionId`.
    pub fn session_id(&self) -> Option<String> {
        let dig = |v: &serde_json::Value| {
            v.get("sessionId")
                .and_then(|s| s.as_str())
                .map(ToOwned::to_owned)
        };
        match self {
            AcpMessage::Method { params, .. } => dig(params),
            AcpMessage::Response { result } => dig(result),
            _ => None,
        }
    }

    /// For a `session/update` notification, the `sessionUpdate` discriminator
    /// (`agent_message_chunk`, `tool_call`, `tool_call_update`, `plan`, …).
    /// ACP nests the update under `params.update` in the spec, but some
    /// emitters flatten it onto `params`; check both.
    pub fn session_update_kind(params: &serde_json::Value) -> Option<&str> {
        params
            .get("update")
            .and_then(|u| u.get("sessionUpdate"))
            .or_else(|| params.get("sessionUpdate"))
            .and_then(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_session_update_notification() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hello"}}}}"#;
        match serde_json::from_str::<AcpMessage>(line).unwrap() {
            AcpMessage::Method { method, params } => {
                assert_eq!(method, "session/update");
                assert_eq!(
                    AcpMessage::session_update_kind(&params),
                    Some("agent_message_chunk")
                );
            }
            other => panic!("expected Method, got {other:?}"),
        }
    }

    #[test]
    fn dispatches_flattened_session_update() {
        // Some emitters put `sessionUpdate` directly on params.
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","sessionUpdate":"tool_call","content":{}}}"#;
        match serde_json::from_str::<AcpMessage>(line).unwrap() {
            AcpMessage::Method { params, .. } => {
                assert_eq!(AcpMessage::session_update_kind(&params), Some("tool_call"));
            }
            other => panic!("expected Method, got {other:?}"),
        }
    }

    #[test]
    fn dispatches_prompt_response_with_stop_reason() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#;
        match serde_json::from_str::<AcpMessage>(line).unwrap() {
            AcpMessage::Response { result } => {
                assert_eq!(
                    result.get("stopReason").and_then(|s| s.as_str()),
                    Some("end_turn")
                );
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn dispatches_error_response() {
        let line = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"boom"}}"#;
        match serde_json::from_str::<AcpMessage>(line).unwrap() {
            AcpMessage::Error { error } => {
                assert_eq!(error.get("message").and_then(|m| m.as_str()), Some("boom"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn session_id_extracted_from_prompt_request_and_response() {
        let req = r#"{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":"abc","prompt":[]}}"#;
        assert_eq!(
            serde_json::from_str::<AcpMessage>(req)
                .unwrap()
                .session_id()
                .as_deref(),
            Some("abc")
        );
        let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"sessionId":"def"}}"#;
        assert_eq!(
            serde_json::from_str::<AcpMessage>(resp)
                .unwrap()
                .session_id()
                .as_deref(),
            Some("def")
        );
    }

    #[test]
    fn unknown_shape_is_preserved() {
        let line = r#"{"jsonrpc":"2.0","id":1}"#;
        assert!(matches!(
            serde_json::from_str::<AcpMessage>(line).unwrap(),
            AcpMessage::Unknown(_)
        ));
    }
}
