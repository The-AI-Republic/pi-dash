#![forbid(unsafe_code)]

//! Agent chat session model (D-15, stage 5).
//!
//! Ports `AgentChatSession`
//! (`apps/api/pi_dash/runner/models.py:1249-1307`): row struct +
//! column list. `status` reuses the L1
//! [`pidash_types::runner_runs::AgentChatSessionStatus`].
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-03-models-chat.golden.json`
//! (FX-RUN-03 `AgentChatSession`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;
use pidash_types::runner_runs::AgentChatSessionStatus;

/// Physical table (`Meta.db_table`, `models.py:1288`).
pub const TABLE: &str = "agent_chat_session";
/// Default ordering (`Meta.ordering`, `models.py:1289`).
pub const ORDERING: &[&str] = &["-last_message_at", "-created_at"];

/// Columns in declaration order (`models.py:1250-1285`), FK entries
/// as the Django attnames.
pub const COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "runner_id",
    "created_by_id",
    "pod_id",
    "status",
    "agent_kind",
    "local_thread_id",
    "local_session_id",
    "cwd",
    "model",
    "active_turn_id",
    "active_message_id",
    "close_requested",
    "last_message_at",
    "closed_at",
    "error",
    "created_at",
    "updated_at",
];

/// `status` bound (`models.py:1267-1272`, `max_length=24`).
pub const STATUS_MAX_LENGTH: usize = 24;
/// `agent_kind` bound (`models.py:1273`, `max_length=24`).
pub const AGENT_KIND_MAX_LENGTH: usize = 24;
/// `local_thread_id` bound (`models.py:1274`, `max_length=128`).
pub const LOCAL_THREAD_ID_MAX_LENGTH: usize = 128;
/// `local_session_id` bound (`models.py:1275`, `max_length=128`).
pub const LOCAL_SESSION_ID_MAX_LENGTH: usize = 128;
/// `model` bound (`models.py:1277`, `max_length=128`).
pub const MODEL_MAX_LENGTH: usize = 128;
/// `active_turn_id` bound (`models.py:1278`, `max_length=128`).
pub const ACTIVE_TURN_ID_MAX_LENGTH: usize = 128;

/// `status` Django-side default (`models.py:1267-1272`,
/// `default=AgentChatSessionStatus.OPEN`).
pub const DEFAULT_STATUS: AgentChatSessionStatus = AgentChatSessionStatus::Open;
/// `close_requested` Django-side default (`models.py:1280`,
/// `default=False`).
pub const DEFAULT_CLOSE_REQUESTED: bool = false;
/// Shared `default=""` for `agent_kind` (`models.py:1273`),
/// `local_thread_id` (`models.py:1274`), `local_session_id`
/// (`models.py:1275`), `cwd` (`models.py:1276`), `model`
/// (`models.py:1277`), `active_turn_id` (`models.py:1278`) and
/// `error` (`models.py:1283`).
pub const EMPTY_TEXT: &str = "";

/// `workspace` FK: `CASCADE` (`models.py:1251-1255`).
pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `runner` FK: `CASCADE` (`models.py:1256`).
pub const RUNNER_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `created_by` FK: `PROTECT` (`models.py:1257-1261`).
pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::Protect;
/// `pod` FK: `PROTECT` (`models.py:1262-1266`).
pub const POD_ON_DELETE: OnDelete = OnDelete::Protect;

/// Workspace + runner + status lookup index (`models.py:1291-1294`).
pub const WORKSPACE_RUNNER_STATUS_INDEX: &str = "ac_sess_ws_run_stat_idx";
/// Fields of [`WORKSPACE_RUNNER_STATUS_INDEX`], Django field names.
pub const WORKSPACE_RUNNER_STATUS_INDEX_FIELDS: &[&str] = &["workspace", "runner", "status"];
/// Creator + runner + status lookup index (`models.py:1295-1298`).
pub const USER_RUNNER_STATUS_INDEX: &str = "ac_sess_user_run_stat_idx";
/// Fields of [`USER_RUNNER_STATUS_INDEX`], Django field names.
pub const USER_RUNNER_STATUS_INDEX_FIELDS: &[&str] = &["created_by", "runner", "status"];
/// Runner + status lookup index (`models.py:1299-1302`).
pub const RUNNER_STATUS_INDEX: &str = "ac_sess_run_stat_idx";
/// Fields of [`RUNNER_STATUS_INDEX`], Django field names.
pub const RUNNER_STATUS_INDEX_FIELDS: &[&str] = &["runner", "status"];
/// Last-message sweep index (`models.py:1303-1306`).
pub const LAST_MESSAGE_INDEX: &str = "ac_sess_last_msg_idx";
/// Fields of [`LAST_MESSAGE_INDEX`], Django field names.
pub const LAST_MESSAGE_INDEX_FIELDS: &[&str] = &["last_message_at"];

/// One chat session row, declaration order. `active_message_id` is
/// a bare nullable `UUIDField` (`models.py:1279`) — no FK — hence
/// `Option<Uuid>` with no `OnDelete`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatSession {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub runner_id: uuid::Uuid,
    pub created_by_id: uuid::Uuid,
    pub pod_id: uuid::Uuid,
    pub status: AgentChatSessionStatus,
    pub agent_kind: String,
    pub local_thread_id: String,
    pub local_session_id: String,
    pub cwd: String,
    pub model: String,
    pub active_turn_id: String,
    pub active_message_id: Option<uuid::Uuid>,
    pub close_requested: bool,
    pub last_message_at: Option<chrono::DateTime<chrono::Utc>>,
    pub closed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn session_fixture() -> serde_json::Value {
        ts::model(&ts::fx03(), "AgentChatSession").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&session_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = session_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
        assert!(m["constraints"].as_array().expect("c").is_empty());
        for (name, fields) in [
            (
                WORKSPACE_RUNNER_STATUS_INDEX,
                WORKSPACE_RUNNER_STATUS_INDEX_FIELDS,
            ),
            (USER_RUNNER_STATUS_INDEX, USER_RUNNER_STATUS_INDEX_FIELDS),
            (RUNNER_STATUS_INDEX, RUNNER_STATUS_INDEX_FIELDS),
            (LAST_MESSAGE_INDEX, LAST_MESSAGE_INDEX_FIELDS),
        ] {
            let index = ts::index(&m, name);
            assert_eq!(index["fields"], serde_json::json!(fields), "{name}");
        }
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = session_fixture();
        assert_eq!(
            ts::field(&m, "id")["default"].as_str(),
            Some("callable:<uuid4>")
        );
        for (field, to, on_delete) in [
            ("workspace", "workspaces", "CASCADE"),
            ("runner", "runner", "CASCADE"),
            ("created_by", "users", "PROTECT"),
            ("pod", "pod", "PROTECT"),
        ] {
            let f = ts::field(&m, field);
            assert_eq!(f["type"].as_str(), Some("ForeignKey"), "{field}");
            assert_eq!(f["db_type"].as_str(), Some("uuid"), "{field}");
            assert_eq!(f["null"].as_bool(), Some(false), "{field}");
            assert_eq!(f["rel"]["to"].as_str(), Some(to), "{field}");
            assert_eq!(f["rel"]["on_delete"].as_str(), Some(on_delete), "{field}");
        }
        assert_eq!(WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(RUNNER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(CREATED_BY_ON_DELETE, OnDelete::Protect);
        assert_eq!(POD_ON_DELETE, OnDelete::Protect);
        for (field, bound) in [
            ("status", STATUS_MAX_LENGTH),
            ("agent_kind", AGENT_KIND_MAX_LENGTH),
            ("local_thread_id", LOCAL_THREAD_ID_MAX_LENGTH),
            ("local_session_id", LOCAL_SESSION_ID_MAX_LENGTH),
            ("model", MODEL_MAX_LENGTH),
            ("active_turn_id", ACTIVE_TURN_ID_MAX_LENGTH),
        ] {
            assert_eq!(
                ts::field(&m, field)["max_length"].as_u64(),
                Some(bound as u64),
                "{field}"
            );
        }
        assert_eq!(
            ts::field(&m, "status")["default"].as_str(),
            Some(DEFAULT_STATUS.value())
        );
        assert_eq!(ts::field(&m, "status")["db_index"].as_bool(), Some(true));
        for field in [
            "agent_kind",
            "local_thread_id",
            "local_session_id",
            "cwd",
            "model",
            "active_turn_id",
            "error",
        ] {
            assert_eq!(
                ts::field(&m, field)["default"].as_str(),
                Some(EMPTY_TEXT),
                "{field}"
            );
        }
        assert_eq!(ts::field(&m, "cwd")["db_type"].as_str(), Some("text"));
        assert_eq!(ts::field(&m, "error")["db_type"].as_str(), Some("text"));
        let active_message = ts::field(&m, "active_message_id");
        assert_eq!(active_message["type"].as_str(), Some("UUIDField"));
        assert_eq!(active_message["null"].as_bool(), Some(true));
        assert_eq!(
            ts::field(&m, "close_requested")["default"].as_bool(),
            Some(DEFAULT_CLOSE_REQUESTED)
        );
        assert_eq!(
            ts::field(&m, "last_message_at")["null"].as_bool(),
            Some(true)
        );
        assert_eq!(ts::field(&m, "closed_at")["null"].as_bool(), Some(true));
        assert_eq!(
            ts::field(&m, "created_at")["auto_now_add"].as_bool(),
            Some(true)
        );
        assert_eq!(
            ts::field(&m, "updated_at")["auto_now"].as_bool(),
            Some(true)
        );
    }
}
