//! Stream-JSON event shapes emitted by `claude --print --output-format
//! stream-json`. We model the envelope (`type` + `subtype`) plus the fields
//! we actually read; the full message body is retained as a
//! `serde_json::Value` so the daemon can ship it to local history verbatim
//! without us having to track every upstream schema revision.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

/// Top-level stream-JSON event. Dispatch is on the top-level `type` tag;
/// unknown tags collapse to [`StreamEvent::Unknown`] so forward-compatible
/// upstream changes don't crash the bridge.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// `{"type":"system","subtype":"init","session_id":"...",...}` —
    /// the very first frame. We capture `session_id` as the thread id so
    /// history/IPC can link follow-up work to the same conversation.
    System(SystemEvent),
    /// `{"type":"assistant","message":{...}}` — one message from Claude.
    /// May contain `text`, `tool_use`, or `thinking` content blocks.
    Assistant(AssistantEvent),
    /// `{"type":"user","message":{...}}` — tool-result echoes from the
    /// harness back to the model. Useful for transcripts.
    User(UserEvent),
    /// `{"type":"result","subtype":"success|error_max_turns|error_during_execution",
    ///   "result":"...","usage":{...}}` — terminal frame.
    Result(ResultEvent),
    /// Anything else: new event types Claude may emit that we don't map
    /// yet. Preserved verbatim in history.
    Unknown(serde_json::Value),
}

impl<'de> Deserialize<'de> for StreamEvent {
    // We dispatch manually instead of using `#[serde(untagged)]` because
    // several variants have overlapping field sets (e.g. `system` and
    // `result` both carry `subtype` + `session_id`). An untagged enum
    // would greedily match the first compatible variant and silently
    // misroute frames. Tagging on `type` is unambiguous and keeps a
    // catch-all for forward-compat.
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

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantEvent {
    pub message: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserEvent {
    pub message: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
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
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub usage: Option<serde_json::Value>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl StreamEvent {
    /// Stable event discriminator for routing + history. Mirrors the
    /// `method` slot used by the Codex bridge so `HistoryEntry::CodexEvent`
    /// (kept for on-disk JSONL compat) remains meaningful.
    pub fn method(&self) -> &str {
        match self {
            StreamEvent::System(_) => "system",
            StreamEvent::Assistant(_) => "assistant/message",
            StreamEvent::User(_) => "user/toolResult",
            StreamEvent::Result(_) => "result",
            StreamEvent::Unknown(_) => "unknown",
        }
    }
}

/// Input envelope written to Claude's stdin when `--input-format stream-json`
/// is in effect. For MVP we only ever send a single user turn.
#[derive(Debug, Clone, Serialize)]
pub struct UserInput<'a> {
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub message: UserMessage<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserMessage<'a> {
    pub role: &'static str,
    pub content: &'a str,
}

impl<'a> UserInput<'a> {
    pub fn user_text(content: &'a str) -> Self {
        Self {
            ty: "user",
            message: UserMessage {
                role: "user",
                content,
            },
        }
    }
}
