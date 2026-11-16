#![forbid(unsafe_code)]

//! Agent run tool-call model (D-15, stage 5).
//!
//! Ports `AgentRunToolCall`
//! (`apps/api/pi_dash/runner/models.py:1187-1216`): row struct +
//! column list. `status` reuses the L1
//! [`pidash_types::runner_runs::ToolCallStatus`]. D-11's
//! `db::dispatch::tool_call` keeps its own read shape of this table;
//! this is the runner-domain full port.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-02-models-runs.golden.json`
//! (FX-RUN-02 `AgentRunToolCall`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;
use pidash_types::runner_runs::ToolCallStatus;

/// Physical table (`Meta.db_table`, `models.py:1209`).
pub const TABLE: &str = "agent_run_tool_call";

/// Columns in declaration order (`models.py:1190-1206`), FK entry as
/// the Django attname.
pub const COLUMNS: &[&str] = &[
    "id",
    "agent_run_id",
    "tool_call_id",
    "source",
    "server_key",
    "tool_name",
    "risk",
    "status",
    "request_fingerprint",
    "result_fingerprint",
    "idempotency_key_hash",
    "external_operation_id",
    "safe_replay_result",
    "error_code",
    "prepared_at",
    "submitted_at",
    "completed_at",
];

/// `tool_call_id` bound (`models.py:1192`, `max_length=255`).
pub const TOOL_CALL_ID_MAX_LENGTH: usize = 255;
/// `source` bound (`models.py:1193`, `max_length=16`).
pub const SOURCE_MAX_LENGTH: usize = 16;
/// `server_key` bound (`models.py:1194`, `max_length=64`).
pub const SERVER_KEY_MAX_LENGTH: usize = 64;
/// `tool_name` bound (`models.py:1195`, `max_length=255`).
pub const TOOL_NAME_MAX_LENGTH: usize = 255;
/// `risk` bound (`models.py:1196`, `max_length=16`).
pub const RISK_MAX_LENGTH: usize = 16;
/// `status` bound (`models.py:1197`, `max_length=16`).
pub const STATUS_MAX_LENGTH: usize = 16;
/// `request_fingerprint` bound (`models.py:1198`, `max_length=64`).
pub const REQUEST_FINGERPRINT_MAX_LENGTH: usize = 64;
/// `result_fingerprint` bound (`models.py:1199`, `max_length=64`).
pub const RESULT_FINGERPRINT_MAX_LENGTH: usize = 64;
/// `idempotency_key_hash` bound (`models.py:1200`, `max_length=64`).
pub const IDEMPOTENCY_KEY_HASH_MAX_LENGTH: usize = 64;
/// `external_operation_id` bound (`models.py:1201`,
/// `max_length=255`).
pub const EXTERNAL_OPERATION_ID_MAX_LENGTH: usize = 255;
/// `error_code` bound (`models.py:1203`, `max_length=64`).
pub const ERROR_CODE_MAX_LENGTH: usize = 64;

/// `status` Django-side default (`models.py:1197`,
/// `default=ToolCallStatus.PREPARED`).
pub const DEFAULT_STATUS: ToolCallStatus = ToolCallStatus::Prepared;
/// Shared `default=""` for `server_key` (`models.py:1194`),
/// `result_fingerprint` (`models.py:1199`),
/// `idempotency_key_hash` (`models.py:1200`),
/// `external_operation_id` (`models.py:1201`) and `error_code`
/// (`models.py:1203`).
pub const EMPTY_TEXT: &str = "";

/// `agent_run` FK: `CASCADE` (`models.py:1191`).
pub const AGENT_RUN_ON_DELETE: OnDelete = OnDelete::Cascade;

/// Tool-call identity per run (`models.py:1211-1214`).
pub const TOOL_CALL_UNIQUE: &str = "agent_run_tool_call_unique";
/// Fields of [`TOOL_CALL_UNIQUE`], Django field names.
pub const TOOL_CALL_UNIQUE_FIELDS: &[&str] = &["agent_run", "tool_call_id"];

/// Run + status lookup index (`models.py:1216`).
pub const TOOL_STATUS_INDEX: &str = "agent_run_tool_status_idx";
/// Fields of [`TOOL_STATUS_INDEX`], Django field names.
pub const TOOL_STATUS_INDEX_FIELDS: &[&str] = &["agent_run", "status"];

/// One tool-call ledger row, declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunToolCall {
    pub id: uuid::Uuid,
    pub agent_run_id: uuid::Uuid,
    pub tool_call_id: String,
    pub source: String,
    pub server_key: String,
    pub tool_name: String,
    pub risk: String,
    pub status: ToolCallStatus,
    pub request_fingerprint: String,
    pub result_fingerprint: String,
    pub idempotency_key_hash: String,
    pub external_operation_id: String,
    pub safe_replay_result: Option<serde_json::Value>,
    pub error_code: String,
    pub prepared_at: chrono::DateTime<chrono::Utc>,
    pub submitted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;

    fn tool_call_fixture() -> serde_json::Value {
        ts::model(&ts::fx02(), "AgentRunToolCall").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&tool_call_fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let m = tool_call_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert!(m["ordering"].as_array().expect("o").is_empty());
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
        let uniq = ts::constraint(&m, TOOL_CALL_UNIQUE);
        assert_eq!(uniq["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(uniq["fields"], serde_json::json!(TOOL_CALL_UNIQUE_FIELDS));
        let index = ts::index(&m, TOOL_STATUS_INDEX);
        assert_eq!(index["fields"], serde_json::json!(TOOL_STATUS_INDEX_FIELDS));
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = tool_call_fixture();
        assert_eq!(
            ts::field(&m, "id")["default"].as_str(),
            Some("callable:<uuid4>")
        );
        let run = ts::field(&m, "agent_run");
        assert_eq!(run["type"].as_str(), Some("ForeignKey"));
        assert_eq!(run["rel"]["to"].as_str(), Some("agent_run"));
        assert_eq!(run["rel"]["on_delete"].as_str(), Some("CASCADE"));
        assert_eq!(AGENT_RUN_ON_DELETE, OnDelete::Cascade);
        for (field, bound) in [
            ("tool_call_id", TOOL_CALL_ID_MAX_LENGTH),
            ("source", SOURCE_MAX_LENGTH),
            ("server_key", SERVER_KEY_MAX_LENGTH),
            ("tool_name", TOOL_NAME_MAX_LENGTH),
            ("risk", RISK_MAX_LENGTH),
            ("status", STATUS_MAX_LENGTH),
            ("request_fingerprint", REQUEST_FINGERPRINT_MAX_LENGTH),
            ("result_fingerprint", RESULT_FINGERPRINT_MAX_LENGTH),
            ("idempotency_key_hash", IDEMPOTENCY_KEY_HASH_MAX_LENGTH),
            ("external_operation_id", EXTERNAL_OPERATION_ID_MAX_LENGTH),
            ("error_code", ERROR_CODE_MAX_LENGTH),
        ] {
            assert_eq!(
                ts::field(&m, field)["max_length"].as_u64(),
                Some(bound as u64),
                "{field}"
            );
            assert_eq!(
                ts::field(&m, field)["null"].as_bool(),
                Some(false),
                "{field}"
            );
        }
        assert_eq!(
            ts::field(&m, "status")["default"].as_str(),
            Some(DEFAULT_STATUS.value())
        );
        for field in [
            "server_key",
            "result_fingerprint",
            "idempotency_key_hash",
            "external_operation_id",
            "error_code",
        ] {
            assert_eq!(
                ts::field(&m, field)["default"].as_str(),
                Some(EMPTY_TEXT),
                "{field}"
            );
        }
        assert_eq!(
            ts::field(&m, "safe_replay_result")["null"].as_bool(),
            Some(true)
        );
        assert_eq!(
            ts::field(&m, "prepared_at")["auto_now_add"].as_bool(),
            Some(true)
        );
        assert_eq!(ts::field(&m, "submitted_at")["null"].as_bool(), Some(true));
        assert_eq!(ts::field(&m, "completed_at")["null"].as_bool(), Some(true));
    }
}
