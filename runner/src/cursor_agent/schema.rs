//! Stream-JSON event shapes emitted by `cursor-agent --print --output-format
//! stream-json`. Modelled the same way as the Claude Code bridge: capture the
//! envelope (`type` + `subtype`) plus the handful of fields we actually read,
//! and retain the full body as a `serde_json::Value` so the daemon can ship it
//! to local history verbatim without tracking every upstream schema revision.
//!
//! Cursor's stream-json format is intentionally close to Claude's
//! (`system`/`user`/`assistant`/`result`) with one extra family — explicit
//! `tool_call` events carrying a `subtype` of `started` / `completed`.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

/// Top-level stream-JSON event. Dispatch is on the top-level `type` tag;
/// unknown tags collapse to [`StreamEvent::Unknown`] so forward-compatible
/// upstream changes don't crash the bridge.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// `{"type":"system","subtype":"init","session_id":"...","model":"...",
    ///   "cwd":"...","permissionMode":"...","apiKeySource":"..."}` — the very
    /// first frame. We capture `session_id` as the thread id so history/IPC
    /// can link follow-up work to the same conversation.
    System(SystemEvent),
    /// `{"type":"assistant","message":{...},"session_id":"..."}` — one message
    /// segment from the model (emitted between tool calls).
    Assistant(MessageEvent),
    /// `{"type":"user","message":{...},"session_id":"..."}` — the prompt echo
    /// and/or tool-result echoes. Useful for transcripts.
    User(MessageEvent),
    /// `{"type":"tool_call","subtype":"started|completed","call_id":"...",
    ///   "tool_call":{...},"session_id":"..."}` — tool execution lifecycle.
    ToolCall(ToolCallEvent),
    /// `{"type":"result","subtype":"success","is_error":false,
    ///   "duration_ms":..,"result":"...","session_id":"..."}` — terminal frame.
    Result(ResultEvent),
    /// Anything else: new event types cursor-agent may emit that we don't map
    /// yet. Preserved verbatim in history.
    Unknown(serde_json::Value),
}

impl<'de> Deserialize<'de> for StreamEvent {
    // Dispatch manually on `type` rather than `#[serde(untagged)]`: several
    // variants share field sets (`system` and `result` both carry `subtype` +
    // `session_id`), so an untagged enum would greedily misroute frames.
    // Tagging on `type` is unambiguous and keeps a catch-all for forward-compat.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        match ty {
            "system" => serde_json::from_value(v)
                .map(StreamEvent::System)
                .map_err(D::Error::custom),
            "assistant" => serde_json::from_value(v)
                .map(StreamEvent::Assistant)
                .map_err(D::Error::custom),
            "user" => serde_json::from_value(v)
                .map(StreamEvent::User)
                .map_err(D::Error::custom),
            "tool_call" => serde_json::from_value(v)
                .map(StreamEvent::ToolCall)
                .map_err(D::Error::custom),
            "result" => serde_json::from_value(v)
                .map(StreamEvent::Result)
                .map_err(D::Error::custom),
            _ => Ok(StreamEvent::Unknown(v)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemEvent {
    pub subtype: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// Shared shape for `assistant` and `user` events: both wrap a `message`
/// object (`{role, content}`) and carry the session id.
#[derive(Debug, Clone, Deserialize)]
pub struct MessageEvent {
    pub message: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallEvent {
    pub subtype: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResultEvent {
    pub subtype: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s1","model":"cursor-x","permissionMode":"force"}"#;
        match serde_json::from_str::<StreamEvent>(line).unwrap() {
            StreamEvent::System(s) => {
                assert_eq!(s.subtype, "init");
                assert_eq!(s.session_id.as_deref(), Some("s1"));
                assert_eq!(
                    s.rest.get("model").and_then(|m| m.as_str()),
                    Some("cursor-x")
                );
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    #[test]
    fn dispatches_assistant_and_user() {
        let a = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]},"session_id":"s1"}"#;
        assert!(matches!(
            serde_json::from_str::<StreamEvent>(a).unwrap(),
            StreamEvent::Assistant(_)
        ));
        let u = r#"{"type":"user","message":{"role":"user","content":[]},"session_id":"s1"}"#;
        assert!(matches!(
            serde_json::from_str::<StreamEvent>(u).unwrap(),
            StreamEvent::User(_)
        ));
    }

    #[test]
    fn dispatches_tool_call_subtypes() {
        let started = r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"shellToolCall":{}},"session_id":"s1"}"#;
        match serde_json::from_str::<StreamEvent>(started).unwrap() {
            StreamEvent::ToolCall(t) => assert_eq!(t.subtype, "started"),
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn dispatches_result_terminal() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1200,"result":"done","session_id":"s1"}"#;
        match serde_json::from_str::<StreamEvent>(line).unwrap() {
            StreamEvent::Result(r) => {
                assert_eq!(r.subtype, "success");
                assert_eq!(r.is_error, Some(false));
                assert_eq!(r.duration_ms, Some(1200));
                assert_eq!(r.result.as_deref(), Some("done"));
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn unknown_type_is_preserved() {
        let line = r#"{"type":"telemetry","foo":1}"#;
        assert!(matches!(
            serde_json::from_str::<StreamEvent>(line).unwrap(),
            StreamEvent::Unknown(_)
        ));
    }
}
