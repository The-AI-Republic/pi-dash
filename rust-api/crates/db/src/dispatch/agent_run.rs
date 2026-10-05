#![forbid(unsafe_code)]

//! Agent run read shape for dispatch (D-11, stage 5).
//!
//! Ports exactly the `AgentRun` columns dispatch touches
//! (`apps/api/pi_dash/runner/models.py:872-1035`), as recorded in the
//! fixture's `dispatch_touched_subset.agent_run`: 22 physical columns
//! in fixture order. The full model carries 41 columns; the rest
//! (`owner`, `runner`, `parent_run`, `terminal_*`, `prompt_manifest`,
//! `phase_kind`, `run_config`, `required_capabilities`, `thread_id`,
//! `agent_metadata`, `refusal_category`, the `*_tokens` generated
//! columns, `created_at`, `assigned_at`, `queue_position`, `ended_at`)
//! belong to the runner domain (D-13…D-15), which owns these tables —
//! this sub-issue ports no writes and no runner-owned reads.
//!
//! Column order in [`READ_COLUMNS`]: the fixture subset order, using
//! the Django attnames (`workspace_id`, …) for the six foreign keys,
//! like the D-20 `v1_cycles_modules::cycle::COLUMNS` port. The two
//! reverse relations in the fixture subset (`tool_calls`, `events`)
//! are not columns and are excluded; the child rows themselves are
//! [`super::event::AgentRunEvent`] and
//! [`super::tool_call::AgentRunToolCall`].
//!
//! Every application-level default below is Django-side (the live
//! table carries no `column_default` for these); Rust inserts — owned
//! by D-13…D-15, not here — must supply these values explicitly.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-02-models.golden.json`
//! (`dispatch_touched_subset.agent_run`, `agent_run_columns`,
//! `agent_run_meta`).
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};

use super::status::{AgentRunStatus, AgentRunTrigger};
use crate::integrations::OnDelete;
use pidash_types::dispatch::AgentExecutorKind;

/// Physical table (`Meta.db_table`, `models.py:1038`).
pub const TABLE: &str = "agent_run";
/// Default ordering (`Meta.ordering`, `models.py:1039`).
pub const ORDERING: &str = "-created_at";

/// Dispatch-touched columns in fixture subset order, FK entries as
/// Django attnames. Reads select this list; the runner-owned
/// remainder of the 40-column table is out of scope.
pub const READ_COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "created_by_id",
    "pod_id",
    "pinned_runner_id",
    "work_item_id",
    "scheduler_binding_id",
    "status",
    "executor_kind",
    "dispatch_attempts",
    "cancel_requested_at",
    "cancel_reason",
    "error_code",
    "tool_plan",
    "prompt",
    "trigger",
    "lease_expires_at",
    "started_at",
    "llm_model",
    "usage",
    "done_payload",
    "error",
];

/// `status` bound (`models.py:947-952`, `max_length=24`).
pub const STATUS_MAX_LENGTH: usize = 24;
/// `executor_kind` bound (`models.py:953-958`, `max_length=24`).
pub const EXECUTOR_KIND_MAX_LENGTH: usize = 24;
/// `cancel_reason` bound (`models.py:961`, `max_length=512`).
pub const CANCEL_REASON_MAX_LENGTH: usize = 512;
/// `error_code` bound (`models.py:962`, `max_length=64`).
pub const ERROR_CODE_MAX_LENGTH: usize = 64;
/// `trigger` bound (`models.py:972-977`, `max_length=24`).
pub const TRIGGER_MAX_LENGTH: usize = 24;
/// `llm_model` bound (`models.py:1015`, `max_length=128`).
pub const LLM_MODEL_MAX_LENGTH: usize = 128;

/// `status` Django-side default (`models.py:947-952`,
/// `default=AgentRunStatus.QUEUED`).
pub const DEFAULT_STATUS: AgentRunStatus = AgentRunStatus::Queued;
/// `executor_kind` Django-side default (`models.py:953-958`,
/// `default=AgentExecutorKind.LOCAL_RUNNER`).
pub const DEFAULT_EXECUTOR_KIND: AgentExecutorKind = AgentExecutorKind::LocalRunner;
/// `dispatch_attempts` Django-side default (`models.py:959`,
/// `default=0`).
pub const DEFAULT_DISPATCH_ATTEMPTS: i32 = 0;
/// `trigger` Django-side default (`models.py:972-977`,
/// `default=AgentRunTrigger.DIRECT`): a run-creation path that does
/// not set the trigger is treated as human-initiated rather than
/// silently automatic.
pub const DEFAULT_TRIGGER: AgentRunTrigger = AgentRunTrigger::Direct;
/// Shared `default=""` for `cancel_reason` (`models.py:961`),
/// `error_code` (`models.py:962`), `prompt` (`models.py:966`),
/// `llm_model` (`models.py:1015`) and `error` (`models.py:1005`).
pub const EMPTY_TEXT: &str = "";

/// Fresh `tool_plan` default (`models.py:963`, `default=dict`).
pub fn default_tool_plan() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Fresh `usage` default (`models.py:1020`, `default=dict`).
pub fn default_usage() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// `workspace` FK: `CASCADE` (`models.py:874-878`).
pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `created_by` FK: `PROTECT`, non-nullable (`models.py:891-896`).
pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::Protect;
/// `pod` FK: `PROTECT` (`models.py:898-902`).
pub const POD_ON_DELETE: OnDelete = OnDelete::Protect;
/// `pinned_runner` FK: `SET_NULL`, nullable (`models.py:915-921`).
pub const PINNED_RUNNER_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `work_item` FK: `SET_NULL`, nullable (`models.py:922-928`).
pub const WORK_ITEM_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `scheduler_binding` FK: `SET_NULL`, nullable (`models.py:932-938`).
pub const SCHEDULER_BINDING_ON_DELETE: OnDelete = OnDelete::SetNull;

/// One agent-run row, dispatch-touched columns only. Timestamps are
/// stored UTC (`DateTimeField`); `dispatch_attempts` is a
/// `PositiveIntegerField` (`models.py:959`), hence `i32` like the
/// D-02 `repository_count` port; `executor_kind` reuses the L1
/// [`AgentExecutorKind`] port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub created_by_id: uuid::Uuid,
    pub pod_id: uuid::Uuid,
    pub pinned_runner_id: Option<uuid::Uuid>,
    pub work_item_id: Option<uuid::Uuid>,
    pub scheduler_binding_id: Option<uuid::Uuid>,
    pub status: AgentRunStatus,
    pub executor_kind: AgentExecutorKind,
    pub dispatch_attempts: i32,
    pub cancel_requested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cancel_reason: String,
    pub error_code: String,
    pub tool_plan: serde_json::Value,
    pub prompt: String,
    /// The raw stored trigger, never parsed: Django's `TextChoices`
    /// are choices-only (no DB check), so legacy or hand-written
    /// rows can carry values outside [`AgentRunTrigger`] (runner
    /// migration 0029) and every read path carries them through.
    pub trigger: String,
    pub lease_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub llm_model: String,
    pub usage: serde_json::Value,
    pub done_payload: Option<serde_json::Value>,
    pub error: String,
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
    /// records Django field names (`workspace`, …) while
    /// [`READ_COLUMNS`] uses the attnames Django actually stores
    /// (`workspace_id`, …). A `ForeignKey` declaration maps `name` to
    /// `{name}_id`; every other field maps to itself; the two
    /// `(reverse)` entries are relations, not columns, and are
    /// skipped.
    fn subset_columns(v: &Value) -> Vec<String> {
        let columns = v["agent_run_columns"]
            .as_array()
            .expect("agent_run_columns is an array");
        let is_fk = |name: &str| {
            columns.iter().any(|c| {
                c["name"].as_str() == Some(name) && c["type"].as_str() == Some("ForeignKey")
            })
        };
        v["dispatch_touched_subset"]["agent_run"]
            .as_array()
            .expect("subset is an array")
            .iter()
            .filter_map(|e| e.as_str())
            .filter(|e| !e.ends_with("(reverse)"))
            .map(|name| {
                if is_fk(name) {
                    format!("{name}_id")
                } else {
                    name.to_string()
                }
            })
            .collect()
    }

    #[test]
    fn read_columns_match_fixture_subset() {
        assert_eq!(owned(READ_COLUMNS), subset_columns(&fixture()));
    }

    #[test]
    fn meta_matches_fixture() {
        let meta = &fixture()["agent_run_meta"];
        assert_eq!(TABLE, meta["db_table"].as_str().expect("db_table"));
        assert_eq!(
            serde_json::json!([ORDERING]),
            meta["ordering"],
            "Meta.ordering"
        );
    }

    #[test]
    fn defaults_match_fixture_column_entries() {
        let columns = &fixture()["agent_run_columns"];
        let entry = |name: &str| {
            columns
                .as_array()
                .expect("columns is an array")
                .iter()
                .find(|c| c["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("column {name}"))
        };
        // Enum defaults are recorded as AST dumps naming the member;
        // assert the dump names the same member the const holds.
        assert!(entry("status")["default"]
            .as_str()
            .expect("status default")
            .contains("QUEUED"));
        assert_eq!(DEFAULT_STATUS, AgentRunStatus::Queued);
        assert!(entry("executor_kind")["default"]
            .as_str()
            .expect("executor_kind default")
            .contains("LOCAL_RUNNER"));
        assert_eq!(DEFAULT_EXECUTOR_KIND, AgentExecutorKind::LocalRunner);
        assert!(entry("trigger")["default"]
            .as_str()
            .expect("trigger default")
            .contains("DIRECT"));
        assert_eq!(DEFAULT_TRIGGER, AgentRunTrigger::Direct);
        assert_eq!(
            entry("dispatch_attempts")["default"],
            serde_json::json!(DEFAULT_DISPATCH_ATTEMPTS)
        );
        for name in [
            "cancel_reason",
            "error_code",
            "prompt",
            "llm_model",
            "error",
        ] {
            assert_eq!(
                entry(name)["default"],
                serde_json::json!(EMPTY_TEXT),
                "default of {name}"
            );
        }
        // `default=dict` is recorded as the `dict` AST name; the Rust
        // default is a fresh empty object.
        for (name, default) in [
            ("tool_plan", default_tool_plan()),
            ("usage", default_usage()),
        ] {
            assert!(entry(name)["default"]
                .as_str()
                .expect("dict default")
                .contains("dict"));
            assert_eq!(default, serde_json::json!({}), "default of {name}");
        }
        // `done_payload` is `null=True` with no default: nullable
        // with no Rust-side default.
        assert_eq!(entry("done_payload")["null"], serde_json::json!(true));
        assert!(entry("done_payload").get("default").is_none());
        // Nullability of the subset FK + timestamp columns.
        for name in [
            "pinned_runner",
            "work_item",
            "scheduler_binding",
            "cancel_requested_at",
            "lease_expires_at",
            "started_at",
        ] {
            assert_eq!(
                entry(name)["null"],
                serde_json::json!(true),
                "null of {name}"
            );
        }
        assert_eq!(entry("created_by")["null"], serde_json::json!(false));
    }

    #[test]
    fn bounds_and_delete_rules_match_source() {
        let columns = &fixture()["agent_run_columns"];
        let max_length = |name: &str| {
            columns
                .as_array()
                .expect("columns is an array")
                .iter()
                .find(|c| c["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("column {name}"))["max_length"]
                .as_u64()
                .unwrap_or_else(|| panic!("max_length of {name}")) as usize
        };
        assert_eq!(STATUS_MAX_LENGTH, max_length("status"));
        assert_eq!(EXECUTOR_KIND_MAX_LENGTH, max_length("executor_kind"));
        assert_eq!(CANCEL_REASON_MAX_LENGTH, max_length("cancel_reason"));
        assert_eq!(ERROR_CODE_MAX_LENGTH, max_length("error_code"));
        assert_eq!(TRIGGER_MAX_LENGTH, max_length("trigger"));
        assert_eq!(LLM_MODEL_MAX_LENGTH, max_length("llm_model"));
        // `on_delete` is recorded as an AST dump; assert the dump
        // names the same action the const holds.
        let on_delete = |name: &str| {
            columns
                .as_array()
                .expect("columns is an array")
                .iter()
                .find(|c| c["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("column {name}"))["on_delete"]
                .as_str()
                .unwrap_or_else(|| panic!("on_delete of {name}"))
                .to_string()
        };
        assert!(on_delete("workspace").contains("CASCADE"));
        assert_eq!(WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert!(on_delete("created_by").contains("PROTECT"));
        assert_eq!(CREATED_BY_ON_DELETE, OnDelete::Protect);
        assert!(on_delete("pod").contains("PROTECT"));
        assert_eq!(POD_ON_DELETE, OnDelete::Protect);
        assert!(on_delete("pinned_runner").contains("SET_NULL"));
        assert_eq!(PINNED_RUNNER_ON_DELETE, OnDelete::SetNull);
        assert!(on_delete("work_item").contains("SET_NULL"));
        assert_eq!(WORK_ITEM_ON_DELETE, OnDelete::SetNull);
        assert!(on_delete("scheduler_binding").contains("SET_NULL"));
        assert_eq!(SCHEDULER_BINDING_ON_DELETE, OnDelete::SetNull);
    }
}
