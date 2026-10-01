#![forbid(unsafe_code)]

//! Agent run tool-call read shape for dispatch (D-11, stage 5).
//!
//! Ports the `AgentRunToolCall` columns dispatch touches
//! (`apps/api/pi_dash/runner/models.py:1187-1206`), as recorded in
//! the fixture's `dispatch_touched_subset.agent_run_tool_call`: 15
//! physical columns in fixture order. `external_operation_id`
//! (`models.py:1201`) and `prepared_at` (`models.py:1204`) are
//! runner-owned reads, out of scope here, as is every write — the
//! tables are owned by D-13…D-15.
//!
//! Column order in [`READ_COLUMNS`]: the fixture subset order, with
//! the FK entry as the Django attname (`agent_run_id`), like the
//! D-20 `v1_cycles_modules::cycle::COLUMNS` port.
//!
//! Also ports `ToolCallStatus` (`models.py:1178-1184`), the
//! `TextChoices` behind the `status` column (`models.py:1197`):
//! values verbatim, in declaration order, with the same enum API as
//! the L1 [`pidash_types::dispatch::AgentExecutorKind`] port
//! (PIDASHCONV-482).
//!
//! Every application-level default below is Django-side (the live
//! table carries no `column_default` for these).
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-02-models.golden.json`
//! (`tool_call_status_values`,
//! `dispatch_touched_subset.agent_run_tool_call`,
//! `agent_run_tool_call_columns`, `agent_run_tool_call_meta`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;

/// Tool-call lifecycle states (`models.py:1178-1184`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Prepared,
    Submitted,
    Succeeded,
    Failed,
    Unknown,
    Denied,
}

impl ToolCallStatus {
    /// The `TextChoices` value (`prepared`, …).
    pub fn value(&self) -> &'static str {
        match self {
            ToolCallStatus::Prepared => "prepared",
            ToolCallStatus::Submitted => "submitted",
            ToolCallStatus::Succeeded => "succeeded",
            ToolCallStatus::Failed => "failed",
            ToolCallStatus::Unknown => "unknown",
            ToolCallStatus::Denied => "denied",
        }
    }

    /// The human label (`models.py:1179-1184`).
    pub fn label(&self) -> &'static str {
        match self {
            ToolCallStatus::Prepared => "Prepared",
            ToolCallStatus::Submitted => "Submitted",
            ToolCallStatus::Succeeded => "Succeeded",
            ToolCallStatus::Failed => "Failed",
            ToolCallStatus::Unknown => "Outcome unknown",
            ToolCallStatus::Denied => "Denied",
        }
    }

    /// Parse a stored value (`ToolCallStatus.values` membership).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "prepared" => Some(ToolCallStatus::Prepared),
            "submitted" => Some(ToolCallStatus::Submitted),
            "succeeded" => Some(ToolCallStatus::Succeeded),
            "failed" => Some(ToolCallStatus::Failed),
            "unknown" => Some(ToolCallStatus::Unknown),
            "denied" => Some(ToolCallStatus::Denied),
            _ => None,
        }
    }
}

impl std::fmt::Display for ToolCallStatus {
    /// `str(member)`: the value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Physical table (`Meta.db_table`, `models.py:1209`).
pub const TABLE: &str = "agent_run_tool_call";
/// Idempotency-ledger identity (`Meta.constraints`,
/// `models.py:1210-1215`): the provider's call id is unique per run.
pub const UNIQUE_CONSTRAINT: &str = "agent_run_tool_call_unique";
/// Fields of [`UNIQUE_CONSTRAINT`].
pub const UNIQUE_CONSTRAINT_FIELDS: &[&str] = &["agent_run", "tool_call_id"];
/// Per-run status lookup (`Meta.indexes`, `models.py:1216`).
pub const TOOL_STATUS_INDEX: &str = "agent_run_tool_status_idx";
/// Fields of [`TOOL_STATUS_INDEX`].
pub const TOOL_STATUS_INDEX_FIELDS: &[&str] = &["agent_run", "status"];

/// Dispatch-touched columns in fixture subset order, FK entry as the
/// Django attname.
pub const READ_COLUMNS: &[&str] = &[
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
    "safe_replay_result",
    "error_code",
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
/// `error_code` bound (`models.py:1203`, `max_length=64`).
pub const ERROR_CODE_MAX_LENGTH: usize = 64;

/// `status` Django-side default (`models.py:1197`,
/// `default=ToolCallStatus.PREPARED`).
pub const DEFAULT_STATUS: ToolCallStatus = ToolCallStatus::Prepared;
/// Shared `default=""` for `server_key` (`models.py:1194`),
/// `result_fingerprint` (`models.py:1199`), `idempotency_key_hash`
/// (`models.py:1200`) and `error_code` (`models.py:1203`).
pub const EMPTY_TEXT: &str = "";

/// `agent_run` FK: `CASCADE` (`models.py:1191`).
pub const AGENT_RUN_ON_DELETE: OnDelete = OnDelete::Cascade;

/// One ledger row, dispatch-touched columns only. Timestamps are
/// stored UTC (`DateTimeField`).
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
    pub safe_replay_result: Option<serde_json::Value>,
    pub error_code: String,
    pub submitted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-02-models.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    /// Copy a const into an owned vec so assertions compare two
    /// runtime values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Fixture subset entries mapped to physical columns: the fixture
    /// records Django field names while [`READ_COLUMNS`] uses the
    /// attnames Django actually stores (`agent_run_id` for the FK).
    fn subset_columns(v: &Value) -> Vec<String> {
        v["dispatch_touched_subset"]["agent_run_tool_call"]
            .as_array()
            .expect("subset is an array")
            .iter()
            .filter_map(|e| e.as_str())
            .map(|name| {
                if name == "agent_run" {
                    "agent_run_id".to_string()
                } else {
                    name.to_string()
                }
            })
            .collect()
    }

    #[test]
    fn tool_call_status_values_match_fixture_verbatim_in_order() {
        let statuses = [
            ToolCallStatus::Prepared,
            ToolCallStatus::Submitted,
            ToolCallStatus::Succeeded,
            ToolCallStatus::Failed,
            ToolCallStatus::Unknown,
            ToolCallStatus::Denied,
        ];
        let values: Vec<&str> = statuses.iter().map(ToolCallStatus::value).collect();
        assert_eq!(
            serde_json::json!(values),
            fixture()["tool_call_status_values"]
        );
        // Labels come from the source (`models.py:1179-1184`); the
        // fixture records values only, so pin the full label set here.
        let labels: Vec<&str> = statuses.iter().map(ToolCallStatus::label).collect();
        assert_eq!(
            labels,
            [
                "Prepared",
                "Submitted",
                "Succeeded",
                "Failed",
                "Outcome unknown",
                "Denied",
            ]
        );
    }

    #[test]
    fn tool_call_status_serde_round_trips_values_and_rejects_unknown() {
        for status in [
            ToolCallStatus::Prepared,
            ToolCallStatus::Submitted,
            ToolCallStatus::Succeeded,
            ToolCallStatus::Failed,
            ToolCallStatus::Unknown,
            ToolCallStatus::Denied,
        ] {
            let rendered = serde_json::to_string(&status).expect("serializes");
            assert_eq!(rendered, format!("\"{}\"", status.value()));
            let back: ToolCallStatus = serde_json::from_str(&rendered).expect("parses");
            assert_eq!(back, status);
            assert_eq!(ToolCallStatus::from_value(status.value()), Some(status));
        }
        assert_eq!(ToolCallStatus::from_value("bogus"), None);
        assert!(serde_json::from_str::<ToolCallStatus>("\"bogus\"").is_err());
        assert_eq!(ToolCallStatus::Prepared.to_string(), "prepared");
    }

    #[test]
    fn read_columns_match_fixture_subset() {
        assert_eq!(owned(READ_COLUMNS), subset_columns(&fixture()));
    }

    #[test]
    fn meta_matches_source() {
        // `db_table` is recorded verbatim; the constraint/index AST
        // dumps are truncated in the fixture, so the names are pinned
        // against the source (`models.py:1209-1216`) here.
        let meta = &fixture()["agent_run_tool_call_meta"];
        assert_eq!(TABLE, meta["db_table"].as_str().expect("db_table"));
        assert_eq!(UNIQUE_CONSTRAINT, "agent_run_tool_call_unique");
        assert_eq!(UNIQUE_CONSTRAINT_FIELDS, &["agent_run", "tool_call_id"]);
        assert_eq!(TOOL_STATUS_INDEX, "agent_run_tool_status_idx");
        assert_eq!(TOOL_STATUS_INDEX_FIELDS, &["agent_run", "status"]);
    }

    #[test]
    fn defaults_and_rules_match_fixture_column_entries() {
        let columns = &fixture()["agent_run_tool_call_columns"];
        let entry = |name: &str| {
            columns
                .as_array()
                .expect("columns is an array")
                .iter()
                .find(|c| c["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("column {name}"))
        };
        assert!(entry("status")["default"]
            .as_str()
            .expect("status default")
            .contains("PREPARED"));
        assert_eq!(DEFAULT_STATUS, ToolCallStatus::Prepared);
        for name in [
            "server_key",
            "result_fingerprint",
            "idempotency_key_hash",
            "error_code",
        ] {
            assert_eq!(
                entry(name)["default"],
                serde_json::json!(EMPTY_TEXT),
                "default of {name}"
            );
        }
        // `safe_replay_result` is `null=True` with no default:
        // nullable with no Rust-side default.
        assert_eq!(entry("safe_replay_result")["null"], serde_json::json!(true));
        assert!(entry("safe_replay_result").get("default").is_none());
        for name in ["submitted_at", "completed_at"] {
            assert_eq!(
                entry(name)["null"],
                serde_json::json!(true),
                "null of {name}"
            );
        }
        let max_length = |name: &str| {
            entry(name)["max_length"]
                .as_u64()
                .unwrap_or_else(|| panic!("max_length of {name}")) as usize
        };
        assert_eq!(TOOL_CALL_ID_MAX_LENGTH, max_length("tool_call_id"));
        assert_eq!(SOURCE_MAX_LENGTH, max_length("source"));
        assert_eq!(SERVER_KEY_MAX_LENGTH, max_length("server_key"));
        assert_eq!(TOOL_NAME_MAX_LENGTH, max_length("tool_name"));
        assert_eq!(RISK_MAX_LENGTH, max_length("risk"));
        assert_eq!(STATUS_MAX_LENGTH, max_length("status"));
        assert_eq!(
            REQUEST_FINGERPRINT_MAX_LENGTH,
            max_length("request_fingerprint")
        );
        assert_eq!(
            RESULT_FINGERPRINT_MAX_LENGTH,
            max_length("result_fingerprint")
        );
        assert_eq!(
            IDEMPOTENCY_KEY_HASH_MAX_LENGTH,
            max_length("idempotency_key_hash")
        );
        assert_eq!(ERROR_CODE_MAX_LENGTH, max_length("error_code"));
        assert!(entry("agent_run")["on_delete"]
            .as_str()
            .expect("on_delete")
            .contains("CASCADE"));
        assert_eq!(AGENT_RUN_ON_DELETE, OnDelete::Cascade);
    }
}
