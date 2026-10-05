#![forbid(unsafe_code)]

//! Move a work item to another project (`utils/issue_move.py:1-393`).
//!
//! Port of [`move_work_item_to_project`] (`:122-393`) +
//! [`IssueMoveError`] (`:109-120`) and the cancel helper
//! ([`send_project_move_cancel`], `:70-107`). Both entry points (the
//! API-key surface and the web-app surface) delegate here so the two
//! stay byte-for-byte identical; D-18's API move endpoint reuses this
//! module.
//!
//! # Layering: a seam, not direct calls
//!
//! This crate carries no database handle, so the move is generic over
//! the [`MoveStore`] seam (the `PubsubStore` / `CreationSeam`
//! precedent): one method per underlying effect, each naming the SQL
//! text the pool implementation executes verbatim. Guards run before
//! the transaction, the transaction body runs inside it, and the
//! outcome carries everything after it — in Python order:
//!
//! 1. guards (`:136-171`): blank-ref check → source fetch → resolve →
//!    same-project no-op → membership → `current_instance` capture →
//!    `requested_data` → target state;
//! 2. transaction (`:173-373`): issue lock → advisory lock → handoff
//!    lock → sequence → default pod → issue save → handoff transitions
//!    → sequence rotate → assignee prune/repoint → label/cycle/module/
//!    relation deletes → children detach → comment/description id lists
//!    → the 12 project/workspace repoints → immediate-handoff create;
//! 3. post-commit (`MovePostCommit`, `:364-372`): the cancel frame, then
//!    one drain per source pod — the executing layer registers these
//!    on the foundation post-commit wrapper
//!    (`pidash_db::tx::AfterCommit`);
//! 4. enqueues (`:374-392`): `issue_activity` then `model_activity`
//!    (plain `.delay`, after commit — not `on_commit`).
//!
//! # Failure policy
//!
//! [`IssueMoveError`] (400/403/409, exact messages) for the recoverable
//! modes; `DoesNotExist` propagates as [`MoveError::IssueNotFound`] and
//! the resolve miss as [`MoveError::ProjectNotFound`] (distinct 404
//! details, as in Python); anything the seam reports is
//! [`MoveError::Store`] (a 500, as Django's uncaught exception). The
//! cancel send is best-effort with `RunnerOfflineError` discernment
//! ([`send_project_move_cancel`]); the drain actions run isolated per
//! pod, in first-seen order (Python iterates a set — no order is
//! contractual).
//!
//! # Reuse notes (no new edges)
//!
//! * `PROJECT_MOVE_HANDOFF_CONFIG_KEY` (`types::orchestration`),
//!   `advisory_lock_key` + `CREATE_ADVISORY_LOCK_SQL` +
//!   `CREATE_SEQ_SCAN_SQL` + `saved_sequence_id` +
//!   `CREATE_SAVE_DEFAULT_STATE_SQL` + `CREATE_SAVE_FALLBACK_STATE_SQL`
//!   (`dispatch::tools`), `Role::Member` (`pidash_auth::permissions`),
//!   `render_issue` + row/input types (`v1_work_items::shape_issue`),
//!   `relations_summary`/`summary_list`/`BlockerRow`/`summary_sql`/
//!   `has_open_blockers_sql` (`orchestration::blockers`),
//!   `classify_lookup`/`ProjectLookup`/`NOT_FOUND_DETAIL`
//!   (`pidash_db::app_project`), `DEFAULT_FOR_PROJECT_ID_SQL`
//!   (`pidash_db::runner_enroll`), `send_to_runner` + `PubsubStore` +
//!   `OutboxError::RunnerOffline` (`runner_sessions::pubsub`,
//!   `pidash_db::runner_sessions`), `AgentRunStatus`
//!   (`pidash_db::dispatch`), `ISSUE_ACTIVITY_TASK`
//!   (`app_project::tasks`), `MODEL_ACTIVITY_TASK`
//!   (`v1_projects::tasks`) — all reused, never re-ported.
//! * `_create_project_move_handoff_run` runs behind
//!   [`MoveStore::create_handoff_run`]: the pool implementation maps
//!   the already-fetched parent row to a `HandoffCreateRequest` and
//!   calls `orchestration::creation::create_project_move_handoff_run`
//!   (no new read — the row is already locked).
//! * `drain_pod_by_id` runs behind [`MovePostCommit::DrainPod`]: the
//!   executing layer runs the `runner_sessions::drain` recipe
//!   (`DRAIN_POD_BY_ID_LOOKUP_SQL` + idle loop + `DrainEffect`s).
//! * `current_instance` renders through the PIDASHCONV-660 read shape
//!   with no `fields=`/`expand=`, single payload, no relations block
//!   (no context viewer), and the blocker summary appended.
//! * `json.dumps(..., cls=DjangoJSONEncoder)` is [`py_dumps`]: no shared
//!   public helper exists in the services-reachable crates, so the
//!   separators + `ensure_ascii` + float-`repr` algorithm is ported
//!   here from `pidash_db::runner_sessions::machine_outbox` (private
//!   there). All rendered values are JSON-native (datetimes/UUIDs cross
//!   pre-rendered), so the encoder extras never trigger.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `next_sequence` is `max + 1 if max else 1` (the `saved_sequence_id`
//!   rule — a zero max yields 1, not 0).
//! * The no-op same-project return skips every write AND every guard
//!   after the resolve (no membership check, no state check).
//! * `dispatch_immediate` is False exactly when handoff runs exist
//!   (the attribute default is True).
//! * Queryset `.update()` never moves `updated_at` (no `auto_now`);
//!   queryset `.delete()` soft-deletes (`deleted_at` stamp, no tasks).
//! * The `epoch` enqueue stamp reuses the frozen `now` (Python calls
//!   `timezone.now()` a second time — same second in practice,
//!   deterministic here).
//!
//! Fixture: `rust-api/fixtures/app_issues/queries/FX-ISS-13.move.json`
//! (`error_matrix`, `move_db_effects`, `current_instance_shape`). Every
//! section is replayed by the `#[cfg(test)]` suite below.

use std::fmt::Write as _;

/// Re-exported for the pool implementation + the 404 handler: the
/// generic resolve-miss detail (`"Project not found"`).
pub use pidash_db::app_project::models::project::NOT_FOUND_DETAIL;
use pidash_db::app_project::models::project::{classify_lookup, ProjectLookup};
use pidash_db::dispatch::AgentRunStatus;
/// Re-exported for the pool implementation: the
/// `Pod.default_for_project_id` lookup.
pub use pidash_db::runner_enroll::columns::pod::DEFAULT_FOR_PROJECT_ID_SQL;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_types::orchestration::PROJECT_MOVE_HANDOFF_CONFIG_KEY;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::pr_links::TaskEnqueue;
use super::serializers_detail::serialize_drf_datetime;
use crate::app_project::tasks::ISSUE_ACTIVITY_TASK;
use crate::dispatch::tools::{advisory_lock_key, saved_sequence_id};
/// Re-exported for the pool implementation: the advisory lock, the
/// sequence scan, and the default/fallback state lookups the seam
/// methods execute.
pub use crate::dispatch::tools::{
    CREATE_ADVISORY_LOCK_SQL, CREATE_SAVE_DEFAULT_STATE_SQL, CREATE_SAVE_FALLBACK_STATE_SQL,
    CREATE_SEQ_SCAN_SQL,
};
use crate::orchestration::blockers::{summary_list, BlockerRow};
use crate::runner_sessions::pubsub::{send_to_runner, PubsubStore};
use crate::v1_projects::tasks::MODEL_ACTIVITY_TASK;
use crate::v1_work_items::shape_issue::{
    issue_url, render_issue, BlockerSummary, IssueRow, RepresentationInput, SummaryItem,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Recoverable failure of a move (`IssueMoveError`, `:109-120`).
/// `status_code` is the HTTP status the calling view returns; `message`
/// is the `{"error": ...}` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueMoveError {
    pub message: String,
    pub status_code: u16,
}

impl IssueMoveError {
    fn new(message: &str, status_code: u16) -> Self {
        Self {
            message: message.to_owned(),
            status_code,
        }
    }
}

/// `target_ref` blank after strip (`:136-138`).
pub const PROJECT_REQUIRED_MESSAGE: &str = "project is required";

/// No active `role >= MEMBER` membership in the target (`:153-157`).
pub const NO_PERMISSION_MESSAGE: &str =
    "You do not have permission to move work items into the target project";

/// No non-triage state in the target (`:170-171`).
pub const NO_WORKFLOW_STATE_MESSAGE: &str = "Target project does not have a workflow state";

/// Handoff runs exist but the target has no default pod (`:207-208`).
pub const NO_DEFAULT_POD_MESSAGE: &str = "Target project does not have a default runner pod";

/// More than one executing run with a runner (`:264-267`).
pub const MULTIPLE_ACTIVE_RUNS_MESSAGE: &str =
    "Issue has multiple active agent runs; cancel them before moving it";

/// Failure modes of [`move_work_item_to_project`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveError {
    /// A recoverable 400/403/409 with its exact message.
    Issue(IssueMoveError),
    /// The source issue is not visible (`Issue.DoesNotExist` propagates,
    /// `:140`/`:182`) — the handler renders the standard DRF 404 detail
    /// body.
    IssueNotFound,
    /// `Project.resolve` raised (`:141`) — the handler renders
    /// [`NOT_FOUND_DETAIL`] (`"Project not found"`) as the 404 detail.
    ProjectNotFound,
    /// Anything the seam reports (connection, mapping, unexpected
    /// constraint) or a render failure — a 500, as Django's uncaught
    /// exception.
    Store(StoreError),
}

impl From<StoreError> for MoveError {
    fn from(error: StoreError) -> Self {
        MoveError::Store(error)
    }
}

/// A storage/read failure from the [`MoveStore`] seam, or a
/// `current_instance` render failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

// ---------------------------------------------------------------------------
// SQL text (Django shape; the pool implementation executes verbatim)
// ---------------------------------------------------------------------------

/// `issues` columns in `_meta` order (the `SourceIssueRow` projection,
/// shared by the three issue SELECTs).
pub const ISSUE_COLUMNS: &str = "\"issues\".\"id\", \"issues\".\"created_at\", \"issues\".\"updated_at\", \"issues\".\"created_by_id\", \"issues\".\"updated_by_id\", \"issues\".\"deleted_at\", \"issues\".\"project_id\", \"issues\".\"workspace_id\", \"issues\".\"parent_id\", \"issues\".\"state_id\", \"issues\".\"point\", \"issues\".\"estimate_point_id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"description_stripped\", \"issues\".\"description_binary\", \"issues\".\"priority\", \"issues\".\"complexity_score\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"completed_at\", \"issues\".\"archived_at\", \"issues\".\"is_draft\", \"issues\".\"external_source\", \"issues\".\"external_id\", \"issues\".\"type_id\", \"issues\".\"git_work_branch\", \"issues\".\"workpad\", \"issues\".\"created_via\", \"issues\".\"assigned_pod_id\", \"issues\".\"agent_executor\"";

/// `agent_run` columns in `_meta` order (the `HandoffRunRow` projection).
pub const AGENT_RUN_COLUMNS: &str = "\"agent_run\".\"id\", \"agent_run\".\"workspace_id\", \"agent_run\".\"owner_id\", \"agent_run\".\"created_by_id\", \"agent_run\".\"pod_id\", \"agent_run\".\"runner_id\", \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"work_item_id\", \"agent_run\".\"scheduler_binding_id\", \"agent_run\".\"parent_run_id\", \"agent_run\".\"status\", \"agent_run\".\"executor_kind\", \"agent_run\".\"dispatch_attempts\", \"agent_run\".\"cancel_requested_at\", \"agent_run\".\"cancel_reason\", \"agent_run\".\"error_code\", \"agent_run\".\"tool_plan\", \"agent_run\".\"terminal_hooks_applied_at\", \"agent_run\".\"terminal_capacity_released_at\", \"agent_run\".\"prompt\", \"agent_run\".\"trigger\", \"agent_run\".\"prompt_manifest\", \"agent_run\".\"phase_kind\", \"agent_run\".\"run_config\", \"agent_run\".\"required_capabilities\", \"agent_run\".\"thread_id\", \"agent_run\".\"agent_metadata\", \"agent_run\".\"lease_expires_at\", \"agent_run\".\"done_payload\", \"agent_run\".\"error\", \"agent_run\".\"refusal_category\", \"agent_run\".\"llm_model\", \"agent_run\".\"usage\", \"agent_run\".\"input_tokens\", \"agent_run\".\"output_tokens\", \"agent_run\".\"total_tokens\", \"agent_run\".\"created_at\", \"agent_run\".\"assigned_at\", \"agent_run\".\"queue_position\", \"agent_run\".\"started_at\", \"agent_run\".\"ended_at\"";

/// Source-issue fetch (`:140`): `Issue.issue_objects.get(workspace__slug,
/// project_id, pk)`. Full row; `issue_objects` scope (soft-delete +
/// triage/archived/draft exclusions — the `states` conjunct rides a LEFT
/// JOIN, so stateless rows are excluded, verbatim) + the `projects` join
/// for `project__archived_at` + the `workspaces` join for the slug. `$1`
/// slug, `$2` project id, `$3` pk. (Django's `.get()` `LIMIT 21`
/// collapses to 1: the pk lookup returns at most one row.)
pub const SOURCE_ISSUE_SQL: &str = "SELECT \"issues\".\"id\", \"issues\".\"created_at\", \"issues\".\"updated_at\", \"issues\".\"created_by_id\", \"issues\".\"updated_by_id\", \"issues\".\"deleted_at\", \"issues\".\"project_id\", \"issues\".\"workspace_id\", \"issues\".\"parent_id\", \"issues\".\"state_id\", \"issues\".\"point\", \"issues\".\"estimate_point_id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"description_stripped\", \"issues\".\"description_binary\", \"issues\".\"priority\", \"issues\".\"complexity_score\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"completed_at\", \"issues\".\"archived_at\", \"issues\".\"is_draft\", \"issues\".\"external_source\", \"issues\".\"external_id\", \"issues\".\"type_id\", \"issues\".\"git_work_branch\", \"issues\".\"workpad\", \"issues\".\"created_via\", \"issues\".\"assigned_pod_id\", \"issues\".\"agent_executor\" FROM \"issues\" INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"archived_at\" IS NULL AND NOT (\"issues\".\"is_draft\" = TRUE) AND NOT (\"states\".\"group\" = 'triage') AND \"projects\".\"archived_at\" IS NULL AND \"workspaces\".\"slug\" = $1 AND \"issues\".\"project_id\" = $2 AND \"issues\".\"id\" = $3) LIMIT 1";

/// Locked re-fetch (`:182-184`): [`SOURCE_ISSUE_SQL`] with
/// `select_for_update(of=("self",))` — the lock scopes to the base row
/// (a bare `FOR UPDATE` would ask Postgres to lock the nullable side of
/// the `states` outer join and 500).
pub const SOURCE_ISSUE_LOCK_SQL: &str = "SELECT \"issues\".\"id\", \"issues\".\"created_at\", \"issues\".\"updated_at\", \"issues\".\"created_by_id\", \"issues\".\"updated_by_id\", \"issues\".\"deleted_at\", \"issues\".\"project_id\", \"issues\".\"workspace_id\", \"issues\".\"parent_id\", \"issues\".\"state_id\", \"issues\".\"point\", \"issues\".\"estimate_point_id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"description_stripped\", \"issues\".\"description_binary\", \"issues\".\"priority\", \"issues\".\"complexity_score\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"completed_at\", \"issues\".\"archived_at\", \"issues\".\"is_draft\", \"issues\".\"external_source\", \"issues\".\"external_id\", \"issues\".\"type_id\", \"issues\".\"git_work_branch\", \"issues\".\"workpad\", \"issues\".\"created_via\", \"issues\".\"assigned_pod_id\", \"issues\".\"agent_executor\" FROM \"issues\" INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"archived_at\" IS NULL AND NOT (\"issues\".\"is_draft\" = TRUE) AND NOT (\"states\".\"group\" = 'triage') AND \"projects\".\"archived_at\" IS NULL AND \"workspaces\".\"slug\" = $1 AND \"issues\".\"project_id\" = $2 AND \"issues\".\"id\" = $3) LIMIT 1 FOR UPDATE OF \"issues\"";

/// `Project.resolve` pk arm (`project.py:197-199`): minimal projection
/// (`id`, `workspace_id`, `identifier` — the only columns the move
/// reads); `workspace__slug` join; explicit `deleted_at` guard;
/// `-created_at` ordering. `$1` pk, `$2` slug.
pub const PROJECT_RESOLVE_BY_PK_SQL: &str = "SELECT \"projects\".\"id\", \"projects\".\"workspace_id\", \"projects\".\"identifier\" FROM \"projects\" INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2) ORDER BY \"projects\".\"created_at\" DESC LIMIT 1";

/// `Project.resolve` identifier arm (`project.py:206-211`): exact
/// equality on the stripped, upper-cased input (never `iexact`, so the
/// composite btree is used). `$1` identifier, `$2` slug.
pub const PROJECT_RESOLVE_BY_IDENTIFIER_SQL: &str = "SELECT \"projects\".\"id\", \"projects\".\"workspace_id\", \"projects\".\"identifier\" FROM \"projects\" INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"projects\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $2 AND \"projects\".\"identifier\" = $1) ORDER BY \"projects\".\"created_at\" DESC LIMIT 1";

/// Target-membership probe (`:146-152`): active `role >= MEMBER`
/// membership. `$1` slug, `$2` target project id, `$3` actor id.
pub const MEMBER_EXISTS_SQL: &str = "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $1 AND \"project_members\".\"project_id\" = $2 AND \"project_members\".\"member_id\" = $3 AND \"project_members\".\"role\" >= 15 AND \"project_members\".\"is_active\") LIMIT 1";

/// Lazy `issue.workspace.slug` for `get_url` (`issue.py:429-434`):
/// plain pk fetch under the default soft-delete scope. `$1` workspace id.
pub const WORKSPACE_SLUG_SQL: &str =
    "SELECT \"workspaces\".\"slug\" FROM \"workspaces\" WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"id\" = $1) LIMIT 1";

/// Lazy `issue.project.identifier` for `get_url`. `$1` project id.
pub const PROJECT_IDENTIFIER_SQL: &str =
    "SELECT \"projects\".\"identifier\" FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1) LIMIT 1";

/// `assignees` id list (`issue.py:452-456`): `values_list("assignee_id")`
/// in queryset (`-created_at`) order. `$1` issue id.
pub const ASSIGNEE_IDS_SQL: &str = "SELECT \"issue_assignees\".\"assignee_id\" FROM \"issue_assignees\" WHERE (\"issue_assignees\".\"deleted_at\" IS NULL AND \"issue_assignees\".\"issue_id\" = $1) ORDER BY \"issue_assignees\".\"created_at\" DESC";

/// `labels` id list (`issue.py:466-468`). `$1` issue id.
pub const LABEL_IDS_SQL: &str = "SELECT \"issue_labels\".\"label_id\" FROM \"issue_labels\" WHERE (\"issue_labels\".\"deleted_at\" IS NULL AND \"issue_labels\".\"issue_id\" = $1) ORDER BY \"issue_labels\".\"created_at\" DESC";

/// Outstanding-work lock (`:192-199`): full rows, newest first, `FOR
/// UPDATE` (no `SKIP LOCKED` — the move waits). `AgentRun` extends plain
/// `models.Model`: no soft-delete scope. `$1` issue id.
pub const HANDOFF_RUNS_LOCK_SQL: &str = "SELECT \"agent_run\".\"id\", \"agent_run\".\"workspace_id\", \"agent_run\".\"owner_id\", \"agent_run\".\"created_by_id\", \"agent_run\".\"pod_id\", \"agent_run\".\"runner_id\", \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"work_item_id\", \"agent_run\".\"scheduler_binding_id\", \"agent_run\".\"parent_run_id\", \"agent_run\".\"status\", \"agent_run\".\"executor_kind\", \"agent_run\".\"dispatch_attempts\", \"agent_run\".\"cancel_requested_at\", \"agent_run\".\"cancel_reason\", \"agent_run\".\"error_code\", \"agent_run\".\"tool_plan\", \"agent_run\".\"terminal_hooks_applied_at\", \"agent_run\".\"terminal_capacity_released_at\", \"agent_run\".\"prompt\", \"agent_run\".\"trigger\", \"agent_run\".\"prompt_manifest\", \"agent_run\".\"phase_kind\", \"agent_run\".\"run_config\", \"agent_run\".\"required_capabilities\", \"agent_run\".\"thread_id\", \"agent_run\".\"agent_metadata\", \"agent_run\".\"lease_expires_at\", \"agent_run\".\"done_payload\", \"agent_run\".\"error\", \"agent_run\".\"refusal_category\", \"agent_run\".\"llm_model\", \"agent_run\".\"usage\", \"agent_run\".\"input_tokens\", \"agent_run\".\"output_tokens\", \"agent_run\".\"total_tokens\", \"agent_run\".\"created_at\", \"agent_run\".\"assigned_at\", \"agent_run\".\"queue_position\", \"agent_run\".\"started_at\", \"agent_run\".\"ended_at\" FROM \"agent_run\" WHERE (\"agent_run\".\"work_item_id\" = $1 AND \"agent_run\".\"status\" IN ('queued', 'assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth', 'paused_awaiting_input')) ORDER BY \"agent_run\".\"created_at\" DESC FOR UPDATE";

/// Issue save (`:223-235`): exactly the `update_fields`, in list order.
/// `$1` project, `$2` workspace, `$3` sequence, `$4` state, `$5`
/// assigned pod (NULL when no handoff and no default), `$6`-`$8` NULL
/// parent/estimate/type, `$9` updated stamp, `$10` id.
pub const ISSUE_MOVE_UPDATE_SQL: &str = "UPDATE \"issues\" SET \"project_id\" = $1, \"workspace_id\" = $2, \"sequence_id\" = $3, \"state_id\" = $4, \"assigned_pod_id\" = $5, \"parent_id\" = $6, \"estimate_point_id\" = $7, \"type_id\" = $8, \"updated_at\" = $9 WHERE \"issues\".\"id\" = $10";

/// Inert-run close (`:282-284`): `$1` run id, `$2` end stamp.
pub const INERT_RUN_UPDATE_SQL: &str = "UPDATE \"agent_run\" SET \"status\" = 'cancelled', \"ended_at\" = $2, \"queue_position\" = NULL WHERE \"agent_run\".\"id\" = $1";

/// Executing-parent barrier (`:302`): `$1` run id, `$2` run_config with
/// the handoff marker merged.
pub const HANDOFF_PARENT_UPDATE_SQL: &str = "UPDATE \"agent_run\" SET \"status\" = 'cancel_requested', \"run_config\" = $2, \"queue_position\" = NULL WHERE \"agent_run\".\"id\" = $1";

/// Old sequence rows detached (`:310`): `issue=NULL` where the project
/// is not the target. `$1` issue id, `$2` target project id.
pub const SEQUENCE_DETACH_SQL: &str = "UPDATE \"issue_sequences\" SET \"issue_id\" = NULL WHERE (\"issue_sequences\".\"deleted_at\" IS NULL AND \"issue_sequences\".\"issue_id\" = $1 AND NOT (\"issue_sequences\".\"project_id\" = $2))";

/// Target sequence row (`:311`): full create; `workspace_id` resolves
/// from the already-fetched target project row (Python pays a lazy
/// `project.workspace` fetch at `save()` — same value, no extra query
/// here). `$1` id, `$2`/`$3` created/updated, `$4` created_by (the move
/// actor), `$5` issue, `$6` sequence, `$7` project, `$8` workspace.
pub const SEQUENCE_CREATE_SQL: &str = "INSERT INTO \"issue_sequences\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \"sequence\", \"deleted\") VALUES ($1, $2, $3, $4, NULL, NULL, $7, $8, $5, $6, FALSE)";

/// Assignee prune (`:313-319`, soft): drop rows whose assignee is not an
/// active `role >= MEMBER` member of the TARGET (nested subquery,
/// verbatim). `$1` deleted stamp, `$2` issue, `$3` target project.
pub const ASSIGNEE_PRUNE_SQL: &str = "UPDATE \"issue_assignees\" SET \"deleted_at\" = $1 WHERE (\"issue_assignees\".\"deleted_at\" IS NULL AND \"issue_assignees\".\"issue_id\" = $2 AND NOT (\"issue_assignees\".\"assignee_id\" IN (SELECT \"project_members\".\"member_id\" FROM \"project_members\" WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"project_id\" = $3 AND \"project_members\".\"member_id\" IN (SELECT \"issue_assignees\".\"assignee_id\" FROM \"issue_assignees\" WHERE (\"issue_assignees\".\"deleted_at\" IS NULL AND \"issue_assignees\".\"issue_id\" = $2)) AND \"project_members\".\"role\" >= 15 AND \"project_members\".\"is_active\"))))";

/// Surviving assignees re-pointed (`:320-323`). `$1` target project,
/// `$2` target workspace, `$3` issue.
pub const ASSIGNEE_REPOINT_SQL: &str = "UPDATE \"issue_assignees\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_assignees\".\"deleted_at\" IS NULL AND \"issue_assignees\".\"issue_id\" = $3)";

/// Label wipe (`:325`, soft). `$1` deleted stamp, `$2` issue.
pub const LABEL_DELETE_SQL: &str = "UPDATE \"issue_labels\" SET \"deleted_at\" = $1 WHERE (\"issue_labels\".\"deleted_at\" IS NULL AND \"issue_labels\".\"issue_id\" = $2)";

/// Cycle wipe (`:326`, soft). `$1` deleted stamp, `$2` issue.
pub const CYCLE_DELETE_SQL: &str = "UPDATE \"cycle_issues\" SET \"deleted_at\" = $1 WHERE (\"cycle_issues\".\"deleted_at\" IS NULL AND \"cycle_issues\".\"issue_id\" = $2)";

/// Module wipe (`:327`, soft). `$1` deleted stamp, `$2` issue.
pub const MODULE_DELETE_SQL: &str = "UPDATE \"module_issues\" SET \"deleted_at\" = $1 WHERE (\"module_issues\".\"deleted_at\" IS NULL AND \"module_issues\".\"issue_id\" = $2)";

/// Relation wipe (`:328`, soft): both directions. `$1` deleted stamp, `$2` issue.
pub const RELATION_DELETE_SQL: &str = "UPDATE \"issue_relations\" SET \"deleted_at\" = $1 WHERE (\"issue_relations\".\"deleted_at\" IS NULL AND (\"issue_relations\".\"issue_id\" = $2 OR \"issue_relations\".\"related_issue_id\" = $2))";

/// Children of other projects detached (`:329`): `Issue.objects`
/// (plain soft-delete scope — triage/draft children detach too). `$1`
/// issue, `$2` target project.
pub const CHILDREN_DETACH_SQL: &str = "UPDATE \"issues\" SET \"parent_id\" = NULL WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"parent_id\" = $1 AND NOT (\"issues\".\"project_id\" = $2))";

/// Moved comment ids (`:331`): `-created_at` order (list order feeds the
/// `IN` lists below; order is unobservable there). `$1` issue.
pub const COMMENT_IDS_SQL: &str = "SELECT \"issue_comments\".\"id\" FROM \"issue_comments\" WHERE (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"issue_id\" = $1) ORDER BY \"issue_comments\".\"created_at\" DESC";

/// Moved comment description ids (`:332-334`). `$1` issue.
pub const DESCRIPTION_IDS_SQL: &str = "SELECT \"issue_comments\".\"description_id\" FROM \"issue_comments\" WHERE (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"issue_id\" = $1 AND \"issue_comments\".\"description_id\" IS NOT NULL) ORDER BY \"issue_comments\".\"created_at\" DESC";

/// Final refetch (`:393`): `issue_objects` scope + `select_related`
/// joins; minimal projection (full issue row + the url parts the
/// handler re-renders). `$1` pk.
pub const MOVED_ISSUE_REFETCH_SQL: &str = "SELECT \"issues\".\"id\", \"issues\".\"created_at\", \"issues\".\"updated_at\", \"issues\".\"created_by_id\", \"issues\".\"updated_by_id\", \"issues\".\"deleted_at\", \"issues\".\"project_id\", \"issues\".\"workspace_id\", \"issues\".\"parent_id\", \"issues\".\"state_id\", \"issues\".\"point\", \"issues\".\"estimate_point_id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"description_stripped\", \"issues\".\"description_binary\", \"issues\".\"priority\", \"issues\".\"complexity_score\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"completed_at\", \"issues\".\"archived_at\", \"issues\".\"is_draft\", \"issues\".\"external_source\", \"issues\".\"external_id\", \"issues\".\"type_id\", \"issues\".\"git_work_branch\", \"issues\".\"workpad\", \"issues\".\"created_via\", \"issues\".\"assigned_pod_id\", \"issues\".\"agent_executor\", \"workspaces\".\"slug\", \"projects\".\"identifier\" FROM \"issues\" INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"archived_at\" IS NULL AND NOT (\"issues\".\"is_draft\" = TRUE) AND NOT (\"states\".\"group\" = 'triage') AND \"projects\".\"archived_at\" IS NULL AND \"issues\".\"id\" = $1) LIMIT 1";

// ---------------------------------------------------------------------------
// Project/workspace repoints (`:336-351`)
// ---------------------------------------------------------------------------

/// The 12 related tables re-pointed to the target project/workspace
/// (`update_kwargs`, `:336-351`), in Python call order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepointTarget {
    IssueLink,
    IssueMention,
    IssueSubscriber,
    IssueReaction,
    IssueVote,
    IssueVersion,
    IssueDescriptionVersion,
    IssueActivity,
    IssueComment,
    CommentReaction,
    Description,
    FileAsset,
}

/// Python call order (`:340-351`).
pub const REPOINT_ORDER: &[RepointTarget] = &[
    RepointTarget::IssueLink,
    RepointTarget::IssueMention,
    RepointTarget::IssueSubscriber,
    RepointTarget::IssueReaction,
    RepointTarget::IssueVote,
    RepointTarget::IssueVersion,
    RepointTarget::IssueDescriptionVersion,
    RepointTarget::IssueActivity,
    RepointTarget::IssueComment,
    RepointTarget::CommentReaction,
    RepointTarget::Description,
    RepointTarget::FileAsset,
];

/// The UPDATE for one [`RepointTarget`]. `$1` target project, `$2`
/// target workspace, `$3` issue id, `$4` the id list (`comment_ids` or
/// `description_ids`, bound as an array — an empty list matches nothing,
/// as Django's empty `__in` does). Every statement carries its default
/// soft-delete scope (queryset `.update()` applies the manager filter);
/// queryset updates never move `updated_at` (no `auto_now`).
pub fn repoint_sql(target: RepointTarget) -> &'static str {
    match target {
        RepointTarget::IssueLink => "UPDATE \"issue_links\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_links\".\"deleted_at\" IS NULL AND \"issue_links\".\"issue_id\" = $3)",
        RepointTarget::IssueMention => "UPDATE \"issue_mentions\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_mentions\".\"deleted_at\" IS NULL AND \"issue_mentions\".\"issue_id\" = $3)",
        RepointTarget::IssueSubscriber => "UPDATE \"issue_subscribers\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_subscribers\".\"deleted_at\" IS NULL AND \"issue_subscribers\".\"issue_id\" = $3)",
        RepointTarget::IssueReaction => "UPDATE \"issue_reactions\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_reactions\".\"deleted_at\" IS NULL AND \"issue_reactions\".\"issue_id\" = $3)",
        RepointTarget::IssueVote => "UPDATE \"issue_votes\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_votes\".\"deleted_at\" IS NULL AND \"issue_votes\".\"issue_id\" = $3)",
        RepointTarget::IssueVersion => "UPDATE \"issue_versions\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_versions\".\"deleted_at\" IS NULL AND \"issue_versions\".\"issue_id\" = $3)",
        RepointTarget::IssueDescriptionVersion => "UPDATE \"issue_description_versions\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_description_versions\".\"deleted_at\" IS NULL AND \"issue_description_versions\".\"issue_id\" = $3)",
        RepointTarget::IssueActivity => "UPDATE \"issue_activities\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_activities\".\"deleted_at\" IS NULL AND (\"issue_activities\".\"issue_id\" = $3 OR \"issue_activities\".\"issue_comment_id\" = ANY($4)))",
        RepointTarget::IssueComment => "UPDATE \"issue_comments\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"issue_id\" = $3)",
        RepointTarget::CommentReaction => "UPDATE \"comment_reactions\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"comment_reactions\".\"deleted_at\" IS NULL AND \"comment_reactions\".\"comment_id\" = ANY($4))",
        RepointTarget::Description => "UPDATE \"descriptions\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"descriptions\".\"deleted_at\" IS NULL AND \"descriptions\".\"id\" = ANY($4))",
        RepointTarget::FileAsset => "UPDATE \"file_assets\" SET \"project_id\" = $1, \"workspace_id\" = $2 WHERE (\"file_assets\".\"deleted_at\" IS NULL AND (\"file_assets\".\"issue_id\" = $3 OR \"file_assets\".\"comment_id\" = ANY($4)))",
    }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `issues` row as the move consumes it: identity for the guards
/// plus every column the `current_instance` render reads. Datetimes
/// cross raw (`DateTime<Utc>`); the render formats them via
/// `serializers_detail::serialize_drf_datetime` (API shape is always
/// UTC — `TIME_ZONE = "UTC"`, no per-request conversion).
#[derive(Debug, Clone, PartialEq)]
pub struct SourceIssueRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub state_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub estimate_point_id: Option<Uuid>,
    pub assigned_pod_id: Option<Uuid>,
    pub type_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub point: Option<i64>,
    pub name: String,
    pub description_html: String,
    pub description_binary: Option<Vec<u8>>,
    pub priority: String,
    pub complexity_score: i64,
    pub start_date: Option<chrono::NaiveDate>,
    pub target_date: Option<chrono::NaiveDate>,
    pub sequence_id: i64,
    pub sort_order: f64,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub git_work_branch: String,
    pub created_via: Option<String>,
    pub agent_executor: Option<String>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
}

/// One resolved target project: the only columns the move reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub identifier: String,
}

/// One target workflow state: only the id is consumed (the save writes
/// `state_id`; the `None` check is the 400 arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRow {
    pub id: Uuid,
}

/// One target default pod: only the id is consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PodRow {
    pub id: Uuid,
}

/// One locked handoff run: the columns the transition logic reads
/// (`status`/`runner_id`/`pod_id` for the partition,
/// `id`/`created_by_id`/`run_config`/`trigger` for the parent barrier
/// and the pool-side `HandoffCreateRequest` mapping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffRunRow {
    pub id: Uuid,
    pub status: AgentRunStatus,
    pub runner_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
    pub created_by_id: Uuid,
    pub run_config: Value,
    pub trigger: AgentRunTrigger,
}

/// Re-exported for the [`HandoffRunRow::trigger`] type.
pub use pidash_db::dispatch::AgentRunTrigger;

/// The moved-issue save image (`:210-235`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedIssueFields {
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub sequence_id: i64,
    pub state_id: Uuid,
    pub assigned_pod_id: Option<Uuid>,
}

/// The refreshed issue (`:393`): the post-move row plus the url parts
/// the handler re-renders for the 200 response.
#[derive(Debug, Clone, PartialEq)]
pub struct MovedIssueRow {
    pub row: SourceIssueRow,
    pub workspace_slug: String,
    pub project_identifier: String,
}

// ---------------------------------------------------------------------------
// Seam
// ---------------------------------------------------------------------------

/// Storage seam for the move.
///
/// Methods mirror the ORM calls in `issue_move.py`, one per effect, in
/// Python call order; the pool implementation executes the named SQL
/// text verbatim inside one transaction for the `:173-373` span (the
/// guards and the final refetch run outside it, as in Python). Uuids
/// arrive typed; `now` is the frozen clock (`timezone.now()`).
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam (the `GitStore`
/// precedent).
#[allow(async_fn_in_trait)]
pub trait MoveStore {
    /// Source fetch ([`SOURCE_ISSUE_SQL`], `:140`).
    async fn source_issue(
        &self,
        slug: &str,
        project_id: Uuid,
        pk: Uuid,
    ) -> Result<Option<SourceIssueRow>, StoreError>;

    /// `Project.resolve` (`:141`): the pool picks
    /// [`PROJECT_RESOLVE_BY_PK_SQL`] / [`PROJECT_RESOLVE_BY_IDENTIFIER_SQL`]
    /// by the lookup (classified here via `classify_lookup`).
    async fn resolve_project(
        &self,
        slug: &str,
        lookup: &ProjectLookup,
    ) -> Result<Option<ProjectRow>, StoreError>;

    /// Target-membership probe ([`MEMBER_EXISTS_SQL`], `:146-152`).
    async fn can_move_into_target(
        &self,
        slug: &str,
        target_project_id: Uuid,
        actor_id: Uuid,
    ) -> Result<bool, StoreError>;

    /// Lazy `issue.workspace.slug` for `get_url` ([`WORKSPACE_SLUG_SQL`]).
    async fn representation_workspace_slug(
        &self,
        workspace_id: Uuid,
    ) -> Result<Option<String>, StoreError>;

    /// Lazy `issue.project.identifier` for `get_url`
    /// ([`PROJECT_IDENTIFIER_SQL`]).
    async fn representation_project_identifier(
        &self,
        project_id: Uuid,
    ) -> Result<Option<String>, StoreError>;

    /// `assignees` id list ([`ASSIGNEE_IDS_SQL`]).
    async fn representation_assignee_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError>;

    /// `labels` id list ([`LABEL_IDS_SQL`]).
    async fn representation_label_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError>;

    /// `blocked_by` rows (`blockers::summary_sql(false)`).
    async fn representation_blocked_by(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<BlockerRow>, StoreError>;

    /// `blocking` rows (`blockers::summary_sql(true)`).
    async fn representation_blocking(&self, issue_id: Uuid) -> Result<Vec<BlockerRow>, StoreError>;

    /// Uncapped open-blocker flag (`blockers::has_open_blockers_sql`).
    async fn representation_has_open_blockers(&self, issue_id: Uuid) -> Result<bool, StoreError>;

    /// Default non-triage state (`dispatch::tools::CREATE_SAVE_DEFAULT_STATE_SQL`).
    async fn target_default_state(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<StateRow>, StoreError>;

    /// Fallback non-triage state (`dispatch::tools::CREATE_SAVE_FALLBACK_STATE_SQL`).
    async fn target_fallback_state(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<StateRow>, StoreError>;

    /// Locked re-fetch ([`SOURCE_ISSUE_LOCK_SQL`], `:182-184`).
    async fn lock_source_issue(
        &self,
        slug: &str,
        project_id: Uuid,
        pk: Uuid,
    ) -> Result<Option<SourceIssueRow>, StoreError>;

    /// Per-project creation lock (`dispatch::tools::CREATE_ADVISORY_LOCK_SQL`,
    /// `:185-187`): `$1` is `advisory_lock_key(target_project_id)`,
    /// computed here.
    async fn advisory_lock_project(&self, lock_key: i64) -> Result<(), StoreError>;

    /// Outstanding-work lock ([`HANDOFF_RUNS_LOCK_SQL`], `:192-199`).
    async fn lock_handoff_runs(&self, issue_id: Uuid) -> Result<Vec<HandoffRunRow>, StoreError>;

    /// Target sequence max (`dispatch::tools::CREATE_SEQ_SCAN_SQL`,
    /// `:201-204`); the `+1` rule (`saved_sequence_id`) applies here.
    async fn max_target_sequence(&self, target_project_id: Uuid)
        -> Result<Option<i64>, StoreError>;

    /// Target default pod (`runner_enroll::pod::DEFAULT_FOR_PROJECT_ID_SQL`,
    /// `:206`).
    async fn target_default_pod(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<PodRow>, StoreError>;

    /// Issue save ([`ISSUE_MOVE_UPDATE_SQL`], `:223-235`).
    async fn save_moved_issue(
        &self,
        issue_id: Uuid,
        fields: &MovedIssueFields,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError>;

    /// One inert-run close ([`INERT_RUN_UPDATE_SQL`], `:278-286`), in
    /// list order.
    async fn cancel_inert_run(
        &self,
        run_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError>;

    /// Executing-parent barrier ([`HANDOFF_PARENT_UPDATE_SQL`], `:288-306`):
    /// `run_config` arrives with the handoff marker merged here.
    async fn mark_handoff_parent(&self, run_id: Uuid, run_config: &Value)
        -> Result<(), StoreError>;

    /// Old sequence rows detached ([`SEQUENCE_DETACH_SQL`], `:310`).
    async fn detach_old_sequences(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
    ) -> Result<u64, StoreError>;

    /// Target sequence row ([`SEQUENCE_CREATE_SQL`], `:311`).
    #[allow(clippy::too_many_arguments)]
    async fn create_target_sequence(
        &self,
        sequence_id: Uuid,
        issue_id: Uuid,
        sequence: i64,
        target_project_id: Uuid,
        target_workspace_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Uuid,
    ) -> Result<(), StoreError>;

    /// Assignee prune ([`ASSIGNEE_PRUNE_SQL`], `:313-319`).
    async fn prune_assignees(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StoreError>;

    /// Surviving assignees re-pointed ([`ASSIGNEE_REPOINT_SQL`], `:320-323`).
    async fn repoint_assignees(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
        target_workspace_id: Uuid,
    ) -> Result<u64, StoreError>;

    /// Label wipe ([`LABEL_DELETE_SQL`], `:325`).
    async fn soft_delete_issue_labels(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StoreError>;

    /// Cycle wipe ([`CYCLE_DELETE_SQL`], `:326`).
    async fn soft_delete_cycle_issues(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StoreError>;

    /// Module wipe ([`MODULE_DELETE_SQL`], `:327`).
    async fn soft_delete_module_issues(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StoreError>;

    /// Relation wipe ([`RELATION_DELETE_SQL`], `:328`).
    async fn soft_delete_issue_relations(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StoreError>;

    /// Children of other projects detached ([`CHILDREN_DETACH_SQL`], `:329`).
    async fn detach_children(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
    ) -> Result<u64, StoreError>;

    /// Moved comment ids ([`COMMENT_IDS_SQL`], `:331`).
    async fn moved_comment_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError>;

    /// Moved comment description ids ([`DESCRIPTION_IDS_SQL`], `:332-334`).
    async fn moved_description_ids(&self, issue_id: Uuid) -> Result<Vec<Uuid>, StoreError>;

    /// One project/workspace repoint ([`repoint_sql`], `:340-351`): called
    /// once per [`REPOINT_ORDER`] entry, in order. `comment_ids` /
    /// `description_ids` feed the `$4` lists of the four scoped
    /// statements; issue-scoped statements ignore them.
    async fn repoint_related(
        &self,
        target: RepointTarget,
        issue_id: Uuid,
        comment_ids: &[Uuid],
        description_ids: &[Uuid],
        target_project_id: Uuid,
        target_workspace_id: Uuid,
    ) -> Result<u64, StoreError>;

    /// Immediate-handoff create (`:353-362`): the pool implementation maps
    /// the already-locked `parent_run` to a `HandoffCreateRequest`
    /// (`id`/`created_by_id`/`run_config`/`trigger` — no new read) and
    /// calls `orchestration::creation::create_project_move_handoff_run`.
    async fn create_handoff_run(
        &self,
        issue_id: Uuid,
        parent_run: &HandoffRunRow,
        pod_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError>;

    /// Final refetch ([`MOVED_ISSUE_REFETCH_SQL`], `:393`).
    async fn moved_issue(&self, pk: Uuid) -> Result<Option<MovedIssueRow>, StoreError>;
}

// ---------------------------------------------------------------------------
// Pure helpers: status sets, coercions, frames
// ---------------------------------------------------------------------------

/// Handoff-eligible statuses (`_PROJECT_MOVE_HANDOFF_STATUSES`, `:53-62`),
/// in source order.
pub const PROJECT_MOVE_HANDOFF_STATUSES: &[AgentRunStatus] = &[
    AgentRunStatus::Queued,
    AgentRunStatus::Assigned,
    AgentRunStatus::WaitingForWorktree,
    AgentRunStatus::Running,
    AgentRunStatus::CancelRequested,
    AgentRunStatus::AwaitingApproval,
    AgentRunStatus::AwaitingReauth,
    AgentRunStatus::PausedAwaitingInput,
];

/// Close-and-recreate statuses (`_IMMEDIATE_HANDOFF_STATUSES`, `:64-67`).
pub const IMMEDIATE_HANDOFF_STATUSES: &[AgentRunStatus] =
    &[AgentRunStatus::Queued, AgentRunStatus::PausedAwaitingInput];

/// Whether a handoff row executes on a runner and must enter the
/// cancel barrier (`:254-259`): outside the immediate set with a runner.
pub fn is_executing_run(run: &HandoffRunRow) -> bool {
    !IMMEDIATE_HANDOFF_STATUSES.contains(&run.status) && run.runner_id.is_some()
}

/// `str(target_ref or "")` (`:136`): the views pass the raw JSON value.
/// Falsy values (`null`, `""`, `0`, `0.0`, `false`, `[]`, `{}`) coerce to
/// `""` (the `or ""`); surviving scalars render via Python `str()`
/// (`True`/`False`, integer digits, float `repr`); surviving arrays /
/// objects render compact-JSON — either spelling 404s downstream (every
/// `repr` punctuation they contain is identifier-forbidden), so the
/// rendering is unobservable. The caller trims (`.strip()`).
pub fn target_ref_text(raw: &Value) -> String {
    match raw {
        Value::Null => String::new(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => String::new(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i == 0 {
                    String::new()
                } else {
                    i.to_string()
                }
            } else if let Some(u) = n.as_u64() {
                if u == 0 {
                    String::new()
                } else {
                    u.to_string()
                }
            } else if let Some(f) = n.as_f64() {
                if f == 0.0 {
                    String::new()
                } else {
                    dumps_float(f)
                }
            } else {
                String::new()
            }
        }
        Value::String(s) => s.clone(),
        Value::Array(items) if items.is_empty() => String::new(),
        Value::Object(map) if map.is_empty() => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// `dict(run_config or {})` + the handoff marker (`:293-298`). A missing
/// or non-object `run_config` starts from `{}` (invalid data degrades
/// gracefully — Python would crash spreading it).
pub fn handoff_run_config(
    existing: &Value,
    source_project_id: &Uuid,
    target_project_id: &Uuid,
    target_pod_id: &Uuid,
) -> Value {
    let mut map = match existing {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    map.insert(
        PROJECT_MOVE_HANDOFF_CONFIG_KEY.to_owned(),
        serde_json::json!({
            "source_project_id": source_project_id.to_string(),
            "target_project_id": target_project_id.to_string(),
            "target_pod_id": target_pod_id.to_string(),
        }),
    );
    Value::Object(map)
}

/// The cancel frame (`:83-89`), in key order (`v`, `type`, `run_id`,
/// `reason`). The outbox envelopes it (adds `mid`) at send time.
pub fn cancel_frame(run_id: &Uuid) -> Map<String, Value> {
    let mut frame = Map::with_capacity(4);
    frame.insert("v".to_owned(), Value::Number(1.into()));
    frame.insert("type".to_owned(), Value::String("cancel".to_owned()));
    frame.insert("run_id".to_owned(), Value::String(run_id.to_string()));
    frame.insert(
        "reason".to_owned(),
        Value::String("issue_moved_projects".to_owned()),
    );
    frame
}

/// The persisted-cancel log line (`:92-97`): emitted when the runner is
/// offline — the `CANCEL_REQUESTED` row makes session-open redeliver.
pub fn offline_cancel_line(runner_id: &Uuid, run_id: &Uuid) -> String {
    format!(
        "issue_move: runner {runner_id} offline; cancellation for run {run_id} will be redelivered at session open"
    )
}

/// The delivery-failure log line (`:103-106`): emitted for any other
/// send failure, which never 500s the move.
pub fn failed_cancel_line(run_id: &Uuid, error: &dyn std::fmt::Display) -> String {
    format!("issue_move: failed to deliver cancellation for run {run_id}: {error}")
}

// ---------------------------------------------------------------------------
// `json.dumps` port (separators + `ensure_ascii` + float `repr`)
// ---------------------------------------------------------------------------

/// `json.dumps(value, cls=DjangoJSONEncoder)` for JSON-native values:
/// default separators (`', '`, `': '`), `ensure_ascii` quoting, key
/// order preserved, floats in CPython `repr` spelling (`NaN`/`Infinity`
/// words, `allow_nan=True`). Algorithm ported from
/// `pidash_db::runner_sessions::machine_outbox` (private there). All
/// `current_instance` values cross pre-rendered (datetimes as DRF
/// strings, UUIDs as strings), so the encoder extras never trigger.
pub fn py_dumps(value: &Value) -> String {
    let mut out = String::new();
    dumps_value(value, &mut out);
    out
}

fn dumps_map(map: &Map<String, Value>, out: &mut String) {
    out.push('{');
    for (i, (key, value)) in map.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        dumps_string(key, out);
        out.push_str(": ");
        dumps_value(value, out);
    }
    out.push('}');
}

fn dumps_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&dumps_number(n)),
        Value::String(s) => dumps_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dumps_value(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => dumps_map(map, out),
    }
}

fn dumps_number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    match n.as_f64() {
        Some(f) => dumps_float(f),
        // Unreachable without `arbitrary_precision`: every Number is
        // i64, u64 or f64.
        None => n.to_string(),
    }
}

/// CPython `repr()` spelling for a finite float, with `json`-mode
/// (`NaN`/`Infinity`) non-finite words. Shortest digits come from Ryū
/// (via `serde_json::Number`): the shortest spelling that round-trips —
/// exactly `repr`'s rule. Only the fixed/exponent choice (`-4 <= e10 <=
/// 15` is fixed) and the exponent shape (`e±XX`, two digits minimum)
/// are applied here, so no re-rounding occurs.
fn dumps_float(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        if f.is_sign_negative() {
            return "-Infinity".to_string();
        }
        return "Infinity".to_string();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let ryu = serde_json::Number::from_f64(f)
        .expect("finite float")
        .to_string();
    let neg = f.is_sign_negative();
    let mant = ryu.trim_start_matches('-');
    let (mant, exp10) = match mant.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().expect("ryu exponent")),
        None => (mant, 0),
    };
    let frac_len = mant.split_once('.').map_or(0, |(_, fr)| fr.len() as i32);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_start_matches('0').to_string();
    // `f` is finite and nonzero, so a nonzero digit always survives.
    debug_assert!(!digits.is_empty());
    let exp = exp10 - frac_len + digits.len() as i32 - 1;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if (-4..=15).contains(&exp) {
        // Fixed notation; the point sits after `exp + 1` digits.
        let point = exp + 1;
        if point <= 0 {
            out.push_str("0.");
            for _ in point..0 {
                out.push('0');
            }
            out.push_str(&digits);
        } else if point as usize >= digits.len() {
            out.push_str(&digits);
            for _ in digits.len()..point as usize {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            let point = point as usize;
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{exp:+03}");
    }
    out
}

/// CPython `json.dumps` string quoting with `ensure_ascii=True`:
/// short escapes, `\u00xx` for other C0 controls and DEL, `\uXXXX`
/// for everything non-ASCII (astral chars as surrogate pairs), all
/// lowercase hex.
fn dumps_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if (c as u32) >= 0x80 => {
                let n = c as u32;
                if n > 0xffff {
                    let n = n - 0x10000;
                    let _ = write!(
                        out,
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (n >> 10),
                        0xdc00 + (n & 0x3ff)
                    );
                } else {
                    let _ = write!(out, "\\u{n:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------------------
// Outcome, post-commit actions, enqueues
// ---------------------------------------------------------------------------

/// Post-commit actions (`:364-372`): the executing layer registers these
/// on `pidash_db::tx::AfterCommit`, in order — the cancel first, then one
/// drain per source pod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MovePostCommit {
    /// Deliver the cancel frame via [`send_project_move_cancel`].
    SendCancel { runner_id: Uuid, run_id: Uuid },
    /// Run the `runner_sessions::drain` recipe for the pod
    /// (`DRAIN_POD_BY_ID_LOOKUP_SQL` + idle loop + `DrainEffect`s).
    DrainPod { pod_id: Uuid },
}

/// A completed move: the refreshed issue plus everything after the
/// transaction — the pre-move snapshot strings, the orchestration flag,
/// the post-commit actions, and the two enqueues.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveOutcome {
    pub issue: MovedIssueRow,
    /// `json.dumps(IssueSerializer(source).data)` — captured BEFORE the
    /// transaction from the SOURCE issue (`:159`).
    pub current_instance: String,
    /// `json.dumps({"project": str(target)})` (`:160`).
    pub requested_data: String,
    /// `dispatch_immediate` for the explicit orchestration entry call:
    /// False exactly when handoff runs exist (`:218-222`; the attribute
    /// default is True).
    pub dispatch_immediate: bool,
    pub post_commit: Vec<MovePostCommit>,
    /// `issue_activity` then `model_activity` (`:374-392`).
    pub enqueues: Vec<TaskEnqueue>,
}

/// Result of [`move_work_item_to_project`]: either a completed move or
/// the same-project no-op (returned unchanged, no writes at all,
/// `:142-144`).
#[derive(Debug, Clone, PartialEq)]
pub enum MoveResult {
    Moved(MoveOutcome),
    AlreadyThere(SourceIssueRow),
}

/// `issue_activity.delay(...)` kwargs (`:374-382`), in call order:
/// `type`, `requested_data`, `actor_id`, `issue_id`, `project_id`,
/// `current_instance`, `epoch`. `actor_id` is `str(actor.id)`.
pub fn issue_activity_kwargs(
    activity_type: &str,
    requested_data: &str,
    actor_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
    current_instance: &str,
    epoch: i64,
) -> TaskEnqueue {
    let mut kwargs = Map::with_capacity(7);
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.to_owned()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(current_instance.to_owned()),
    );
    kwargs.insert("epoch".to_owned(), Value::Number(epoch.into()));
    TaskEnqueue {
        task: ISSUE_ACTIVITY_TASK,
        args: Vec::new(),
        kwargs,
    }
}

/// `model_activity.delay(...)` kwargs (`:383-391`), in call order:
/// `model_name`, `model_id`, `requested_data`, `current_instance`,
/// `actor_id`, `slug`, `origin`. `requested_data` is the DICT (not the
/// JSON string); `actor_id` passes the UUID object at the Python call
/// (vs `str()` in `issue_activity`) — identical hyphenated strings on
/// the JSON wire. (The fixture annotates this arg "INT, not str"; the
/// model says `User.id` is a UUID — the wire bytes agree either way.)
pub fn move_model_activity_kwargs(
    model_id: &Uuid,
    requested_data: Value,
    current_instance: &str,
    actor_id: &Uuid,
    slug: &str,
    origin: &str,
) -> TaskEnqueue {
    let mut kwargs = Map::with_capacity(7);
    kwargs.insert("model_name".to_owned(), Value::String("issue".to_owned()));
    kwargs.insert("model_id".to_owned(), Value::String(model_id.to_string()));
    kwargs.insert("requested_data".to_owned(), requested_data);
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(current_instance.to_owned()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    TaskEnqueue {
        task: MODEL_ACTIVITY_TASK,
        args: Vec::new(),
        kwargs,
    }
}

/// What [`send_project_move_cancel`] did with its failures: warning
/// lines in the Python logger text, for the caller to log (this crate
/// has no `tracing` dependency). Empty means delivered (or discarded by
/// the outbox without complaint).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CancelOutcome {
    pub warnings: Vec<String>,
}

/// Best-effort cancellation for a project-move handoff
/// (`_send_project_move_cancel`, `:70-107`).
///
/// Cancel frames are deliberately not buffered for offline runners —
/// [`OutboxError::RunnerOffline`] becomes the redelivery log line (the
/// persisted `CANCEL_REQUESTED` status makes session-open return a
/// cancel frame on reconnect). Any other send failure becomes the
/// delivery-failure line: a transient outbox failure must not turn a
/// committed move into a 500. Nothing here propagates.
pub async fn send_project_move_cancel<P: PubsubStore>(
    store: &P,
    runner_id: Uuid,
    run_id: Uuid,
) -> CancelOutcome {
    let frame = cancel_frame(&run_id);
    match send_to_runner(store, runner_id, &frame).await {
        Ok(outcome) => CancelOutcome {
            warnings: outcome.warnings,
        },
        Err(OutboxError::RunnerOffline { .. }) => CancelOutcome {
            warnings: vec![offline_cancel_line(&runner_id, &run_id)],
        },
        Err(error) => CancelOutcome {
            warnings: vec![failed_cancel_line(&run_id, &error)],
        },
    }
}

// ---------------------------------------------------------------------------
// The move
// ---------------------------------------------------------------------------

/// Render `current_instance` (`:159`): the PIDASHCONV-660 read shape over
/// the SOURCE issue — no `fields=`/`expand=`, single payload, no
/// relations block (no context viewer), blocker summary appended.
async fn render_current_instance<S: MoveStore>(
    store: &S,
    issue: &SourceIssueRow,
    web_base_url: Option<&str>,
) -> Result<String, StoreError> {
    // Lazy reads in Python order: url parts (during the Base pass),
    // assignee/label id lists, then the blocker summary triple.
    let workspace_slug = store
        .representation_workspace_slug(issue.workspace_id)
        .await?;
    let project_identifier = store
        .representation_project_identifier(issue.project_id)
        .await?;
    let assignee_ids = store.representation_assignee_ids(issue.id).await?;
    let label_ids = store.representation_label_ids(issue.id).await?;
    let blocked_by_rows = store.representation_blocked_by(issue.id).await?;
    let blocking_rows = store.representation_blocking(issue.id).await?;
    let has_open_blockers = store.representation_has_open_blockers(issue.id).await?;

    // Owned render buffer: every `&str` below borrows from these locals.
    let id_s = issue.id.to_string();
    let project_s = issue.project_id.to_string();
    let workspace_s = issue.workspace_id.to_string();
    let parent_s = issue.parent_id.map(|id| id.to_string());
    let state_s = issue.state_id.map(|id| id.to_string());
    let estimate_s = issue.estimate_point_id.map(|id| id.to_string());
    let pod_s = issue.assigned_pod_id.map(|id| id.to_string());
    let created_by_s = issue.created_by_id.map(|id| id.to_string());
    let updated_by_s = issue.updated_by_id.map(|id| id.to_string());
    let created_at_s = serialize_drf_datetime(issue.created_at);
    let updated_at_s = serialize_drf_datetime(issue.updated_at);
    let deleted_at_s = issue.deleted_at.map(serialize_drf_datetime);
    let completed_at_s = issue.completed_at.map(serialize_drf_datetime);
    let archived_at_s = issue.archived_at.map(serialize_drf_datetime);
    let start_date_s = issue.start_date.map(|d| d.to_string());
    let target_date_s = issue.target_date.map(|d| d.to_string());
    let url = issue_url(
        web_base_url,
        workspace_slug.as_deref(),
        project_identifier.as_deref(),
        Some(issue.sequence_id),
    );
    let blocked_by_owned = summary_list(&blocked_by_rows);
    let blocking_owned = summary_list(&blocking_rows);
    let blocked_by: Vec<SummaryItem> = blocked_by_owned
        .iter()
        .map(|item| SummaryItem {
            identifier: item.identifier.clone(),
            state: item.state.as_deref(),
            state_group: item.state_group.as_deref(),
        })
        .collect();
    let blocking: Vec<SummaryItem> = blocking_owned
        .iter()
        .map(|item| SummaryItem {
            identifier: item.identifier.clone(),
            state: item.state.as_deref(),
            state_group: item.state_group.as_deref(),
        })
        .collect();
    let blockers = BlockerSummary {
        blocked_by,
        blocking,
        has_open_blockers,
    };
    let assignee_strs: Vec<String> = assignee_ids.iter().map(ToString::to_string).collect();
    let assignee_refs: Vec<&str> = assignee_strs.iter().map(String::as_str).collect();
    let label_strs: Vec<String> = label_ids.iter().map(ToString::to_string).collect();
    let label_refs: Vec<&str> = label_strs.iter().map(String::as_str).collect();

    let row = IssueRow {
        id: &id_s,
        type_id: issue.type_id.as_deref(),
        url,
        created_at: &created_at_s,
        updated_at: &updated_at_s,
        deleted_at: deleted_at_s.as_deref(),
        point: issue.point,
        name: &issue.name,
        description_html: &issue.description_html,
        description_binary: issue.description_binary.as_deref(),
        priority: &issue.priority,
        complexity_score: issue.complexity_score,
        start_date: start_date_s.as_deref(),
        target_date: target_date_s.as_deref(),
        sequence_id: issue.sequence_id,
        sort_order: issue.sort_order,
        completed_at: completed_at_s.as_deref(),
        archived_at: archived_at_s.as_deref(),
        is_draft: issue.is_draft,
        external_source: issue.external_source.as_deref(),
        external_id: issue.external_id.as_deref(),
        git_work_branch: &issue.git_work_branch,
        created_via: issue.created_via.as_deref(),
        agent_executor: issue.agent_executor.as_deref(),
        created_by: created_by_s.as_deref(),
        updated_by: updated_by_s.as_deref(),
        project: &project_s,
        workspace: &workspace_s,
        parent: parent_s.as_deref(),
        state: state_s.as_deref(),
        estimate_point: estimate_s.as_deref(),
        assigned_pod: pod_s.as_deref(),
    };
    let input = RepresentationInput {
        row: &row,
        fields: None,
        expand: &[],
        is_list: false,
        assignee_ids: &assignee_refs,
        assignee_rows: &[],
        label_ids: &label_refs,
        expanded_labels: &[],
        blockers: Some(&blockers),
        relations: None,
        expansions: &[],
    };
    let data = render_issue(&input)
        .map_err(|error| StoreError(format!("current_instance render failed: {error}")))?;
    Ok(py_dumps(&Value::Object(data)))
}

/// Move work item `pk` (in `project_id`) into the project `target_ref`
/// (`move_work_item_to_project`, `:122-393`).
///
/// `target_ref` is the raw JSON value (UUID or workspace-scoped
/// identifier — coerced per `str(x or "")`); `actor_id` is the acting
/// user; `origin` is the request host for webhooks; `web_base_url` is
/// the resolved `WEB_URL`/`APP_BASE_URL` config (`None` when
/// unconfigured — the `url` key is then omitted); `now` is the frozen
/// clock (`timezone.now()` — also the `epoch` stamp).
///
/// Returns the refreshed issue. Raises [`MoveError::Issue`] (400/403/409)
/// for recoverable failures; [`MoveError::IssueNotFound`] /
/// [`MoveError::ProjectNotFound`] propagate for the 404s.
#[allow(clippy::too_many_arguments)]
pub async fn move_work_item_to_project<S: MoveStore>(
    store: &S,
    slug: &str,
    project_id: Uuid,
    pk: Uuid,
    target_ref: &Value,
    actor_id: Uuid,
    origin: &str,
    web_base_url: Option<&str>,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<MoveResult, MoveError> {
    let target_text = target_ref_text(target_ref);
    let target_text = target_text.trim();
    if target_text.is_empty() {
        return Err(MoveError::Issue(IssueMoveError::new(
            PROJECT_REQUIRED_MESSAGE,
            400,
        )));
    }

    let issue = store
        .source_issue(slug, project_id, pk)
        .await?
        .ok_or(MoveError::IssueNotFound)?;
    let lookup = classify_lookup(target_text);
    let target_project = store
        .resolve_project(slug, &lookup)
        .await?
        .ok_or(MoveError::ProjectNotFound)?;
    if target_project.id == issue.project_id {
        // Already in the target project — nothing to do.
        return Ok(MoveResult::AlreadyThere(issue));
    }

    if !store
        .can_move_into_target(slug, target_project.id, actor_id)
        .await?
    {
        return Err(MoveError::Issue(IssueMoveError::new(
            NO_PERMISSION_MESSAGE,
            403,
        )));
    }

    let current_instance = render_current_instance(store, &issue, web_base_url).await?;
    let requested_data = py_dumps(&serde_json::json!({"project": target_project.id.to_string()}));

    // `A.first() or B.first()`: the fallback query only runs when the
    // default misses.
    let target_state = match store.target_default_state(target_project.id).await? {
        Some(state) => Some(state),
        None => store.target_fallback_state(target_project.id).await?,
    };
    let Some(target_state) = target_state else {
        return Err(MoveError::Issue(IssueMoveError::new(
            NO_WORKFLOW_STATE_MESSAGE,
            400,
        )));
    };

    // -- transaction --------------------------------------------------
    // The pool implementation runs this span inside one transaction; the
    // guards above and the refetch + enqueues below run outside it.
    let issue = store
        .lock_source_issue(slug, project_id, pk)
        .await?
        .ok_or(MoveError::IssueNotFound)?;
    store
        .advisory_lock_project(advisory_lock_key(&target_project.id))
        .await?;

    let handoff_runs = store.lock_handoff_runs(issue.id).await?;

    let last_sequence = store.max_target_sequence(target_project.id).await?;
    let next_sequence = saved_sequence_id(last_sequence);

    let target_pod = store.target_default_pod(target_project.id).await?;
    if !handoff_runs.is_empty() && target_pod.is_none() {
        return Err(MoveError::Issue(IssueMoveError::new(
            NO_DEFAULT_POD_MESSAGE,
            409,
        )));
    }
    let source_project_id = issue.project_id;
    store
        .save_moved_issue(
            issue.id,
            &MovedIssueFields {
                project_id: target_project.id,
                workspace_id: target_project.workspace_id,
                sequence_id: next_sequence,
                state_id: target_state.id,
                assigned_pod_id: target_pod.map(|pod| pod.id),
            },
            now,
        )
        .await?;

    let mut immediate_handoff_parent: Option<&HandoffRunRow> = None;
    let mut cancel_after_commit: Option<(Uuid, Uuid)> = None;
    let mut source_pods_to_drain: Vec<Uuid> = Vec::new();
    if !handoff_runs.is_empty() {
        let executing: Vec<&HandoffRunRow> = handoff_runs
            .iter()
            .filter(|run| is_executing_run(run))
            .collect();
        if executing.len() > 1 {
            // Orchestration's one-active-run rule should prevent this.
            // Refuse rather than declare live duplicates cancelled.
            return Err(MoveError::Issue(IssueMoveError::new(
                MULTIPLE_ACTIVE_RUNS_MESSAGE,
                409,
            )));
        }
        let handoff_parent = executing.first().copied().unwrap_or(&handoff_runs[0]);
        let mut inert: Vec<&HandoffRunRow> = handoff_runs
            .iter()
            .filter(|run| run.id != handoff_parent.id)
            .collect();
        if executing.is_empty() {
            inert.push(handoff_parent);
        }
        for inert_run in &inert {
            store.cancel_inert_run(inert_run.id, now).await?;
            if let Some(pod_id) = inert_run.pod_id {
                // A set in Python (dedupe, unordered); first-seen order
                // here — identical for the single-pod norm.
                if !source_pods_to_drain.contains(&pod_id) {
                    source_pods_to_drain.push(pod_id);
                }
            }
        }
        if !executing.is_empty() {
            let merged = handoff_run_config(
                &handoff_parent.run_config,
                &source_project_id,
                &target_project.id,
                &target_pod
                    .expect("handoff runs imply a default pod (409 arm above)")
                    .id,
            );
            store
                .mark_handoff_parent(handoff_parent.id, &merged)
                .await?;
            cancel_after_commit = handoff_parent
                .runner_id
                .map(|runner| (runner, handoff_parent.id));
        } else {
            immediate_handoff_parent = Some(handoff_parent);
        }
    }

    store
        .detach_old_sequences(issue.id, target_project.id)
        .await?;
    store
        .create_target_sequence(
            Uuid::new_v4(),
            issue.id,
            next_sequence,
            target_project.id,
            target_project.workspace_id,
            now,
            actor_id,
        )
        .await?;

    store
        .prune_assignees(issue.id, target_project.id, now)
        .await?;
    store
        .repoint_assignees(issue.id, target_project.id, target_project.workspace_id)
        .await?;

    store.soft_delete_issue_labels(issue.id, now).await?;
    store.soft_delete_cycle_issues(issue.id, now).await?;
    store.soft_delete_module_issues(issue.id, now).await?;
    store.soft_delete_issue_relations(issue.id, now).await?;
    store.detach_children(issue.id, target_project.id).await?;

    let comment_ids = store.moved_comment_ids(issue.id).await?;
    let description_ids = store.moved_description_ids(issue.id).await?;

    for target in REPOINT_ORDER {
        store
            .repoint_related(
                *target,
                issue.id,
                &comment_ids,
                &description_ids,
                target_project.id,
                target_project.workspace_id,
            )
            .await?;
    }

    if let (Some(parent), Some(pod)) = (immediate_handoff_parent, target_pod) {
        store
            .create_handoff_run(issue.id, parent, pod.id, now)
            .await?;
    }

    let mut post_commit = Vec::new();
    if let Some((runner_id, run_id)) = cancel_after_commit {
        post_commit.push(MovePostCommit::SendCancel { runner_id, run_id });
    }
    for pod_id in &source_pods_to_drain {
        post_commit.push(MovePostCommit::DrainPod { pod_id: *pod_id });
    }

    let enqueues = vec![
        issue_activity_kwargs(
            "issue.activity.updated",
            &requested_data,
            &actor_id,
            &pk,
            &target_project.id,
            &current_instance,
            now.timestamp(),
        ),
        move_model_activity_kwargs(
            &pk,
            serde_json::json!({"project": target_project.id.to_string()}),
            &current_instance,
            &actor_id,
            slug,
            origin,
        ),
    ];

    let moved = store
        .moved_issue(pk)
        .await?
        .ok_or(MoveError::IssueNotFound)?;
    Ok(MoveResult::Moved(MoveOutcome {
        issue: moved,
        current_instance,
        requested_data,
        dispatch_immediate: handoff_runs.is_empty(),
        post_commit,
        enqueues,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::runner_sessions::RunnerSession;
    use std::cell::RefCell;

    /// Load the FX-ISS-13 fixture.
    fn fixture() -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/app_issues/queries/FX-ISS-13.move.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("FX-ISS-13 fixture exists");
        serde_json::from_str(&text).expect("FX-ISS-13 fixture parses")
    }

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 123_456_000).expect("valid test clock")
    }

    const SLUG: &str = "acme";
    const ORIGIN: &str = "https://app.example.test";

    fn source_row() -> SourceIssueRow {
        SourceIssueRow {
            id: uuid(1),
            project_id: uuid(0x10),
            workspace_id: uuid(0x20),
            state_id: Some(uuid(0x30)),
            parent_id: Some(uuid(0x31)),
            estimate_point_id: Some(uuid(0x32)),
            assigned_pod_id: Some(uuid(0x33)),
            type_id: Some("bug".to_owned()),
            created_at: now(),
            updated_at: now(),
            deleted_at: None,
            point: Some(3),
            name: "Move me".to_owned(),
            description_html: "<p>body</p>".to_owned(),
            description_binary: None,
            priority: "high".to_owned(),
            complexity_score: 4,
            start_date: None,
            target_date: None,
            sequence_id: 11,
            sort_order: 65535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            git_work_branch: String::new(),
            created_via: None,
            agent_executor: None,
            created_by_id: Some(uuid(0x40)),
            updated_by_id: None,
        }
    }

    fn target_project() -> ProjectRow {
        ProjectRow {
            id: uuid(0x50),
            workspace_id: uuid(0x20),
            identifier: "ENG".to_owned(),
        }
    }

    fn handoff_row(id: u128, status: AgentRunStatus, runner: Option<u128>) -> HandoffRunRow {
        HandoffRunRow {
            id: uuid(id),
            status,
            runner_id: runner.map(uuid),
            pod_id: Some(uuid(0x60)),
            created_by_id: uuid(0x40),
            run_config: serde_json::json!({"keep": "me"}),
            trigger: AgentRunTrigger::Tick,
        }
    }

    /// Recording fake [`MoveStore`] with scripted reads.
    struct FakeStore {
        calls: RefCell<Vec<String>>,
        source: Option<SourceIssueRow>,
        resolve: Option<ProjectRow>,
        member: bool,
        ws_slug: Option<String>,
        proj_ident: Option<String>,
        assignees: Vec<Uuid>,
        labels: Vec<Uuid>,
        blocked_by: Vec<BlockerRow>,
        blocking: Vec<BlockerRow>,
        open_blockers: bool,
        default_state: Option<StateRow>,
        fallback_state: Option<StateRow>,
        handoff: Vec<HandoffRunRow>,
        max_seq: Option<i64>,
        pod: Option<PodRow>,
        comment_ids: Vec<Uuid>,
        desc_ids: Vec<Uuid>,
        moved: Option<MovedIssueRow>,
        saved: RefCell<Vec<MovedIssueFields>>,
        inert_closed: RefCell<Vec<Uuid>>,
        parent_marked: RefCell<Vec<(Uuid, Value)>>,
        handoff_created: RefCell<Vec<(Uuid, Uuid)>>,
        repoints: RefCell<Vec<RepointTarget>>,
        advisory_keys: RefCell<Vec<i64>>,
    }

    impl FakeStore {
        fn scripted() -> Self {
            let source = source_row();
            let moved_row = SourceIssueRow {
                project_id: uuid(0x50),
                sequence_id: 8,
                state_id: Some(uuid(0x70)),
                assigned_pod_id: Some(uuid(0x61)),
                parent_id: None,
                estimate_point_id: None,
                type_id: None,
                ..source.clone()
            };
            Self {
                calls: RefCell::new(Vec::new()),
                source: Some(source),
                resolve: Some(target_project()),
                member: true,
                ws_slug: Some(SLUG.to_owned()),
                proj_ident: Some("ACME_SRC".to_owned()),
                assignees: vec![uuid(0x40)],
                labels: vec![uuid(0x41)],
                blocked_by: Vec::new(),
                blocking: Vec::new(),
                open_blockers: false,
                default_state: Some(StateRow { id: uuid(0x70) }),
                fallback_state: Some(StateRow { id: uuid(0x71) }),
                handoff: Vec::new(),
                max_seq: Some(7),
                pod: Some(PodRow { id: uuid(0x61) }),
                comment_ids: vec![uuid(0x80)],
                desc_ids: vec![uuid(0x81)],
                moved: Some(MovedIssueRow {
                    row: moved_row,
                    workspace_slug: SLUG.to_owned(),
                    project_identifier: "ENG".to_owned(),
                }),
                saved: RefCell::new(Vec::new()),
                inert_closed: RefCell::new(Vec::new()),
                parent_marked: RefCell::new(Vec::new()),
                handoff_created: RefCell::new(Vec::new()),
                repoints: RefCell::new(Vec::new()),
                advisory_keys: RefCell::new(Vec::new()),
            }
        }

        fn log(&self, call: &str) {
            self.calls.borrow_mut().push(call.to_owned());
        }
    }

    #[allow(async_fn_in_trait)]
    impl MoveStore for FakeStore {
        async fn source_issue(
            &self,
            _slug: &str,
            _project_id: Uuid,
            _pk: Uuid,
        ) -> Result<Option<SourceIssueRow>, StoreError> {
            self.log("source_issue");
            Ok(self.source.clone())
        }

        async fn resolve_project(
            &self,
            _slug: &str,
            _lookup: &ProjectLookup,
        ) -> Result<Option<ProjectRow>, StoreError> {
            self.log("resolve_project");
            Ok(self.resolve.clone())
        }

        async fn can_move_into_target(
            &self,
            _slug: &str,
            _target_project_id: Uuid,
            _actor_id: Uuid,
        ) -> Result<bool, StoreError> {
            self.log("can_move_into_target");
            Ok(self.member)
        }

        async fn representation_workspace_slug(
            &self,
            _workspace_id: Uuid,
        ) -> Result<Option<String>, StoreError> {
            self.log("representation_workspace_slug");
            Ok(self.ws_slug.clone())
        }

        async fn representation_project_identifier(
            &self,
            _project_id: Uuid,
        ) -> Result<Option<String>, StoreError> {
            self.log("representation_project_identifier");
            Ok(self.proj_ident.clone())
        }

        async fn representation_assignee_ids(
            &self,
            _issue_id: Uuid,
        ) -> Result<Vec<Uuid>, StoreError> {
            self.log("representation_assignee_ids");
            Ok(self.assignees.clone())
        }

        async fn representation_label_ids(&self, _issue_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
            self.log("representation_label_ids");
            Ok(self.labels.clone())
        }

        async fn representation_blocked_by(
            &self,
            _issue_id: Uuid,
        ) -> Result<Vec<BlockerRow>, StoreError> {
            self.log("representation_blocked_by");
            Ok(self.blocked_by.clone())
        }

        async fn representation_blocking(
            &self,
            _issue_id: Uuid,
        ) -> Result<Vec<BlockerRow>, StoreError> {
            self.log("representation_blocking");
            Ok(self.blocking.clone())
        }

        async fn representation_has_open_blockers(
            &self,
            _issue_id: Uuid,
        ) -> Result<bool, StoreError> {
            self.log("representation_has_open_blockers");
            Ok(self.open_blockers)
        }

        async fn target_default_state(
            &self,
            _target_project_id: Uuid,
        ) -> Result<Option<StateRow>, StoreError> {
            self.log("target_default_state");
            Ok(self.default_state)
        }

        async fn target_fallback_state(
            &self,
            _target_project_id: Uuid,
        ) -> Result<Option<StateRow>, StoreError> {
            self.log("target_fallback_state");
            Ok(self.fallback_state)
        }

        async fn lock_source_issue(
            &self,
            _slug: &str,
            _project_id: Uuid,
            _pk: Uuid,
        ) -> Result<Option<SourceIssueRow>, StoreError> {
            self.log("lock_source_issue");
            Ok(self.source.clone())
        }

        async fn advisory_lock_project(&self, lock_key: i64) -> Result<(), StoreError> {
            self.log("advisory_lock_project");
            self.advisory_keys.borrow_mut().push(lock_key);
            Ok(())
        }

        async fn lock_handoff_runs(
            &self,
            _issue_id: Uuid,
        ) -> Result<Vec<HandoffRunRow>, StoreError> {
            self.log("lock_handoff_runs");
            Ok(self.handoff.clone())
        }

        async fn max_target_sequence(
            &self,
            _target_project_id: Uuid,
        ) -> Result<Option<i64>, StoreError> {
            self.log("max_target_sequence");
            Ok(self.max_seq)
        }

        async fn target_default_pod(
            &self,
            _target_project_id: Uuid,
        ) -> Result<Option<PodRow>, StoreError> {
            self.log("target_default_pod");
            Ok(self.pod)
        }

        async fn save_moved_issue(
            &self,
            _issue_id: Uuid,
            fields: &MovedIssueFields,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<(), StoreError> {
            self.log("save_moved_issue");
            self.saved.borrow_mut().push(fields.clone());
            Ok(())
        }

        async fn cancel_inert_run(
            &self,
            run_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<(), StoreError> {
            self.log("cancel_inert_run");
            self.inert_closed.borrow_mut().push(run_id);
            Ok(())
        }

        async fn mark_handoff_parent(
            &self,
            run_id: Uuid,
            run_config: &Value,
        ) -> Result<(), StoreError> {
            self.log("mark_handoff_parent");
            self.parent_marked
                .borrow_mut()
                .push((run_id, run_config.clone()));
            Ok(())
        }

        async fn detach_old_sequences(
            &self,
            _issue_id: Uuid,
            _target_project_id: Uuid,
        ) -> Result<u64, StoreError> {
            self.log("detach_old_sequences");
            Ok(1)
        }

        async fn create_target_sequence(
            &self,
            _sequence_id: Uuid,
            _issue_id: Uuid,
            _sequence: i64,
            _target_project_id: Uuid,
            _target_workspace_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
            _actor_id: Uuid,
        ) -> Result<(), StoreError> {
            self.log("create_target_sequence");
            Ok(())
        }

        async fn prune_assignees(
            &self,
            _issue_id: Uuid,
            _target_project_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<u64, StoreError> {
            self.log("prune_assignees");
            Ok(0)
        }

        async fn repoint_assignees(
            &self,
            _issue_id: Uuid,
            _target_project_id: Uuid,
            _target_workspace_id: Uuid,
        ) -> Result<u64, StoreError> {
            self.log("repoint_assignees");
            Ok(1)
        }

        async fn soft_delete_issue_labels(
            &self,
            _issue_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<u64, StoreError> {
            self.log("soft_delete_issue_labels");
            Ok(1)
        }

        async fn soft_delete_cycle_issues(
            &self,
            _issue_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<u64, StoreError> {
            self.log("soft_delete_cycle_issues");
            Ok(1)
        }

        async fn soft_delete_module_issues(
            &self,
            _issue_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<u64, StoreError> {
            self.log("soft_delete_module_issues");
            Ok(1)
        }

        async fn soft_delete_issue_relations(
            &self,
            _issue_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<u64, StoreError> {
            self.log("soft_delete_issue_relations");
            Ok(1)
        }

        async fn detach_children(
            &self,
            _issue_id: Uuid,
            _target_project_id: Uuid,
        ) -> Result<u64, StoreError> {
            self.log("detach_children");
            Ok(0)
        }

        async fn moved_comment_ids(&self, _issue_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
            self.log("moved_comment_ids");
            Ok(self.comment_ids.clone())
        }

        async fn moved_description_ids(&self, _issue_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
            self.log("moved_description_ids");
            Ok(self.desc_ids.clone())
        }

        async fn repoint_related(
            &self,
            target: RepointTarget,
            _issue_id: Uuid,
            _comment_ids: &[Uuid],
            _description_ids: &[Uuid],
            _target_project_id: Uuid,
            _target_workspace_id: Uuid,
        ) -> Result<u64, StoreError> {
            self.log("repoint_related");
            self.repoints.borrow_mut().push(target);
            Ok(1)
        }

        async fn create_handoff_run(
            &self,
            _issue_id: Uuid,
            parent_run: &HandoffRunRow,
            pod_id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
        ) -> Result<(), StoreError> {
            self.log("create_handoff_run");
            self.handoff_created
                .borrow_mut()
                .push((parent_run.id, pod_id));
            Ok(())
        }

        async fn moved_issue(&self, _pk: Uuid) -> Result<Option<MovedIssueRow>, StoreError> {
            self.log("moved_issue");
            Ok(self.moved.clone())
        }
    }

    /// Recording fake [`PubsubStore`] with a scripted send result.
    struct FakePubsub {
        send_result: RefCell<Option<Result<(), OutboxError>>>,
        sent: RefCell<Vec<(Uuid, Map<String, Value>)>>,
    }

    #[allow(async_fn_in_trait)]
    impl PubsubStore for FakePubsub {
        async fn enqueue_for_runner(
            &self,
            runner_id: Uuid,
            message: &Map<String, Value>,
        ) -> Result<Option<String>, OutboxError> {
            self.sent.borrow_mut().push((runner_id, message.clone()));
            match self.send_result.borrow_mut().take() {
                Some(result) => result.map(|()| None),
                None => Ok(None),
            }
        }

        async fn enqueue_for_machine(
            &self,
            _dev_machine_id: Uuid,
            _message: &Map<String, Value>,
        ) -> Result<Option<String>, pidash_db::runner_sessions::machine_outbox::MachineOutboxError>
        {
            Ok(None)
        }

        async fn active_runner_sessions(
            &self,
            _runner_id: Uuid,
        ) -> Result<Vec<RunnerSession>, OutboxError> {
            Ok(Vec::new())
        }

        async fn revoke_runner_session(
            &self,
            _session_id: Uuid,
            _reason: &str,
        ) -> Result<(), OutboxError> {
            Ok(())
        }

        async fn clear_session_marker(&self, _session_id: Uuid) -> Result<(), OutboxError> {
            Ok(())
        }

        async fn publish_session_eviction(
            &self,
            _runner_id: Uuid,
            _old_session_id: Uuid,
            _new_session_id: &str,
        ) -> Result<(), OutboxError> {
            Ok(())
        }
    }

    // -- error matrix ----------------------------------------------------

    #[test]
    fn error_matrix_matches_fixture() {
        let fx = fixture();
        let matrix = fx["error_matrix"]
            .as_array()
            .expect("error_matrix is a list");
        let by_code: Vec<(u16, &str)> = matrix
            .iter()
            .map(|row| {
                (
                    row["code"].as_u64().unwrap() as u16,
                    row["message"].as_str().unwrap(),
                )
            })
            .collect();
        assert!(by_code.contains(&(400, PROJECT_REQUIRED_MESSAGE)));
        assert!(by_code.contains(&(403, NO_PERMISSION_MESSAGE)));
        assert!(by_code.contains(&(400, NO_WORKFLOW_STATE_MESSAGE)));
        assert!(by_code.contains(&(409, NO_DEFAULT_POD_MESSAGE)));
        assert!(by_code.contains(&(409, MULTIPLE_ACTIVE_RUNS_MESSAGE)));
        // The 404 + no-op rows are descriptive; the arms below pin them.
        assert!(by_code.iter().any(|(code, _)| *code == 404));
        assert!(by_code.iter().any(|(code, _)| *code == 200));
    }

    async fn move_call(store: &FakeStore, target_ref: &Value) -> Result<MoveResult, MoveError> {
        move_work_item_to_project(
            store,
            SLUG,
            uuid(0x10),
            uuid(1),
            target_ref,
            uuid(0x40),
            ORIGIN,
            Some("https://app.example.test"),
            &now(),
        )
        .await
    }

    #[tokio::test]
    async fn blank_ref_is_400_without_store_touch() {
        for raw in [
            Value::Null,
            Value::String(String::new()),
            Value::String("   ".to_owned()),
            serde_json::json!(0),
            serde_json::json!(false),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            let store = FakeStore::scripted();
            let err = move_call(&store, &raw).await.expect_err("blank ref");
            assert_eq!(
                err,
                MoveError::Issue(IssueMoveError::new(PROJECT_REQUIRED_MESSAGE, 400)),
                "{raw}"
            );
            assert!(store.calls.borrow().is_empty(), "{raw}");
        }
    }

    #[test]
    fn target_ref_coercion_matches_str_of_or() {
        assert_eq!(target_ref_text(&serde_json::json!(" ENG ")), " ENG ");
        assert_eq!(target_ref_text(&serde_json::json!(123)), "123");
        assert_eq!(target_ref_text(&serde_json::json!(true)), "True");
        assert_eq!(target_ref_text(&serde_json::json!(1.5)), "1.5");
        // Surviving containers render (and 404 downstream).
        assert_eq!(target_ref_text(&serde_json::json!(["x"])), "[\"x\"]");
    }

    #[tokio::test]
    async fn not_found_arms() {
        let target = serde_json::json!("ENG");
        let mut store = FakeStore::scripted();
        store.source = None;
        assert_eq!(
            move_call(&store, &target).await.expect_err("issue miss"),
            MoveError::IssueNotFound
        );
        let mut store = FakeStore::scripted();
        store.resolve = None;
        assert_eq!(
            move_call(&store, &target).await.expect_err("resolve miss"),
            MoveError::ProjectNotFound
        );
    }

    #[tokio::test]
    async fn same_project_noop_writes_nothing() {
        let mut store = FakeStore::scripted();
        store.resolve = Some(ProjectRow {
            id: uuid(0x10),
            workspace_id: uuid(0x20),
            identifier: "SRC".to_owned(),
        });
        let target = serde_json::json!("SRC");
        match move_call(&store, &target).await.expect("no-op") {
            MoveResult::AlreadyThere(row) => assert_eq!(row.id, uuid(1)),
            MoveResult::Moved(_) => panic!("expected no-op"),
        }
        // Only the two guard reads; no membership check, no state check.
        assert_eq!(
            *store.calls.borrow(),
            vec!["source_issue".to_owned(), "resolve_project".to_owned()]
        );
    }

    #[tokio::test]
    async fn permission_and_state_arms() {
        let target = serde_json::json!("ENG");
        let mut store = FakeStore::scripted();
        store.member = false;
        assert_eq!(
            move_call(&store, &target).await.expect_err("403"),
            MoveError::Issue(IssueMoveError::new(NO_PERMISSION_MESSAGE, 403))
        );

        let mut store = FakeStore::scripted();
        store.default_state = None;
        store.fallback_state = None;
        assert_eq!(
            move_call(&store, &target).await.expect_err("no state"),
            MoveError::Issue(IssueMoveError::new(NO_WORKFLOW_STATE_MESSAGE, 400))
        );
        // Fallback runs only when the default misses.
        let mut store = FakeStore::scripted();
        store.default_state = None;
        let result = move_call(&store, &target)
            .await
            .expect("fallback state moves");
        assert!(matches!(result, MoveResult::Moved(_)));
        assert!(store
            .calls
            .borrow()
            .contains(&"target_fallback_state".to_owned()));

        let store = FakeStore::scripted();
        move_call(&store, &target).await.expect("moves");
        assert!(!store
            .calls
            .borrow()
            .contains(&"target_fallback_state".to_owned()));
    }

    // -- happy path -------------------------------------------------------

    #[tokio::test]
    async fn move_without_handoff_full_order() {
        let store = FakeStore::scripted();
        let target = serde_json::json!("ENG");
        let outcome = match move_call(&store, &target).await.expect("moves") {
            MoveResult::Moved(outcome) => outcome,
            MoveResult::AlreadyThere(_) => panic!("expected a move"),
        };
        let calls = store.calls.borrow();
        let head = &calls[..13];
        assert_eq!(
            head,
            [
                "source_issue",
                "resolve_project",
                "can_move_into_target",
                "representation_workspace_slug",
                "representation_project_identifier",
                "representation_assignee_ids",
                "representation_label_ids",
                "representation_blocked_by",
                "representation_blocking",
                "representation_has_open_blockers",
                "target_default_state",
                "lock_source_issue",
                "advisory_lock_project",
            ]
        );
        assert_eq!(
            &calls[13..26],
            [
                "lock_handoff_runs",
                "max_target_sequence",
                "target_default_pod",
                "save_moved_issue",
                "detach_old_sequences",
                "create_target_sequence",
                "prune_assignees",
                "repoint_assignees",
                "soft_delete_issue_labels",
                "soft_delete_cycle_issues",
                "soft_delete_module_issues",
                "soft_delete_issue_relations",
                "detach_children",
            ]
        );
        assert_eq!(
            &calls[26..28],
            ["moved_comment_ids", "moved_description_ids"]
        );
        assert_eq!(calls[28..40], ["repoint_related"; 12]);
        assert_eq!(&calls[40..], ["moved_issue"]);
        assert_eq!(calls.len(), 41);

        // Repoint order is Python's call order.
        assert_eq!(*store.repoints.borrow(), REPOINT_ORDER);

        // Save image: max(7)+1, default state, default pod, cleared fields.
        assert_eq!(
            *store.saved.borrow(),
            vec![MovedIssueFields {
                project_id: uuid(0x50),
                workspace_id: uuid(0x20),
                sequence_id: 8,
                state_id: uuid(0x70),
                assigned_pod_id: Some(uuid(0x61)),
            }]
        );
        // Advisory key derives from the TARGET project id.
        assert_eq!(
            *store.advisory_keys.borrow(),
            vec![advisory_lock_key(&uuid(0x50))]
        );

        // No handoff: immediate dispatch, no post-commit, moved row back.
        assert!(outcome.dispatch_immediate);
        assert!(outcome.post_commit.is_empty());
        assert!(store.inert_closed.borrow().is_empty());
        assert!(store.handoff_created.borrow().is_empty());
        assert_eq!(outcome.issue.row.sequence_id, 8);
        assert_eq!(
            outcome.requested_data,
            format!("{{\"project\": \"{}\"}}", uuid(0x50))
        );

        // Enqueues: issue_activity then model_activity.
        assert_eq!(outcome.enqueues.len(), 2);
        let (first, second) = (&outcome.enqueues[0], &outcome.enqueues[1]);
        assert_eq!(first.task, ISSUE_ACTIVITY_TASK);
        assert_eq!(
            first.kwargs["type"],
            serde_json::json!("issue.activity.updated")
        );
        assert_eq!(
            first.kwargs["requested_data"],
            Value::String(outcome.requested_data.clone())
        );
        assert_eq!(
            first.kwargs["actor_id"],
            serde_json::json!(uuid(0x40).to_string())
        );
        assert_eq!(
            first.kwargs["issue_id"],
            serde_json::json!(uuid(1).to_string())
        );
        assert_eq!(
            first.kwargs["project_id"],
            serde_json::json!(uuid(0x50).to_string())
        );
        assert_eq!(
            first.kwargs["current_instance"],
            Value::String(outcome.current_instance.clone())
        );
        assert_eq!(first.kwargs["epoch"], serde_json::json!(now().timestamp()));
        assert_eq!(second.task, MODEL_ACTIVITY_TASK);
        assert_eq!(second.kwargs["model_name"], serde_json::json!("issue"));
        assert_eq!(
            second.kwargs["model_id"],
            serde_json::json!(uuid(1).to_string())
        );
        assert_eq!(
            second.kwargs["requested_data"],
            serde_json::json!({"project": uuid(0x50).to_string()})
        );
        assert_eq!(
            second.kwargs["actor_id"],
            serde_json::json!(uuid(0x40).to_string())
        );
        assert_eq!(second.kwargs["slug"], serde_json::json!(SLUG));
        assert_eq!(second.kwargs["origin"], serde_json::json!(ORIGIN));
    }

    #[test]
    fn requested_data_bytes_have_dumps_separators() {
        let target = uuid(0x50);
        let bytes = py_dumps(&serde_json::json!({"project": target.to_string()}));
        assert_eq!(bytes, format!("{{\"project\": \"{target}\"}}"));
    }

    // -- handoff branches --------------------------------------------------

    #[tokio::test]
    async fn immediate_handoff_closes_and_recreates() {
        let mut store = FakeStore::scripted();
        store.handoff = vec![
            handoff_row(0x90, AgentRunStatus::Queued, None),
            handoff_row(0x91, AgentRunStatus::PausedAwaitingInput, None),
        ];
        let target = serde_json::json!("ENG");
        let outcome = match move_call(&store, &target).await.expect("moves") {
            MoveResult::Moved(outcome) => outcome,
            MoveResult::AlreadyThere(_) => panic!("expected a move"),
        };
        // Both rows inert (parent rejoins when nothing executes): closed
        // in list order, then ONE fresh row off the first.
        assert_eq!(*store.inert_closed.borrow(), vec![uuid(0x91), uuid(0x90)]);
        assert_eq!(
            *store.handoff_created.borrow(),
            vec![(uuid(0x90), uuid(0x61))]
        );
        assert!(store.parent_marked.borrow().is_empty());
        assert!(!outcome.dispatch_immediate);
        // Both inert rows share one pod: a single drain, no cancel.
        assert_eq!(
            outcome.post_commit,
            vec![MovePostCommit::DrainPod { pod_id: uuid(0x60) }]
        );
    }

    #[tokio::test]
    async fn executing_handoff_enters_cancel_barrier() {
        let mut store = FakeStore::scripted();
        store.handoff = vec![
            handoff_row(0x90, AgentRunStatus::Running, Some(0x95)),
            handoff_row(0x91, AgentRunStatus::Queued, None),
        ];
        let target = serde_json::json!("ENG");
        let outcome = match move_call(&store, &target).await.expect("moves") {
            MoveResult::Moved(outcome) => outcome,
            MoveResult::AlreadyThere(_) => panic!("expected a move"),
        };
        // The queued row closes; the running row enters the barrier.
        assert_eq!(*store.inert_closed.borrow(), vec![uuid(0x91)]);
        assert!(store.handoff_created.borrow().is_empty());
        let marked = store.parent_marked.borrow();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].0, uuid(0x90));
        assert_eq!(
            marked[0].1,
            serde_json::json!({
                "keep": "me",
                PROJECT_MOVE_HANDOFF_CONFIG_KEY: {
                    "source_project_id": uuid(0x10).to_string(),
                    "target_project_id": uuid(0x50).to_string(),
                    "target_pod_id": uuid(0x61).to_string(),
                },
            })
        );
        assert!(!outcome.dispatch_immediate);
        // Cancel first, then the inert row's pod drain.
        assert_eq!(
            outcome.post_commit,
            vec![
                MovePostCommit::SendCancel {
                    runner_id: uuid(0x95),
                    run_id: uuid(0x90),
                },
                MovePostCommit::DrainPod { pod_id: uuid(0x60) },
            ]
        );
    }

    #[tokio::test]
    async fn handoff_guard_arms() {
        let target = serde_json::json!("ENG");
        // Handoff runs but no default pod → 409.
        let mut store = FakeStore::scripted();
        store.handoff = vec![handoff_row(0x90, AgentRunStatus::Queued, None)];
        store.pod = None;
        assert_eq!(
            move_call(&store, &target).await.expect_err("no pod"),
            MoveError::Issue(IssueMoveError::new(NO_DEFAULT_POD_MESSAGE, 409))
        );
        // Two executing runs with runners → 409.
        let mut store = FakeStore::scripted();
        store.handoff = vec![
            handoff_row(0x90, AgentRunStatus::Running, Some(0x95)),
            handoff_row(0x91, AgentRunStatus::Assigned, Some(0x96)),
        ];
        assert_eq!(
            move_call(&store, &target).await.expect_err("two executing"),
            MoveError::Issue(IssueMoveError::new(MULTIPLE_ACTIVE_RUNS_MESSAGE, 409))
        );
        // Executing WITHOUT a runner is not "executing" (runnerless rows
        // join the inert set).
        let mut store = FakeStore::scripted();
        store.handoff = vec![
            handoff_row(0x90, AgentRunStatus::Running, None),
            handoff_row(0x91, AgentRunStatus::Running, Some(0x96)),
        ];
        let outcome = match move_call(&store, &target).await.expect("moves") {
            MoveResult::Moved(outcome) => outcome,
            MoveResult::AlreadyThere(_) => panic!("expected a move"),
        };
        assert_eq!(*store.inert_closed.borrow(), vec![uuid(0x90)]);
        assert_eq!(store.parent_marked.borrow().len(), 1);
        assert_eq!(outcome.post_commit.len(), 2);
    }

    // -- cancel discernment -------------------------------------------------

    fn offline_pubsub() -> FakePubsub {
        FakePubsub {
            send_result: RefCell::new(Some(Err(OutboxError::RunnerOffline {
                runner_id: uuid(0x95).to_string(),
                message_type: "cancel".to_owned(),
            }))),
            sent: RefCell::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn cancel_frame_bytes_and_offline_discernment() {
        let fx = fixture();
        let golden = &fx["move_db_effects"]["post_commit_actions"]["cancel_frame"]["bytes"];
        let frame = cancel_frame(&uuid(0x90));
        assert_eq!(frame["v"], serde_json::json!(1));
        assert_eq!(frame["type"], golden["type"]);
        assert_eq!(frame["reason"], golden["reason"]);
        assert_eq!(frame["run_id"], serde_json::json!(uuid(0x90).to_string()));
        let keys: Vec<&str> = frame.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["v", "type", "run_id", "reason"]);

        // Offline → the redelivery line, never an error.
        let pubsub = offline_pubsub();
        let out = send_project_move_cancel(&pubsub, uuid(0x95), uuid(0x90)).await;
        assert_eq!(
            out.warnings,
            vec![offline_cancel_line(&uuid(0x95), &uuid(0x90))]
        );
        assert_eq!(pubsub.sent.borrow().len(), 1);
        assert_eq!(pubsub.sent.borrow()[0].0, uuid(0x95));
        // The frame is enveloped (mid added) at send time.
        assert!(pubsub.sent.borrow()[0].1.contains_key("mid"));
    }

    #[tokio::test]
    async fn cancel_other_failures_warn_without_500() {
        // A swallowed transport failure surfaces as the outbox warning.
        let pubsub = FakePubsub {
            send_result: RefCell::new(Some(Err(OutboxError::UnknownMessageType(
                "bogus".to_owned(),
            )))),
            sent: RefCell::new(Vec::new()),
        };
        let out = send_project_move_cancel(&pubsub, uuid(0x95), uuid(0x90)).await;
        assert_eq!(out.warnings.len(), 1);
        assert!(
            out.warnings[0].contains("send_to_runner enqueue failed"),
            "{}",
            out.warnings[0]
        );

        // A clean send warns nothing.
        let pubsub = FakePubsub {
            send_result: RefCell::new(Some(Ok(()))),
            sent: RefCell::new(Vec::new()),
        };
        let out = send_project_move_cancel(&pubsub, uuid(0x95), uuid(0x90)).await;
        assert!(out.warnings.is_empty());
    }

    // -- current_instance ----------------------------------------------------

    #[tokio::test]
    async fn current_instance_is_the_source_shape() {
        let store = FakeStore::scripted();
        let target = serde_json::json!("ENG");
        let outcome = match move_call(&store, &target).await.expect("moves") {
            MoveResult::Moved(outcome) => outcome,
            MoveResult::AlreadyThere(_) => panic!("expected a move"),
        };
        let instance: Value =
            serde_json::from_str(&outcome.current_instance).expect("current_instance parses");
        // SOURCE values (pre-move), not target ones.
        assert_eq!(
            instance["project"],
            serde_json::json!(uuid(0x10).to_string())
        );
        assert_eq!(instance["sequence_id"], serde_json::json!(11));
        assert_eq!(instance["name"], serde_json::json!("Move me"));
        assert_eq!(
            instance["assignees"],
            serde_json::json!([uuid(0x40).to_string()])
        );
        assert_eq!(
            instance["labels"],
            serde_json::json!([uuid(0x41).to_string()])
        );
        // Single payload: blocker summary appended, no relations block
        // (no context viewer).
        assert!(instance.get("relations_summary").is_some());
        assert_eq!(instance["has_open_blockers"], serde_json::json!(false));
        assert!(instance.get("relations").is_none());
        // Excluded keys stay out; configured base renders the url.
        assert!(instance.get("description_json").is_none());
        assert!(instance.get("workpad").is_none());
        assert_eq!(
            instance["url"],
            serde_json::json!("https://app.example.test/acme/browse/ACME_SRC-11")
        );
        // DRF datetime rendering (micros kept, Z-suffixed).
        assert_eq!(
            instance["created_at"],
            serde_json::json!("2023-11-14T22:13:20.123456Z")
        );
    }

    #[tokio::test]
    async fn current_instance_omits_url_when_unconfigured() {
        let store = FakeStore::scripted();
        let out = move_work_item_to_project(
            &store,
            SLUG,
            uuid(0x10),
            uuid(1),
            &serde_json::json!("ENG"),
            uuid(0x40),
            ORIGIN,
            None,
            &now(),
        )
        .await
        .expect("moves");
        let outcome = match out {
            MoveResult::Moved(outcome) => outcome,
            MoveResult::AlreadyThere(_) => panic!("expected a move"),
        };
        let instance: Value = serde_json::from_str(&outcome.current_instance).unwrap();
        assert!(instance.get("url").is_none());
    }

    // -- dumps -----------------------------------------------------------------

    #[test]
    fn dumps_matches_python_separators_and_ascii() {
        // Separators `', '` / `': '`, key order preserved.
        let value = serde_json::json!({"b": [1, true, null], "a": "x"});
        let mut map = Map::new();
        map.insert("b".to_owned(), value["b"].clone());
        map.insert("a".to_owned(), value["a"].clone());
        assert_eq!(
            py_dumps(&Value::Object(map)),
            "{\"b\": [1, true, null], \"a\": \"x\"}"
        );
        // ensure_ascii: control escapes, DEL, BMP, astral pairs.
        assert_eq!(py_dumps(&serde_json::json!("é")), "\"\\u00e9\"");
        assert_eq!(py_dumps(&serde_json::json!("𝄞")), "\"\\ud834\\udd1e\"");
        assert_eq!(
            py_dumps(&serde_json::json!("a\nb\t\"q\"\\")),
            "\"a\\nb\\t\\\"q\\\"\\\\\""
        );
        assert_eq!(
            py_dumps(&serde_json::json!("\u{0}\u{7f}")),
            "\"\\u0000\\u007f\""
        );
        // Floats: repr spelling, never bare integers.
        assert_eq!(py_dumps(&serde_json::json!(65535.0)), "65535.0");
        assert_eq!(py_dumps(&serde_json::json!(0.1)), "0.1");
        assert_eq!(py_dumps(&serde_json::json!(1e16)), "1e+16");
        assert_eq!(py_dumps(&serde_json::json!(0.0001)), "0.0001");
        assert_eq!(py_dumps(&serde_json::json!(0.00001)), "1e-05");
        assert_eq!(py_dumps(&serde_json::json!(-0.0)), "-0.0");
    }

    #[test]
    fn dumps_cross_checked_against_cpython() {
        // Oracle vectors produced by CPython `json.dumps` (no lock-in:
        // recompute with `python3 -c "import json; print(json.dumps(...))"`).
        for (input, expected) in [
            ("héllo", "\"h\\u00e9llo\""),
            ("quote\"back\\slash", "\"quote\\\"back\\\\slash\""),
            ("tab\there", "\"tab\\there\""),
        ] {
            assert_eq!(py_dumps(&serde_json::json!(input)), expected, "{input:?}");
        }
        // Float oracle vectors (CPython repr).
        for (input, expected) in [
            // Shortest-round-trip tie breaks toward the true value:
            // the nearest f64 to the decimal literal renders `...412`.
            (153838026194641.13_f64, "153838026194641.12"),
            (1.0 / 3.0, "0.3333333333333333"),
            (123456789.0, "123456789.0"),
            (1.5e-7, "1.5e-07"),
        ] {
            assert_eq!(py_dumps(&serde_json::json!(input)), expected, "{input}");
        }
    }

    // -- SQL shape ----------------------------------------------------------

    #[test]
    fn sql_texts_carry_scope_locks_and_order() {
        use pidash_auth::permissions::Role;
        // The `role >= 15` literal is `ROLE.MEMBER.value`.
        assert_eq!(Role::Member.value(), 15);
        assert!(MEMBER_EXISTS_SQL.contains("\"role\" >= 15"));
        assert!(ASSIGNEE_PRUNE_SQL.contains("\"role\" >= 15"));
        // Lock forms.
        assert!(SOURCE_ISSUE_LOCK_SQL.contains("FOR UPDATE OF \"issues\""));
        assert!(
            HANDOFF_RUNS_LOCK_SQL.contains("ORDER BY \"agent_run\".\"created_at\" DESC FOR UPDATE")
        );
        assert!(!HANDOFF_RUNS_LOCK_SQL.contains("SKIP LOCKED"));
        // The handoff status set matches the module const.
        for status in PROJECT_MOVE_HANDOFF_STATUSES {
            assert!(HANDOFF_RUNS_LOCK_SQL.contains(status.value()), "{status:?}");
        }
        assert_eq!(PROJECT_MOVE_HANDOFF_STATUSES.len(), 8);
        // RESOLVE arms: pk vs exact-identifier equality.
        assert!(PROJECT_RESOLVE_BY_PK_SQL.contains("\"projects\".\"id\" = $1"));
        assert!(PROJECT_RESOLVE_BY_IDENTIFIER_SQL.contains("\"projects\".\"identifier\" = $1"));
        assert!(!PROJECT_RESOLVE_BY_IDENTIFIER_SQL.contains("LOWER("));
        // The save writes exactly the update_fields, in order.
        let set = &ISSUE_MOVE_UPDATE_SQL[ISSUE_MOVE_UPDATE_SQL.find("SET ").unwrap()
            ..ISSUE_MOVE_UPDATE_SQL.find(" WHERE").unwrap()];
        assert_eq!(
            set,
            "SET \"project_id\" = $1, \"workspace_id\" = $2, \"sequence_id\" = $3, \"state_id\" = $4, \"assigned_pod_id\" = $5, \"parent_id\" = $6, \"estimate_point_id\" = $7, \"type_id\" = $8, \"updated_at\" = $9"
        );
        // All 12 repoints set the same pair.
        assert_eq!(REPOINT_ORDER.len(), 12);
        for target in REPOINT_ORDER {
            let sql = repoint_sql(*target);
            assert!(
                sql.contains("SET \"project_id\" = $1, \"workspace_id\" = $2"),
                "{target:?}"
            );
            assert!(sql.contains("\"deleted_at\" IS NULL"), "{target:?}");
        }
        // Soft deletes stamp deleted_at; queryset updates never move updated_at.
        for sql in [
            LABEL_DELETE_SQL,
            CYCLE_DELETE_SQL,
            MODULE_DELETE_SQL,
            RELATION_DELETE_SQL,
            ASSIGNEE_PRUNE_SQL,
        ] {
            assert!(sql.contains("SET \"deleted_at\" = $1"), "{sql}");
        }
        for sql in [
            ASSIGNEE_REPOINT_SQL,
            CHILDREN_DETACH_SQL,
            SEQUENCE_DETACH_SQL,
        ] {
            assert!(!sql.contains("updated_at"), "{sql}");
        }
        assert_eq!(
            repoint_sql(RepointTarget::IssueActivity)
                .matches("$4")
                .count(),
            1
        );
    }
}
