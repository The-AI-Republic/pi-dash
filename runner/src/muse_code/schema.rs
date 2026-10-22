//! JSONL record shapes emitted by `muse exec --json`.
//!
//! Muse Code (Meta) is closed-source and its event schema is not publicly
//! documented, so this module is modeled on frames captured from a real
//! binary (`muse 1.3.0-R3401.1`) — see `runner/tests/fixtures/muse_code/`.
//! Every line is an event-sourced *record* envelope with no top-level `type`:
//!
//! ```json
//! {"record_type":"event","payload_type":"run.terminal.completed",
//!  "payload":{"kind":"run_terminal","terminal":"completed","text":"..."},
//!  "sequence":654,"stream":{"id":"01a0b6ab-...","kind":"session"},
//!  "schema_version":1,"payload_schema_version":1,"id":"...", ...}
//! ```
//!
//! Dispatch is on `payload_type` (`run.terminal.completed`,
//! `run.output.delta`, `tool.result`, `task.lifecycle.*`, ...). The full body is
//! retained so the daemon can ship it to local history verbatim. A line that is
//! valid JSON but not a record envelope collapses to [`StreamEvent::Unknown`];
//! the bridge refuses to start a run on one (see `Bridge::wait_for_init`), so a
//! future envelope change fails the first frame loudly instead of silently
//! streaming a whole run of unrecognized frames.

use serde::{Deserialize, Deserializer};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A `{"record_type", "payload_type", "payload", ...}` envelope.
    Record(Record),
    /// Valid JSON that is not a record envelope. Preserved verbatim in history.
    Unknown(serde_json::Value),
}

impl<'de> Deserialize<'de> for StreamEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(match Record::from_value(&v) {
            Some(r) => StreamEvent::Record(r),
            None => StreamEvent::Unknown(v),
        })
    }
}

/// One record envelope. `payload_type` is the discriminator; `payload` holds
/// the type-specific body; `raw` is the full line as emitted.
#[derive(Debug, Clone)]
pub struct Record {
    /// `event` / `status` / `reconciliation`.
    pub record_type: String,
    pub payload_type: String,
    pub payload: serde_json::Value,
    pub raw: serde_json::Value,
}

impl Record {
    fn from_value(v: &serde_json::Value) -> Option<Self> {
        let record_type = v.get("record_type")?.as_str()?.to_owned();
        let payload_type = v.get("payload_type")?.as_str()?.to_owned();
        Some(Self {
            record_type,
            payload_type,
            payload: v.get("payload").cloned().unwrap_or(serde_json::Value::Null),
            raw: v.clone(),
        })
    }

    /// The Muse session id: the envelope's `stream.id` when `stream.kind` is
    /// `session`. It is a UUID and is exactly what `muse exec --session-id`
    /// accepts to resume the session on a later turn. Non-UUID ids are
    /// rejected so they can never reach `--session-id`.
    pub fn session_id(&self) -> Option<String> {
        let stream = self.raw.get("stream")?;
        if stream.get("kind")?.as_str()? != "session" {
            return None;
        }
        let id = stream.get("id")?.as_str()?;
        Uuid::parse_str(id).ok().map(|_| id.to_owned())
    }

    /// A string field of `payload`.
    pub fn payload_str(&self, key: &str) -> Option<&str> {
        self.payload.get(key).and_then(|v| v.as_str())
    }

    /// `payload.event.task_kind` on `task.lifecycle.proposed` records, e.g.
    /// `tool.bash`, `model.meta.response`, `reminder.agent.skill-reminder`.
    pub fn task_kind(&self) -> Option<&str> {
        self.payload
            .get("event")
            .and_then(|e| e.get("task_kind"))
            .and_then(|v| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(line: &str) -> Record {
        match serde_json::from_str::<StreamEvent>(line).unwrap() {
            StreamEvent::Record(r) => r,
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn parses_terminal_completed_record() {
        let r = record(
            r#"{"record_type":"event","payload_type":"run.terminal.completed","payload":{"kind":"run_terminal","terminal":"completed","reason":null,"text":"all done"},"sequence":654,"stream":{"id":"01a0b6ab-0000-7000-8000-000000000001","kind":"session"},"schema_version":1}"#,
        );
        assert_eq!(r.record_type, "event");
        assert_eq!(r.payload_type, "run.terminal.completed");
        assert_eq!(r.payload_str("terminal"), Some("completed"));
        assert_eq!(r.payload_str("text"), Some("all done"));
        assert_eq!(
            r.session_id().as_deref(),
            Some("01a0b6ab-0000-7000-8000-000000000001")
        );
    }

    #[test]
    fn session_id_requires_session_stream_and_uuid() {
        let task = record(
            r#"{"record_type":"event","payload_type":"x","payload":{},"stream":{"id":"01a0b6ab-0000-7000-8000-000000000001","kind":"task"}}"#,
        );
        assert_eq!(task.session_id(), None);
        let not_uuid = record(
            r#"{"record_type":"event","payload_type":"x","payload":{},"stream":{"id":"muse-abc","kind":"session"}}"#,
        );
        assert_eq!(not_uuid.session_id(), None);
    }

    #[test]
    fn task_kind_reads_proposed_event() {
        let r = record(
            r#"{"record_type":"event","payload_type":"task.lifecycle.proposed","payload":{"kind":"task_lifecycle","event":{"kind":"proposed","task_kind":"tool.bash","task_id":"t1"}}}"#,
        );
        assert_eq!(r.task_kind(), Some("tool.bash"));
    }

    #[test]
    fn non_envelope_json_is_unknown() {
        // The shape the original bridge assumed (Claude-style `type` tag) is not
        // a Muse record and must not be mistaken for one.
        for line in [
            r#"{"type":"result","subtype":"success","result":"done"}"#,
            r#"{"payload_type":"run.terminal.completed"}"#,
            r#"[1,2,3]"#,
        ] {
            assert!(
                matches!(
                    serde_json::from_str::<StreamEvent>(line).unwrap(),
                    StreamEvent::Unknown(_)
                ),
                "{line}"
            );
        }
    }
}
