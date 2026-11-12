#![forbid(unsafe_code)]

//! Agent chat event model (D-15, stage 5).
//!
//! Ports `AgentChatEvent`
//! (`apps/api/pi_dash/runner/models.py:1353-1388`): row struct +
//! column list.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-03-models-chat.golden.json`
//! (FX-RUN-03 `AgentChatEvent`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;

/// Physical table (`Meta.db_table`, `models.py:1370`).
pub const TABLE: &str = "agent_chat_event";
/// Default ordering (`Meta.ordering`, `models.py:1371`).
pub const ORDERING: &[&str] = &["session", "seq"];

/// Columns in declaration order (`models.py:1354-1367`), FK entries
/// as the Django attnames.
pub const COLUMNS: &[&str] = &[
    "id",
    "session_id",
    "message_id",
    "seq",
    "source_key",
    "kind",
    "payload",
    "created_at",
];

/// `source_key` bound (`models.py:1364`, `max_length=160`).
pub const SOURCE_KEY_MAX_LENGTH: usize = 160;
/// `kind` bound (`models.py:1365`, `max_length=64`).
pub const KIND_MAX_LENGTH: usize = 64;

/// `source_key` Django-side default (`models.py:1364`,
/// `default=""`). Empty keys are exempt from the source-key unique
/// constraint — only non-empty keys dedupe.
pub const DEFAULT_SOURCE_KEY: &str = "";

/// Fresh `payload` default (`models.py:1366`, `default=dict`).
pub fn default_payload() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// `session` FK: `CASCADE` (`models.py:1355`).
pub const SESSION_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `message` FK: `SET_NULL`, nullable (`models.py:1356-1362`).
pub const MESSAGE_ON_DELETE: OnDelete = OnDelete::SetNull;

/// Event identity per session (`models.py:1373-1376`).
pub const SESSION_SEQ_UNIQUE: &str = "ac_evt_sess_seq_uniq";
/// Fields of [`SESSION_SEQ_UNIQUE`], Django field names.
pub const SESSION_SEQ_UNIQUE_FIELDS: &[&str] = &["session", "seq"];

/// Source-key dedupe per session (`models.py:1377-1381`).
pub const SOURCE_KEY_UNIQUE: &str = "ac_evt_source_key_uniq";
/// Fields of [`SOURCE_KEY_UNIQUE`], Django field names.
pub const SOURCE_KEY_UNIQUE_FIELDS: &[&str] = &["session", "source_key"];
/// Condition of [`SOURCE_KEY_UNIQUE`] as SQL (`models.py:1379`,
/// `~Q(source_key="")`): the empty key is exempt.
pub const SOURCE_KEY_UNIQUE_CONDITION_SQL: &str = "\"agent_chat_event\".\"source_key\" <> ''";

/// Session + creation lookup index (`models.py:1383-1387`).
pub const SESSION_CREATED_INDEX: &str = "ac_evt_sess_created_idx";
/// Fields of [`SESSION_CREATED_INDEX`], Django field names.
pub const SESSION_CREATED_INDEX_FIELDS: &[&str] = &["session", "created_at"];

/// One chat event row, declaration order. `id` is a `BigAutoField`
/// (`models.py:1354`), hence `i64`; `seq` is a
/// `PositiveIntegerField` (`models.py:1363`), hence `i32` like the
/// D-02 `repository_count` port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatEvent {
    pub id: i64,
    pub session_id: uuid::Uuid,
    pub message_id: Option<uuid::Uuid>,
    pub seq: i32,
    pub source_key: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn event_fixture() -> serde_json::Value {
        ts::model(&ts::fx03(), "AgentChatEvent").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&event_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = event_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
        let seq = ts::constraint(&m, SESSION_SEQ_UNIQUE);
        assert_eq!(seq["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(seq["fields"], serde_json::json!(SESSION_SEQ_UNIQUE_FIELDS));
        let source = ts::constraint(&m, SOURCE_KEY_UNIQUE);
        assert_eq!(source["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(
            source["fields"],
            serde_json::json!(SOURCE_KEY_UNIQUE_FIELDS)
        );
        assert_eq!(
            source["condition"].as_str(),
            Some("(NOT (AND: ('source_key', '')))")
        );
        assert_eq!(
            SOURCE_KEY_UNIQUE_CONDITION_SQL,
            "\"agent_chat_event\".\"source_key\" <> ''"
        );
        let index = ts::index(&m, SESSION_CREATED_INDEX);
        assert_eq!(
            index["fields"],
            serde_json::json!(SESSION_CREATED_INDEX_FIELDS)
        );
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = event_fixture();
        let id = ts::field(&m, "id");
        assert_eq!(id["type"].as_str(), Some("BigAutoField"));
        assert_eq!(id["db_type"].as_str(), Some("bigint"));
        let session = ts::field(&m, "session");
        assert_eq!(session["type"].as_str(), Some("ForeignKey"));
        assert_eq!(session["null"].as_bool(), Some(false));
        assert_eq!(session["rel"]["to"].as_str(), Some("agent_chat_session"));
        assert_eq!(session["rel"]["on_delete"].as_str(), Some("CASCADE"));
        assert_eq!(SESSION_ON_DELETE, OnDelete::Cascade);
        let message = ts::field(&m, "message");
        assert_eq!(message["null"].as_bool(), Some(true));
        assert_eq!(message["rel"]["to"].as_str(), Some("agent_chat_message"));
        assert_eq!(message["rel"]["on_delete"].as_str(), Some("SET_NULL"));
        assert_eq!(MESSAGE_ON_DELETE, OnDelete::SetNull);
        assert_eq!(ts::field(&m, "seq")["db_type"].as_str(), Some("integer"));
        let source_key = ts::field(&m, "source_key");
        assert_eq!(source_key["db_type"].as_str(), Some("varchar(160)"));
        assert_eq!(source_key["max_length"].as_u64(), Some(160));
        assert_eq!(source_key["default"].as_str(), Some(DEFAULT_SOURCE_KEY));
        assert_eq!(SOURCE_KEY_MAX_LENGTH, 160);
        let kind = ts::field(&m, "kind");
        assert_eq!(kind["db_type"].as_str(), Some("varchar(64)"));
        assert_eq!(kind["max_length"].as_u64(), Some(64));
        assert_eq!(KIND_MAX_LENGTH, 64);
        assert_eq!(
            ts::field(&m, "payload")["default"].as_str(),
            Some("callable:<dict>")
        );
        assert!(default_payload().is_object());
        assert_eq!(
            ts::field(&m, "created_at")["auto_now_add"].as_bool(),
            Some(true)
        );
    }
}
