#![forbid(unsafe_code)]

//! Agent chat message model (D-15, stage 5).
//!
//! Ports `AgentChatMessage`
//! (`apps/api/pi_dash/runner/models.py:1310-1350`): row struct +
//! column list. `role` / `status` reuse the L1
//! [`pidash_types::runner_runs::AgentChatMessageRole`] /
//! [`pidash_types::runner_runs::AgentChatMessageStatus`].
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-03-models-chat.golden.json`
//! (FX-RUN-03 `AgentChatMessage`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;
use pidash_types::runner_runs::{AgentChatMessageRole, AgentChatMessageStatus};

/// Physical table (`Meta.db_table`, `models.py:1329`).
pub const TABLE: &str = "agent_chat_message";
/// Default ordering (`Meta.ordering`, `models.py:1330`).
pub const ORDERING: &[&str] = &["session", "seq"];

/// Columns in declaration order (`models.py:1311-1326`), FK entry as
/// the Django attname.
pub const COLUMNS: &[&str] = &[
    "id",
    "session_id",
    "role",
    "content",
    "content_parts",
    "status",
    "local_item_id",
    "local_turn_id",
    "seq",
    "created_at",
    "completed_at",
];

/// `role` bound (`models.py:1313`, `max_length=16`).
pub const ROLE_MAX_LENGTH: usize = 16;
/// `status` bound (`models.py:1316-1321`, `max_length=24`).
pub const STATUS_MAX_LENGTH: usize = 24;
/// `local_item_id` bound (`models.py:1322`, `max_length=128`).
pub const LOCAL_ITEM_ID_MAX_LENGTH: usize = 128;
/// `local_turn_id` bound (`models.py:1323`, `max_length=128`).
pub const LOCAL_TURN_ID_MAX_LENGTH: usize = 128;

/// `status` Django-side default (`models.py:1316-1321`,
/// `default=AgentChatMessageStatus.COMPLETED`).
pub const DEFAULT_STATUS: AgentChatMessageStatus = AgentChatMessageStatus::Completed;
/// Shared `default=""` for `content` (`models.py:1314`),
/// `local_item_id` (`models.py:1322`) and `local_turn_id`
/// (`models.py:1323`).
pub const EMPTY_TEXT: &str = "";

/// Fresh `content_parts` default (`models.py:1315`,
/// `default=list`).
pub fn default_content_parts() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

/// `session` FK: `CASCADE` (`models.py:1312`).
pub const SESSION_ON_DELETE: OnDelete = OnDelete::Cascade;

/// Message identity per session (`models.py:1331-1336`).
pub const SESSION_SEQ_UNIQUE: &str = "ac_msg_sess_seq_uniq";
/// Fields of [`SESSION_SEQ_UNIQUE`], Django field names.
pub const SESSION_SEQ_UNIQUE_FIELDS: &[&str] = &["session", "seq"];

/// Session + creation lookup index (`models.py:1338-1341`).
pub const SESSION_CREATED_INDEX: &str = "ac_msg_sess_created_idx";
/// Fields of [`SESSION_CREATED_INDEX`], Django field names.
pub const SESSION_CREATED_INDEX_FIELDS: &[&str] = &["session", "created_at"];
/// Session + turn lookup index (`models.py:1342-1345`).
pub const SESSION_TURN_INDEX: &str = "ac_msg_sess_turn_idx";
/// Fields of [`SESSION_TURN_INDEX`], Django field names.
pub const SESSION_TURN_INDEX_FIELDS: &[&str] = &["session", "local_turn_id"];
/// Session + item lookup index (`models.py:1346-1349`).
pub const SESSION_ITEM_INDEX: &str = "ac_msg_sess_item_idx";
/// Fields of [`SESSION_ITEM_INDEX`], Django field names.
pub const SESSION_ITEM_INDEX_FIELDS: &[&str] = &["session", "local_item_id"];

/// One chat message row, declaration order. `seq` is a
/// `PositiveIntegerField` (`models.py:1324`), hence `i32` like the
/// D-02 `repository_count` port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatMessage {
    pub id: uuid::Uuid,
    pub session_id: uuid::Uuid,
    pub role: AgentChatMessageRole,
    pub content: String,
    pub content_parts: serde_json::Value,
    pub status: AgentChatMessageStatus,
    pub local_item_id: String,
    pub local_turn_id: String,
    pub seq: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn message_fixture() -> serde_json::Value {
        ts::model(&ts::fx03(), "AgentChatMessage").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&message_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = message_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
        let uniq = ts::constraint(&m, SESSION_SEQ_UNIQUE);
        assert_eq!(uniq["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(uniq["fields"], serde_json::json!(SESSION_SEQ_UNIQUE_FIELDS));
        for (name, fields) in [
            (SESSION_CREATED_INDEX, SESSION_CREATED_INDEX_FIELDS),
            (SESSION_TURN_INDEX, SESSION_TURN_INDEX_FIELDS),
            (SESSION_ITEM_INDEX, SESSION_ITEM_INDEX_FIELDS),
        ] {
            let index = ts::index(&m, name);
            assert_eq!(index["fields"], serde_json::json!(fields), "{name}");
        }
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = message_fixture();
        assert_eq!(
            ts::field(&m, "id")["default"].as_str(),
            Some("callable:<uuid4>")
        );
        let session = ts::field(&m, "session");
        assert_eq!(session["type"].as_str(), Some("ForeignKey"));
        assert_eq!(session["rel"]["to"].as_str(), Some("agent_chat_session"));
        assert_eq!(session["rel"]["on_delete"].as_str(), Some("CASCADE"));
        assert_eq!(SESSION_ON_DELETE, OnDelete::Cascade);
        for (field, bound) in [
            ("role", ROLE_MAX_LENGTH),
            ("status", STATUS_MAX_LENGTH),
            ("local_item_id", LOCAL_ITEM_ID_MAX_LENGTH),
            ("local_turn_id", LOCAL_TURN_ID_MAX_LENGTH),
        ] {
            assert_eq!(
                ts::field(&m, field)["max_length"].as_u64(),
                Some(bound as u64),
                "{field}"
            );
        }
        assert_eq!(
            ts::field(&m, "role")["default"].as_str(),
            Some("NOT_PROVIDED"),
            "role has no default: every row sets it"
        );
        assert_eq!(ts::field(&m, "role")["db_index"].as_bool(), Some(true));
        assert_eq!(ts::field(&m, "content")["db_type"].as_str(), Some("text"));
        assert_eq!(
            ts::field(&m, "content_parts")["default"].as_str(),
            Some("callable:<list>")
        );
        assert!(default_content_parts().is_array());
        assert_eq!(
            ts::field(&m, "status")["default"].as_str(),
            Some(DEFAULT_STATUS.value())
        );
        assert_eq!(ts::field(&m, "status")["db_index"].as_bool(), Some(true));
        for field in ["content", "local_item_id", "local_turn_id"] {
            assert_eq!(
                ts::field(&m, field)["default"].as_str(),
                Some(EMPTY_TEXT),
                "{field}"
            );
        }
        assert_eq!(ts::field(&m, "seq")["db_type"].as_str(), Some("integer"));
        assert_eq!(
            ts::field(&m, "created_at")["auto_now_add"].as_bool(),
            Some(true)
        );
        assert_eq!(ts::field(&m, "completed_at")["null"].as_bool(), Some(true));
    }
}
