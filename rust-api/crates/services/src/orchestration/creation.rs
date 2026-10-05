#![forbid(unsafe_code)]

//! Run-creation builders + parenting + handoff (D-12 L6, stage 5).
//!
//! Port of the creation core of `orchestration/service.py`
//! (`01a93e17`, all line numbers below are against that revision):
//!
//! * [`create_and_dispatch_run`] — `_create_and_dispatch_run` (`:735-830`).
//! * [`create_continuation_run`] — `_create_continuation_run` (`:443-525`).
//! * [`parent_for_next_run`] + [`select_parent_for_next_run`] —
//!   `parent_for_next_run` (`:110-154`).
//! * [`phase_kind_for_issue`] — `_phase_kind_for_issue` (`:157-167`).
//! * [`pinned_runner_for`] — `_pinned_runner_for` (`:528-547`).
//! * [`run_config_for_issue`] — `_run_config_for_issue` (`:550-566`).
//! * [`resolve_fallback_creator`] — `_resolve_fallback_creator` (`:303-316`).
//! * [`resolve_pod_for_issue`] — `_resolve_pod_for_issue` (`:319-338`).
//! * [`complete_project_move_handoff`] — `complete_project_move_handoff`
//!   (`:644-732`).
//! * [`create_project_move_handoff_run`] —
//!   `_create_project_move_handoff_run` (`:569-641`).
//!
//! Out of scope here (owned by siblings): the entry dispatchers
//! (`handle_issue_state_transition`, `handle_issue_comment`,
//! `dispatch_scheduler_run` — L7/L8), the clock (`scheduling` — L5), and
//! the relations/blockers reads (L3/L4, already in this module).
//!
//! # Seams
//!
//! The services crate carries no `sqlx` dependency, so — per the
//! `GitStore` precedent (`integrations/accounts.rs`) — every effect in
//! the Python control flow arrives through a seam trait, and the Python
//! order stays here, verbatim:
//!
//! * [`CreationSeam`] — one method per query shape (SQL text in the
//!   adjacent `*_SQL` consts, which the pool implementation must execute
//!   verbatim) plus the three `cloud_agent.creation` calls
//!   (`execution_fields`, `lock_cloud_creation_capacity`,
//!   `dispatch_after_commit` — D-11 L6, merged jobs-side). Services code
//!   must not call `jobs::dispatch` (the `jobs → services` edge is
//!   load-bearing; calling back would cycle), so the jobs-side
//!   `LiveCreationStore` (`pidash-jobs`, `orchestration/creation_jobs.rs`)
//!   implements this trait by delegating those three methods to the
//!   merged `jobs::dispatch` functions.
//! * [`FinalizeAgentRunSeam`] — the one-method seam for the
//!   `finalize_agent_run(run.id, FAILED, …)` call sites in this module.
//!   Finalization is runner D-15 (downstream of D-12 — no edge, it would
//!   deadlock); D-15 implements this trait later. L7 reuses it via its
//!   L6 edge.
//!
//! Prompt rendering (`build_first_turn`) runs here through the merged
//! same-crate [`prompting`][crate::prompting] pieces (`build_context` +
//! `build_first_turn` + `kind_for`): the seam preloads one
//! [`RenderBundle`] (the api prompt-preview fetch side, mirrored query
//! for query) and [`render_first_turn`] assembles + composes, mapping
//! the message-preserving [`PromptComposeError`][crate::prompting::composer::PromptComposeError]
//! onto the `render-failed` path exactly like the
//! `(PromptRenderError, RecipeNotFound, PromptRegistryError)` catch.
//!
//! # Transactions
//!
//! Python holds one `transaction.atomic()` per builder call (create,
//! finalize-or-render, prompt save, `dispatch_after_commit` capture). The
//! jobs-side driver opens one [`Transaction`][pidash_db::tx::Transaction],
//! runs the whole driver on it, commits, then drains the collected
//! dispatches via `dispatch_agent_run` (post-commit, never inline) — so
//! the lock→insert→save atomicity and the on-commit dispatch survive the
//! port. `execution_fields` runs on its own short transaction inside the
//! seam method (the inner atomic at `creation.py:55-70`).
//!
//! Fixture: `rust-api/fixtures/orchestration/fx06_creation/` (FX-ORCH-06),
//! replayed by the suite below (fake seam) and the jobs-side live suite.
//!
//! Ported bugs and quirks (translate, don't redesign):
//!
//! * The handoff replacement stamps no `phase_kind` (`:604-616` passes
//!   none — the column default `""` lands, unlike the `kind_for` stamp
//!   of the other two builders).
//! * A manual over-capacity `lock_cloud_creation_capacity` raise
//!   propagates (the `try` wraps `execution_fields` only) —
//!   [`CreationError::CapacityRefused`].
//! * `resume_parent_run` is a bare FK follow: a dangling id raises
//!   (`RelatedObjectDoesNotExist`) — [`CreationError::MissingRow`].
//! * The nested `atomic` in the handoff path is a savepoint; the port
//!   runs it flat on the same transaction (no partial-rollback path
//!   exists inside, so the difference is unobservable).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::dispatch::status::{AgentRunStatus, AgentRunTrigger};
use pidash_db::orchestration::is_terminal_status;
use pidash_db::orchestration::runs::ACTIVE_STATUSES;
use pidash_types::dispatch::AgentExecutorKind;
pub use pidash_types::orchestration::PROJECT_MOVE_HANDOFF_CONFIG_KEY;
use pidash_types::orchestration::{ContinuationOutcome, PhaseConfig, StateRef, TransitionOutcome};

use crate::dispatch::UserFlags;
use crate::prompting::{composer, context, recipes};

// ---------------------------------------------------------------------------
// Reasons, triggers, literals
// ---------------------------------------------------------------------------

/// `AgentRunTrigger.TICK` is the only automatic issue trigger
/// (`runner/models.py:273`, `AUTOMATIC_ISSUE_TRIGGERS`). The trigger is
/// the raw stored string: Django never validates the column on read,
/// so an unrecognized value is simply not a member (migration 0029:
/// neither human nor automatic).
pub fn is_automatic_issue_trigger(trigger: &str) -> bool {
    trigger == AgentRunTrigger::Tick.value()
}

/// `run_is_human_triggered` (`runner/models.py:279-281`): the trigger is
/// one of `HUMAN_TRIGGERS` (`models.py:259-266`: state_transition,
/// run_ai, comment_and_run, direct). Unrecognized values answer false,
/// as the `in` check does in Python.
pub fn is_human_triggered(trigger: &str) -> bool {
    [
        AgentRunTrigger::StateTransition.value(),
        AgentRunTrigger::RunAi.value(),
        AgentRunTrigger::CommentAndRun.value(),
        AgentRunTrigger::Direct.value(),
    ]
    .contains(&trigger)
}

/// The user whose overrides apply to the run (`composer.py:372-380`):
/// human-triggered runs resolve with `created_by`, automatic runs use
/// workspace + defaults only.
pub fn user_id_for_run(trigger: &str, created_by_id: Uuid) -> Option<String> {
    is_human_triggered(trigger).then(|| created_by_id.to_string())
}

/// The render-failure reason (`service.py:521,637,822`).
pub const REASON_RENDER_FAILED: &str = "render-failed";

/// The created reason (`service.py:525,830`).
pub const REASON_CREATED: &str = "created";

/// The `prompt_build_failed` error code (`service.py:514,631,818`).
pub const ERROR_CODE_PROMPT_BUILD_FAILED: &str = "prompt_build_failed";

/// The moved-again suppression marker (`service.py:693`).
pub const SUPPRESSED_ISSUE_MOVED_AGAIN: &str = "issue_moved_again";

/// `RunnerStatus.REVOKED` (`runner/models.py:183`): fixture-pinned
/// literal — runner models are unmerged (no D-13/D-15 edge).
pub const RUNNER_STATUS_REVOKED: &str = "revoked";

/// `AgentRunStatus.QUEUED` for the INSERT literal.
pub const STATUS_QUEUED: &str = "queued";

/// New runs start unowned: `owner` stays NULL until assignment captures
/// `runner.owner` (`service.py:798`).
pub const OWNER_UNASSIGNED: Option<Uuid> = None;

// ---------------------------------------------------------------------------
// SQL: creation flow (executed verbatim by the jobs-side store)
// ---------------------------------------------------------------------------

/// The issue projection every unit reads (`service.py` passes full issue
/// rows; this projects the consumed columns). `$1` is the issue id.
pub const ISSUE_SELECT_SQL: &str = "SELECT id, workspace_id, project_id, state_id, parent_id, \
    created_by_id, assigned_pod_id, agent_executor, git_work_branch, workpad, name, \
    description_stripped, priority, sequence_id, target_date FROM issues WHERE id = $1";

/// The project projection (`service.py` reads `issue.project` for the
/// executor scope, repo snapshot, creator fallback and clock policy).
/// `$1` is the project id.
pub const PROJECT_SELECT_SQL: &str = "SELECT id, workspace_id, identifier, name, description, \
    repo_url, base_branch, default_agent_executor, project_lead_id, default_assignee_id, \
    agent_default_max_ticks, agent_default_interval_seconds, \
    agent_review_default_interval_seconds, agent_test_default_interval_seconds \
    FROM projects WHERE id = $1";

/// The state projection (registry reads `name` + `group` only). `$1` is
/// the state id.
pub const STATE_SELECT_SQL: &str = "SELECT id, name, \"group\" FROM states WHERE id = $1";

/// The consumed `agent_run` projection for creation reads. Django
/// selects the full 41-column row; the L2 `READ_COLUMNS` projection
/// lacks `parent_run_id` / `run_config` / `phase_kind` /
/// `prompt_manifest` / `ended_at`, so creation projects its own.
pub const RUN_VIEW_COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "created_by_id",
    "pod_id",
    "runner_id",
    "pinned_runner_id",
    "parent_run_id",
    "work_item_id",
    "status",
    "trigger",
    "executor_kind",
    "phase_kind",
    "run_config",
    "tool_plan",
    "error_code",
    "error",
    "prompt",
    "prompt_manifest",
    "ended_at",
];

/// `_latest_prior_run` (`service.py:106-107`) over [`RUN_VIEW_COLUMNS`].
/// `$1` is the work-item id.
pub fn latest_prior_run_sql() -> String {
    format!(
        "SELECT {} FROM agent_run WHERE work_item_id = $1 ORDER BY created_at DESC LIMIT 1",
        RUN_VIEW_COLUMNS.join(", ")
    )
}

/// `_active_run_for` (`service.py:84-103`) over [`RUN_VIEW_COLUMNS`]:
/// the L2 [`ACTIVE_STATUSES`] set as a literal `IN` list, newest first.
/// `$1` is the work-item id.
pub fn active_run_sql() -> String {
    let statuses: Vec<String> = ACTIVE_STATUSES
        .iter()
        .map(|status| format!("'{}'", status.value()))
        .collect();
    format!(
        "SELECT {} FROM agent_run WHERE work_item_id = $1 AND status IN ({}) \
         ORDER BY created_at DESC LIMIT 1",
        RUN_VIEW_COLUMNS.join(", "),
        statuses.join(", ")
    )
}

/// One unlocked run row (`service.py:689` replacement lookup, resume
/// parent follow). `$1` is the run id.
pub fn run_select_sql() -> String {
    format!(
        "SELECT {} FROM agent_run WHERE id = $1",
        RUN_VIEW_COLUMNS.join(", ")
    )
}

/// The parent's runner for pinning (`service.py:538-540`:
/// `parent.runner`). `$1` is the runner id.
pub const RUNNER_SELECT_SQL: &str = "SELECT id, pod_id, status FROM runner WHERE id = $1";

/// The assigned pod (`service.py:329`): the default `Pod.objects`
/// manager excludes soft-deleted rows (`runner/models.py:42-49`). `$1`
/// is the pod id.
pub const ASSIGNED_POD_SELECT_SQL: &str =
    "SELECT id, project_id FROM pod WHERE id = $1 AND deleted_at IS NULL";

/// `Pod.default_for_project_id` (`runner/models.py:174-176`) under the
/// default manager: first live default pod of the project. `$1` is the
/// project id.
pub const DEFAULT_POD_SELECT_SQL: &str = "SELECT id, project_id FROM pod \
    WHERE project_id = $1 AND is_default AND deleted_at IS NULL \
    ORDER BY id LIMIT 1";

/// The hand-back lineage pointer (`service.py:152`): the ticker's
/// `resume_parent_run_id`, or NULL when the issue never armed a ticker.
/// `$1` is the issue id.
pub const TICKER_RESUME_SELECT_SQL: &str =
    "SELECT resume_parent_run_id FROM issue_agent_ticker WHERE issue_id = $1";

/// The unlocked work-item id read that opens the handoff
/// (`service.py:656-660`). `$1` is the run id.
pub const WORK_ITEM_ID_SELECT_SQL: &str = "SELECT work_item_id FROM agent_run WHERE id = $1";

/// The handoff issue lock (`service.py:664-669`): `Issue.all_objects`
/// (soft-deleted rows included — no `deleted_at` guard)
/// `select_for_update(of=("self",))`. `$1` is the issue id. The store
/// re-reads the project/state bundle for the locked row.
pub const ISSUE_LOCK_SQL: &str = "SELECT id FROM issues WHERE id = $1 FOR UPDATE OF issues";

/// The handoff source-run lock (`service.py:673-678`):
/// `select_for_update(of=("self",))` over [`RUN_VIEW_COLUMNS`]. `$1` is
/// the run id. Lock order is issue → run (`service.py:653-655`).
pub fn run_lock_sql() -> String {
    format!(
        "SELECT {} FROM agent_run WHERE id = $1 FOR UPDATE OF agent_run",
        RUN_VIEW_COLUMNS.join(", ")
    )
}

/// The full-row INSERT (`AgentRun.objects.create`, `service.py:478-495`,
/// `:604-616`, `:785-799`): Django inserts every concrete column, so
/// this lists every non-generated column in model order (the three
/// `JSONKeyBigIntegerField` generated columns excluded). Fixed values
/// are literals (`queued`, `0`, `""`, `{}`, `[]`, NULLs); `$1..$14`
/// carry the caller-supplied fields in [`NewAgentRun`] order.
pub const RUN_INSERT_SQL: &str =
    "INSERT INTO agent_run (id, workspace_id, owner_id, created_by_id, \
    pod_id, runner_id, pinned_runner_id, work_item_id, scheduler_binding_id, parent_run_id, \
    status, executor_kind, dispatch_attempts, cancel_requested_at, cancel_reason, error_code, \
    tool_plan, terminal_hooks_applied_at, terminal_capacity_released_at, prompt, trigger, \
    prompt_manifest, phase_kind, run_config, required_capabilities, thread_id, agent_metadata, \
    lease_expires_at, done_payload, error, refusal_category, llm_model, usage, created_at, \
    assigned_at, queue_position, started_at, ended_at) \
    VALUES ($1, $2, NULL, $3, $4, NULL, $5, $6, NULL, $7, 'queued', $8, 0, NULL, '', $9, $10, \
    NULL, NULL, '', $11, NULL, $12, $13, '[]', '', '{}', NULL, NULL, '', '', '', '{}', $14, \
    NULL, NULL, NULL, NULL)";

/// `run.save(update_fields=["prompt", "prompt_manifest"])`
/// (`service.py:522,639,824`). `$1`/`$2` are prompt/manifest, `$3` the
/// run id.
pub const PROMPT_UPDATE_SQL: &str =
    "UPDATE agent_run SET prompt = $1, prompt_manifest = $2 WHERE id = $3";

/// `source.save(update_fields=["run_config"])` (`service.py:696,707,731`).
/// `$1` is the config, `$2` the run id.
pub const RUN_CONFIG_UPDATE_SQL: &str = "UPDATE agent_run SET run_config = $1 WHERE id = $2";

/// The finalize lock (`agent_run_finalization.py:61`): first-writer-wins
/// — the row must still be non-terminal. `$1` is the run id; the
/// terminal set is a literal `NOT IN` list.
pub fn finalize_lock_sql() -> String {
    let terminal: Vec<String> = [
        AgentRunStatus::Completed,
        AgentRunStatus::Failed,
        AgentRunStatus::Cancelled,
        AgentRunStatus::Blocked,
        AgentRunStatus::Refused,
    ]
    .iter()
    .map(|status| format!("'{}'", status.value()))
    .collect();
    format!(
        "SELECT {} FROM agent_run WHERE id = $1 AND status NOT IN ({}) FOR UPDATE OF agent_run",
        RUN_VIEW_COLUMNS.join(", "),
        terminal.join(", ")
    )
}

/// The finalize write (`agent_run_finalization.py:52-71`):
/// status/ended_at/queue NULL/terminal NULLs + the updates
/// (`error_code`, `error`). `$1` ended_at, `$2` error_code, `$3` error,
/// `$4` the run id.
pub const FINALIZE_UPDATE_SQL: &str = "UPDATE agent_run SET status = 'failed', ended_at = $1, \
    queue_position = NULL, terminal_hooks_applied_at = NULL, \
    terminal_capacity_released_at = NULL, error_code = $2, error = $3 WHERE id = $4";

/// The cloud terminal-event guard (`agent_run_finalization.py:72-75`).
/// `$1` is the run id.
pub const TERMINAL_EVENT_EXISTS_SQL: &str =
    "SELECT EXISTS(SELECT 1 FROM agent_run_event WHERE agent_run_id = $1 AND kind = 'terminal')";

/// The next event seq (`agent_run_finalization.py:76-78`):
/// `max(seq) + 1`, `1` when no rows. `$1` is the run id.
pub const TERMINAL_EVENT_SEQ_SQL: &str =
    "SELECT COALESCE(MAX(seq), 0) + 1 FROM agent_run_event WHERE agent_run_id = $1";

/// The cloud terminal event (`agent_run_finalization.py:79-84`): `$1`
/// run id, `$2` seq, `$3` the `{"status", "error_code"}` payload, `$4`
/// created_at.
pub const TERMINAL_EVENT_INSERT_SQL: &str = "INSERT INTO agent_run_event \
    (agent_run_id, seq, kind, payload, created_at) VALUES ($1, $2, 'terminal', $3, $4)";

/// Creator flags for the executor seam (`user_has_llm_config` reads
/// `is_active` / `is_bot`, `agent_execution.py:76`). `$1` is the user id.
pub const USER_FLAGS_SELECT_SQL: &str = "SELECT is_active, is_bot FROM users WHERE id = $1";

// ---------------------------------------------------------------------------
// SQL: render bundle (mirrors the api prompt-preview loaders)
// ---------------------------------------------------------------------------

/// `issue.labels` (`context.py:536`): live link + live label, newest
/// first. `$1` is the issue id.
pub const BUNDLE_LABELS_SQL: &str = "SELECT l.name FROM issue_labels il \
    JOIN labels l ON l.id = il.label_id \
    WHERE il.issue_id = $1 AND il.deleted_at IS NULL AND l.deleted_at IS NULL \
    ORDER BY l.created_at DESC";

/// `issue.assignees` (`context.py:537`): display name / email pairs. `$1`
/// is the issue id.
pub const BUNDLE_ASSIGNEES_SQL: &str = "SELECT u.display_name, u.email FROM issue_assignees ia \
    JOIN users u ON u.id = ia.assignee_id WHERE ia.issue_id = $1 AND ia.deleted_at IS NULL \
    ORDER BY u.id";

/// `State.objects.filter(project=…)` (`context.py:538-544`): triage
/// excluded, `sequence` order. `$1` is the project id.
pub const BUNDLE_PROJECT_STATES_SQL: &str = "SELECT name, \"group\", description FROM states \
    WHERE project_id = $1 AND deleted_at IS NULL AND \"group\" != 'triage' ORDER BY sequence";

/// Direct children (`context.py:94-107`): live, non-triage,
/// non-archived, non-draft, oldest first. `$1` is the issue id.
pub const BUNDLE_CHILDREN_SQL: &str = "SELECT i.id, i.name, i.sequence_id, p.identifier, s.name \
    FROM issues i JOIN projects p ON p.id = i.project_id LEFT JOIN states s ON s.id = i.state_id \
    WHERE i.parent_id = $1 AND i.deleted_at IS NULL AND s.\"group\" != 'triage' \
    AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft \
    ORDER BY i.created_at ASC";

/// Relation rows, both directions (`relation.py` shape): live rows,
/// newest first. `$1` is the issue id.
pub const BUNDLE_RELATIONS_SQL: &str = "SELECT issue_id, related_issue_id, relation_type \
    FROM issue_relations WHERE (issue_id = $1 OR related_issue_id = $1) AND deleted_at IS NULL \
    ORDER BY created_at DESC";

/// Relation targets: live issues with their own project's identifier.
/// `$1` is the id array.
pub const BUNDLE_RELATION_TARGETS_SQL: &str = "SELECT i.id, i.name, i.sequence_id, p.identifier, \
    s.name, s.\"group\" FROM issues i JOIN projects p ON p.id = i.project_id \
    LEFT JOIN states s ON s.id = i.state_id WHERE i.id = ANY($1) AND i.deleted_at IS NULL";

/// Unfolded comments, chronological (`context.py:297-329`):
/// `fold`-labeled rows excluded. `$1` is the issue id.
pub const BUNDLE_COMMENTS_SQL: &str =
    "SELECT c.comment_stripped, c.speaker_type, c.speaker_label, \
    c.speaker_agent_run_id, c.created_at, u.display_name, u.email, u.username, u.is_bot \
    FROM issue_comments c LEFT JOIN users u ON u.id = c.actor_id \
    WHERE c.issue_id = $1 AND c.deleted_at IS NULL \
    AND NOT (c.labels @> ARRAY['fold']::varchar[]) ORDER BY c.created_at ASC";

/// Attached review links, newest first (`context.py:472-494`). `$1` is
/// the issue id.
pub const BUNDLE_CODE_REVIEWS_SQL: &str = "SELECT url, title, state, merged, draft, provider, \
    external_iid FROM git_code_review_links WHERE issue_id = $1 AND deleted_at IS NULL \
    ORDER BY created_at DESC";

/// The first bound remote (`context.py:434-469`): oldest live binding.
/// `$1` is the project id.
pub const BUNDLE_REMOTE_SQL: &str = "SELECT r.provider, r.host_url, r.full_name \
    FROM git_repository_bindings b JOIN git_repositories r ON r.id = b.repository_id \
    WHERE b.project_id = $1 AND b.deleted_at IS NULL AND r.deleted_at IS NULL \
    ORDER BY b.created_at ASC LIMIT 1";

/// One ancestor-chain hop (`context.py:55-75`): plain FK follow, no
/// soft-delete filtering. `$1` is the issue id.
pub const BUNDLE_ANCESTOR_HOP_SQL: &str =
    "SELECT name, project_id, parent_id FROM issues WHERE id = $1";

/// Chain-node project identifier (`context.py:35-43`). `$1` is the
/// project id.
pub const BUNDLE_PROJECT_IDENTIFIER_SQL: &str = "SELECT identifier FROM projects WHERE id = $1";

/// Chain-node sequence (`context.py:35-43`). `$1` is the issue id.
pub const BUNDLE_SEQUENCE_SQL: &str = "SELECT sequence_id FROM issues WHERE id = $1";

/// The direct parent's inlined columns (`context.py:582-593`) with its
/// live comment count. `$1` is the parent issue id.
pub const BUNDLE_PARENT_COLS_SQL: &str = "SELECT i.name, i.state_id, i.git_work_branch, \
    (SELECT COUNT(*) FROM issue_comments c WHERE c.issue_id = i.id AND c.deleted_at IS NULL) \
    FROM issues i WHERE i.id = $1";

/// The direct parent's description (`context.py:588`). `$1` is the
/// parent issue id.
pub const BUNDLE_PARENT_DESCRIPTION_SQL: &str =
    "SELECT description_stripped FROM issues WHERE id = $1";

/// Prior-run count for the attempt number (`context.py:730`): every run
/// on the issue except the new one. `$1` is the issue id, `$2` the new
/// run id.
pub const BUNDLE_PRIOR_RUN_COUNT_SQL: &str =
    "SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1 AND id != $2";

/// The ticker budget row (`context.py:341-398`). `$1` is the issue id.
pub const BUNDLE_TICKER_SQL: &str =
    "SELECT used, waited, granted, enabled FROM issue_agent_ticker WHERE issue_id = $1";

/// One run's done payload for the parent-payload pick
/// (`context.py:401-416`). `$1` is the run id.
pub const BUNDLE_DONE_PAYLOAD_SQL: &str = "SELECT done_payload FROM agent_run WHERE id = $1";

/// Active overrides for the compose index (`composer.py:96-109`): the
/// workspace rows plus, when a user applies, their rows. `$1` is the
/// workspace id, `$2` the optional user id (`= NULL` never matches, so
/// one shape serves both callers).
pub const BUNDLE_OVERRIDES_SQL: &str = "SELECT section_key, body, version, user_id \
    FROM prompt_section_override \
    WHERE is_active AND workspace_id = $1 AND (user_id IS NULL OR user_id = $2)";

/// The workspace slug + name (`context.py:567-570`). `$1` is the
/// workspace id.
pub const BUNDLE_WORKSPACE_SQL: &str = "SELECT slug, name FROM workspaces WHERE id = $1";

/// The INSERT as executed: [`RUN_INSERT_SQL`] plus `RETURNING` the
/// [`RUN_VIEW_COLUMNS`] projection, like `objects.create`.
pub fn run_insert_returning_sql() -> String {
    format!(
        "{} RETURNING {}",
        RUN_INSERT_SQL,
        RUN_VIEW_COLUMNS.join(", ")
    )
}

// ---------------------------------------------------------------------------
// Row views (plain data over the SQL above)
// ---------------------------------------------------------------------------

/// The consumed `issues` projection ([`ISSUE_SELECT_SQL`]).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// `None` only for the pathological project-less issue (R8 — the
    /// DB FK forbids it; the fixture uses an in-memory instance).
    pub project_id: Option<Uuid>,
    pub state_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub created_by_id: Option<Uuid>,
    pub assigned_pod_id: Option<Uuid>,
    /// `Issue.agent_executor`: NULL/blank inherits the project default.
    pub agent_executor: Option<String>,
    pub git_work_branch: Option<String>,
    pub workpad: Option<String>,
    pub name: Option<String>,
    pub description_stripped: Option<String>,
    pub priority: Option<String>,
    pub sequence_id: i32,
    pub target_date: Option<chrono::NaiveDate>,
}

/// The consumed `projects` projection ([`PROJECT_SELECT_SQL`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub identifier: String,
    pub name: String,
    pub description: Option<String>,
    pub repo_url: Option<String>,
    pub base_branch: Option<String>,
    pub default_agent_executor: String,
    pub project_lead_id: Option<Uuid>,
    pub default_assignee_id: Option<Uuid>,
    /// `agent_default_max_ticks` (`-1` is the infinite pool).
    pub pool: i64,
    pub interval_impl: i64,
    pub interval_review: i64,
    pub interval_test: i64,
}

/// The consumed `states` projection ([`STATE_SELECT_SQL`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateView {
    pub id: Uuid,
    pub name: String,
    pub group: String,
}

/// The consumed `agent_run` projection ([`RUN_VIEW_COLUMNS`]). The
/// trigger is the raw stored string, never parsed: Django's
/// `TextChoices` are choices-only (no DB check), so legacy or
/// hand-written rows can carry values outside
/// [`AgentRunTrigger`] (migration 0029) and every read path carries
/// them through.
#[derive(Debug, Clone, PartialEq)]
pub struct RunView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub created_by_id: Uuid,
    pub pod_id: Uuid,
    pub runner_id: Option<Uuid>,
    pub pinned_runner_id: Option<Uuid>,
    pub parent_run_id: Option<Uuid>,
    pub work_item_id: Option<Uuid>,
    pub status: AgentRunStatus,
    pub trigger: String,
    pub executor_kind: AgentExecutorKind,
    pub phase_kind: String,
    pub run_config: Value,
    pub tool_plan: Value,
    pub error_code: String,
    pub error: String,
    pub prompt: String,
    pub prompt_manifest: Option<Value>,
    pub ended_at: Option<DateTime<Utc>>,
}

impl RunView {
    /// `AgentRun.is_terminal` (`runner/models.py:1137-1144`).
    pub fn is_terminal(&self) -> bool {
        is_terminal_status(self.status)
    }
}

/// The consumed `runner` projection ([`RUNNER_SELECT_SQL`]). The status
/// stays a string: runner models are unmerged, so `revoked` is the
/// fixture-pinned [`RUNNER_STATUS_REVOKED`] literal (no D-13/D-15 edge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerView {
    pub id: Uuid,
    pub pod_id: Uuid,
    pub status: String,
}

/// The consumed `pod` projection (live rows only, per `PodManager`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodView {
    pub id: Uuid,
    pub project_id: Uuid,
}

/// The locked handoff bundle (`service.py:664-669` + the
/// `select_related` follows): the locked issue plus its project and
/// state.
#[derive(Debug, Clone, PartialEq)]
pub struct LockedIssue {
    pub issue: IssueView,
    pub project: ProjectView,
    pub state: Option<StateView>,
}

/// One `agent_run` INSERT ([`RUN_INSERT_SQL`]): the caller-supplied half
/// of the full-row write, in `$1..$14` order.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAgentRun {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub created_by_id: Uuid,
    pub pod_id: Uuid,
    pub pinned_runner_id: Option<Uuid>,
    pub work_item_id: Uuid,
    pub parent_run_id: Option<Uuid>,
    pub executor_kind: AgentExecutorKind,
    /// The `error_code` splat (`""` except the managed waiting path).
    pub error_code: String,
    pub tool_plan: Value,
    /// The raw stamped value: the handoff builder inherits the
    /// parent's verbatim (`service.py:612`), which may itself be
    /// outside [`AgentRunTrigger`].
    pub trigger: String,
    /// The handoff builder passes `""` (it stamps no kind).
    pub phase_kind: String,
    pub run_config: Value,
    /// `created_at` (`auto_now_add`, supplied explicitly).
    pub now: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Executor DTOs (services-side mirror of the D-11 shapes)
// ---------------------------------------------------------------------------

/// The deferred quota refusal (`{"code", "detail"}` — no retry-after,
/// `creation.py:51,70,175`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionError {
    pub code: String,
    pub detail: String,
}

/// The executor-specific fields for one creation (`execution_fields`,
/// `creation.py:11-89`): mirrors `jobs::dispatch::ExecutionFields`
/// without importing jobs.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionFields {
    pub executor_kind: AgentExecutorKind,
    pub tool_plan: Value,
    /// The `pinned_runner` dict entry: `None` when `execution_fields`
    /// returned no such key (local path — the computed pin survives
    /// the `pop` default), `Some(pin)` otherwise (cloud: always
    /// `None`, clobbering the computed pin per the cloud CHECK
    /// constraint; managed: the pick). The outer `Option` is the
    /// key presence the `pop` observes.
    pub pinned_runner_entry: Option<Option<Uuid>>,
    /// `desktop_not_connected` on the managed automatic-waiting path
    /// only — a real column the sweep reads back.
    pub error_code: Option<String>,
    /// The deferred quota refusal (not a column).
    pub cloud_admission_error: Option<AdmissionError>,
}

impl ExecutionFields {
    /// The local-runner fields (`creation.py:89`): no pin key, no
    /// error, no admission refusal.
    pub fn local(tool_plan: Value) -> Self {
        Self {
            executor_kind: AgentExecutorKind::LocalRunner,
            tool_plan,
            pinned_runner_entry: None,
            error_code: None,
            cloud_admission_error: None,
        }
    }

    /// Merge the pin exactly like
    /// `execution.pop("pinned_runner", pinned_runner)`: a present key
    /// wins (even `None`), an absent key keeps the computed pin.
    pub fn merge_pin(&self, computed: Option<Uuid>) -> Option<Uuid> {
        self.pinned_runner_entry.unwrap_or(computed)
    }
}

/// The creator half of an `execution_fields` call: `None` is the
/// anonymous caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorRequest {
    pub id: Uuid,
    pub flags: UserFlags,
}

/// One `execution_fields` call (`creation.py:11-20`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub default_agent_executor: String,
    pub run_kind: String,
    pub has_issue: bool,
    pub actor: Option<ActorRequest>,
    pub automatic: bool,
    /// `Issue.agent_executor` (`None`/blank inherits the default).
    pub requested: Option<String>,
}

// ---------------------------------------------------------------------------
// Render bundle (preloaded prompt inputs)
// ---------------------------------------------------------------------------

/// The ticker budget row (`context.py:341-398`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickerBudget {
    pub used: i64,
    pub waited: i64,
    pub granted: i64,
    pub enabled: bool,
}

/// Every preloaded input [`render_first_turn`] needs: the api
/// prompt-preview fetch side (`api/src/prompting`, `load_issue_context`)
/// as data. The store fills it with the `BUNDLE_*_SQL` queries;
/// assembly stays here.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderBundle {
    pub issue: IssueView,
    pub project: ProjectView,
    pub workspace_slug: String,
    pub workspace_name: String,
    pub state: Option<StateView>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub project_states: Vec<context::ProjectStateView>,
    pub children: Vec<context::IssueRef>,
    pub relation_rows: Vec<context::RelationRow>,
    pub refs: HashMap<String, context::IssueRef>,
    pub directional_refs: HashMap<String, context::DirectionalRef>,
    pub comments: Vec<context::CommentView>,
    pub reviews: Vec<context::CodeReviewView>,
    pub remote: Option<context::RemoteView>,
    pub adapter: Option<context::AdapterNames>,
    /// The direct parent inlined (`context.py:582-593`); the loader
    /// walks `[issue, parent, … root]` with the cycle guard + depth-50
    /// cap (`context.py:55-75`) and inlines node 2.
    pub parent: Option<context::ParentView>,
    /// The full trail, only past a grandparent (`context.py:600-604`).
    pub lineage: Vec<context::LineageNode>,
    pub prior_run_count: i64,
    /// `None` when the issue never armed a ticker.
    pub ticker: Option<TickerBudget>,
    /// The new run's direct parent `done_payload` (by id, may be absent).
    pub direct_parent_payload: Option<Value>,
    /// The ticker-stashed resume parent `done_payload` (may be absent).
    pub ticker_parent_payload: Option<Value>,
    pub override_rows: Vec<composer::OverrideRow>,
}

/// The run half of a render call.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRenderRef {
    pub run_id: Uuid,
    pub parent_run_id: Option<Uuid>,
    /// The raw stored trigger (`context.py:627` passes it to the
    /// template verbatim).
    pub trigger: String,
    pub executor_kind: AgentExecutorKind,
    pub tool_plan: Value,
    pub created_by_id: Uuid,
}

/// The rendered turn: prompt text plus the JSON manifest stamp.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTurn {
    pub text: String,
    pub manifest: Value,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure the creation drivers report.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CreationError {
    /// Any database failure (the store stringifies its `sqlx::Error` —
    /// this crate carries no `sqlx`).
    #[error("database error: {0}")]
    Db(String),
    /// A manual over-capacity `lock_cloud_creation_capacity` raise
    /// (`creation.py:173-174`): uncaught in Python, so it propagates.
    /// Carries `str(exc)` verbatim.
    #[error("cloud capacity refused: {0}")]
    CapacityRefused(String),
    /// A bare FK follow hit a dangling id (`RelatedObjectDoesNotExist`).
    #[error("missing row: {0}")]
    MissingRow(String),
}

/// An `execution_fields` failure: a refusal for the
/// `(ValueError, RuntimeError)` catch, or a store failure that
/// propagates (a DB error is not in the caught tuple).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ExecutionError {
    /// `str(exc)` verbatim.
    #[error("{0}")]
    Refused(String),
    /// Any database failure.
    #[error(transparent)]
    Store(#[from] CreationError),
}

// ---------------------------------------------------------------------------
// Pure units
// ---------------------------------------------------------------------------

/// The prompt kind a run created for the issue *now* will render
/// (`_phase_kind_for_issue`, `service.py:157-167`): stamped on
/// `AgentRun.phase_kind`.
pub fn phase_kind_for_issue(state: Option<StateView>) -> String {
    let as_ref = state.as_ref().map(|state| StateRef {
        group: &state.group,
        name: &state.name,
    });
    let template = pidash_types::orchestration::template_name_for(as_ref.as_ref());
    recipes::kind_for(template, recipes::WORK_KIND_CODING).to_owned()
}

/// A run snapshot refreshed for the issue's current project
/// (`_run_config_for_issue`, `service.py:550-566`): model/approval
/// overrides survive, project-derived repo fields are replaced, and
/// the private handoff marker is stripped. `base` is never mutated.
pub fn run_config_for_issue(
    repo_url: Option<&str>,
    repo_ref: Option<&str>,
    git_work_branch: Option<&str>,
    base: Option<&Value>,
) -> Value {
    let mut config: Map<String, Value> =
        base.and_then(Value::as_object).cloned().unwrap_or_default();
    config.remove(PROJECT_MOVE_HANDOFF_CONFIG_KEY);
    let or_null = |value: Option<&str>| match value {
        Some(text) if !text.is_empty() => Value::String(text.to_owned()),
        _ => Value::Null,
    };
    config.insert("repo_url".to_owned(), or_null(repo_url));
    config.insert("repo_ref".to_owned(), or_null(repo_ref));
    config.insert("git_work_branch".to_owned(), or_null(git_work_branch));
    Value::Object(config)
}

/// The runner to pin a follow-up to (`_pinned_runner_for`,
/// `service.py:528-547`): the parent's runner when still eligible.
/// `runner` is `None` when the parent has no runner; `target_pod_id`
/// is the run's pod (`None` skips the pod check).
pub fn pinned_runner_for(runner: Option<&RunnerView>, target_pod_id: Option<Uuid>) -> Option<Uuid> {
    let runner = runner?;
    if runner.status == RUNNER_STATUS_REVOKED {
        return None;
    }
    if target_pod_id.is_some_and(|pod| runner.pod_id != pod) {
        return None;
    }
    Some(runner.id)
}

/// The latest-run input to [`select_parent_for_next_run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentCandidate<'a> {
    pub id: Uuid,
    pub phase_kind: &'a str,
}

/// `(parent, fresh_session)` for a run created on the issue *now*
/// (`parent_for_next_run`, `service.py:110-154`): same stage → the
/// latest run; `fresh_session_on_entry` stage → no parent; hand-back
/// into In Progress → the ticker-captured implementation run, else a
/// fresh session.
pub fn select_parent_for_next_run(
    latest: Option<ParentCandidate<'_>>,
    state: Option<StateView>,
    cross_stage: Option<bool>,
    resume_parent_run_id: Option<Uuid>,
) -> (Option<Uuid>, bool) {
    let Some(latest) = latest else {
        return (None, true);
    };
    let as_ref = state.as_ref().map(|state| StateRef {
        group: &state.group,
        name: &state.name,
    });
    let cfg: Option<&PhaseConfig> = pidash_types::orchestration::phase_config_for(as_ref.as_ref());
    let Some(cfg) = cfg else {
        return (Some(latest.id), false);
    };
    let cross_stage = cross_stage.unwrap_or_else(|| {
        let latest_kind = if latest.phase_kind.is_empty() {
            recipes::KIND_CODING_TASK
        } else {
            latest.phase_kind
        };
        let template = pidash_types::orchestration::template_name_for(as_ref.as_ref());
        latest_kind != recipes::kind_for(template, recipes::WORK_KIND_CODING)
    });
    if !cross_stage {
        return (Some(latest.id), false);
    }
    if cfg.fresh_session_on_entry {
        return (None, true);
    }
    if let Some(resume) = resume_parent_run_id {
        return (Some(resume), false);
    }
    (None, true)
}

/// The fallback `created_by` chain (`_resolve_fallback_creator`,
/// `service.py:303-316`): issue creator → project lead → default
/// assignee → `None`.
pub fn fallback_creator_select(
    created_by_id: Option<Uuid>,
    project_lead_id: Option<Uuid>,
    default_assignee_id: Option<Uuid>,
) -> Option<Uuid> {
    created_by_id.or(project_lead_id).or(default_assignee_id)
}

/// The pod pick (`_resolve_pod_for_issue`, `service.py:319-338`):
/// the live assigned pod when the fetch hit, else the project
/// default. The caller returns `None` without fetching when the
/// issue has no project.
pub fn resolve_pod_select(assigned: Option<&PodView>, default: Option<&PodView>) -> Option<Uuid> {
    assigned.or(default).map(|pod| pod.id)
}

/// The handoff marker on a source run's config (`service.py:682-685`):
/// `{}` / missing / null → `None`.
pub fn handoff_marker(run_config: &Value) -> Option<Map<String, Value>> {
    let marker = run_config
        .get(PROJECT_MOVE_HANDOFF_CONFIG_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    (!marker.is_empty()).then_some(marker)
}

/// Stamp one marker key back onto a source config (the
/// `replacement_run_id` / `suppressed` writes, `service.py:693-696,
/// :704-707, :728-731`).
pub fn stamp_handoff_marker(run_config: &Value, key: &str, value: Value) -> Value {
    let mut config = run_config.as_object().cloned().unwrap_or_default();
    let mut marker = config
        .get(PROJECT_MOVE_HANDOFF_CONFIG_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    marker.insert(key.to_owned(), value);
    config.insert(
        PROJECT_MOVE_HANDOFF_CONFIG_KEY.to_owned(),
        Value::Object(marker),
    );
    Value::Object(config)
}

// ---------------------------------------------------------------------------
// Render assembly (services-side; the seam preloads the bundle)
// ---------------------------------------------------------------------------

/// The issue's budget pool + clock (`_tick_context`,
/// `context.py:341-398`): cap = pool + granted + waited (`-1` is
/// infinite), interval through the current phase's column. `None`
/// (JSON null) when no ticker row exists.
pub fn tick_value(
    ticker: Option<&TickerBudget>,
    pool: i64,
    interval_impl: i64,
    interval_review: i64,
    interval_test: i64,
    state: Option<&StateView>,
) -> Option<Value> {
    let ticker = ticker?;
    let cap = if pool == -1 {
        context::TickCap::Infinite
    } else {
        context::TickCap::Finite(pool + ticker.granted + ticker.waited)
    };
    let wait_allowance = if pool == -1 {
        0
    } else {
        (pool - ticker.waited).max(0)
    };
    // `cadence_fields_for`: the registered ticking state's column, else
    // the implementation default (`agent_phases.py:191-200`).
    let interval_seconds = match state {
        Some(state) if state.group == "started" && state.name == "In Progress" => interval_impl,
        Some(state) if state.group == "review" && state.name == "In Review" => interval_review,
        Some(state) if state.group == "test" && state.name == "In Test" => interval_test,
        _ => interval_impl,
    };
    context::tick_context(&context::TickerView {
        used: ticker.used,
        waited: ticker.waited,
        enabled: ticker.enabled,
        cap,
        wait_allowance,
        interval_seconds,
    })
}

/// The review prompt's implementation payload (`context.py:401-416`):
/// the run's own parent, else the ticker-stashed resume parent.
pub fn parent_done_payload(
    direct_parent_payload: Option<&Value>,
    ticker_parent_payload: Option<&Value>,
) -> String {
    let picked = context::resolve_parent_payload(direct_parent_payload, ticker_parent_payload);
    context::parent_done_payload_json(picked)
}

/// Render the prompt for the run executing the issue
/// (`build_first_turn`, `composer.py:383-415`): assemble the context
/// from the preloaded bundle, compose (cloud runs take the
/// defaults-only path with a versioned manifest), and return the text
/// plus the manifest stamp. `Err` carries the composer message —
/// the `str(exc)` of the `_PROMPT_BUILD_ERRORS` catch.
/// `extra_toolsets_schema_tool` is the EE seam value, consulted by the
/// caller only when the plan enables extra toolsets.
pub fn render_first_turn(
    bundle: &RenderBundle,
    kind: &str,
    run: &RunRenderRef,
    extra_toolsets_schema_tool: &str,
) -> Result<RenderedTurn, String> {
    let issue = &bundle.issue;
    let project = &bundle.project;
    let related =
        context::related_context(&issue.id.to_string(), &bundle.relation_rows, &bundle.refs);
    let by_type = context::directional_relations_context(
        &issue.id.to_string(),
        &bundle.relation_rows,
        &bundle.directional_refs,
    );
    let relations = context::relations_context(&by_type);
    let repo = context::repo_context(
        &context::ProjectRepoView {
            url: project.repo_url.clone(),
            base_branch: project.base_branch.clone(),
        },
        issue.git_work_branch.as_deref(),
        bundle.remote.as_ref(),
        bundle.adapter.as_ref(),
    );
    let input = context::IssueContextInput {
        issue_id: issue.id.to_string(),
        project_identifier: project.identifier.clone(),
        sequence_id: i64::from(issue.sequence_id),
        title: issue.name.clone(),
        description_stripped: issue.description_stripped.clone(),
        state_name: bundle.state.as_ref().map(|state| state.name.clone()),
        state_group: bundle.state.as_ref().map(|state| state.group.clone()),
        priority: issue.priority.clone(),
        labels: bundle.labels.clone(),
        assignees: bundle.assignees.clone(),
        target_date_iso: issue
            .target_date
            .map(|date| date.format("%Y-%m-%d").to_string()),
        project_states: bundle.project_states.clone(),
        workspace_slug: bundle.workspace_slug.clone(),
        workspace_name: bundle.workspace_name.clone(),
        project_id: project.id.to_string(),
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        repo,
        code_reviews: context::code_reviews_context(&bundle.reviews),
        parent: bundle.parent.clone(),
        ancestors: bundle.lineage.clone(),
        children: Value::Array(context::children_context(&bundle.children)),
        related: Value::Array(related),
        relations,
        run_id: run.run_id.to_string(),
        run_template_name: kind.to_owned(),
        attempt: context::compute_attempt(bundle.prior_run_count),
        trigger: Some(run.trigger.clone()),
        executor_kind: run.executor_kind.value().to_owned(),
        available_tools: run.tool_plan.get("tools").cloned().unwrap_or(Value::Null),
        unavailable_capabilities: run
            .tool_plan
            .get("unavailable_capabilities")
            .cloned()
            .unwrap_or(Value::Null),
        extra_toolsets_enabled: run
            .tool_plan
            .get("extra_toolsets")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        // Named by the EE seam, consulted only when enabled
        // (`context.py:514-517`); CE names none.
        extra_toolsets_schema_tool: extra_toolsets_schema_tool.to_owned(),
        limits: run.tool_plan.get("limits").cloned().unwrap_or(Value::Null),
        tick: tick_value(
            bundle.ticker.as_ref(),
            project.pool,
            project.interval_impl,
            project.interval_review,
            project.interval_test,
            bundle.state.as_ref(),
        ),
        comments_section: context::comments_section(&bundle.comments),
        parent_done_payload: parent_done_payload(
            bundle.direct_parent_payload.as_ref(),
            bundle.ticker_parent_payload.as_ref(),
        ),
        workpad_body: issue.workpad.clone().unwrap_or_default(),
    };
    let context_value = context::build_context(&input);
    // Override keys are workspace/user UUID strings (`load_index`),
    // not the slug.
    let workspace_id = issue.workspace_id.to_string();
    let user_id = user_id_for_run(&run.trigger, run.created_by_id);
    let tool_catalog_version = run
        .tool_plan
        .get("catalog_version")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    // `getattr(run, "executor_kind", None)` (`composer.py:412`): creation
    // runs always carry the column, so this is always `Some`.
    let executor_kind = Some(run.executor_kind.value());
    let turn = context::build_first_turn(
        kind,
        &context_value,
        &composer::build_override_index(
            Some(workspace_id.as_str()),
            user_id.as_deref(),
            &bundle.override_rows,
        ),
        Some(workspace_id.as_str()),
        user_id.as_deref(),
        executor_kind,
        tool_catalog_version,
    )
    .map_err(|error| error.message().to_owned())?;
    let manifest = turn
        .manifest
        .as_ref()
        .map(|manifest| manifest.to_json())
        .unwrap_or(Value::Null);
    Ok(RenderedTurn {
        text: turn.text,
        manifest,
    })
}

// ---------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------

/// Storage + executor seam for the creation drivers.
///
/// Methods mirror the Django ORM calls in `service.py`, one per query
/// shape; the SQL text for each lives in the adjacent `*_SQL` consts,
/// which the pool implementation must execute verbatim. The three
/// `cloud_agent.creation` calls are methods here (the jobs-side store
/// delegates them to the merged `jobs::dispatch` functions) so the
/// Python control flow stays services-side in verbatim order.
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam.
#[allow(async_fn_in_trait)]
pub trait CreationSeam {
    /// The issue row ([`ISSUE_SELECT_SQL`]).
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError>;
    /// The project row ([`PROJECT_SELECT_SQL`]).
    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError>;
    /// The state row ([`STATE_SELECT_SQL`]), or `None` when the id is
    /// `None` (a set id with no row is [`CreationError::MissingRow`],
    /// the dangling-FK raise).
    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError>;
    /// `_latest_prior_run` ([`latest_prior_run_sql`]).
    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError>;
    /// `_active_run_for` ([`active_run_sql`]).
    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError>;
    /// One unlocked run row ([`run_select_sql`]).
    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError>;
    /// The parent's runner ([`RUNNER_SELECT_SQL`]).
    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError>;
    /// The live assigned pod ([`ASSIGNED_POD_SELECT_SQL`]).
    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError>;
    /// The live project-default pod ([`DEFAULT_POD_SELECT_SQL`]).
    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError>;
    /// The ticker's `resume_parent_run_id` ([`TICKER_RESUME_SELECT_SQL`]):
    /// `None` covers both a NULL pointer and no ticker row.
    async fn resume_parent_run_id(&mut self, issue_id: Uuid)
        -> Result<Option<Uuid>, CreationError>;
    /// The unlocked work-item id ([`WORK_ITEM_ID_SELECT_SQL`]).
    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError>;
    /// The handoff issue lock + bundle ([`ISSUE_LOCK_SQL`], issue → run
    /// order; `all_objects`, no deleted guard).
    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError>;
    /// The handoff source-run lock ([`run_lock_sql`]).
    async fn lock_run_for_handoff(
        &mut self,
        run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError>;
    /// Creator flags for the executor seam ([`USER_FLAGS_SELECT_SQL`]).
    async fn user_flags(&mut self, user_id: Uuid) -> Result<UserFlags, CreationError>;
    /// The full-row INSERT ([`RUN_INSERT_SQL`] + `RETURNING` the
    /// [`RUN_VIEW_COLUMNS`] projection, like `objects.create`).
    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError>;
    /// `run.save(update_fields=["prompt", "prompt_manifest"])`
    /// ([`PROMPT_UPDATE_SQL`]).
    async fn save_prompt(
        &mut self,
        run_id: Uuid,
        prompt: &str,
        manifest: &Value,
    ) -> Result<(), CreationError>;
    /// `source.save(update_fields=["run_config"])`
    /// ([`RUN_CONFIG_UPDATE_SQL`]).
    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError>;
    /// `execution_fields` (`creation.py:11-89`). [`ExecutionError::Refused`]
    /// carries `str(exc)` verbatim for the `(ValueError, RuntimeError)`
    /// catch; [`ExecutionError::Store`] propagates.
    async fn execution_fields(
        &mut self,
        req: &ExecutionRequest,
    ) -> Result<ExecutionFields, ExecutionError>;
    /// `lock_cloud_creation_capacity` (`creation.py:155-177`) on the
    /// caller's insertion transaction. A manual over-capacity raise
    /// surfaces as [`CreationError::CapacityRefused`] and propagates.
    async fn lock_cloud_creation_capacity(
        &mut self,
        workspace_id: Uuid,
        executor_kind: AgentExecutorKind,
        automatic: bool,
    ) -> Result<Option<AdmissionError>, CreationError>;
    /// `dispatch_after_commit` (`creation.py:148-152`): collect the run
    /// id for post-commit dispatch. Nothing executes inline.
    fn dispatch_after_commit(&mut self, run_id: Uuid);
    /// Every preloaded prompt input for the new run (the `BUNDLE_*_SQL`
    /// queries). `parent_run_id` feeds the direct-parent payload pick;
    /// `trigger` + `created_by_id` feed the override user pick
    /// (`user_id_for_run`) without an extra run read.
    async fn render_bundle(
        &mut self,
        issue_id: Uuid,
        run_id: Uuid,
        parent_run_id: Option<Uuid>,
        trigger: &str,
        created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError>;
    /// The EE schema-tool name (`extra_toolsets_schema_tool`,
    /// `context.py:512-517`): consulted only when the run's plan
    /// enables extra toolsets. CE names none.
    fn extra_toolsets_schema_tool(&self) -> String;
}

/// The one-method seam for the `finalize_agent_run(run.id, FAILED, …)`
/// call sites in this module (`agent_run_finalization.py:48-86`).
/// Finalization is runner D-15 (downstream of D-12 — no edge, it would
/// deadlock): D-15 implements this trait later, and L7 reuses it via
/// its L6 edge. The jobs-side store implements the row effects now
/// (status/ended_at/NULLs + updates, the cloud terminal event, the
/// terminal-effects capture) without executing terminal effects.
#[allow(async_fn_in_trait)]
pub trait FinalizeAgentRunSeam {
    /// Mark the run FAILED with the updates and return the refreshed
    /// row (`finalize_agent_run` + `refresh_from_db`). The Python
    /// ignores the first-writer-wins verdict and refreshes anyway —
    /// so does this: an already-terminal row is returned as-is.
    async fn finalize_failed_run(
        &mut self,
        run_id: Uuid,
        error_code: &str,
        error: &str,
        now: DateTime<Utc>,
    ) -> Result<RunView, CreationError>;
}

// ---------------------------------------------------------------------------
// Drivers (Python control flow, verbatim order)
// ---------------------------------------------------------------------------

/// One `_create_and_dispatch_run` call (`service.py:735-743`).
#[derive(Debug, Clone, PartialEq)]
pub struct CreateDispatchRequest {
    pub issue_id: Uuid,
    /// The pre-resolved parent (`parent_for_next_run`), `None` for a
    /// first run.
    pub parent: Option<RunView>,
    pub creator_id: Uuid,
    pub pod_id: Uuid,
    pub fresh_session: bool,
    /// The trigger to stamp, carried verbatim like Django's
    /// `trigger: str` (`service.py:742`): usually a member value,
    /// but the dispatch path forwards the stored `triggered_by`
    /// string, which may sit outside [`AgentRunTrigger`].
    pub trigger: String,
    /// The clock for `created_at` / `ended_at` (frozen in tests).
    pub now: DateTime<Utc>,
}

/// One `_create_continuation_run` call (`service.py:443`).
#[derive(Debug, Clone, PartialEq)]
pub struct ContinuationRequest {
    pub issue_id: Uuid,
    pub parent: RunView,
    pub creator_id: Uuid,
    pub pod_id: Uuid,
    /// The trigger to stamp, carried verbatim like Django's
    /// `trigger: str` (`service.py:443`): usually a member value,
    /// but the dispatch path forwards the stored `triggered_by`
    /// string, which may sit outside [`AgentRunTrigger`].
    pub trigger: String,
    /// The clock for `created_at` / `ended_at` (frozen in tests).
    pub now: DateTime<Utc>,
}

/// One `_create_project_move_handoff_run` call (`service.py:569`).
#[derive(Debug, Clone, PartialEq)]
pub struct HandoffCreateRequest {
    pub issue_id: Uuid,
    /// The stopped source run (lineage parent + creator donor).
    pub parent: RunView,
    pub pod_id: Uuid,
    /// The clock for `created_at` / `ended_at` (frozen in tests).
    pub now: DateTime<Utc>,
}

/// Fail the run with `prompt_build_failed` and map to the
/// `render-failed` reason (`service.py:506-521, :625-638, :810-822`).
async fn fail_render<S: FinalizeAgentRunSeam>(
    fin: &mut S,
    run_id: Uuid,
    message: &str,
    now: DateTime<Utc>,
) -> Result<RunView, CreationError> {
    fin.finalize_failed_run(
        run_id,
        ERROR_CODE_PROMPT_BUILD_FAILED,
        &format!("prompt build failed: {message}"),
        now,
    )
    .await
}

/// Render the new run's first turn through the preloaded bundle
/// (`run.prompt = build_first_turn(issue, run)`). The outer error is a
/// store failure (propagates, like an ORM error outside the
/// `_PROMPT_BUILD_ERRORS` catch); the inner `Err` is the composer
/// message for the `render-failed` path.
async fn render_new_run<C: CreationSeam>(
    seam: &mut C,
    issue_id: Uuid,
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
    kind: &str,
    run_ref: &RunRenderRef,
) -> Result<Result<RenderedTurn, String>, CreationError> {
    let bundle = seam
        .render_bundle(
            issue_id,
            run_id,
            parent_run_id,
            &run_ref.trigger,
            run_ref.created_by_id,
        )
        .await?;
    let schema_tool = if run_ref
        .tool_plan
        .get("extra_toolsets")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        seam.extra_toolsets_schema_tool()
    } else {
        String::new()
    };
    Ok(render_first_turn(&bundle, kind, run_ref, &schema_tool))
}

/// Create a fresh `AgentRun` and dispatch it
/// (`_create_and_dispatch_run`, `service.py:735-830`).
///
/// `fresh_session=True` clears `parent_run` and `pinned_runner` so the
/// new phase's template body lands as the system prompt of a brand-new
/// agent session.
pub async fn create_and_dispatch_run<S: CreationSeam + FinalizeAgentRunSeam>(
    seam: &mut S,
    req: &CreateDispatchRequest,
) -> Result<TransitionOutcome, CreationError> {
    let issue = seam.issue(req.issue_id).await?;
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let project = seam.project(project_id).await?;
    let automatic = is_automatic_issue_trigger(&req.trigger);
    let flags = seam.user_flags(req.creator_id).await?;
    let execution = match seam
        .execution_fields(&ExecutionRequest {
            project_id: project.id,
            workspace_id: project.workspace_id,
            default_agent_executor: project.default_agent_executor.clone(),
            run_kind: "issue".to_owned(),
            has_issue: true,
            actor: Some(ActorRequest {
                id: req.creator_id,
                flags,
            }),
            automatic,
            requested: issue.agent_executor.clone(),
        })
        .await
    {
        Ok(execution) => execution,
        Err(ExecutionError::Refused(message)) => {
            return Ok(TransitionOutcome {
                created_run: None,
                reason: message,
            })
        }
        Err(ExecutionError::Store(error)) => return Err(error),
    };
    let mut admission_error = execution.cloud_admission_error.clone();
    admission_error = seam
        .lock_cloud_creation_capacity(project.workspace_id, execution.executor_kind, automatic)
        .await?
        .or(admission_error);
    let effective_parent = if req.fresh_session {
        None
    } else {
        req.parent.clone()
    };
    let mut computed_pin = None;
    if !req.fresh_session {
        if let Some(parent) = effective_parent.as_ref() {
            if let Some(runner_id) = parent.runner_id {
                let runner = seam.runner(runner_id).await?;
                computed_pin = pinned_runner_for(runner.as_ref(), Some(req.pod_id));
            }
        }
    }
    let pinned_runner = execution.merge_pin(computed_pin);
    let state = seam.state(issue.state_id).await?;
    let kind = phase_kind_for_issue(state);
    let run_id = Uuid::new_v4();
    let run = seam
        .insert_run(&NewAgentRun {
            id: run_id,
            workspace_id: issue.workspace_id,
            created_by_id: req.creator_id,
            pod_id: req.pod_id,
            pinned_runner_id: pinned_runner,
            work_item_id: issue.id,
            parent_run_id: effective_parent.as_ref().map(|parent| parent.id),
            executor_kind: execution.executor_kind,
            error_code: execution.error_code.clone().unwrap_or_default(),
            tool_plan: execution.tool_plan.clone(),
            trigger: req.trigger.clone(),
            phase_kind: kind.clone(),
            run_config: run_config_for_issue(
                project.repo_url.as_deref(),
                project.base_branch.as_deref(),
                issue.git_work_branch.as_deref(),
                None,
            ),
            now: req.now,
        })
        .await?;
    if let Some(admission) = admission_error {
        let failed = seam
            .finalize_failed_run(run.id, &admission.code, &admission.detail, req.now)
            .await?;
        return Ok(TransitionOutcome {
            created_run: Some(failed.id),
            reason: admission.code,
        });
    }
    let turn = match render_new_run(
        seam,
        issue.id,
        run.id,
        run.parent_run_id,
        &kind,
        &RunRenderRef {
            run_id: run.id,
            parent_run_id: run.parent_run_id,
            trigger: run.trigger.clone(),
            executor_kind: run.executor_kind,
            tool_plan: run.tool_plan.clone(),
            created_by_id: run.created_by_id,
        },
    )
    .await?
    {
        Ok(turn) => turn,
        Err(message) => {
            let failed = fail_render(seam, run.id, &message, req.now).await?;
            return Ok(TransitionOutcome {
                created_run: Some(failed.id),
                reason: REASON_RENDER_FAILED.to_owned(),
            });
        }
    };
    seam.save_prompt(run.id, &turn.text, &turn.manifest).await?;
    seam.dispatch_after_commit(run.id);
    Ok(TransitionOutcome {
        created_run: Some(run.id),
        reason: REASON_CREATED.to_owned(),
    })
}

/// Create R_next as a follow-up to `parent` with optional pin
/// (`_create_continuation_run`, `service.py:443-525`).
///
/// "Continuation" is a historical name: the new run renders a
/// self-sufficient full prompt on a fresh agent session — `parent` is
/// lineage / pin selection only.
pub async fn create_continuation_run<S: CreationSeam + FinalizeAgentRunSeam>(
    seam: &mut S,
    req: &ContinuationRequest,
) -> Result<ContinuationOutcome, CreationError> {
    let issue = seam.issue(req.issue_id).await?;
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let project = seam.project(project_id).await?;
    let mut computed_pin = None;
    if let Some(runner_id) = req.parent.runner_id {
        let runner = seam.runner(runner_id).await?;
        computed_pin = pinned_runner_for(runner.as_ref(), Some(req.pod_id));
    }
    let automatic = is_automatic_issue_trigger(&req.trigger);
    let flags = seam.user_flags(req.creator_id).await?;
    let execution = match seam
        .execution_fields(&ExecutionRequest {
            project_id: project.id,
            workspace_id: project.workspace_id,
            default_agent_executor: project.default_agent_executor.clone(),
            run_kind: "issue".to_owned(),
            has_issue: true,
            actor: Some(ActorRequest {
                id: req.creator_id,
                flags,
            }),
            automatic,
            requested: issue.agent_executor.clone(),
        })
        .await
    {
        Ok(execution) => execution,
        Err(ExecutionError::Refused(message)) => {
            return Ok(ContinuationOutcome {
                created_run: None,
                coalesced_into: None,
                reason: message,
            })
        }
        Err(ExecutionError::Store(error)) => return Err(error),
    };
    let mut admission_error = execution.cloud_admission_error.clone();
    admission_error = seam
        .lock_cloud_creation_capacity(project.workspace_id, execution.executor_kind, automatic)
        .await?
        .or(admission_error);
    // Continuation inserts the literal repo snapshot, not the
    // `_run_config_for_issue` refresh (`service.py:489-493`): same
    // three keys, no base to strip.
    let run_config = run_config_for_issue(
        project.repo_url.as_deref(),
        project.base_branch.as_deref(),
        issue.git_work_branch.as_deref(),
        None,
    );
    let state = seam.state(issue.state_id).await?;
    let kind = phase_kind_for_issue(state);
    let run_id = Uuid::new_v4();
    let run = seam
        .insert_run(&NewAgentRun {
            id: run_id,
            workspace_id: issue.workspace_id,
            created_by_id: req.creator_id,
            pod_id: req.pod_id,
            pinned_runner_id: execution.merge_pin(computed_pin),
            work_item_id: issue.id,
            parent_run_id: Some(req.parent.id),
            executor_kind: execution.executor_kind,
            error_code: execution.error_code.clone().unwrap_or_default(),
            tool_plan: execution.tool_plan.clone(),
            trigger: req.trigger.clone(),
            phase_kind: kind.clone(),
            run_config,
            now: req.now,
        })
        .await?;
    if let Some(admission) = admission_error {
        let failed = seam
            .finalize_failed_run(run.id, &admission.code, &admission.detail, req.now)
            .await?;
        return Ok(ContinuationOutcome {
            created_run: Some(failed.id),
            coalesced_into: None,
            reason: admission.code,
        });
    }
    let turn = match render_new_run(
        seam,
        issue.id,
        run.id,
        run.parent_run_id,
        &kind,
        &RunRenderRef {
            run_id: run.id,
            parent_run_id: run.parent_run_id,
            trigger: run.trigger.clone(),
            executor_kind: run.executor_kind,
            tool_plan: run.tool_plan.clone(),
            created_by_id: run.created_by_id,
        },
    )
    .await?
    {
        Ok(turn) => turn,
        Err(message) => {
            let failed = fail_render(seam, run.id, &message, req.now).await?;
            return Ok(ContinuationOutcome {
                created_run: Some(failed.id),
                coalesced_into: None,
                reason: REASON_RENDER_FAILED.to_owned(),
            });
        }
    };
    seam.save_prompt(run.id, &turn.text, &turn.manifest).await?;
    seam.dispatch_after_commit(run.id);
    Ok(ContinuationOutcome {
        created_run: Some(run.id),
        coalesced_into: None,
        reason: REASON_CREATED.to_owned(),
    })
}

/// Create a fresh target-project run after the source run has stopped
/// (`_create_project_move_handoff_run`, `service.py:569-641`).
///
/// The source run stays the lineage parent for audit/history only;
/// runner affinity is cleared (a source-pod runner can never consume
/// a target-pod run) while a managed pin for the destination is
/// preserved. Unlike the other builders this stamps no `phase_kind`.
pub async fn create_project_move_handoff_run<S: CreationSeam + FinalizeAgentRunSeam>(
    seam: &mut S,
    req: &HandoffCreateRequest,
) -> Result<RunView, CreationError> {
    let issue = seam.issue(req.issue_id).await?;
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let project = seam.project(project_id).await?;
    let flags = seam.user_flags(req.parent.created_by_id).await?;
    // The executor seam falls back to a local row here (unlike the
    // other builders, which surface the reason): the handoff must
    // land somewhere visible (`service.py:591-596`).
    let execution = seam
        .execution_fields(&ExecutionRequest {
            project_id: project.id,
            workspace_id: project.workspace_id,
            default_agent_executor: project.default_agent_executor.clone(),
            run_kind: "issue".to_owned(),
            has_issue: true,
            actor: Some(ActorRequest {
                id: req.parent.created_by_id,
                flags,
            }),
            automatic: true,
            requested: issue.agent_executor.clone(),
        })
        .await;
    let execution = match execution {
        Ok(execution) => execution,
        Err(ExecutionError::Refused(_)) => ExecutionFields::local(Value::Object(Map::new())),
        Err(ExecutionError::Store(error)) => return Err(error),
    };
    let admission_error = execution.cloud_admission_error.clone();
    let pinned_runner = execution.merge_pin(None);
    if let Some(existing) = seam.active_run_for(issue.id).await? {
        return Ok(existing);
    }
    let run_config = run_config_for_issue(
        project.repo_url.as_deref(),
        project.base_branch.as_deref(),
        issue.git_work_branch.as_deref(),
        Some(&req.parent.run_config),
    );
    let run_id = Uuid::new_v4();
    let run = seam
        .insert_run(&NewAgentRun {
            id: run_id,
            workspace_id: issue.workspace_id,
            created_by_id: req.parent.created_by_id,
            pod_id: req.pod_id,
            pinned_runner_id: pinned_runner,
            work_item_id: issue.id,
            parent_run_id: Some(req.parent.id),
            executor_kind: execution.executor_kind,
            error_code: execution.error_code.clone().unwrap_or_default(),
            tool_plan: execution.tool_plan.clone(),
            trigger: req.parent.trigger.clone(),
            phase_kind: String::new(),
            run_config,
            now: req.now,
        })
        .await?;
    if let Some(admission) = admission_error {
        return seam
            .finalize_failed_run(run.id, &admission.code, &admission.detail, req.now)
            .await;
    }
    // The render kind still comes from the issue's current state even
    // though the row stamps no `phase_kind`.
    let state = seam.state(issue.state_id).await?;
    let kind = phase_kind_for_issue(state);
    let turn = match render_new_run(
        seam,
        issue.id,
        run.id,
        run.parent_run_id,
        &kind,
        &RunRenderRef {
            run_id: run.id,
            parent_run_id: run.parent_run_id,
            trigger: run.trigger.clone(),
            executor_kind: run.executor_kind,
            tool_plan: run.tool_plan.clone(),
            created_by_id: run.created_by_id,
        },
    )
    .await?
    {
        Ok(turn) => turn,
        Err(message) => return fail_render(seam, run.id, &message, req.now).await,
    };
    seam.save_prompt(run.id, &turn.text, &turn.manifest).await?;
    seam.dispatch_after_commit(run.id);
    let mut run = run;
    run.prompt = turn.text;
    run.prompt_manifest = Some(turn.manifest);
    Ok(run)
}

/// Python truthiness for a marker value (`if replacement_id`).
fn marker_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) if text.is_empty() => None,
        Value::String(text) => Some(text.clone()),
        Value::Bool(false) => None,
        Value::Bool(true) => Some("True".to_owned()),
        Value::Number(number) if number.as_i64() == Some(0) => None,
        Value::Number(number) if number.as_u64() == Some(0) => None,
        Value::Number(number) if number.as_f64() == Some(0.0) => None,
        Value::Array(items) if items.is_empty() => None,
        Value::Object(map) if map.is_empty() => None,
        other => Some(other.to_string()),
    }
}

/// Create the target-project replacement for a stopped source run
/// (`complete_project_move_handoff`, `service.py:644-732`).
///
/// Locks issue → run so a racing project move cannot deadlock the
/// callback. Guards: unknown run / non-terminal source / no marker →
/// `None`; `replacement_run_id` → the recorded row (idempotent);
/// issue moved again → suppress; active run → it wins.
pub async fn complete_project_move_handoff<S: CreationSeam + FinalizeAgentRunSeam>(
    seam: &mut S,
    run_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Option<RunView>, CreationError> {
    let work_item_id = seam.work_item_id_for_run(run_id).await?;
    let Some(work_item_id) = work_item_id else {
        return Ok(None);
    };
    let locked = seam.lock_issue_for_handoff(work_item_id).await?;
    let Some(locked) = locked else {
        return Ok(None);
    };
    let source = seam.lock_run_for_handoff(run_id).await?;
    let Some(source) = source else {
        return Ok(None);
    };
    if !source.is_terminal() {
        return Ok(None);
    }
    let marker = handoff_marker(&source.run_config);
    let Some(marker) = marker else {
        return Ok(None);
    };
    if let Some(replacement_id) = marker.get("replacement_run_id").and_then(marker_text) {
        let replacement_id = replacement_id.parse::<Uuid>().map_err(|_| {
            CreationError::MissingRow(format!("bad replacement_run_id {replacement_id:?}"))
        })?;
        // Unlocked re-read, as in Python (may itself be `None`).
        return seam.run(replacement_id).await;
    }
    if let Some(target_project_id) = marker.get("target_project_id").and_then(marker_text) {
        if locked.issue.project_id.map(|id| id.to_string()) != Some(target_project_id.clone()) {
            let stamped = stamp_handoff_marker(
                &source.run_config,
                "suppressed",
                Value::String(SUPPRESSED_ISSUE_MOVED_AGAIN.to_owned()),
            );
            seam.save_run_config(source.id, &stamped).await?;
            return Ok(None);
        }
    }
    if let Some(active) = seam.active_run_for(locked.issue.id).await? {
        // A concurrent explicit Run AI won the window between the
        // terminal commit and this callback: treat that row as the
        // replacement rather than violating the one-active-run
        // invariant.
        let stamped = stamp_handoff_marker(
            &source.run_config,
            "replacement_run_id",
            Value::String(active.id.to_string()),
        );
        seam.save_run_config(source.id, &stamped).await?;
        return Ok(Some(active));
    }
    let mut pod = None;
    if let Some(target_pod_id) = marker.get("target_pod_id").and_then(marker_text) {
        let target_pod_id = target_pod_id.parse::<Uuid>().map_err(|_| {
            CreationError::MissingRow(format!("bad target_pod_id {target_pod_id:?}"))
        })?;
        // `filter(pk=…, project_id=…)` via the live-pod read plus the
        // project guard: same row returned iff both match.
        if let Some(candidate) = seam.assigned_pod(target_pod_id).await? {
            if Some(candidate.project_id) == locked.issue.project_id {
                pod = Some(candidate);
            }
        }
    }
    let pod = match pod {
        Some(pod) => pod,
        None => {
            let project_id = locked
                .issue
                .project_id
                .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
            let Some(pod) = seam.default_pod_for_project(project_id).await? else {
                return Ok(None);
            };
            pod
        }
    };
    // The nested `atomic` is a savepoint; the port runs flat on the
    // same transaction (no partial-rollback path inside).
    let replacement = create_project_move_handoff_run(
        seam,
        &HandoffCreateRequest {
            issue_id: locked.issue.id,
            parent: source.clone(),
            pod_id: pod.id,
            now,
        },
    )
    .await?;
    let stamped = stamp_handoff_marker(
        &source.run_config,
        "replacement_run_id",
        Value::String(replacement.id.to_string()),
    );
    seam.save_run_config(source.id, &stamped).await?;
    Ok(Some(replacement))
}

/// `(parent, fresh_session)` for a run created on the issue *now*
/// (`parent_for_next_run`, `service.py:110-154`). `state_id` is the
/// stage the run is for (defaults to the issue's state); `cross_stage`
/// overrides the kind derivation. Shared by the transition dispatch,
/// the ticker's queued entries and Run AI so every path parents the
/// same way.
pub async fn parent_for_next_run<C: CreationSeam>(
    seam: &mut C,
    issue_id: Uuid,
    state_id: Option<Uuid>,
    cross_stage: Option<bool>,
) -> Result<(Option<RunView>, bool), CreationError> {
    let latest = seam.latest_prior_run(issue_id).await?;
    let Some(latest) = latest else {
        return Ok((None, true));
    };
    let state_id = match state_id {
        Some(id) => Some(id),
        None => seam.issue(issue_id).await?.state_id,
    };
    let state = seam.state(state_id).await?;
    // Python reads the ticker lazily on the hand-back path only
    // (`getattr(issue, "agent_ticker", None)`); the eager fetch
    // returns identical values (one extra SELECT on the other
    // paths, unobservable in row semantics).
    let resume = seam.resume_parent_run_id(issue_id).await?;
    let (parent_id, fresh) = select_parent_for_next_run(
        Some(ParentCandidate {
            id: latest.id,
            phase_kind: &latest.phase_kind,
        }),
        state,
        cross_stage,
        resume,
    );
    let parent = match parent_id {
        None => None,
        Some(id) if id == latest.id => Some(latest),
        Some(id) => {
            // The bare `ticker.resume_parent_run` FK follow: a
            // dangling id raises.
            let Some(run) = seam.run(id).await? else {
                return Err(CreationError::MissingRow(format!(
                    "dangling resume_parent_run_id {id}"
                )));
            };
            Some(run)
        }
    };
    Ok((parent, fresh))
}

/// Pick a fallback `created_by` for callers that don't pass `actor`
/// (`_resolve_fallback_creator`, `service.py:303-316`): the issue's
/// creator, else the project lead, else the default assignee.
pub async fn resolve_fallback_creator<C: CreationSeam>(
    seam: &mut C,
    issue_id: Uuid,
) -> Result<Option<Uuid>, CreationError> {
    let issue = seam.issue(issue_id).await?;
    if issue.created_by_id.is_some() {
        return Ok(issue.created_by_id);
    }
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let project = seam.project(project_id).await?;
    Ok(fallback_creator_select(
        None,
        project.project_lead_id,
        project.default_assignee_id,
    ))
}

/// Resolve the pod a run for this issue belongs to
/// (`_resolve_pod_for_issue`, `service.py:319-338`): the live assigned
/// pod when set, else the project's live default. `None` only when
/// both are gone (or the issue has no project).
pub async fn resolve_pod_for_issue<C: CreationSeam>(
    seam: &mut C,
    issue_id: Uuid,
) -> Result<Option<Uuid>, CreationError> {
    let issue = seam.issue(issue_id).await?;
    let assigned = match issue.assigned_pod_id {
        Some(pod_id) => seam.assigned_pod(pod_id).await?,
        None => None,
    };
    if assigned.is_some() {
        return Ok(assigned.map(|pod| pod.id));
    }
    let Some(project_id) = issue.project_id else {
        return Ok(None);
    };
    let default = seam.default_pod_for_project(project_id).await?;
    Ok(resolve_pod_select(None, default.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static CREATE_DISPATCH: &str = include_str!(
        "../../../../fixtures/orchestration/fx06_creation/create_dispatch.before_after.json"
    );
    static CONTINUATION: &str = include_str!(
        "../../../../fixtures/orchestration/fx06_creation/continuation.before_after.json"
    );
    static PARENT_PIN: &str =
        include_str!("../../../../fixtures/orchestration/fx06_creation/parent_pin.matrix.json");
    static RESOLVERS: &str =
        include_str!("../../../../fixtures/orchestration/fx06_creation/resolvers.golden.json");
    static HANDOFF: &str =
        include_str!("../../../../fixtures/orchestration/fx06_creation/handoff.before_after.json");

    fn fixture(raw: &str) -> Value {
        serde_json::from_str(raw).expect("fixture parses")
    }

    /// Deterministic UUIDs (`fx6-…` style labels map to ids in tests).
    fn uid(tag: u8) -> Uuid {
        Uuid::parse_str(&format!("66666666-aaaa-bbbb-cccc-0000000000{tag:02x}")).expect("uuid")
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
            .expect("frozen clock")
            .with_timezone(&Utc)
    }

    fn issue_view() -> IssueView {
        IssueView {
            id: uid(0x01),
            workspace_id: uid(0x10),
            project_id: Some(uid(0x20)),
            state_id: Some(uid(0x30)),
            parent_id: None,
            created_by_id: Some(uid(0x40)),
            assigned_pod_id: None,
            agent_executor: None,
            git_work_branch: None,
            workpad: None,
            name: Some("FX6 C1".to_owned()),
            description_stripped: None,
            priority: None,
            sequence_id: 1,
            target_date: None,
        }
    }

    fn project_view() -> ProjectView {
        ProjectView {
            id: uid(0x20),
            workspace_id: uid(0x10),
            identifier: "FX6".to_owned(),
            name: "FX6".to_owned(),
            description: None,
            repo_url: Some("https://example.com/fx6.git".to_owned()),
            base_branch: Some("main".to_owned()),
            default_agent_executor: "local_runner".to_owned(),
            project_lead_id: None,
            default_assignee_id: None,
            pool: 10,
            interval_impl: 10800,
            interval_review: 10800,
            interval_test: 10800,
        }
    }

    fn state_view(name: &str, group: &str) -> StateView {
        StateView {
            id: uid(0x30),
            name: name.to_owned(),
            group: group.to_owned(),
        }
    }

    fn run_view(id: Uuid) -> RunView {
        RunView {
            id,
            workspace_id: uid(0x10),
            created_by_id: uid(0x40),
            pod_id: uid(0x50),
            runner_id: None,
            pinned_runner_id: None,
            parent_run_id: None,
            work_item_id: Some(uid(0x01)),
            status: AgentRunStatus::Queued,
            trigger: AgentRunTrigger::StateTransition.value().to_owned(),
            executor_kind: AgentExecutorKind::LocalRunner,
            phase_kind: "coding-task".to_owned(),
            run_config: json!({}),
            tool_plan: json!({}),
            error_code: String::new(),
            error: String::new(),
            prompt: String::new(),
            prompt_manifest: None,
            ended_at: None,
        }
    }

    fn test_bundle(
        issue: IssueView,
        project: ProjectView,
        state: Option<StateView>,
    ) -> RenderBundle {
        RenderBundle {
            issue,
            project,
            workspace_slug: "fx6-ws".to_owned(),
            workspace_name: "FX6 ws".to_owned(),
            state,
            labels: Vec::new(),
            assignees: Vec::new(),
            project_states: vec![
                context::ProjectStateView {
                    name: "Todo".to_owned(),
                    group: "unstarted".to_owned(),
                    description: None,
                },
                context::ProjectStateView {
                    name: "In Progress".to_owned(),
                    group: "started".to_owned(),
                    description: None,
                },
                context::ProjectStateView {
                    name: "In Review".to_owned(),
                    group: "review".to_owned(),
                    description: None,
                },
                context::ProjectStateView {
                    name: "In Test".to_owned(),
                    group: "test".to_owned(),
                    description: None,
                },
                context::ProjectStateView {
                    name: "Done".to_owned(),
                    group: "completed".to_owned(),
                    description: None,
                },
            ],
            children: Vec::new(),
            relation_rows: Vec::new(),
            refs: HashMap::new(),
            directional_refs: HashMap::new(),
            comments: Vec::new(),
            reviews: Vec::new(),
            remote: None,
            adapter: None,
            parent: None,
            lineage: Vec::new(),
            prior_run_count: 0,
            ticker: None,
            direct_parent_payload: None,
            ticker_parent_payload: None,
            override_rows: Vec::new(),
        }
    }

    /// Scripted store: every read is a map lookup, every write is
    /// recorded. `execution` / `lock` / `bundle` are the per-test
    /// seam scripts.
    struct FakeSeam {
        issues: HashMap<Uuid, IssueView>,
        projects: HashMap<Uuid, ProjectView>,
        states: HashMap<Uuid, StateView>,
        runs: std::cell::RefCell<HashMap<Uuid, RunView>>,
        runners: HashMap<Uuid, RunnerView>,
        pods: HashMap<Uuid, PodView>,
        latest: HashMap<Uuid, Uuid>,
        active: HashMap<Uuid, Uuid>,
        resume: HashMap<Uuid, Uuid>,
        execution: Option<Result<ExecutionFields, ExecutionError>>,
        lock: Option<Result<Option<AdmissionError>, CreationError>>,
        bundle: Option<RenderBundle>,
        bundle_err: Option<String>,
        dispatched: Vec<Uuid>,
        inserted: std::cell::RefCell<Vec<NewAgentRun>>,
        saved_prompts: std::cell::RefCell<HashMap<Uuid, (String, Value)>>,
        saved_configs: std::cell::RefCell<HashMap<Uuid, Value>>,
        finalized: Vec<(Uuid, String, String)>,
        lock_order: std::cell::RefCell<Vec<&'static str>>,
    }

    impl Default for FakeSeam {
        fn default() -> Self {
            Self {
                issues: HashMap::new(),
                projects: HashMap::new(),
                states: HashMap::new(),
                runs: std::cell::RefCell::new(HashMap::new()),
                runners: HashMap::new(),
                pods: HashMap::new(),
                latest: HashMap::new(),
                active: HashMap::new(),
                resume: HashMap::new(),
                execution: None,
                lock: None,
                bundle: None,
                bundle_err: None,
                dispatched: Vec::new(),
                inserted: std::cell::RefCell::new(Vec::new()),
                saved_prompts: std::cell::RefCell::new(HashMap::new()),
                saved_configs: std::cell::RefCell::new(HashMap::new()),
                finalized: Vec::new(),
                lock_order: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl FakeSeam {
        fn minimal() -> Self {
            let mut seam = Self::default();
            seam.issues.insert(uid(0x01), issue_view());
            seam.projects.insert(uid(0x20), project_view());
            seam.states
                .insert(uid(0x30), state_view("In Progress", "started"));
            seam.pods.insert(
                uid(0x50),
                PodView {
                    id: uid(0x50),
                    project_id: uid(0x20),
                },
            );
            seam.execution = Some(Ok(ExecutionFields::local(json!({}))));
            seam.lock = Some(Ok(None));
            seam
        }

        fn with_bundle(mut self) -> Self {
            let bundle = test_bundle(
                self.issues[&uid(0x01)].clone(),
                self.projects[&uid(0x20)].clone(),
                Some(self.states[&uid(0x30)].clone()),
            );
            self.bundle = Some(bundle);
            self
        }

        fn fail_render(mut self, body: &str) -> Self {
            let mut bundle = self.bundle.clone().expect("bundle first");
            // `autonomy` is overridable and in the coding-task recipe
            // (`intro` is locked and would skip the chain).
            bundle.override_rows.push(composer::OverrideRow {
                workspace_id: uid(0x10).to_string(),
                section_key: "autonomy".to_owned(),
                body: body.to_owned(),
                version: 1,
                is_active: true,
                user_id: None,
            });
            self.bundle = Some(bundle);
            self
        }
    }

    impl CreationSeam for FakeSeam {
        async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
            self.issues
                .get(&issue_id)
                .cloned()
                .ok_or_else(|| CreationError::Db(format!("no issue {issue_id}")))
        }

        async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
            self.projects
                .get(&project_id)
                .cloned()
                .ok_or_else(|| CreationError::Db(format!("no project {project_id}")))
        }

        async fn state(
            &mut self,
            state_id: Option<Uuid>,
        ) -> Result<Option<StateView>, CreationError> {
            match state_id {
                None => Ok(None),
                Some(id) => match self.states.get(&id) {
                    Some(state) => Ok(Some(state.clone())),
                    None => Err(CreationError::MissingRow(format!("no state {id}"))),
                },
            }
        }

        async fn latest_prior_run(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            Ok(self
                .latest
                .get(&issue_id)
                .and_then(|id| self.runs.borrow().get(id).cloned()))
        }

        async fn active_run_for(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            Ok(self
                .active
                .get(&issue_id)
                .and_then(|id| self.runs.borrow().get(id).cloned()))
        }

        async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
            Ok(self.runs.borrow().get(&run_id).cloned())
        }

        async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
            Ok(self.runners.get(&runner_id).cloned())
        }

        async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
            Ok(self.pods.get(&pod_id).cloned())
        }

        async fn default_pod_for_project(
            &mut self,
            project_id: Uuid,
        ) -> Result<Option<PodView>, CreationError> {
            Ok(self
                .pods
                .values()
                .find(|pod| pod.project_id == project_id)
                .cloned())
        }

        async fn resume_parent_run_id(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<Uuid>, CreationError> {
            Ok(self.resume.get(&issue_id).copied())
        }

        async fn work_item_id_for_run(
            &mut self,
            run_id: Uuid,
        ) -> Result<Option<Uuid>, CreationError> {
            Ok(self
                .runs
                .borrow()
                .get(&run_id)
                .and_then(|run| run.work_item_id))
        }

        async fn lock_issue_for_handoff(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<LockedIssue>, CreationError> {
            self.lock_order.borrow_mut().push("issue");
            let Some(issue) = self.issues.get(&issue_id).cloned() else {
                return Ok(None);
            };
            let project_id = issue.project_id.expect("handoff issue has a project");
            let project = self.projects[&project_id].clone();
            let state = match issue.state_id {
                Some(id) => Some(self.states[&id].clone()),
                None => None,
            };
            Ok(Some(LockedIssue {
                issue,
                project,
                state,
            }))
        }

        async fn lock_run_for_handoff(
            &mut self,
            run_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            self.lock_order.borrow_mut().push("run");
            Ok(self.runs.borrow().get(&run_id).cloned())
        }

        async fn user_flags(&mut self, _user_id: Uuid) -> Result<UserFlags, CreationError> {
            Ok(UserFlags {
                is_active: true,
                is_bot: false,
            })
        }

        async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
            self.inserted.borrow_mut().push(row.clone());
            let run = RunView {
                id: row.id,
                workspace_id: row.workspace_id,
                created_by_id: row.created_by_id,
                pod_id: row.pod_id,
                runner_id: None,
                pinned_runner_id: row.pinned_runner_id,
                parent_run_id: row.parent_run_id,
                work_item_id: Some(row.work_item_id),
                status: AgentRunStatus::Queued,
                trigger: row.trigger.clone(),
                executor_kind: row.executor_kind,
                phase_kind: row.phase_kind.clone(),
                run_config: row.run_config.clone(),
                tool_plan: row.tool_plan.clone(),
                error_code: row.error_code.clone(),
                error: String::new(),
                prompt: String::new(),
                prompt_manifest: None,
                ended_at: None,
            };
            self.runs.borrow_mut().insert(row.id, run.clone());
            Ok(run)
        }

        async fn save_prompt(
            &mut self,
            run_id: Uuid,
            prompt: &str,
            manifest: &Value,
        ) -> Result<(), CreationError> {
            self.saved_prompts
                .borrow_mut()
                .insert(run_id, (prompt.to_owned(), manifest.clone()));
            if let Some(run) = self.runs.borrow_mut().get_mut(&run_id) {
                run.prompt = prompt.to_owned();
                run.prompt_manifest = Some(manifest.clone());
            }
            Ok(())
        }

        async fn save_run_config(
            &mut self,
            run_id: Uuid,
            config: &Value,
        ) -> Result<(), CreationError> {
            self.saved_configs
                .borrow_mut()
                .insert(run_id, config.clone());
            if let Some(run) = self.runs.borrow_mut().get_mut(&run_id) {
                run.run_config = config.clone();
            }
            Ok(())
        }

        async fn execution_fields(
            &mut self,
            _req: &ExecutionRequest,
        ) -> Result<ExecutionFields, ExecutionError> {
            self.execution.clone().expect("script execution")
        }

        async fn lock_cloud_creation_capacity(
            &mut self,
            _workspace_id: Uuid,
            _executor_kind: AgentExecutorKind,
            _automatic: bool,
        ) -> Result<Option<AdmissionError>, CreationError> {
            self.lock.clone().expect("script lock")
        }

        fn dispatch_after_commit(&mut self, run_id: Uuid) {
            self.dispatched.push(run_id);
        }

        async fn render_bundle(
            &mut self,
            _issue_id: Uuid,
            _run_id: Uuid,
            _parent_run_id: Option<Uuid>,
            _trigger: &str,
            _created_by_id: Uuid,
        ) -> Result<RenderBundle, CreationError> {
            if let Some(err) = self.bundle_err.clone() {
                return Err(CreationError::Db(err));
            }
            self.bundle
                .clone()
                .ok_or_else(|| CreationError::Db("no bundle".to_owned()))
        }

        fn extra_toolsets_schema_tool(&self) -> String {
            String::new()
        }
    }

    impl FinalizeAgentRunSeam for FakeSeam {
        async fn finalize_failed_run(
            &mut self,
            run_id: Uuid,
            error_code: &str,
            error: &str,
            now: DateTime<Utc>,
        ) -> Result<RunView, CreationError> {
            self.finalized
                .push((run_id, error_code.to_owned(), error.to_owned()));
            let mut runs = self.runs.borrow_mut();
            let run = runs.get_mut(&run_id).expect("run exists");
            run.status = AgentRunStatus::Failed;
            run.error_code = error_code.to_owned();
            run.error = error.to_owned();
            run.ended_at = Some(now);
            Ok(run.clone())
        }
    }

    // -- pure units ------------------------------------------------------

    #[test]
    fn phase_kind_matches_fixture_states() {
        let cases = fixture(PARENT_PIN);
        assert_eq!(
            phase_kind_for_issue(Some(state_view("In Progress", "started"))),
            cases["kinds"]["in_progress"].as_str().unwrap()
        );
        assert_eq!(
            phase_kind_for_issue(Some(state_view("In Review", "review"))),
            cases["kinds"]["in_review"].as_str().unwrap()
        );
        assert_eq!(
            phase_kind_for_issue(Some(state_view("In Test", "test"))),
            cases["kinds"]["in_test"].as_str().unwrap()
        );
        // Non-ticking state falls back to the coding default.
        assert_eq!(
            phase_kind_for_issue(Some(state_view("Todo", "unstarted"))),
            cases["kinds"]["KIND_CODING_TASK"].as_str().unwrap()
        );
        assert_eq!(
            phase_kind_for_issue(None),
            cases["kinds"]["KIND_CODING_TASK"].as_str().unwrap()
        );
        // The C1/C3 stamps.
        let dispatch = fixture(CREATE_DISPATCH);
        assert_eq!(
            phase_kind_for_issue(Some(state_view("In Progress", "started"))),
            dispatch["cases"]["C1_created_no_parent"]["after"]["phase_kind"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            phase_kind_for_issue(Some(state_view("In Review", "review"))),
            dispatch["cases"]["C3_fresh_session_drops_parent_and_pin"]["after"]["phase_kind"]
                .as_str()
                .unwrap()
        );
    }

    #[test]
    fn run_config_refresh_matches_r9_r10() {
        let cases = fixture(RESOLVERS);
        let r9 = &cases["cases"]["R9_run_config_refresh"];
        let refreshed = run_config_for_issue(
            Some("https://example.com/fx6b.git"),
            Some("develop"),
            Some("fx6/feature"),
            Some(&r9["base"]),
        );
        assert_eq!(refreshed, r9["result"]);
        // The base dict is never mutated.
        assert_eq!(r9["base"], r9["base_unmutated"]);
        let r10 = &cases["cases"]["R10_run_config_no_base"];
        assert_eq!(
            run_config_for_issue(
                Some("https://example.com/fx6b.git"),
                Some("develop"),
                Some("fx6/feature"),
                None
            ),
            r10["result"]
        );
        // Empty strings collapse to null (`or None`).
        assert_eq!(
            run_config_for_issue(Some(""), Some(""), Some(""), None),
            json!({"repo_url": null, "repo_ref": null, "git_work_branch": null})
        );
    }

    #[test]
    fn pin_matrix_matches_m1_m5() {
        let runner = RunnerView {
            id: uid(0x60),
            pod_id: uid(0x50),
            status: "online".to_owned(),
        };
        // M1: no runner.
        assert_eq!(pinned_runner_for(None, Some(uid(0x50))), None);
        // M2: revoked.
        let revoked = RunnerView {
            status: RUNNER_STATUS_REVOKED.to_owned(),
            ..runner.clone()
        };
        assert_eq!(pinned_runner_for(Some(&revoked), Some(uid(0x50))), None);
        // M3: pod mismatch.
        assert_eq!(pinned_runner_for(Some(&runner), Some(uid(0x51))), None);
        // M4: eligible.
        assert_eq!(
            pinned_runner_for(Some(&runner), Some(uid(0x50))),
            Some(uid(0x60))
        );
        // M5: no target pod skips the check.
        assert_eq!(pinned_runner_for(Some(&runner), None), Some(uid(0x60)));
    }

    #[test]
    fn parent_matrix_matches_p1_p8() {
        let latest = |phase_kind: &'static str| ParentCandidate {
            id: uid(0x70),
            phase_kind,
        };
        let in_progress = || Some(state_view("In Progress", "started"));
        let in_review = || Some(state_view("In Review", "review"));
        let todo = || Some(state_view("Todo", "unstarted"));
        // P1: no latest.
        assert_eq!(
            select_parent_for_next_run(None, in_progress(), None, None),
            (None, true)
        );
        // P2: non-ticking state.
        assert_eq!(
            select_parent_for_next_run(Some(latest("coding-task")), todo(), None, None),
            (Some(uid(0x70)), false)
        );
        // P3: same stage.
        assert_eq!(
            select_parent_for_next_run(Some(latest("coding-task")), in_progress(), None, None),
            (Some(uid(0x70)), false)
        );
        // P4: cross-stage into fresh-entry review.
        assert_eq!(
            select_parent_for_next_run(Some(latest("coding-task")), in_review(), None, None),
            (None, true)
        );
        // P5: hand-back with resume parent.
        assert_eq!(
            select_parent_for_next_run(
                Some(latest("review")),
                in_progress(),
                None,
                Some(uid(0x71))
            ),
            (Some(uid(0x71)), false)
        );
        // P6: hand-back without.
        assert_eq!(
            select_parent_for_next_run(Some(latest("review")), in_progress(), None, None),
            (None, true)
        );
        // P7: explicit cross_stage=false wins over kinds.
        assert_eq!(
            select_parent_for_next_run(Some(latest("review")), in_progress(), Some(false), None),
            (Some(uid(0x70)), false)
        );
        // P8: explicit cross_stage=true wins over same kind.
        assert_eq!(
            select_parent_for_next_run(Some(latest("review")), in_review(), Some(true), None),
            (None, true)
        );
        // Empty phase_kind derives the coding default (`or KIND_CODING_TASK`).
        assert_eq!(
            select_parent_for_next_run(Some(latest("")), in_progress(), None, None),
            (Some(uid(0x70)), false)
        );
    }

    #[test]
    fn creator_chain_matches_r1_r4() {
        assert_eq!(
            fallback_creator_select(Some(uid(1)), Some(uid(2)), Some(uid(3))),
            Some(uid(1))
        );
        assert_eq!(
            fallback_creator_select(None, Some(uid(2)), Some(uid(3))),
            Some(uid(2))
        );
        assert_eq!(
            fallback_creator_select(None, None, Some(uid(3))),
            Some(uid(3))
        );
        assert_eq!(fallback_creator_select(None, None, None), None);
    }

    #[test]
    fn pod_select_prefers_assigned() {
        let assigned = PodView {
            id: uid(0x51),
            project_id: uid(0x20),
        };
        let default = PodView {
            id: uid(0x50),
            project_id: uid(0x20),
        };
        assert_eq!(
            resolve_pod_select(Some(&assigned), Some(&default)),
            Some(uid(0x51))
        );
        assert_eq!(resolve_pod_select(None, Some(&default)), Some(uid(0x50)));
        assert_eq!(resolve_pod_select(None, None), None);
    }

    #[test]
    fn handoff_marker_helpers() {
        let config = json!({"_project_move_handoff": {"target_project_id": "p"}});
        assert!(handoff_marker(&config).is_some());
        assert_eq!(handoff_marker(&json!({})), None);
        assert_eq!(handoff_marker(&json!({"_project_move_handoff": {}})), None);
        assert_eq!(
            handoff_marker(&json!({"_project_move_handoff": null})),
            None
        );
        let stamped = stamp_handoff_marker(&config, "replacement_run_id", json!("r"));
        assert_eq!(
            stamped["_project_move_handoff"],
            json!({"target_project_id": "p", "replacement_run_id": "r"})
        );
        // Other config keys survive the stamp.
        let config = json!({"model": "m", "_project_move_handoff": {}});
        assert_eq!(
            stamp_handoff_marker(&config, "suppressed", json!("issue_moved_again")),
            json!({"model": "m", "_project_move_handoff": {"suppressed": "issue_moved_again"}})
        );
    }

    #[test]
    fn trigger_predicates() {
        assert!(is_automatic_issue_trigger(AgentRunTrigger::Tick.value()));
        assert!(!is_automatic_issue_trigger(
            AgentRunTrigger::StateTransition.value()
        ));
        for trigger in [
            AgentRunTrigger::StateTransition.value(),
            AgentRunTrigger::RunAi.value(),
            AgentRunTrigger::CommentAndRun.value(),
            AgentRunTrigger::Direct.value(),
        ] {
            assert!(is_human_triggered(trigger), "{trigger:?}");
            assert!(user_id_for_run(trigger, uid(1)).is_some());
        }
        assert!(!is_automatic_issue_trigger(
            AgentRunTrigger::Scheduler.value()
        ));
        for trigger in [
            AgentRunTrigger::Tick.value(),
            AgentRunTrigger::Scheduler.value(),
        ] {
            assert!(!is_human_triggered(trigger), "{trigger:?}");
            assert_eq!(user_id_for_run(trigger, uid(1)), None);
        }
        // Unrecognized stored values (migration 0029's
        // blocker_completed rows, hand-written rows) are neither
        // human nor automatic, as the Python `in` checks answer.
        for trigger in ["human", "blocker_completed", "", "bogus"] {
            assert!(!is_automatic_issue_trigger(trigger), "{trigger:?}");
            assert!(!is_human_triggered(trigger), "{trigger:?}");
            assert_eq!(user_id_for_run(trigger, uid(1)), None);
        }
    }

    // -- render assembly (real compose, offline) --------------------------

    #[test]
    fn render_coding_turn_matches_c1_shape() {
        let dispatch = fixture(CREATE_DISPATCH);
        let after = &dispatch["cases"]["C1_created_no_parent"]["after"];
        let bundle = test_bundle(
            issue_view(),
            project_view(),
            Some(state_view("In Progress", "started")),
        );
        let turn = render_first_turn(
            &bundle,
            "coding-task",
            &RunRenderRef {
                run_id: uid(0x80),
                parent_run_id: None,
                trigger: AgentRunTrigger::StateTransition.value().to_owned(),
                executor_kind: AgentExecutorKind::LocalRunner,
                tool_plan: json!({}),
                created_by_id: uid(0x40),
            },
            "",
        )
        .expect("renders");
        for marker in [
            "FX6-1",
            "FX6 C1",
            "In Progress (group: started)",
            "https://example.com/fx6.git",
            "coding-task",
        ] {
            assert!(turn.text.contains(marker), "missing {marker}");
        }
        let manifest = turn.manifest.as_array().expect("bare list");
        assert_eq!(
            manifest.len(),
            after["prompt_manifest"].as_array().unwrap().len()
        );
        assert_eq!(
            manifest[0]["section_key"],
            after["prompt_manifest"][0]["section_key"]
        );
    }

    #[test]
    fn render_review_turn_matches_c3_shape() {
        let dispatch = fixture(CREATE_DISPATCH);
        let after = &dispatch["cases"]["C3_fresh_session_drops_parent_and_pin"]["after"];
        let bundle = test_bundle(
            issue_view(),
            project_view(),
            Some(state_view("In Review", "review")),
        );
        let turn = render_first_turn(
            &bundle,
            "review",
            &RunRenderRef {
                run_id: uid(0x80),
                parent_run_id: Some(uid(0x70)),
                trigger: AgentRunTrigger::StateTransition.value().to_owned(),
                executor_kind: AgentExecutorKind::LocalRunner,
                tool_plan: json!({}),
                created_by_id: uid(0x40),
            },
            "",
        )
        .expect("renders");
        assert!(turn.text.contains("You are reviewing"));
        assert!(turn.text.contains("FX6-1"));
        assert_eq!(
            turn.manifest.as_array().expect("bare list").len(),
            after["prompt_manifest"].as_array().unwrap().len()
        );
    }

    // -- drivers ----------------------------------------------------------

    fn dispatch_req(parent: Option<RunView>) -> CreateDispatchRequest {
        CreateDispatchRequest {
            issue_id: uid(0x01),
            parent,
            creator_id: uid(0x40),
            pod_id: uid(0x50),
            fresh_session: false,
            trigger: AgentRunTrigger::StateTransition.value().to_owned(),
            now: now(),
        }
    }

    #[tokio::test]
    async fn c1_created_no_parent() {
        let dispatch = fixture(CREATE_DISPATCH);
        let case = &dispatch["cases"]["C1_created_no_parent"];
        let mut seam = FakeSeam::minimal().with_bundle();
        let outcome = create_and_dispatch_run(&mut seam, &dispatch_req(None))
            .await
            .expect("creates");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        let created = outcome.created_run.expect("created");
        assert_eq!(seam.dispatched, vec![created]);
        assert!(seam.finalized.is_empty());
        let inserted = seam.inserted.borrow();
        assert_eq!(inserted.len(), 1);
        let row = &inserted[0];
        assert_eq!(row.id, created);
        assert_eq!(row.parent_run_id, None);
        assert_eq!(row.pinned_runner_id, None);
        assert_eq!(
            row.phase_kind,
            case["after"]["phase_kind"].as_str().unwrap()
        );
        assert_eq!(row.trigger, AgentRunTrigger::StateTransition.value());
        assert_eq!(row.executor_kind, AgentExecutorKind::LocalRunner);
        assert_eq!(row.tool_plan, json!({}));
        assert_eq!(
            row.run_config,
            json!({
                "repo_url": "https://example.com/fx6.git",
                "repo_ref": "main",
                "git_work_branch": null,
            })
        );
        let (prompt, manifest) = seam.saved_prompts.borrow()[&created].clone();
        assert!(prompt.contains("FX6-1"));
        assert_eq!(
            manifest.as_array().unwrap().len(),
            case["after"]["prompt_manifest"].as_array().unwrap().len()
        );
        // Owner stays NULL: the INSERT hardcodes it.
        assert!(RUN_INSERT_SQL.contains("owner_id"));
    }

    #[tokio::test]
    async fn c2_parent_linkage_and_pin() {
        let mut seam = FakeSeam::minimal().with_bundle();
        let mut parent = run_view(uid(0x70));
        parent.status = AgentRunStatus::Completed;
        parent.runner_id = Some(uid(0x60));
        seam.runners.insert(
            uid(0x60),
            RunnerView {
                id: uid(0x60),
                pod_id: uid(0x50),
                status: "online".to_owned(),
            },
        );
        let outcome = create_and_dispatch_run(&mut seam, &dispatch_req(Some(parent)))
            .await
            .expect("creates");
        assert_eq!(outcome.reason, "created");
        let inserted = seam.inserted.borrow();
        assert_eq!(inserted[0].parent_run_id, Some(uid(0x70)));
        assert_eq!(inserted[0].pinned_runner_id, Some(uid(0x60)));
    }

    #[tokio::test]
    async fn c3_fresh_session_drops_parent_and_pin() {
        let mut seam = FakeSeam::minimal();
        seam.states
            .insert(uid(0x30), state_view("In Review", "review"));
        let seam = seam.with_bundle();
        let mut seam = seam;
        let mut parent = run_view(uid(0x70));
        parent.runner_id = Some(uid(0x60));
        seam.runners.insert(
            uid(0x60),
            RunnerView {
                id: uid(0x60),
                pod_id: uid(0x50),
                status: "online".to_owned(),
            },
        );
        let mut req = dispatch_req(Some(parent));
        req.fresh_session = true;
        let outcome = create_and_dispatch_run(&mut seam, &req)
            .await
            .expect("creates");
        assert_eq!(outcome.reason, "created");
        let inserted = seam.inserted.borrow();
        assert_eq!(inserted[0].parent_run_id, None);
        assert_eq!(inserted[0].pinned_runner_id, None);
        assert_eq!(inserted[0].phase_kind, "review");
        let created = outcome.created_run.unwrap();
        let manifest = &seam.saved_prompts.borrow()[&created].1;
        assert_eq!(manifest.as_array().unwrap().len(), 10);
    }

    #[tokio::test]
    async fn c4_admission_error_failed() {
        let dispatch = fixture(CREATE_DISPATCH);
        let case = &dispatch["cases"]["C4_admission_error_failed"];
        let mut seam = FakeSeam::minimal().with_bundle();
        seam.lock = Some(Ok(Some(AdmissionError {
            code: "run_quota_exceeded".to_owned(),
            detail: "Cloud Agent queue is full for this workspace".to_owned(),
        })));
        let outcome = create_and_dispatch_run(&mut seam, &dispatch_req(None))
            .await
            .expect("failed, not error");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        let created = outcome.created_run.expect("row exists");
        assert!(seam.dispatched.is_empty());
        assert!(seam.saved_prompts.borrow().is_empty());
        assert_eq!(
            seam.finalized,
            vec![(
                created,
                "run_quota_exceeded".to_owned(),
                "Cloud Agent queue is full for this workspace".to_owned()
            )]
        );
        let run = seam.runs.borrow()[&created].clone();
        assert_eq!(run.status, AgentRunStatus::Failed);
        assert_eq!(
            run.error_code,
            case["after"]["error_code"].as_str().unwrap()
        );
        assert_eq!(run.error, case["after"]["error"].as_str().unwrap());
        assert_eq!(run.ended_at, Some(now()));
        assert!(run.prompt.is_empty());
        assert_eq!(run.prompt_manifest, None);
    }

    #[tokio::test]
    async fn c5_render_failed() {
        let mut seam = FakeSeam::minimal().with_bundle().fail_render("{{ broken");
        let outcome = create_and_dispatch_run(&mut seam, &dispatch_req(None))
            .await
            .expect("failed, not error");
        assert_eq!(outcome.reason, "render-failed");
        let created = outcome.created_run.expect("row exists");
        assert!(seam.dispatched.is_empty());
        let (id, code, error) = seam.finalized[0].clone();
        assert_eq!(id, created);
        assert_eq!(code, "prompt_build_failed");
        assert!(error.starts_with("prompt build failed: "), "{error}");
    }

    #[tokio::test]
    async fn c6_executor_unavailable_no_run() {
        let dispatch = fixture(CREATE_DISPATCH);
        let case = &dispatch["cases"]["C6_executor_unavailable_no_run"];
        let mut seam = FakeSeam::minimal().with_bundle();
        seam.execution = Some(Err(ExecutionError::Refused("fx6-no-executor".to_owned())));
        let outcome = create_and_dispatch_run(&mut seam, &dispatch_req(None))
            .await
            .expect("reason, not error");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.created_run, None);
        assert!(seam.inserted.borrow().is_empty());
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn lock_capacity_raise_propagates() {
        let mut seam = FakeSeam::minimal().with_bundle();
        seam.lock = Some(Err(CreationError::CapacityRefused(
            "Cloud Agent queue is full for this workspace".to_owned(),
        )));
        let err = create_and_dispatch_run(&mut seam, &dispatch_req(None))
            .await
            .expect_err("propagates");
        assert!(matches!(err, CreationError::CapacityRefused(_)));
    }

    fn continuation_req(parent: RunView) -> ContinuationRequest {
        ContinuationRequest {
            issue_id: uid(0x01),
            parent,
            creator_id: uid(0x40),
            pod_id: uid(0x50),
            trigger: AgentRunTrigger::CommentAndRun.value().to_owned(),
            now: now(),
        }
    }

    #[tokio::test]
    async fn k1_created_with_parent_and_pin() {
        let cases = fixture(CONTINUATION);
        let case = &cases["cases"]["K1_created_with_parent_and_pin"];
        let mut seam = FakeSeam::minimal().with_bundle();
        let mut parent = run_view(uid(0x70));
        parent.status = AgentRunStatus::Completed;
        parent.runner_id = Some(uid(0x60));
        seam.runners.insert(
            uid(0x60),
            RunnerView {
                id: uid(0x60),
                pod_id: uid(0x50),
                status: "online".to_owned(),
            },
        );
        let outcome = create_continuation_run(&mut seam, &continuation_req(parent))
            .await
            .expect("creates");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.coalesced_into, None);
        let created = outcome.created_run.expect("created");
        assert_eq!(seam.dispatched, vec![created]);
        let inserted = seam.inserted.borrow();
        assert_eq!(inserted[0].parent_run_id, Some(uid(0x70)));
        assert_eq!(inserted[0].pinned_runner_id, Some(uid(0x60)));
        assert_eq!(inserted[0].trigger, AgentRunTrigger::CommentAndRun.value());
    }

    #[tokio::test]
    async fn k2_admission_error_failed() {
        let mut seam = FakeSeam::minimal().with_bundle();
        seam.lock = Some(Ok(Some(AdmissionError {
            code: "run_quota_exceeded".to_owned(),
            detail: "Cloud Agent queue is full for this workspace".to_owned(),
        })));
        let outcome = create_continuation_run(&mut seam, &continuation_req(run_view(uid(0x70))))
            .await
            .expect("failed, not error");
        assert_eq!(outcome.reason, "run_quota_exceeded");
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn k3_render_failed() {
        let mut seam = FakeSeam::minimal().with_bundle().fail_render("{{ broken");
        let outcome = create_continuation_run(&mut seam, &continuation_req(run_view(uid(0x70))))
            .await
            .expect("failed, not error");
        assert_eq!(outcome.reason, "render-failed");
        assert_eq!(seam.finalized[0].1, "prompt_build_failed");
    }

    #[tokio::test]
    async fn k4_executor_unavailable_no_run() {
        let cases = fixture(CONTINUATION);
        let case = &cases["cases"]["K4_executor_unavailable_no_run"];
        let mut seam = FakeSeam::minimal().with_bundle();
        seam.execution = Some(Err(ExecutionError::Refused("fx6-cloud-down".to_owned())));
        let outcome = create_continuation_run(&mut seam, &continuation_req(run_view(uid(0x70))))
            .await
            .expect("reason, not error");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.created_run, None);
        assert!(seam.inserted.borrow().is_empty());
    }

    #[tokio::test]
    async fn managed_pin_key_wins_over_computed() {
        let mut seam = FakeSeam::minimal().with_bundle();
        let mut parent = run_view(uid(0x70));
        parent.runner_id = Some(uid(0x60));
        seam.runners.insert(
            uid(0x60),
            RunnerView {
                id: uid(0x60),
                pod_id: uid(0x50),
                status: "online".to_owned(),
            },
        );
        seam.execution = Some(Ok(ExecutionFields {
            executor_kind: AgentExecutorKind::ManagedRunner,
            tool_plan: json!({}),
            pinned_runner_entry: Some(Some(uid(0x61))),
            error_code: None,
            cloud_admission_error: None,
        }));
        create_and_dispatch_run(&mut seam, &dispatch_req(Some(parent)))
            .await
            .expect("creates");
        assert_eq!(seam.inserted.borrow()[0].pinned_runner_id, Some(uid(0x61)));
    }

    #[tokio::test]
    async fn cloud_pin_key_clobbers_computed() {
        let mut seam = FakeSeam::minimal().with_bundle();
        let mut parent = run_view(uid(0x70));
        parent.runner_id = Some(uid(0x60));
        seam.runners.insert(
            uid(0x60),
            RunnerView {
                id: uid(0x60),
                pod_id: uid(0x50),
                status: "online".to_owned(),
            },
        );
        seam.execution = Some(Ok(ExecutionFields {
            executor_kind: AgentExecutorKind::CloudAgent,
            tool_plan: json!({"tools": []}),
            pinned_runner_entry: Some(None),
            error_code: None,
            cloud_admission_error: None,
        }));
        create_and_dispatch_run(&mut seam, &dispatch_req(Some(parent)))
            .await
            .expect("creates");
        assert_eq!(seam.inserted.borrow()[0].pinned_runner_id, None);
    }

    // -- resolvers --------------------------------------------------------

    #[tokio::test]
    async fn r1_r4_fallback_creator_chain() {
        let mut seam = FakeSeam::minimal();
        // R1: issue creator.
        assert_eq!(
            resolve_fallback_creator(&mut seam, uid(0x01))
                .await
                .expect("r1"),
            Some(uid(0x40))
        );
        // R2: project lead.
        seam.issues.get_mut(&uid(0x01)).unwrap().created_by_id = None;
        seam.projects.get_mut(&uid(0x20)).unwrap().project_lead_id = Some(uid(0x41));
        assert_eq!(
            resolve_fallback_creator(&mut seam, uid(0x01))
                .await
                .expect("r2"),
            Some(uid(0x41))
        );
        // R3: default assignee.
        seam.projects.get_mut(&uid(0x20)).unwrap().project_lead_id = None;
        seam.projects
            .get_mut(&uid(0x20))
            .unwrap()
            .default_assignee_id = Some(uid(0x42));
        assert_eq!(
            resolve_fallback_creator(&mut seam, uid(0x01))
                .await
                .expect("r3"),
            Some(uid(0x42))
        );
        // R4: none.
        seam.projects
            .get_mut(&uid(0x20))
            .unwrap()
            .default_assignee_id = None;
        assert_eq!(
            resolve_fallback_creator(&mut seam, uid(0x01))
                .await
                .expect("r4"),
            None
        );
    }

    #[tokio::test]
    async fn r5_r8_pod_resolution() {
        let mut seam = FakeSeam::minimal();
        seam.pods.insert(
            uid(0x51),
            PodView {
                id: uid(0x51),
                project_id: uid(0x20),
            },
        );
        // R5: assigned pod wins.
        seam.issues.get_mut(&uid(0x01)).unwrap().assigned_pod_id = Some(uid(0x51));
        assert_eq!(
            resolve_pod_for_issue(&mut seam, uid(0x01))
                .await
                .expect("r5"),
            Some(uid(0x51))
        );
        // R6: project default.
        seam.issues.get_mut(&uid(0x01)).unwrap().assigned_pod_id = None;
        seam.pods.remove(&uid(0x51));
        assert_eq!(
            resolve_pod_for_issue(&mut seam, uid(0x01))
                .await
                .expect("r6"),
            Some(uid(0x50))
        );
        // R7: dangling assigned id falls through to the default.
        seam.issues.get_mut(&uid(0x01)).unwrap().assigned_pod_id = Some(uid(0x52));
        assert_eq!(
            resolve_pod_for_issue(&mut seam, uid(0x01))
                .await
                .expect("r7"),
            Some(uid(0x50))
        );
        // R8: no project, no pod.
        let mut orphan = issue_view();
        orphan.id = uid(0x02);
        orphan.project_id = None;
        orphan.assigned_pod_id = None;
        seam.issues.insert(uid(0x02), orphan);
        assert_eq!(
            resolve_pod_for_issue(&mut seam, uid(0x02))
                .await
                .expect("r8"),
            None
        );
    }

    #[tokio::test]
    async fn p5_handback_async_fetches_resume_run() {
        let mut seam = FakeSeam::minimal();
        let mut latest = run_view(uid(0x70));
        latest.phase_kind = "review".to_owned();
        seam.runs.borrow_mut().insert(uid(0x70), latest);
        let mut resume = run_view(uid(0x71));
        resume.phase_kind = "coding-task".to_owned();
        seam.runs.borrow_mut().insert(uid(0x71), resume);
        seam.latest.insert(uid(0x01), uid(0x70));
        seam.resume.insert(uid(0x01), uid(0x71));
        let (parent, fresh) = parent_for_next_run(&mut seam, uid(0x01), None, None)
            .await
            .expect("parents");
        assert!(!fresh);
        assert_eq!(parent.map(|run| run.id), Some(uid(0x71)));
    }

    #[tokio::test]
    async fn dangling_resume_parent_is_missing_row() {
        let mut seam = FakeSeam::minimal();
        let mut latest = run_view(uid(0x70));
        latest.phase_kind = "review".to_owned();
        seam.runs.borrow_mut().insert(uid(0x70), latest);
        seam.latest.insert(uid(0x01), uid(0x70));
        seam.resume.insert(uid(0x01), uid(0x72));
        let err = parent_for_next_run(&mut seam, uid(0x01), None, None)
            .await
            .expect_err("dangling raises");
        assert!(matches!(err, CreationError::MissingRow(_)));
    }

    // -- handoff ----------------------------------------------------------

    /// H2-style source: cancelled run carrying the move marker.
    fn handoff_seam() -> FakeSeam {
        let seam = FakeSeam::minimal().with_bundle();
        let mut source = run_view(uid(0x90));
        source.status = AgentRunStatus::Cancelled;
        source.run_config = json!({"_project_move_handoff": {
            "target_pod_id": uid(0x50).to_string(),
            "target_project_id": uid(0x20).to_string(),
        }});
        seam.runs.borrow_mut().insert(uid(0x90), source);
        seam
    }

    #[tokio::test]
    async fn h2_happy_path() {
        let cases = fixture(HANDOFF);
        let case = &cases["cases"]["H2_happy_path"];
        let mut seam = handoff_seam();
        let replacement = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff")
            .expect("replacement");
        assert_eq!(replacement.parent_run_id, Some(uid(0x90)));
        assert_eq!(
            replacement.trigger,
            AgentRunTrigger::StateTransition.value()
        );
        // The handoff stamps no phase kind.
        assert_eq!(
            replacement.phase_kind,
            case["replacement"]["phase_kind"].as_str().unwrap()
        );
        assert_eq!(replacement.pinned_runner_id, None);
        assert_eq!(replacement.created_by_id, uid(0x40));
        assert_eq!(replacement.pod_id, uid(0x50));
        assert_eq!(replacement.status, AgentRunStatus::Queued);
        assert!(!replacement.prompt.is_empty());
        assert!(replacement.prompt_manifest.is_some());
        // Lock order: issue → run.
        assert_eq!(*seam.lock_order.borrow(), vec!["issue", "run"]);
        // The marker carries the replacement id.
        let stamped = &seam.saved_configs.borrow()[&uid(0x90)];
        assert_eq!(
            stamped["_project_move_handoff"]["replacement_run_id"],
            json!(replacement.id.to_string())
        );
        assert_eq!(seam.dispatched, vec![replacement.id]);
        // The replacement's config refreshed the repo snapshot (the
        // marker never reaches the new row).
        assert_eq!(
            replacement.run_config,
            json!({
                "repo_url": "https://example.com/fx6.git",
                "repo_ref": "main",
                "git_work_branch": null,
            })
        );
    }

    #[tokio::test]
    async fn handoff_inherits_unknown_parent_trigger_verbatim() {
        // `trigger=parent.trigger` (`service.py:612`) copies the raw
        // stored value, even when it is outside the enum members.
        let mut seam = handoff_seam();
        seam.runs
            .borrow_mut()
            .get_mut(&uid(0x90))
            .expect("source")
            .trigger = "human".to_owned();
        let replacement = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff")
            .expect("replacement");
        assert_eq!(replacement.trigger, "human");
        assert_eq!(seam.inserted.borrow()[0].trigger, "human");
    }

    #[tokio::test]
    async fn h3_replacement_id_idempotent() {
        let mut seam = handoff_seam();
        let first = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff")
            .expect("replacement");
        assert_eq!(seam.inserted.borrow().len(), 1);
        let second = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff")
            .expect("replacement");
        assert_eq!(second.id, first.id);
        assert_eq!(seam.inserted.borrow().len(), 1);
        assert_eq!(seam.dispatched, vec![first.id]);
    }

    #[tokio::test]
    async fn h4_moved_again_suppressed() {
        let cases = fixture(HANDOFF);
        let case = &cases["cases"]["H4_moved_again_suppressed"];
        let mut seam = handoff_seam();
        {
            let mut runs = seam.runs.borrow_mut();
            let source = runs.get_mut(&uid(0x90)).unwrap();
            source.run_config = json!({"_project_move_handoff": {
                "target_pod_id": uid(0x50).to_string(),
                "target_project_id": uid(0x21).to_string(),
            }});
        }
        let result = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff");
        assert_eq!(result, None);
        assert!(seam.inserted.borrow().is_empty());
        let stamped = &seam.saved_configs.borrow()[&uid(0x90)];
        assert_eq!(
            stamped["_project_move_handoff"]["suppressed"],
            case["marker_after"]["suppressed"]
        );
    }

    #[tokio::test]
    async fn h5_active_run_wins() {
        let mut seam = handoff_seam();
        let mut winner = run_view(uid(0x91));
        winner.status = AgentRunStatus::Running;
        seam.runs.borrow_mut().insert(uid(0x91), winner);
        seam.active.insert(uid(0x01), uid(0x91));
        let result = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff")
            .expect("winner");
        assert_eq!(result.id, uid(0x91));
        assert!(seam.inserted.borrow().is_empty());
        assert!(seam.dispatched.is_empty());
        let stamped = &seam.saved_configs.borrow()[&uid(0x90)];
        assert_eq!(
            stamped["_project_move_handoff"]["replacement_run_id"],
            json!(uid(0x91).to_string())
        );
    }

    #[tokio::test]
    async fn h7_guards_return_none() {
        // H7a: unknown run.
        let mut seam = handoff_seam();
        assert_eq!(
            complete_project_move_handoff(&mut seam, uid(0x99), now())
                .await
                .expect("handoff"),
            None
        );
        // H7b: source not terminal.
        let mut seam = handoff_seam();
        seam.runs.borrow_mut().get_mut(&uid(0x90)).unwrap().status = AgentRunStatus::Running;
        assert_eq!(
            complete_project_move_handoff(&mut seam, uid(0x90), now())
                .await
                .expect("handoff"),
            None
        );
        // H7c: no marker.
        let mut seam = handoff_seam();
        seam.runs
            .borrow_mut()
            .get_mut(&uid(0x90))
            .unwrap()
            .run_config = json!({});
        assert_eq!(
            complete_project_move_handoff(&mut seam, uid(0x90), now())
                .await
                .expect("handoff"),
            None
        );
        assert!(seam.inserted.borrow().is_empty());
    }

    #[tokio::test]
    async fn h8_executor_fallback_local() {
        let mut seam = handoff_seam();
        seam.execution = Some(Err(ExecutionError::Refused(
            "fx6-target-cloud-no-llm".to_owned(),
        )));
        let replacement = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff")
            .expect("replacement");
        assert_eq!(replacement.executor_kind, AgentExecutorKind::LocalRunner);
        assert_eq!(replacement.tool_plan, json!({}));
        assert_eq!(replacement.status, AgentRunStatus::Queued);
        assert!(!replacement.prompt.is_empty());
        assert_eq!(seam.dispatched, vec![replacement.id]);
    }

    #[tokio::test]
    async fn h9_active_race_returns_existing() {
        let mut seam = handoff_seam();
        let mut racer = run_view(uid(0x92));
        racer.status = AgentRunStatus::Running;
        seam.runs.borrow_mut().insert(uid(0x92), racer);
        seam.active.insert(uid(0x01), uid(0x92));
        let source = seam.runs.borrow()[&uid(0x90)].clone();
        // Call the inner create directly: the race check precedes the
        // INSERT (`service.py:601-603`).
        let result = create_project_move_handoff_run(
            &mut seam,
            &HandoffCreateRequest {
                issue_id: uid(0x01),
                parent: source,
                pod_id: uid(0x50),
                now: now(),
            },
        )
        .await
        .expect("race");
        assert_eq!(result.id, uid(0x92));
        assert!(seam.inserted.borrow().is_empty());
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn h6_no_target_pod() {
        let mut seam = handoff_seam();
        seam.pods.clear();
        {
            let mut runs = seam.runs.borrow_mut();
            let source = runs.get_mut(&uid(0x90)).unwrap();
            source.run_config = json!({"_project_move_handoff": {
                "target_pod_id": null,
                "target_project_id": uid(0x20).to_string(),
            }});
        }
        let result = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff");
        assert_eq!(result, None);
        assert!(seam.inserted.borrow().is_empty());
    }

    #[tokio::test]
    async fn handoff_target_pod_project_guard() {
        // A live target pod from another project falls through to the
        // issue project's default.
        let mut seam = handoff_seam();
        seam.pods.insert(
            uid(0x53),
            PodView {
                id: uid(0x53),
                project_id: uid(0x21),
            },
        );
        {
            let mut runs = seam.runs.borrow_mut();
            let source = runs.get_mut(&uid(0x90)).unwrap();
            source.run_config = json!({"_project_move_handoff": {
                "target_pod_id": uid(0x53).to_string(),
                "target_project_id": uid(0x20).to_string(),
            }});
        }
        let replacement = complete_project_move_handoff(&mut seam, uid(0x90), now())
            .await
            .expect("handoff")
            .expect("replacement");
        assert_eq!(replacement.pod_id, uid(0x50));
    }

    // -- SQL shapes --------------------------------------------------------

    #[test]
    fn lookup_sql_shapes() {
        let active = active_run_sql();
        for status in [
            "queued",
            "assigned",
            "waiting_for_worktree",
            "running",
            "cancel_requested",
            "awaiting_approval",
            "awaiting_reauth",
        ] {
            assert!(active.contains(&format!("'{status}'")), "{active}");
        }
        assert!(
            active.contains("ORDER BY created_at DESC LIMIT 1"),
            "{active}"
        );
        assert!(active.contains("parent_run_id"), "{active}");
        assert!(active.contains("run_config"), "{active}");
        let latest = latest_prior_run_sql();
        assert!(
            latest.contains("ORDER BY created_at DESC LIMIT 1"),
            "{latest}"
        );
        assert!(!latest.contains("status IN"), "{latest}");
        let locked = run_lock_sql();
        assert!(locked.contains("FOR UPDATE OF agent_run"), "{locked}");
        assert!(ISSUE_LOCK_SQL.contains("FOR UPDATE OF issues"));
        assert!(!ISSUE_LOCK_SQL.contains("deleted_at"));
        let flash = finalize_lock_sql();
        for status in ["completed", "failed", "cancelled", "blocked", "refused"] {
            assert!(flash.contains(&format!("'{status}'")), "{flash}");
        }
        assert!(flash.contains("NOT IN"), "{flash}");
    }

    #[test]
    fn insert_covers_full_row() {
        // Every non-generated column lands explicitly, with the
        // Django-side defaults as literals.
        for column in [
            "owner_id",
            "runner_id",
            "scheduler_binding_id",
            "dispatch_attempts",
            "cancel_reason",
            "terminal_hooks_applied_at",
            "required_capabilities",
            "thread_id",
            "agent_metadata",
            "done_payload",
            "refusal_category",
            "llm_model",
            "usage",
            "created_at",
            "assigned_at",
            "queue_position",
            "started_at",
            "ended_at",
        ] {
            assert!(RUN_INSERT_SQL.contains(column), "missing {column}");
        }
        // Generated token columns are excluded (Postgres computes them).
        for column in ["input_tokens", "output_tokens", "total_tokens"] {
            assert!(!RUN_INSERT_SQL.contains(column), "generated {column}");
        }
        // `$1..$14` in NewAgentRun order.
        assert!(RUN_INSERT_SQL.contains("$14"));
        assert!(!RUN_INSERT_SQL.contains("$15"));
        assert!(PROMPT_UPDATE_SQL.contains("prompt_manifest"));
        assert!(FINALIZE_UPDATE_SQL.contains("queue_position = NULL"));
    }
}
