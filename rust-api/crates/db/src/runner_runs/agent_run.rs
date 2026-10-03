#![forbid(unsafe_code)]

//! Agent run model (D-15, stage 5).
//!
//! Ports `AgentRun` (`apps/api/pi_dash/runner/models.py:872-1161`):
//! all 41 columns/defaults/constraints/indexes (`Meta`,
//! `:1037-1102`), the `save` pod-resolution order + owner→created_by
//! mirror (`:1104-1134`), and the token pseudo-fields as
//! `(usage->>key)::bigint` reads (`:1024-1026`,
//! `fields.py:56-58`). `is_terminal` / `is_active` (`:1136-1159`)
//! live on the L1 [`pidash_types::runner_runs::AgentRunStatus`] and
//! are reused, not re-ported.
//!
//! D-11's `db::dispatch::agent_run` keeps its own 22-column read
//! shape for dispatch queries; this is the runner-domain full port.
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-02-models-runs.golden.json`
//! (FX-RUN-02 `AgentRun`, `agentrun_save`, `run_sets`,
//! `token_read_sql`).
//!
//! Ported bugs: none found in this unit on read-through. Ported
//! as-is: the `hasattr(self.work_item, "project_id")` guard
//! (`:1115`) only fires when `work_item_id` is set yet the relation
//! resolves to `None`, which the FK descriptor never produces — a
//! missing work-item row raises `DoesNotExist` instead of resolving
//! to `None`. Callers therefore pass an already-resolved project id
//! (or `None`) into [`resolve_pod_id`]; the fetch-or-raise lives on
//! the write path, not here.

use serde::{Deserialize, Serialize};

use crate::integrations::OnDelete;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, AgentRunTrigger};

/// Physical table (`Meta.db_table`, `models.py:1038`).
pub const TABLE: &str = "agent_run";
/// Default ordering (`Meta.ordering`, `models.py:1039`).
pub const ORDERING: &[&str] = &["-created_at"];

/// Columns in declaration order (`models.py:873-1035`), FK entries
/// as the Django attnames. The three `*_tokens` entries are real
/// generated columns Postgres derives from `usage`
/// (`JSONKeyBigIntegerField`, `fields.py:21-61`).
pub const COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "owner_id",
    "created_by_id",
    "pod_id",
    "runner_id",
    "pinned_runner_id",
    "work_item_id",
    "scheduler_binding_id",
    "parent_run_id",
    "status",
    "executor_kind",
    "dispatch_attempts",
    "cancel_requested_at",
    "cancel_reason",
    "error_code",
    "tool_plan",
    "terminal_hooks_applied_at",
    "terminal_capacity_released_at",
    "prompt",
    "trigger",
    "prompt_manifest",
    "phase_kind",
    "run_config",
    "required_capabilities",
    "thread_id",
    "agent_metadata",
    "lease_expires_at",
    "done_payload",
    "error",
    "refusal_category",
    "llm_model",
    "usage",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "created_at",
    "assigned_at",
    "queue_position",
    "started_at",
    "ended_at",
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
/// `phase_kind` bound (`models.py:989`, `max_length=32`).
pub const PHASE_KIND_MAX_LENGTH: usize = 32;
/// `thread_id` bound (`models.py:992`, `max_length=128`).
pub const THREAD_ID_MAX_LENGTH: usize = 128;
/// `refusal_category` bound (`models.py:1009-1014`,
/// `max_length=32`).
pub const REFUSAL_CATEGORY_MAX_LENGTH: usize = 32;
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
/// `phase_kind` (`models.py:989`), `thread_id` (`models.py:992`),
/// `error` (`models.py:1005`), `refusal_category`
/// (`models.py:1009-1014`) and `llm_model` (`models.py:1015`).
pub const EMPTY_TEXT: &str = "";

/// Fresh `tool_plan` default (`models.py:963`, `default=dict`).
pub fn default_tool_plan() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Fresh `run_config` default (`models.py:990`, `default=dict`).
pub fn default_run_config() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Fresh `required_capabilities` default (`models.py:991`,
/// `default=list`).
pub fn default_required_capabilities() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

/// Fresh `agent_metadata` default (`models.py:1002`, `default=dict`).
pub fn default_agent_metadata() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Fresh `usage` default (`models.py:1020`, `default=dict`).
pub fn default_usage() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// `workspace` FK: `CASCADE` (`models.py:874-878`).
pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
/// `owner` FK: `SET_NULL`, nullable (`models.py:882-888`).
pub const OWNER_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `created_by` FK: `PROTECT`, non-nullable (`models.py:891-896`).
pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::Protect;
/// `pod` FK: `PROTECT` (`models.py:898-902`).
pub const POD_ON_DELETE: OnDelete = OnDelete::Protect;
/// `runner` FK: `SET_NULL`, nullable (`models.py:903-909`).
pub const RUNNER_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `pinned_runner` FK: `SET_NULL`, nullable (`models.py:915-921`).
pub const PINNED_RUNNER_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `work_item` FK: `SET_NULL`, nullable (`models.py:922-928`).
pub const WORK_ITEM_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `scheduler_binding` FK: `SET_NULL`, nullable
/// (`models.py:932-938`).
pub const SCHEDULER_BINDING_ON_DELETE: OnDelete = OnDelete::SetNull;
/// `parent_run` FK: `SET_NULL`, nullable (`models.py:939-946`).
pub const PARENT_RUN_ON_DELETE: OnDelete = OnDelete::SetNull;

/// Cloud runs carry no local assignment (`models.py:1065-1078`):
/// either a machine executor, or `cloud_agent` with the five
/// local-assignment columns all `NULL`.
pub const CLOUD_CHECK: &str = "agent_run_cloud_has_no_local_assignment";

/// Evaluate [`CLOUD_CHECK`] over a candidate row. Machine executors
/// (`MACHINE_EXECUTORS`: local + managed) always satisfy it; a
/// cloud-agent run satisfies it only with `runner`,
/// `pinned_runner`, `owner`, `assigned_at` and `queue_position` all
/// unset. Any other executor value fails the check, as in Django.
pub fn cloud_assignment_valid(
    executor_kind: AgentExecutorKind,
    runner_id: Option<uuid::Uuid>,
    pinned_runner_id: Option<uuid::Uuid>,
    owner_id: Option<uuid::Uuid>,
    assigned_at: Option<chrono::DateTime<chrono::Utc>>,
    queue_position: Option<i16>,
) -> bool {
    if executor_kind.is_machine_executor() {
        return true;
    }
    executor_kind == AgentExecutorKind::CloudAgent
        && runner_id.is_none()
        && pinned_runner_id.is_none()
        && owner_id.is_none()
        && assigned_at.is_none()
        && queue_position.is_none()
}

/// One active run per work item (`models.py:1079-1101`): `work_item`
/// is unique while set and the status is active. The active set
/// mirrors `is_active` (`models.py:1146-1159`).
pub const ONE_ACTIVE_PER_WORK_ITEM: &str = "agent_run_one_active_per_work_item";
/// Fields of [`ONE_ACTIVE_PER_WORK_ITEM`], Django field names.
pub const ONE_ACTIVE_PER_WORK_ITEM_FIELDS: &[&str] = &["work_item"];
/// Status values in the [`ONE_ACTIVE_PER_WORK_ITEM`] condition
/// (`models.py:1083-1098`), fixture order.
pub const ONE_ACTIVE_CONDITION_STATUSES: &[&str] = &[
    "queued",
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
];

/// Runner + status lookup index (`models.py:1041`).
pub const RUNNER_STATUS_INDEX: &str = "agent_run_runner__1237dd_idx";
/// Fields of [`RUNNER_STATUS_INDEX`], Django field names.
pub const RUNNER_STATUS_INDEX_FIELDS: &[&str] = &["runner", "status"];
/// Owner + status lookup index (`models.py:1042`).
pub const OWNER_STATUS_INDEX: &str = "agent_run_owner_i_75b982_idx";
/// Fields of [`OWNER_STATUS_INDEX`], Django field names.
pub const OWNER_STATUS_INDEX_FIELDS: &[&str] = &["owner", "status"];
/// Workspace + status lookup index (`models.py:1043`).
pub const WORKSPACE_STATUS_INDEX: &str = "agent_run_workspa_987ef0_idx";
/// Fields of [`WORKSPACE_STATUS_INDEX`], Django field names.
pub const WORKSPACE_STATUS_INDEX_FIELDS: &[&str] = &["workspace", "status"];
/// Work-item + status lookup index (`models.py:1044`).
pub const WORK_ITEM_STATUS_INDEX: &str = "agent_run_work_it_d042e7_idx";
/// Fields of [`WORK_ITEM_STATUS_INDEX`], Django field names.
pub const WORK_ITEM_STATUS_INDEX_FIELDS: &[&str] = &["work_item", "status"];
/// Pod + status lookup index (`models.py:1045`).
pub const POD_STATUS_INDEX: &str = "agent_run_pod_status_idx";
/// Fields of [`POD_STATUS_INDEX`], Django field names.
pub const POD_STATUS_INDEX_FIELDS: &[&str] = &["pod", "status"];
/// Creator + status lookup index (`models.py:1046`).
pub const CREATED_BY_STATUS_INDEX: &str = "agent_run_created_status_idx";
/// Fields of [`CREATED_BY_STATUS_INDEX`], Django field names.
pub const CREATED_BY_STATUS_INDEX_FIELDS: &[&str] = &["created_by", "status"];
/// Cloud dispatch scan index (`models.py:1047-1050`).
pub const CLOUD_DISPATCH_INDEX: &str = "agent_run_cloud_dispatch_idx";
/// Fields of [`CLOUD_DISPATCH_INDEX`], Django field names.
pub const CLOUD_DISPATCH_INDEX_FIELDS: &[&str] =
    &["executor_kind", "status", "lease_expires_at", "created_at"];
/// Cloud staleness scan index (`models.py:1051-1054`).
pub const CLOUD_STALE_INDEX: &str = "agent_run_cloud_stale_idx";
/// Fields of [`CLOUD_STALE_INDEX`], Django field names.
pub const CLOUD_STALE_INDEX_FIELDS: &[&str] = &["executor_kind", "status", "started_at"];
/// Terminal-hooks sweep index (`models.py:1055-1058`).
pub const TERM_HOOKS_INDEX: &str = "agent_run_term_hooks_idx";
/// Fields of [`TERM_HOOKS_INDEX`], Django field names.
pub const TERM_HOOKS_INDEX_FIELDS: &[&str] = &["status", "terminal_hooks_applied_at", "ended_at"];
/// Terminal-capacity sweep index (`models.py:1059-1062`).
pub const TERM_CAPACITY_INDEX: &str = "agent_run_term_capacity_idx";
/// Fields of [`TERM_CAPACITY_INDEX`], Django field names.
pub const TERM_CAPACITY_INDEX_FIELDS: &[&str] =
    &["status", "terminal_capacity_released_at", "ended_at"];

/// Back-compat half of `AgentRun.save` (`models.py:1128-1133`):
/// legacy call sites set `owner` but not `created_by`, so `owner`
/// mirrors into `created_by` to hold the `NOT NULL` constraint
/// (under the old model the two were equal).
pub fn resolve_created_by_id(
    created_by_id: Option<uuid::Uuid>,
    owner_id: Option<uuid::Uuid>,
) -> Option<uuid::Uuid> {
    created_by_id.or(owner_id)
}

/// Pod-resolution precedence of `AgentRun.save`
/// (`models.py:1104-1127`): an explicit `pod_id` is kept; else the
/// `work_item.project` default pod (`:1114-1119`); else the
/// single-project workspace default pod (`:1120-1127`); else `None`
/// and the insert fails `NOT NULL` on `pod_id`. Each input is the
/// already-fetched candidate (`None` when its step has nothing to
/// offer); the fetches themselves live on the write path.
pub fn resolve_pod_id(
    explicit_pod_id: Option<uuid::Uuid>,
    work_item_project_default_pod_id: Option<uuid::Uuid>,
    single_project_default_pod_id: Option<uuid::Uuid>,
) -> Option<uuid::Uuid> {
    explicit_pod_id
        .or(work_item_project_default_pod_id)
        .or(single_project_default_pod_id)
}

/// The single-project fallback gate (`models.py:1123-1124`): the
/// workspace's project ids (already sliced to two, `[:2]`) yield a
/// lookup only when exactly one project exists.
pub fn single_project_id(project_ids: &[uuid::Uuid]) -> Option<uuid::Uuid> {
    if project_ids.len() == 1 {
        project_ids.first().copied()
    } else {
        None
    }
}

/// `input_tokens` generated-column definition
/// (`models.py:1024`, `fields.py:56-58`): `GENERATED ALWAYS AS
/// ((usage ->> 'input')::bigint) STORED`. Byte-identical to the
/// recorded `db_type`.
pub const INPUT_TOKENS_DDL: &str =
    "bigint GENERATED ALWAYS AS ((\"usage\" ->> 'input')::bigint) STORED";
/// `output_tokens` generated-column definition (`models.py:1025`).
pub const OUTPUT_TOKENS_DDL: &str =
    "bigint GENERATED ALWAYS AS ((\"usage\" ->> 'output')::bigint) STORED";
/// `total_tokens` generated-column definition (`models.py:1026`).
pub const TOTAL_TOKENS_DDL: &str =
    "bigint GENERATED ALWAYS AS ((\"usage\" ->> 'total')::bigint) STORED";

/// Read expression for the `input_tokens` pseudo-field: the
/// generated column's computation as a query fragment, for
/// builders that aggregate over it.
pub const INPUT_TOKENS_READ_EXPR: &str = "(\"agent_run\".\"usage\" ->> 'input')::bigint";
/// Read expression for the `output_tokens` pseudo-field.
pub const OUTPUT_TOKENS_READ_EXPR: &str = "(\"agent_run\".\"usage\" ->> 'output')::bigint";
/// Read expression for the `total_tokens` pseudo-field.
pub const TOTAL_TOKENS_READ_EXPR: &str = "(\"agent_run\".\"usage\" ->> 'total')::bigint";

/// One agent-run row, declaration order. `dispatch_attempts` is a
/// `PositiveIntegerField` (`models.py:959`), hence `i32` like the
/// D-02 `repository_count` port; `queue_position` is a
/// `PositiveSmallIntegerField` (`models.py:1033`), hence `i16` like
/// the D-22 `visibility` port; the `*_tokens` columns are
/// nullable generated bigints (`fields.py:43` forces
/// `null=True`). `refusal_category` stays a `String`: its default
/// is `""` (`models.py:1013`), which is not a `RefusalCategory`
/// value. Writes are impossible on the token columns by
/// construction (`pre_save` sends `DEFAULT`, `fields.py:60-61`) —
/// inserts must omit them or send `DEFAULT`, and read the computed
/// values back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub owner_id: Option<uuid::Uuid>,
    pub created_by_id: uuid::Uuid,
    pub pod_id: uuid::Uuid,
    pub runner_id: Option<uuid::Uuid>,
    pub pinned_runner_id: Option<uuid::Uuid>,
    pub work_item_id: Option<uuid::Uuid>,
    pub scheduler_binding_id: Option<uuid::Uuid>,
    pub parent_run_id: Option<uuid::Uuid>,
    pub status: AgentRunStatus,
    pub executor_kind: AgentExecutorKind,
    pub dispatch_attempts: i32,
    pub cancel_requested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cancel_reason: String,
    pub error_code: String,
    pub tool_plan: serde_json::Value,
    pub terminal_hooks_applied_at: Option<chrono::DateTime<chrono::Utc>>,
    pub terminal_capacity_released_at: Option<chrono::DateTime<chrono::Utc>>,
    pub prompt: String,
    pub trigger: AgentRunTrigger,
    pub prompt_manifest: Option<serde_json::Value>,
    pub phase_kind: String,
    pub run_config: serde_json::Value,
    pub required_capabilities: serde_json::Value,
    pub thread_id: String,
    pub agent_metadata: serde_json::Value,
    pub lease_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub done_payload: Option<serde_json::Value>,
    pub error: String,
    pub refusal_category: String,
    pub llm_model: String,
    pub usage: serde_json::Value,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub assigned_at: Option<chrono::DateTime<chrono::Utc>>,
    pub queue_position: Option<i16>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_runs::test_support as ts;
    use pidash_types::dispatch::MACHINE_EXECUTORS;

    fn run_fixture() -> serde_json::Value {
        ts::model(&ts::fx02(), "AgentRun").clone()
    }

    #[test]
    fn columns_match_fixture_in_order() {
        assert_eq!(ts::owned(COLUMNS), ts::columns(&run_fixture()));
        assert_eq!(COLUMNS.len(), 41);
    }

    #[test]
    fn meta_matches_fixture() {
        let m = run_fixture();
        assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
        assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
        assert!(m["unique_together"].as_array().expect("ut").is_empty());
    }

    #[test]
    fn cloud_check_matches_fixture_semantics() {
        let m = run_fixture();
        let check = ts::constraint(&m, CLOUD_CHECK);
        assert_eq!(check["type"].as_str(), Some("CheckConstraint"));
        let rendered = check["check"].as_str().expect("check repr");
        for term in [
            "executor_kind__in",
            "CLOUD_AGENT",
            "runner__isnull",
            "pinned_runner__isnull",
            "owner__isnull",
            "assigned_at__isnull",
            "queue_position__isnull",
        ] {
            assert!(rendered.contains(term), "check mentions {term}");
        }
        assert_eq!(
            MACHINE_EXECUTORS,
            [
                AgentExecutorKind::LocalRunner,
                AgentExecutorKind::ManagedRunner
            ]
        );
        let some_id = Some(uuid::Uuid::nil());
        let some_ts = Some(chrono::DateTime::<chrono::Utc>::MIN_UTC);
        // Machine executors satisfy the check whatever else is set.
        for kind in MACHINE_EXECUTORS {
            assert!(
                cloud_assignment_valid(kind, some_id, some_id, some_id, some_ts, Some(3)),
                "{kind:?} with local assignment is valid"
            );
        }
        // Cloud agent with no local assignment is valid.
        assert!(cloud_assignment_valid(
            AgentExecutorKind::CloudAgent,
            None,
            None,
            None,
            None,
            None
        ));
        // Each local-assignment column alone invalidates a cloud run.
        assert!(!cloud_assignment_valid(
            AgentExecutorKind::CloudAgent,
            some_id,
            None,
            None,
            None,
            None
        ));
        assert!(!cloud_assignment_valid(
            AgentExecutorKind::CloudAgent,
            None,
            some_id,
            None,
            None,
            None
        ));
        assert!(!cloud_assignment_valid(
            AgentExecutorKind::CloudAgent,
            None,
            None,
            some_id,
            None,
            None
        ));
        assert!(!cloud_assignment_valid(
            AgentExecutorKind::CloudAgent,
            None,
            None,
            None,
            some_ts,
            None
        ));
        assert!(!cloud_assignment_valid(
            AgentExecutorKind::CloudAgent,
            None,
            None,
            None,
            None,
            Some(0)
        ));
    }

    #[test]
    fn one_active_per_work_item_matches_fixture() {
        let m = run_fixture();
        let uniq = ts::constraint(&m, ONE_ACTIVE_PER_WORK_ITEM);
        assert_eq!(uniq["type"].as_str(), Some("UniqueConstraint"));
        assert_eq!(
            uniq["fields"],
            serde_json::json!(ONE_ACTIVE_PER_WORK_ITEM_FIELDS)
        );
        let condition = uniq["condition"].as_str().expect("condition repr");
        assert!(condition.contains("work_item__isnull', False"));
        for status in ONE_ACTIVE_CONDITION_STATUSES {
            assert!(condition.contains(status), "condition mentions {status}");
            let parsed = AgentRunStatus::from_value(status).expect("L1 parses it");
            assert!(parsed.is_active(), "{status} is active in L1");
        }
        assert_eq!(ONE_ACTIVE_CONDITION_STATUSES.len(), 7);
    }

    #[test]
    fn indexes_match_fixture() {
        let m = run_fixture();
        for (name, fields) in [
            (RUNNER_STATUS_INDEX, RUNNER_STATUS_INDEX_FIELDS),
            (OWNER_STATUS_INDEX, OWNER_STATUS_INDEX_FIELDS),
            (WORKSPACE_STATUS_INDEX, WORKSPACE_STATUS_INDEX_FIELDS),
            (WORK_ITEM_STATUS_INDEX, WORK_ITEM_STATUS_INDEX_FIELDS),
            (POD_STATUS_INDEX, POD_STATUS_INDEX_FIELDS),
            (CREATED_BY_STATUS_INDEX, CREATED_BY_STATUS_INDEX_FIELDS),
            (CLOUD_DISPATCH_INDEX, CLOUD_DISPATCH_INDEX_FIELDS),
            (CLOUD_STALE_INDEX, CLOUD_STALE_INDEX_FIELDS),
            (TERM_HOOKS_INDEX, TERM_HOOKS_INDEX_FIELDS),
            (TERM_CAPACITY_INDEX, TERM_CAPACITY_INDEX_FIELDS),
        ] {
            let index = ts::index(&m, name);
            assert_eq!(index["fields"], serde_json::json!(fields), "{name}");
        }
    }

    #[test]
    fn defaults_types_and_relations_match_fixture() {
        let m = run_fixture();
        assert_eq!(
            ts::field(&m, "id")["default"].as_str(),
            Some("callable:<uuid4>")
        );
        for (field, to, on_delete) in [
            ("workspace", "workspaces", "CASCADE"),
            ("owner", "users", "SET_NULL"),
            ("created_by", "users", "PROTECT"),
            ("pod", "pod", "PROTECT"),
            ("runner", "runner", "SET_NULL"),
            ("pinned_runner", "runner", "SET_NULL"),
            ("work_item", "issues", "SET_NULL"),
            ("scheduler_binding", "scheduler_bindings", "SET_NULL"),
            ("parent_run", "agent_run", "SET_NULL"),
        ] {
            let f = ts::field(&m, field);
            assert_eq!(f["type"].as_str(), Some("ForeignKey"), "{field}");
            assert_eq!(f["db_type"].as_str(), Some("uuid"), "{field}");
            assert_eq!(f["rel"]["to"].as_str(), Some(to), "{field}");
            assert_eq!(f["rel"]["on_delete"].as_str(), Some(on_delete), "{field}");
        }
        assert_eq!(WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(OWNER_ON_DELETE, OnDelete::SetNull);
        assert_eq!(CREATED_BY_ON_DELETE, OnDelete::Protect);
        assert_eq!(POD_ON_DELETE, OnDelete::Protect);
        assert_eq!(RUNNER_ON_DELETE, OnDelete::SetNull);
        assert_eq!(PINNED_RUNNER_ON_DELETE, OnDelete::SetNull);
        assert_eq!(WORK_ITEM_ON_DELETE, OnDelete::SetNull);
        assert_eq!(SCHEDULER_BINDING_ON_DELETE, OnDelete::SetNull);
        assert_eq!(PARENT_RUN_ON_DELETE, OnDelete::SetNull);
        assert_eq!(ts::field(&m, "created_by")["null"].as_bool(), Some(false));
        assert_eq!(ts::field(&m, "pod")["null"].as_bool(), Some(false));
        assert_eq!(ts::field(&m, "owner")["null"].as_bool(), Some(true));
        assert_eq!(
            ts::field(&m, "status")["default"].as_str(),
            Some(DEFAULT_STATUS.value())
        );
        assert_eq!(
            ts::field(&m, "executor_kind")["default"].as_str(),
            Some(DEFAULT_EXECUTOR_KIND.value())
        );
        assert_eq!(
            ts::field(&m, "trigger")["default"].as_str(),
            Some(DEFAULT_TRIGGER.value())
        );
        assert_eq!(
            ts::field(&m, "dispatch_attempts")["default"].as_i64(),
            Some(DEFAULT_DISPATCH_ATTEMPTS.into())
        );
        assert_eq!(
            ts::field(&m, "dispatch_attempts")["db_type"].as_str(),
            Some("integer")
        );
        for field in [
            "cancel_reason",
            "error_code",
            "prompt",
            "phase_kind",
            "thread_id",
            "error",
            "refusal_category",
            "llm_model",
        ] {
            assert_eq!(
                ts::field(&m, field)["default"].as_str(),
                Some(EMPTY_TEXT),
                "{field}"
            );
        }
        for (field, bound) in [
            ("status", STATUS_MAX_LENGTH),
            ("executor_kind", EXECUTOR_KIND_MAX_LENGTH),
            ("cancel_reason", CANCEL_REASON_MAX_LENGTH),
            ("error_code", ERROR_CODE_MAX_LENGTH),
            ("trigger", TRIGGER_MAX_LENGTH),
            ("phase_kind", PHASE_KIND_MAX_LENGTH),
            ("thread_id", THREAD_ID_MAX_LENGTH),
            ("refusal_category", REFUSAL_CATEGORY_MAX_LENGTH),
            ("llm_model", LLM_MODEL_MAX_LENGTH),
        ] {
            assert_eq!(
                ts::field(&m, field)["max_length"].as_u64(),
                Some(bound as u64),
                "{field}"
            );
        }
        for field in ["tool_plan", "run_config", "agent_metadata", "usage"] {
            assert_eq!(
                ts::field(&m, field)["default"].as_str(),
                Some("callable:<dict>"),
                "{field}"
            );
            assert_eq!(
                ts::field(&m, field)["db_type"].as_str(),
                Some("jsonb"),
                "{field}"
            );
        }
        assert_eq!(
            ts::field(&m, "required_capabilities")["default"].as_str(),
            Some("callable:<list>")
        );
        assert!(default_tool_plan().is_object());
        assert!(default_run_config().is_object());
        assert!(default_agent_metadata().is_object());
        assert!(default_usage().is_object());
        assert!(default_required_capabilities().is_array());
        for field in ["prompt_manifest", "done_payload"] {
            assert_eq!(
                ts::field(&m, field)["null"].as_bool(),
                Some(true),
                "{field}"
            );
        }
        assert_eq!(ts::field(&m, "prompt")["db_type"].as_str(), Some("text"));
        assert_eq!(ts::field(&m, "error")["db_type"].as_str(), Some("text"));
        assert_eq!(
            ts::field(&m, "queue_position")["db_type"].as_str(),
            Some("smallint")
        );
        assert_eq!(
            ts::field(&m, "queue_position")["null"].as_bool(),
            Some(true)
        );
        assert_eq!(
            ts::field(&m, "created_at")["auto_now_add"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn save_mirror_and_pod_resolution_match_fixture() {
        let save = &ts::fx02()["agentrun_save"];
        let owner: uuid::Uuid = save["owner_mirror"]["owner_id"]
            .as_str()
            .expect("uuid")
            .parse()
            .expect("parses");
        assert_eq!(
            save["owner_mirror"]["created_by_id"].as_str(),
            save["owner_mirror"]["owner_id"].as_str(),
            "live save mirrored owner into created_by"
        );
        assert_eq!(resolve_created_by_id(None, Some(owner)), Some(owner));
        let creator = uuid::Uuid::max();
        assert_eq!(
            resolve_created_by_id(Some(creator), Some(owner)),
            Some(creator),
            "an explicit created_by wins"
        );
        assert_eq!(resolve_created_by_id(None, None), None);
        let fallback: uuid::Uuid = save["single_project_fallback"]["pod_id"]
            .as_str()
            .expect("uuid")
            .parse()
            .expect("parses");
        assert_eq!(
            save["single_project_fallback"]["pod_id"].as_str(),
            save["single_project_fallback"]["expected_default"].as_str(),
            "live save resolved the single-project default pod"
        );
        assert_eq!(single_project_id(&[fallback]), Some(fallback));
        assert_eq!(single_project_id(&[]), None);
        assert_eq!(single_project_id(&[fallback, creator]), None);
        let explicit = uuid::Uuid::nil();
        assert!(save["explicit_pod_kept"].as_bool().expect("bool"));
        assert_eq!(
            resolve_pod_id(Some(explicit), Some(fallback), Some(creator)),
            Some(explicit)
        );
        assert_eq!(
            resolve_pod_id(None, Some(fallback), Some(creator)),
            Some(fallback)
        );
        assert_eq!(resolve_pod_id(None, None, Some(creator)), Some(creator));
        assert_eq!(resolve_pod_id(None, None, None), None);
        assert_eq!(
            save["no_resolvable_pod"].as_str(),
            Some("IntegrityError: null value in column \"pod_id\" of relation \"agent_run\" violates not-null constraint")
        );
        let order = save["pod_resolution_order"].as_str().expect("rule");
        for step in [
            "explicit pod_id kept",
            "work_item.project",
            "single-project",
        ] {
            assert!(order.contains(step), "rule names {step}");
        }
    }

    #[test]
    fn token_reads_match_fixture() {
        let token = &ts::fx02()["token_read_sql"];
        assert_eq!(
            INPUT_TOKENS_DDL,
            token["generated_db_type_input"].as_str().expect("ddl")
        );
        for (ddl, key) in [(OUTPUT_TOKENS_DDL, "output"), (TOTAL_TOKENS_DDL, "total")] {
            assert!(
                ddl.starts_with("bigint GENERATED ALWAYS AS ((\"usage\" ->> '"),
                "same template"
            );
            assert!(ddl.contains(key), "key {key}");
            assert!(ddl.ends_with("')::bigint) STORED"));
        }
        let m = run_fixture();
        for (field, key) in [
            ("input_tokens", "input"),
            ("output_tokens", "output"),
            ("total_tokens", "total"),
        ] {
            let f = ts::field(&m, field);
            assert_eq!(f["null"].as_bool(), Some(true), "{field}");
            assert_eq!(f["editable"].as_bool(), Some(false), "{field}");
            assert!(
                f["db_type"].as_str().expect("db_type").contains(key),
                "{field} derives from {key}"
            );
        }
        for (expr, key) in [
            (INPUT_TOKENS_READ_EXPR, "input"),
            (OUTPUT_TOKENS_READ_EXPR, "output"),
            (TOTAL_TOKENS_READ_EXPR, "total"),
        ] {
            assert!(expr.contains(&format!("\"agent_run\".\"usage\" ->> '{key}'")));
            assert!(expr.ends_with("::bigint"));
        }
        let values = token["values_input_tokens"].as_str().expect("sql");
        assert!(values.contains("\"agent_run\".\"input_tokens\""));
        assert!(values.contains("ORDER BY \"agent_run\".\"created_at\" DESC"));
        let sum = token["sum_total_tokens"].as_str().expect("sql");
        assert!(sum.contains("SUM(\"agent_run\".\"total_tokens\")"));
        assert!(sum.contains("GROUP BY \"agent_run\".\"workspace_id\""));
    }

    #[test]
    fn run_sets_match_l1_status_sets() {
        use pidash_types::runner_runs::TERMINAL_RUN_STATUSES;
        let sets = &ts::fx02()["run_sets"];
        let mut terminal: Vec<String> = TERMINAL_RUN_STATUSES
            .iter()
            .map(|s| s.value().to_string())
            .collect();
        terminal.sort();
        let mut fixture_terminal: Vec<String> = sets["is_terminal"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        fixture_terminal.sort();
        assert_eq!(terminal, fixture_terminal);
        let mut active: Vec<String> = ONE_ACTIVE_CONDITION_STATUSES
            .iter()
            .map(|s| s.to_string())
            .collect();
        active.sort();
        let mut fixture_active: Vec<String> = sets["is_active"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        fixture_active.sort();
        assert_eq!(active, fixture_active);
    }
}
