#![forbid(unsafe_code)]

//! Matcher drain: pod + runner dispatch (D-14, stage 5).
//!
//! Port of the dispatch half of
//! `apps/api/pi_dash/runner/services/matcher.py`:
//!
//! * `select_runner_in_pod` (`:89-121`) → [`SELECT_RUNNER_IN_POD_SQL`].
//! * `next_queued_run_for_pod` (`:123-145`) → [`NEXT_QUEUED_RUN_FOR_POD_SQL`].
//! * `next_for_runner` (`:147-192`) → [`next_for_runner_sql`].
//! * `drain_pod` (`:194-240`) → [`DRAIN_POD_IDLE_RUNNERS_SQL`] +
//!   [`ASSIGN_RUN_UPDATE_SQL`] + [`plan_assignment`] +
//!   [`DrainEffect::SendAssign`], run per the executor recipe below.
//! * `drain_pod_by_id` (`:242-251`) → [`DRAIN_POD_BY_ID_LOOKUP_SQL`].
//! * `drain_for_runner` (`:253-296`) → [`DRAIN_FOR_RUNNER_LOCK_SQL`] +
//!   [`next_for_runner_sql`] + [`ASSIGN_RUN_UPDATE_SQL`] +
//!   [`plan_assignment`] + [`DrainEffect::SendAssign`].
//! * `drain_for_runner_by_id` (`:298-304`) →
//!   [`DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL`].
//!
//! Out of scope here (sibling sub-issue PIDASHCONV-555):
//! `HEARTBEAT_GRACE` / `NON_TERMINAL_STATUSES` / `BUSY_STATUSES`
//! (`:44-81`) and the legacy/guard half (`:338-465`) live in
//! [`super::guards`]; `_build_assign_msg` (`:306-330`) lives in
//! [`pidash_types::runner_sessions::envelopes::build_assign_msg`].
//! All are reused here, never re-ported.
//!
//! # Layering: plans, not queries
//!
//! This crate has no database handle, so every entry point is pure
//! (the [`super::guards`] / D-15 `runner_runs` precedent): SQL text
//! in Django shape (quoted identifiers, `%s` params rendered as
//! Postgres `$N`), branch predicates over caller-fetched facts, an
//! ordered [`SetClause`](crate::runner_runs::SetClause) assignment
//! plan, and [`DrainEffect`] descriptors the executing layer fires
//! after commit through the foundation post-commit wrapper
//! (`pidash_db::tx::AfterCommit`). The executing layers are the D-14
//! handlers (PIDASHCONV-556/557/558/559) and the D-12/D-15 callers
//! that name these drains in their own effect enums.
//!
//! # Executor recipes (the loops live here, verbatim)
//!
//! `drain_pod(pod_id)` (`:194-240`): open a transaction; fetch the
//! idle list ([`DRAIN_POD_IDLE_RUNNERS_SQL`], `$1` =
//! [`alive_threshold`](super::guards::alive_threshold) evaluated
//! once, `$2` = pod); for each runner row in list order (freshest
//! heartbeat first), run [`next_for_runner_sql`] for its
//! provisioning/visibility and, when a run row comes back, execute
//! [`ASSIGN_RUN_UPDATE_SQL`] (`$1` owner, `$2` runner, `$3`
//! `timezone.now()` evaluated per assignment, `$4` run) and record
//! [`plan_assignment`]; commit; then queue one
//! [`DrainEffect::SendAssign`] per assignment, in loop order, on the
//! post-commit queue. Returns the assignment count; logs
//! [`drain_pod_log`] when non-empty. The loop is runner-first: each
//! idle runner takes its personal-queue-first pick, so a pinned
//! head-of-line run never blocks other runners' unpinned work.
//!
//! `drain_for_runner(runner_id)` (`:253-296`): open a transaction;
//! re-lock the runner ([`DRAIN_FOR_RUNNER_LOCK_SQL`], `$1` =
//! threshold, `$2` = runner) — a miss returns `false` with no
//! further query; run [`next_for_runner_sql`] — a miss returns
//! `false`; execute [`ASSIGN_RUN_UPDATE_SQL`]; commit; queue the
//! single [`DrainEffect::SendAssign`]. Returns `true` and logs
//! [`drain_for_runner_log`].
//!
//! `drain_pod_by_id` (`:242-251`): look the pod up
//! ([`DRAIN_POD_BY_ID_LOOKUP_SQL`], which carries the `Pod.objects`
//! soft-delete scope) — a miss returns `0`; else `drain_pod`.
//! `drain_for_runner_by_id` (`:298-304`): look the runner up
//! ([`DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL`]) — a miss returns `false`;
//! else `drain_for_runner`.
//!
//! # Post-commit dispatch contract
//!
//! Each [`DrainEffect::SendAssign`] runs after its transaction
//! commits via [`send_to_runner`](super::pubsub::send_to_runner)
//! (which envelopes the frame and re-raises only
//! `RunnerOfflineError`). Python registers the sends with
//! `transaction.on_commit` *outside* any `try`, so a dispatch
//! failure propagates to the drain caller **after** the commit: the
//! row stays `ASSIGNED` (fixture `drain_for_runner_sessionless`).
//! The executor must not swallow the offline error — the matcher
//! re-queues on it.
//!
//! # Translation notes
//!
//! * Fixed enum values are literals; caller-supplied ids/timestamps
//!   are `$N` params, each documented on its const. `next_for_runner`
//!   reuses `$1` pod / `$2` runner / `$3` owner at every occurrence
//!   (the D-15 predicate takes one owner param, so semantic
//!   numbering is forced; same value binds each repeat).
//! * The `pod` SELECTs inside the fixture traces are ORM laziness
//!   (`pod=runner.pod` resolves the FK descriptor per runner); Rust
//!   binds `pod_id` from the already-fetched runner row, so no pod
//!   fetch runs on the drain path. `SAVEPOINT` / `RELEASE` /
//!   `BEGIN` / `COMMIT` lines are harness artifacts, not ported.
//! * `ORDER BY 44 ASC` is Django's positional rendering of the
//!   `_is_mine` annotation (41 run columns + 2 `EXISTS` annotations +
//!   the `CASE`); kept verbatim, with the rank position const pinned
//!   against the column count so it cannot drift silently.
//! * A non-private runner makes `filter_runs_usable_by_runner`
//!   return `qs.none()`, whose `.first()` answers `None` without
//!   touching the database — [`next_for_runner_sql`] returns `None`
//!   and the executor issues no query (the D-15 `qs.none()`
//!   precedent).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * The `drain_pod` idle list carries **no** `DESKTOP_BUNDLED`
//!   exclusion (unlike `select_runner_in_pod` `:109`): desktop
//!   runners are listed, then can only take managed runs pinned to
//!   them via the provisioning split. Ported as written.
//! * The pin-rank `CASE` (`THEN 0 ELSE 1`) sorts pinned-to-me before
//!   unpinned with FIFO inside each tier; the provisioning split
//!   then narrows manual runners to `local_runner` and desktop
//!   runners to managed runs pinned to them — the `IN
//!   (MACHINE_EXECUTORS)` term stays in the query next to the
//!   narrower equality. Ported as written.
//! * Dispatch is `on_commit`-only with no `try`: an ONLINE-but-
//!   sessionless runner keeps its `ASSIGNED` row while the caller
//!   observes `RunnerOfflineError` (TRACE bug 2). Ported as written.
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-07-matcher.json`
//! (FX-RSES-07 `select_runner_in_pod`, `next_queued_run_for_pod`,
//! `next_for_runner`, `next_for_runner_desktop`, `drain_pod`,
//! `drain_for_runner`, `drain_for_runner_sessionless`,
//! `drain_for_runner_by_id_missing`, `drain_pod_by_id_missing`).
//! Every section is replayed by the `#[cfg(test)]` suite below.

use pidash_auth::permissions::runner::VISIBILITY_PRIVATE;
use pidash_db::runner_enroll::columns::enums::RUNNER_PROVISIONING_DESKTOP_BUNDLED;
use pidash_db::runner_runs::agent_run::COLUMNS as AGENT_RUN_COLUMNS;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_sessions::envelopes::build_assign_msg;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::runner_runs::guards::runs_usable_by_runner_predicate;
use crate::runner_runs::{SetClause, SetValue};

// ---------------------------------------------------------------------------
// Pod projection (FX-RSES-07 `drain_pod.sql[2]` SELECT list)
// ---------------------------------------------------------------------------

/// `pod` columns in Django `_meta` order — the `SELECT` projection
/// of [`DRAIN_POD_BY_ID_LOOKUP_SQL`], pinned against the fixture's
/// pod SELECT list by the tests below.
pub const POD_COLUMNS: [&str; 10] = [
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "created_by_id",
    "is_default",
    "deleted_at",
    "created_at",
    "updated_at",
];

// ---------------------------------------------------------------------------
// Runner selects (`matcher.py:89-121`, `:194-223`, `:253-279`)
// ---------------------------------------------------------------------------

/// `select_runner_in_pod` (`:89-121`): one online, heartbeat-fresh,
/// idle, non-desktop runner in the pod, freshest heartbeat first.
///
/// `$1` is the [`alive_threshold`](super::guards::alive_threshold),
/// `$2` the pod id. `status` is the `'online'` literal; the
/// `provisioning` arm excludes `'desktop_bundled'`; the `NOT
/// EXISTS` arm is the `.exclude(agent_runs__status__in=BUSY_STATUSES)`
/// reverse join (values in [`BUSY_STATUSES`] order). `FOR UPDATE
/// SKIP LOCKED`: call inside a transaction.
pub const SELECT_RUNNER_IN_POD_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE (\"runner\".\"last_heartbeat_at\" >= $1 AND \"runner\".\"pod_id\" = $2 AND \"runner\".\"status\" = 'online' AND NOT (\"runner\".\"provisioning\" = 'desktop_bundled') AND NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" U1 WHERE (U1.\"status\" IN ('assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth') AND U1.\"runner_id\" = (\"runner\".\"id\")) LIMIT 1))) ORDER BY \"runner\".\"last_heartbeat_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED";

/// `drain_pod` idle list (`:211-223`): same filter as
/// [`SELECT_RUNNER_IN_POD_SQL`] **without** the `DESKTOP_BUNDLED`
/// exclusion and **without** `LIMIT 1` — the full idle set, in
/// drain order. `$1` is the threshold (evaluated once per drain),
/// `$2` the pod id. See the ported-bugs note.
pub const DRAIN_POD_IDLE_RUNNERS_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE (\"runner\".\"last_heartbeat_at\" >= $1 AND \"runner\".\"pod_id\" = $2 AND \"runner\".\"status\" = 'online' AND NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" U1 WHERE (U1.\"status\" IN ('assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth') AND U1.\"runner_id\" = (\"runner\".\"id\")) LIMIT 1))) ORDER BY \"runner\".\"last_heartbeat_at\" DESC FOR UPDATE SKIP LOCKED";

/// `drain_for_runner` re-lock (`:267-279`): the same row back under
/// `FOR UPDATE SKIP LOCKED` when it is still online, fresh and
/// idle. `$1` is the threshold, `$2` the runner id. No explicit
/// `order_by`, so `Runner.Meta.ordering` applies
/// (`-last_heartbeat_at`, `-created_at` — both keys, unlike the
/// selects above). A miss means: return `false`, no further query.
pub const DRAIN_FOR_RUNNER_LOCK_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE (\"runner\".\"last_heartbeat_at\" >= $1 AND \"runner\".\"id\" = $2 AND \"runner\".\"status\" = 'online' AND NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" U1 WHERE (U1.\"status\" IN ('assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth') AND U1.\"runner_id\" = (\"runner\".\"id\")) LIMIT 1))) ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED";

// ---------------------------------------------------------------------------
// Run selects (`matcher.py:123-192`)
// ---------------------------------------------------------------------------

/// `next_queued_run_for_pod` (`:123-145`): the oldest unpinned
/// `QUEUED` machine-executor run in the pod, locked. `$1` is the
/// pod id. Pinned runs are excluded (`IS NULL`); the `IN` list is
/// `MACHINE_EXECUTORS` in tuple order. `FOR UPDATE SKIP LOCKED`:
/// call inside a transaction.
pub const NEXT_QUEUED_RUN_FOR_POD_SQL: &str = "SELECT \"agent_run\".\"id\", \"agent_run\".\"workspace_id\", \"agent_run\".\"owner_id\", \"agent_run\".\"created_by_id\", \"agent_run\".\"pod_id\", \"agent_run\".\"runner_id\", \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"work_item_id\", \"agent_run\".\"scheduler_binding_id\", \"agent_run\".\"parent_run_id\", \"agent_run\".\"status\", \"agent_run\".\"executor_kind\", \"agent_run\".\"dispatch_attempts\", \"agent_run\".\"cancel_requested_at\", \"agent_run\".\"cancel_reason\", \"agent_run\".\"error_code\", \"agent_run\".\"tool_plan\", \"agent_run\".\"terminal_hooks_applied_at\", \"agent_run\".\"terminal_capacity_released_at\", \"agent_run\".\"prompt\", \"agent_run\".\"trigger\", \"agent_run\".\"prompt_manifest\", \"agent_run\".\"phase_kind\", \"agent_run\".\"run_config\", \"agent_run\".\"required_capabilities\", \"agent_run\".\"thread_id\", \"agent_run\".\"agent_metadata\", \"agent_run\".\"lease_expires_at\", \"agent_run\".\"done_payload\", \"agent_run\".\"error\", \"agent_run\".\"refusal_category\", \"agent_run\".\"llm_model\", \"agent_run\".\"usage\", \"agent_run\".\"input_tokens\", \"agent_run\".\"output_tokens\", \"agent_run\".\"total_tokens\", \"agent_run\".\"created_at\", \"agent_run\".\"assigned_at\", \"agent_run\".\"queue_position\", \"agent_run\".\"started_at\", \"agent_run\".\"ended_at\" FROM \"agent_run\" WHERE (\"agent_run\".\"executor_kind\" IN ('local_runner', 'managed_runner') AND \"agent_run\".\"pinned_runner_id\" IS NULL AND \"agent_run\".\"pod_id\" = $1 AND \"agent_run\".\"status\" = 'queued') ORDER BY \"agent_run\".\"created_at\" ASC LIMIT 1 FOR UPDATE SKIP LOCKED";

/// Positional `ORDER BY` of the `_is_mine` annotation in
/// [`next_for_runner_sql`]: 41 run columns plus 2 `EXISTS`
/// annotations plus the `CASE`, i.e.
/// `AGENT_RUN_COLUMNS.len() + 3`. Pinned by the tests below so a
/// column-count change fails loudly instead of re-sorting the tiers.
pub const NEXT_FOR_RUNNER_RANK_POSITION: usize = 44;

/// The `:173` provisioning split: `DESKTOP_BUNDLED` runners take the
/// managed arm, every other provisioning the manual arm.
pub fn is_desktop_provisioning(provisioning: &str) -> bool {
    provisioning == RUNNER_PROVISIONING_DESKTOP_BUNDLED
}

/// `next_for_runner` (`:147-192`): the next `QUEUED` run this runner
/// should take — personal queue first, then the pod general queue,
/// FIFO inside each tier.
///
/// Returns `None` for a non-private runner: the usability filter
/// answers `qs.none()`, whose `.first()` returns `None` without
/// issuing a query. The executor runs no SQL in that case.
///
/// Otherwise the full `SELECT … FOR UPDATE SKIP LOCKED` text: the
/// 41-column run projection plus the two `EXISTS` annotations the
/// usability filter contributes, the pin-rank `CASE`
/// (`pinned_runner_id = $2 THEN 0 ELSE 1`), and the provisioning
/// split (`executor_kind = 'local_runner'` for manual runners;
/// `executor_kind = 'managed_runner' AND pinned_runner_id = $2`
/// for desktop runners). Bindings: `$1` pod id, `$2` runner id,
/// `$3` runner-owner id, repeated at every occurrence. The
/// usability `OR` is
/// [`runs_usable_by_runner_predicate`](crate::runner_runs::guards::runs_usable_by_runner_predicate)
/// (D-15 owns the filter; this module only calls it).
pub fn next_for_runner_sql(provisioning: &str, visibility: i32) -> Option<String> {
    if visibility != VISIBILITY_PRIVATE {
        return None;
    }
    let usability = runs_usable_by_runner_predicate(visibility, "$3", "agent_run")?;
    let mut select = AGENT_RUN_COLUMNS
        .iter()
        .map(|column| format!("\"agent_run\".\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    select.push_str(
        ", EXISTS(SELECT 1 AS \"a\" FROM \"issues\" U0 LEFT OUTER JOIN \"issue_assignees\" U2 ON (U0.\"id\" = U2.\"issue_id\") WHERE (U0.\"deleted_at\" IS NULL AND U0.\"id\" = (\"agent_run\".\"work_item_id\") AND (U0.\"created_by_id\" = $3 OR (U2.\"assignee_id\" = $3 AND U2.\"deleted_at\" IS NULL))) LIMIT 1) AS \"_runner_visible_issue\"",
    );
    select.push_str(
        ", EXISTS(SELECT 1 AS \"a\" FROM \"scheduler_bindings\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"actor_id\" = $3 AND U0.\"id\" = (\"agent_run\".\"scheduler_binding_id\")) LIMIT 1) AS \"_runner_visible_scheduler\"",
    );
    select.push_str(
        ", CASE WHEN \"agent_run\".\"pinned_runner_id\" = $2 THEN 0 ELSE 1 END AS \"_is_mine\"",
    );
    let split = if is_desktop_provisioning(provisioning) {
        format!(
            "\"agent_run\".\"executor_kind\" = '{}' AND \"agent_run\".\"pinned_runner_id\" = $2",
            AgentExecutorKind::ManagedRunner.value()
        )
    } else {
        format!(
            "\"agent_run\".\"executor_kind\" = '{}'",
            AgentExecutorKind::LocalRunner.value()
        )
    };
    Some(format!(
        "SELECT {select} FROM \"agent_run\" WHERE (\"agent_run\".\"executor_kind\" IN ('local_runner', 'managed_runner') AND \"agent_run\".\"pod_id\" = $1 AND \"agent_run\".\"status\" = 'queued' AND (\"agent_run\".\"pinned_runner_id\" = $2 OR \"agent_run\".\"pinned_runner_id\" IS NULL) AND {usability} AND {split}) ORDER BY {rank} ASC, \"agent_run\".\"created_at\" ASC LIMIT 1 FOR UPDATE SKIP LOCKED",
        rank = NEXT_FOR_RUNNER_RANK_POSITION,
    ))
}

// ---------------------------------------------------------------------------
// Assignment write (`matcher.py:228-232`, `:285-289`)
// ---------------------------------------------------------------------------

/// The assignment `UPDATE` (`save(update_fields=["runner", "owner",
/// "status", "assigned_at"])`, `:232` / `:289`). Django emits `SET`
/// in model field-definition order, not `update_fields` order:
/// `owner_id`, `runner_id`, `status`, `assigned_at`. `$1` owner,
/// `$2` runner, `$3` `timezone.now()` (evaluated per assignment),
/// `$4` run. `status` is the `'assigned'` literal.
pub const ASSIGN_RUN_UPDATE_SQL: &str = "UPDATE \"agent_run\" SET \"owner_id\" = $1, \"runner_id\" = $2, \"status\" = 'assigned', \"assigned_at\" = $3 WHERE \"agent_run\".\"id\" = $4";

// ---------------------------------------------------------------------------
// By-id lookups (`matcher.py:242-251`, `:298-304`)
// ---------------------------------------------------------------------------

/// `drain_pod_by_id` pod lookup (`:247`):
/// `Pod.objects.filter(pk=pod_id).first()`. `$1` is the pod id. The
/// `Pod.objects` manager scope (`deleted_at IS NULL`) precedes the
/// pk term — that scope is the soft-delete skip; unordered
/// `.first()` keeps `Pod.Meta.ordering` (`-is_default`,
/// `created_at`). A miss means: return `0`, no transaction.
pub const DRAIN_POD_BY_ID_LOOKUP_SQL: &str = "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1";

/// `drain_for_runner_by_id` runner lookup (`:300`):
/// `Runner.objects.filter(pk=runner_id).first()`. `$1` is the
/// runner id. Plain manager (no scope); unordered `.first()` keeps
/// `Runner.Meta.ordering` (`-last_heartbeat_at`, `-created_at`).
/// The single-condition `WHERE` has no parens. A miss means:
/// return `false`, no transaction.
pub const DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE \"runner\".\"id\" = $1 ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1";

// ---------------------------------------------------------------------------
// Assignment plan + post-commit effect
// ---------------------------------------------------------------------------

/// Caller-fetched facts for one assignment: the locked run row's
/// payload fields plus the assignee's ids. `owner_id` is the
/// runner's owner (`run.owner_id = runner.owner_id`, `:229`).
pub struct AssignmentFacts {
    /// The locked run id (`$4` of [`ASSIGN_RUN_UPDATE_SQL`]).
    pub run_id: Uuid,
    /// The assignee runner id (`$2`).
    pub runner_id: Uuid,
    /// The runner's owner id (`$1`).
    pub owner_id: Uuid,
    /// `run.work_item_id` for the assign frame (`None` renders null).
    pub work_item_id: Option<Uuid>,
    /// `run.prompt` for the assign frame.
    pub prompt: String,
    /// `run.run_config` for the assign frame (`dict.get` lookups).
    pub run_config: Map<String, Value>,
}

/// One committed assignment: the ordered `SET` plan plus the
/// post-commit dispatch. The executor runs
/// [`ASSIGN_RUN_UPDATE_SQL`] inside its transaction, commits, then
/// queues [`DrainEffect::SendAssign`] via `pidash_db::tx`.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignmentPlan {
    /// The assigned run id.
    pub run_id: Uuid,
    /// The assignee runner id.
    pub runner_id: Uuid,
    /// The `SET` clauses in exact Django order
    /// (`owner_id`, `runner_id`, `status`, `assigned_at`).
    /// `assigned_at` is [`SetValue::Now`] (`timezone.now()` at
    /// execution); ids render hyphenated for uuid binding.
    pub set_clauses: Vec<SetClause>,
    /// The `assign` frame ([`build_assign_msg`]), queued as
    /// [`DrainEffect::SendAssign`].
    pub message: Map<String, Value>,
    /// The post-commit dispatch for this assignment.
    pub after_commit: DrainEffect,
}

/// A side effect the drains defer with `transaction.on_commit`, as
/// data. The executor queues each effect after its transaction
/// commits (in assignment order for `drain_pod`) and runs it
/// through [`send_to_runner`](super::pubsub::send_to_runner),
/// letting the offline error propagate — the row stays `ASSIGNED`
/// while the caller observes the failure, exactly as Python.
#[derive(Debug, Clone, PartialEq)]
pub enum DrainEffect {
    /// `send_to_runner(runner.id, _build_assign_msg(run))`
    /// (`:236` / `:293`): deliver the assign frame to the daemon.
    SendAssign {
        /// Target runner.
        runner_id: Uuid,
        /// The [`build_assign_msg`] frame (the outbox appends `mid`).
        message: Map<String, Value>,
    },
}

/// Plan one assignment (`run.runner = …`, `:228-233` / `:285-290`):
/// the ordered `SET` clauses plus the assign frame and its
/// post-commit effect.
pub fn plan_assignment(facts: &AssignmentFacts) -> AssignmentPlan {
    let run_id = facts.run_id.to_string();
    let work_item_id = facts.work_item_id.map(|id| id.to_string());
    let message = build_assign_msg(
        &run_id,
        work_item_id.as_deref(),
        &facts.prompt,
        &facts.run_config,
    );
    AssignmentPlan {
        run_id: facts.run_id,
        runner_id: facts.runner_id,
        set_clauses: vec![
            SetClause {
                column: "owner_id",
                value: SetValue::Text(facts.owner_id.to_string()),
            },
            SetClause {
                column: "runner_id",
                value: SetValue::Text(facts.runner_id.to_string()),
            },
            SetClause {
                column: "status",
                value: SetValue::Text("assigned".to_string()),
            },
            SetClause {
                column: "assigned_at",
                value: SetValue::Now,
            },
        ],
        after_commit: DrainEffect::SendAssign {
            runner_id: facts.runner_id,
            message: message.clone(),
        },
        message,
    }
}

// ---------------------------------------------------------------------------
// Log lines (`matcher.py:238`, `:294`)
// ---------------------------------------------------------------------------

/// `drain_pod` info line (`:238`), logged only when at least one run
/// was assigned: `drain_pod: pod=<uuid> assigned <n> run(s)`.
pub fn drain_pod_log(pod_id: &Uuid, assigned: usize) -> String {
    format!("drain_pod: pod={pod_id} assigned {assigned} run(s)")
}

/// `drain_for_runner` info line (`:294`), logged on assignment:
/// `drain_for_runner: runner=<uuid> assigned run=<uuid>`.
pub fn drain_for_runner_log(runner_id: &Uuid, run_id: &Uuid) -> String {
    format!("drain_for_runner: runner={runner_id} assigned run={run_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_sessions::guards::{BUSY_STATUSES, RUNNER_COLUMNS};
    use pidash_db::runner_enroll::columns::enums::{
        RUNNER_PROVISIONING_MANUAL, RUNNER_STATUS_ONLINE,
    };
    use pidash_types::dispatch::MACHINE_EXECUTORS;
    use pidash_types::runner_runs::AgentRunStatus;
    use serde_json::Value;

    static FX07: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-07-matcher.json");

    fn fx07() -> Value {
        serde_json::from_str(FX07).expect("FX-RSES-07 parses")
    }

    const POD: &str = "'c63d5b13d56745de932a6e5e28568cfb'::uuid";
    const OWNER: &str = "'ce755a5b0c644e838f276218e639df62'::uuid";

    /// Normalize a `next_for_runner` capture: pod → `$1`, the
    /// runner literal → `$2`, owner → `$3`.
    fn normalize_next(sql: &str, runner: &str) -> String {
        sql.replace(POD, "$1")
            .replace(runner, "$2")
            .replace(OWNER, "$3")
    }

    /// The `SELECT` projection of a runner SQL const, as columns.
    fn runner_projection(sql: &str) -> Vec<&str> {
        sql.strip_prefix("SELECT ")
            .expect("SELECT prefix")
            .split_once(" FROM \"runner\"")
            .expect("FROM runner")
            .0
            .split(", ")
            .map(|c| {
                c.strip_prefix("\"runner\".\"")
                    .expect("qualified")
                    .strip_suffix('"')
                    .expect("quoted")
            })
            .collect()
    }

    /// The `SELECT` projection of a run SQL text, as columns
    /// (annotation tails are returned verbatim after the run cols).
    fn run_projection(sql: &str) -> Vec<&str> {
        sql.strip_prefix("SELECT ")
            .expect("SELECT prefix")
            .split_once(" FROM \"agent_run\"")
            .expect("FROM agent_run")
            .0
            .split(", ")
            .map(|c| {
                c.strip_prefix("\"agent_run\".\"")
                    .map_or(c, |rest| rest.strip_suffix('"').expect("quoted"))
            })
            .collect()
    }

    // -- select_runner_in_pod --------------------------------------------

    #[test]
    fn select_runner_in_pod_sql_matches_fixture() {
        let fx = fx07();
        let django = fx["select_runner_in_pod"]["sql"][0].as_str().expect("sql");
        let normalized = django
            .replace("'2026-10-03 00:32:04.303170+00:00'::timestamptz", "$1")
            .replace(POD, "$2");
        assert_eq!(normalized, SELECT_RUNNER_IN_POD_SQL);
        assert_eq!(
            fx07()["select_runner_in_pod"]["picked"].as_str(),
            Some("g-idle")
        );
        assert!(
            fx07()["select_runner_in_pod"]["all_busy_picked"].is_null(),
            "no eligible runner answers None"
        );
        assert_eq!(runner_projection(SELECT_RUNNER_IN_POD_SQL), RUNNER_COLUMNS);
        // Captured WHERE term order: threshold, pod, status,
        // desktop exclusion, busy exclusion.
        let where_clause = SELECT_RUNNER_IN_POD_SQL
            .split_once(" WHERE (")
            .expect("WHERE")
            .1;
        let status_term = format!("\"runner\".\"status\" = '{RUNNER_STATUS_ONLINE}'");
        let desktop_term =
            format!("NOT (\"runner\".\"provisioning\" = '{RUNNER_PROVISIONING_DESKTOP_BUNDLED}')");
        let mut cursor = 0;
        for term in [
            "\"runner\".\"last_heartbeat_at\" >= $1",
            "\"runner\".\"pod_id\" = $2",
            status_term.as_str(),
            desktop_term.as_str(),
            "NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" U1",
        ] {
            let pos = where_clause[cursor..]
                .find(term)
                .unwrap_or_else(|| panic!("term present: {term}"));
            cursor += pos + term.len();
        }
        assert!(SELECT_RUNNER_IN_POD_SQL.ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED"
        ));
    }

    #[test]
    fn busy_in_lists_follow_busy_statuses() {
        let list = BUSY_STATUSES
            .iter()
            .map(|s| format!("'{}'", s.value()))
            .collect::<Vec<_>>()
            .join(", ");
        for sql in [
            SELECT_RUNNER_IN_POD_SQL,
            DRAIN_POD_IDLE_RUNNERS_SQL,
            DRAIN_FOR_RUNNER_LOCK_SQL,
        ] {
            assert!(
                sql.contains(&format!("U1.\"status\" IN ({list})")),
                "NOT EXISTS arm pins BUSY_STATUSES order"
            );
        }
    }

    // -- next_queued_run_for_pod ------------------------------------------

    #[test]
    fn next_queued_run_for_pod_sql_matches_fixture() {
        let fx = fx07();
        let django = fx["next_queued_run_for_pod"]["sql"][0]
            .as_str()
            .expect("sql");
        let normalized = django.replace(POD, "$1");
        assert_eq!(normalized, NEXT_QUEUED_RUN_FOR_POD_SQL);
        let projected = run_projection(NEXT_QUEUED_RUN_FOR_POD_SQL);
        assert_eq!(projected, AGENT_RUN_COLUMNS);
        let machine = MACHINE_EXECUTORS
            .iter()
            .map(|k| format!("'{}'", k.value()))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            NEXT_QUEUED_RUN_FOR_POD_SQL
                .contains(&format!("\"agent_run\".\"executor_kind\" IN ({machine})")),
            "IN list pins MACHINE_EXECUTORS order"
        );
        assert!(NEXT_QUEUED_RUN_FOR_POD_SQL.contains("\"agent_run\".\"pinned_runner_id\" IS NULL"));
        assert!(NEXT_QUEUED_RUN_FOR_POD_SQL
            .ends_with("ORDER BY \"agent_run\".\"created_at\" ASC LIMIT 1 FOR UPDATE SKIP LOCKED"));
    }

    // -- next_for_runner --------------------------------------------------

    #[test]
    fn next_for_runner_manual_sql_matches_fixture() {
        let ours = next_for_runner_sql(RUNNER_PROVISIONING_MANUAL, VISIBILITY_PRIVATE)
            .expect("manual arm");
        let fx = fx07();
        let django = fx["next_for_runner"]["sql"][1].as_str().expect("sql");
        let normalized = normalize_next(django, "'297ebdfd8d3b44aeb295964880f92013'::uuid");
        assert_eq!(normalized, ours);
        assert_eq!(fx07()["next_for_runner"]["picked_is_pinned"], true);
        // Tiers: pin-rank CASE first, FIFO inside each tier.
        assert!(ours.contains(
            "CASE WHEN \"agent_run\".\"pinned_runner_id\" = $2 THEN 0 ELSE 1 END AS \"_is_mine\""
        ));
        assert!(ours.ends_with(
            "ORDER BY 44 ASC, \"agent_run\".\"created_at\" ASC LIMIT 1 FOR UPDATE SKIP LOCKED"
        ));
        assert!(ours.contains(&format!(
            "\"agent_run\".\"executor_kind\" = '{}'",
            AgentExecutorKind::LocalRunner.value()
        )));
        // Manual arm: the split is the bare local equality closing
        // the WHERE — no trailing pin term (contrast desktop).
        assert!(ours.contains(&format!(
            "AND \"agent_run\".\"executor_kind\" = '{}') ORDER BY",
            AgentExecutorKind::LocalRunner.value()
        )));
    }

    #[test]
    fn next_for_runner_desktop_sql_matches_fixture() {
        let ours = next_for_runner_sql(RUNNER_PROVISIONING_DESKTOP_BUNDLED, VISIBILITY_PRIVATE)
            .expect("desktop arm");
        let fx = fx07();
        let django = fx["next_for_runner_desktop"]["sql"][1]
            .as_str()
            .expect("sql");
        let normalized = normalize_next(django, "'44cfb1285f62479bbb9ea0d0910ad6cd'::uuid");
        assert_eq!(normalized, ours);
        assert!(fx07()["next_for_runner_desktop"]["picked"].is_null());
        assert_eq!(fx07()["next_for_runner_desktop"]["after_managed_pin"], true);
        assert!(ours.contains(&format!(
            "\"agent_run\".\"executor_kind\" = '{}' AND \"agent_run\".\"pinned_runner_id\" = $2",
            AgentExecutorKind::ManagedRunner.value()
        )));
    }

    #[test]
    fn next_for_runner_non_private_selects_nothing() {
        assert_eq!(next_for_runner_sql(RUNNER_PROVISIONING_MANUAL, 99), None);
        assert_eq!(
            next_for_runner_sql(RUNNER_PROVISIONING_DESKTOP_BUNDLED, 1),
            None
        );
    }

    #[test]
    fn next_for_runner_projection_and_rank() {
        let ours = next_for_runner_sql(RUNNER_PROVISIONING_MANUAL, VISIBILITY_PRIVATE)
            .expect("manual arm");
        let projected = run_projection(&ours);
        assert_eq!(&projected[..AGENT_RUN_COLUMNS.len()], AGENT_RUN_COLUMNS);
        assert_eq!(projected.len(), AGENT_RUN_COLUMNS.len() + 3);
        assert_eq!(AGENT_RUN_COLUMNS.len(), 41);
        assert_eq!(NEXT_FOR_RUNNER_RANK_POSITION, AGENT_RUN_COLUMNS.len() + 3);
        assert_eq!(AgentRunStatus::Queued.value(), "queued", "status literal");
    }

    #[test]
    fn usability_arms_match_d15_predicate() {
        let ours = next_for_runner_sql(RUNNER_PROVISIONING_MANUAL, VISIBILITY_PRIVATE)
            .expect("manual arm");
        let predicate = runs_usable_by_runner_predicate(VISIBILITY_PRIVATE, "$3", "agent_run")
            .expect("private");
        // The WHERE arm IS the D-15 predicate (called, not copied).
        assert!(ours.contains(&predicate), "WHERE carries the D-15 text");
        // The SELECT annotations are the same EXISTS arms under
        // aliases — pinned against the predicate so a D-15 change
        // fails here instead of drifting silently.
        for alias in ["_runner_visible_issue", "_runner_visible_scheduler"] {
            let marker = format!(") AS \"{alias}\"");
            let table = if alias == "_runner_visible_issue" {
                "issues"
            } else {
                "scheduler_bindings"
            };
            let start = ours
                .find(&format!("EXISTS(SELECT 1 AS \"a\" FROM \"{table}\" U0"))
                .expect("annotation");
            let end = ours[start..].find(&marker).expect("alias") + start;
            let arm = &ours[start..end + 1];
            assert!(
                predicate.contains(arm),
                "annotation arm rides the D-15 predicate: {alias}"
            );
        }
    }

    // -- drain_pod ----------------------------------------------------------

    #[test]
    fn drain_pod_idle_sql_matches_fixture() {
        let fx = fx07();
        let django = fx["drain_pod"]["sql"][1].as_str().expect("sql");
        let normalized = django
            .replace("'2026-10-03 00:32:04.917186+00:00'::timestamptz", "$1")
            .replace(POD, "$2");
        assert_eq!(normalized, DRAIN_POD_IDLE_RUNNERS_SQL);
        assert_eq!(
            runner_projection(DRAIN_POD_IDLE_RUNNERS_SQL),
            RUNNER_COLUMNS
        );
        // No desktop exclusion on the idle list (ported as written;
        // the projection still carries the column).
        let where_only = DRAIN_POD_IDLE_RUNNERS_SQL
            .split_once(" WHERE (")
            .expect("WHERE")
            .1
            .split_once(" ORDER BY ")
            .expect("ORDER BY")
            .0;
        assert!(!where_only.contains("provisioning"));
        assert!(DRAIN_POD_IDLE_RUNNERS_SQL
            .ends_with("ORDER BY \"runner\".\"last_heartbeat_at\" DESC FOR UPDATE SKIP LOCKED"));
        // The only LIMIT is the EXISTS subquery's — the outer list
        // has none (contrast the selects).
        assert_eq!(DRAIN_POD_IDLE_RUNNERS_SQL.matches("LIMIT").count(), 1);
    }

    #[test]
    fn drain_pod_trace_replays() {
        let fx = fx07();
        let trace = &fx["drain_pod"];
        assert_eq!(trace["assigned"], 3);
        let manual =
            next_for_runner_sql(RUNNER_PROVISIONING_MANUAL, VISIBILITY_PRIVATE).expect("manual");
        let desktop = next_for_runner_sql(RUNNER_PROVISIONING_DESKTOP_BUNDLED, VISIBILITY_PRIVATE)
            .expect("desktop");
        // Runner-first loop: manual, desktop, manual — each runner's
        // pick query matches its arm, normalized per runner id.
        for (index, (runner, arm)) in [
            ("'297ebdfd8d3b44aeb295964880f92013'::uuid", &manual),
            ("'44cfb1285f62479bbb9ea0d0910ad6cd'::uuid", &desktop),
            ("'47584f3f13c148a4bb209988e8e74795'::uuid", &manual),
        ]
        .into_iter()
        .enumerate()
        {
            let django = trace["sql"][3 + index * 3].as_str().expect("pick sql");
            assert_eq!(normalize_next(django, runner), *arm, "pick {index}");
        }
        // Assignment writes: SET order owner, runner, status,
        // assigned_at — one per pick, in loop order.
        for (index, (runner, run, stamp)) in [
            (
                "'297ebdfd8d3b44aeb295964880f92013'::uuid",
                "'bc03c3d02e8b460199bee84bca719b9c'::uuid",
                "'2026-10-03 00:33:34.925146+00:00'::timestamptz",
            ),
            (
                "'44cfb1285f62479bbb9ea0d0910ad6cd'::uuid",
                "'49a5538b7aa949e0952e13881b619ed2'::uuid",
                "'2026-10-03 00:33:34.932592+00:00'::timestamptz",
            ),
            (
                "'47584f3f13c148a4bb209988e8e74795'::uuid",
                "'22fb3d92469e44679ac1ec4ec2e3c2d6'::uuid",
                "'2026-10-03 00:33:34.939144+00:00'::timestamptz",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let django = trace["sql"][4 + index * 3].as_str().expect("update sql");
            let normalized = django
                .replace(OWNER, "$1")
                .replace(runner, "$2")
                .replace(stamp, "$3")
                .replace(run, "$4");
            assert_eq!(normalized, ASSIGN_RUN_UPDATE_SQL, "update {index}");
        }
        // Outcomes: q1 lands on idle2, the pinned run assigns, q2
        // stays queued (three runners, three picks).
        assert_eq!(trace["q1_after"]["status"], "assigned");
        assert_eq!(trace["q1_runner_is_idle2"], true);
        assert_eq!(trace["pinned_after_status"], "assigned");
        assert_eq!(trace["q2_still_queued"], "queued");
    }

    #[test]
    fn assign_frames_match_redis_payloads() {
        let fx = fx07();
        let trace = &fx["drain_pod"];
        let redis = trace["redis"].as_array().expect("redis");
        let xadds: Vec<&Value> = redis.iter().filter(|c| c["cmd"] == "xadd").collect();
        assert_eq!(xadds.len(), 3);
        for xadd in xadds {
            let args = xadd["args"].as_array().expect("args");
            // args[1] is the Python dict repr of the stream fields.
            let fields = args[1].as_str().expect("fields");
            let payload_start = fields.find("'payload': '").expect("payload") + 12;
            // The payload ends where the repr's `...<truncated 17 chars>`
            // marker or the closing quote begins; the JSON text is a
            // prefix of the full payload either way.
            let tail = &fields[payload_start..];
            let payload_text = tail.find("...<truncated 17 chars>").map_or_else(
                || tail.rsplit("', '").next().expect("repr tail"),
                |cut| &tail[..cut],
            );
            // Unescape the JSON (repr doubles the backslashes).
            let json_text = payload_text.replace("\\\"", "\"").replace("\\\\", "\\");
            // The truncated tail drops the closing `\"}`; restore it.
            let json_text = if json_text.ends_with('}') {
                json_text
            } else {
                format!("{json_text}\"}}")
            };
            let payload: Value = serde_json::from_str(&json_text).expect("payload parses");
            let mut rebuilt = payload.as_object().expect("object").clone();
            let mid = rebuilt.remove("mid").expect("mid appended last");
            assert!(mid.is_string());
            let keys: Vec<&str> = rebuilt.keys().map(String::as_str).collect();
            assert_eq!(
                keys,
                [
                    "v",
                    "type",
                    "run_id",
                    "work_item_id",
                    "prompt",
                    "repo_url",
                    "repo_ref",
                    "git_work_branch",
                    "expected_codex_model",
                    "approval_policy_overrides",
                    "deadline",
                ]
            );
            let run_id = rebuilt["run_id"].as_str().expect("run_id");
            let work_item_id = rebuilt["work_item_id"].as_str();
            let expected = build_assign_msg(
                run_id,
                work_item_id,
                rebuilt["prompt"].as_str().expect("prompt"),
                &Map::new(),
            );
            assert_eq!(Value::Object(rebuilt), Value::Object(expected));
        }
    }

    // -- drain_for_runner ---------------------------------------------------

    #[test]
    fn drain_for_runner_lock_sql_matches_fixture() {
        let fx = fx07();
        let django = fx["drain_for_runner"]["sql"][1].as_str().expect("sql");
        let normalized = django
            .replace("'2026-10-03 00:32:04.970281+00:00'::timestamptz", "$1")
            .replace("'4d0260535a2e406e8e84ee47c9f6d7ff'::uuid", "$2");
        assert_eq!(normalized, DRAIN_FOR_RUNNER_LOCK_SQL);
        assert_eq!(runner_projection(DRAIN_FOR_RUNNER_LOCK_SQL), RUNNER_COLUMNS);
        // No explicit order_by: both Meta.ordering keys survive.
        assert!(DRAIN_FOR_RUNNER_LOCK_SQL.ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED"
        ));
    }

    #[test]
    fn drain_for_runner_trace_replays() {
        let fx = fx07();
        let trace = &fx["drain_for_runner"];
        assert_eq!(trace["assigned"], true);
        let manual =
            next_for_runner_sql(RUNNER_PROVISIONING_MANUAL, VISIBILITY_PRIVATE).expect("manual");
        let django = trace["sql"][3].as_str().expect("pick sql");
        assert_eq!(
            normalize_next(django, "'4d0260535a2e406e8e84ee47c9f6d7ff'::uuid"),
            manual
        );
        let django = trace["sql"][4].as_str().expect("update sql");
        let normalized = django
            .replace(OWNER, "$1")
            .replace("'4d0260535a2e406e8e84ee47c9f6d7ff'::uuid", "$2")
            .replace("'2026-10-03 00:33:34.984534+00:00'::timestamptz", "$3")
            .replace("'20077b4522634eb3b53a2b6ab178e067'::uuid", "$4");
        assert_eq!(normalized, ASSIGN_RUN_UPDATE_SQL);
        assert_eq!(trace["assigned_run_is_q2_oldest"], true);
        assert_eq!(trace["assigned_run_after"]["status"], "assigned");
        assert_eq!(trace["q3_still_queued"], "queued");
    }

    #[test]
    fn sessionless_keeps_assigned_row_and_raises() {
        let fx = fx07();
        let trace = &fx["drain_for_runner_sessionless"];
        assert_eq!(trace["assigned_return"], true);
        assert_eq!(trace["assigned_run_status"], "assigned");
        assert!(trace["raised"]
            .as_str()
            .expect("raised")
            .starts_with("RunnerOfflineError"));
        assert_eq!(trace["assigned_run_is_q3_oldest"], true);
        assert_eq!(trace["q4_still_queued"], "queued");
        // The lock + pick + update run exactly as on the live path.
        let django = trace["sql"][2].as_str().expect("lock sql");
        let normalized = django
            .replace("'2026-10-03 00:32:05.000033+00:00'::timestamptz", "$1")
            .replace("'0855976a03924c1bb653cce42a6e88c6'::uuid", "$2");
        assert_eq!(normalized, DRAIN_FOR_RUNNER_LOCK_SQL);
        let manual =
            next_for_runner_sql(RUNNER_PROVISIONING_MANUAL, VISIBILITY_PRIVATE).expect("manual");
        let django = trace["sql"][4].as_str().expect("pick sql");
        assert_eq!(
            normalize_next(django, "'0855976a03924c1bb653cce42a6e88c6'::uuid"),
            manual
        );
        let django = trace["sql"][5].as_str().expect("update sql");
        let normalized = django
            .replace(OWNER, "$1")
            .replace("'0855976a03924c1bb653cce42a6e88c6'::uuid", "$2")
            .replace("'2026-10-03 00:33:35.010481+00:00'::timestamptz", "$3")
            .replace("'b904638a37fc43188bdc3c5e1d91de2b'::uuid", "$4");
        assert_eq!(normalized, ASSIGN_RUN_UPDATE_SQL);
        // No stream write: the on_commit send finds no session and
        // raises out of the commit (outbox-owned lookup at sql[8]).
        assert!(trace["redis"].as_array().expect("redis").is_empty());
    }

    // -- by-id lookups --------------------------------------------------------

    #[test]
    fn by_id_lookups_carry_manager_scope_and_ordering() {
        // Pod projection pins against the fixture's pod SELECT list
        // (drain_pod.sql[2]); the lookup adds the manager scope,
        // Meta.ordering and LIMIT 1 per the Django compiler rules.
        let fx = fx07();
        let pod_select = fx["drain_pod"]["sql"][2].as_str().expect("sql");
        let projected: Vec<&str> = pod_select
            .strip_prefix("SELECT ")
            .expect("SELECT")
            .split_once(" FROM \"pod\"")
            .expect("FROM pod")
            .0
            .split(", ")
            .map(|c| {
                c.strip_prefix("\"pod\".\"")
                    .expect("qualified")
                    .strip_suffix('"')
                    .expect("quoted")
            })
            .collect();
        assert_eq!(projected, POD_COLUMNS);
        assert!(DRAIN_POD_BY_ID_LOOKUP_SQL
            .contains("WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1)"));
        assert!(DRAIN_POD_BY_ID_LOOKUP_SQL
            .ends_with("ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1"));
        assert_eq!(
            runner_projection(DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL),
            RUNNER_COLUMNS
        );
        assert!(DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL.contains("WHERE \"runner\".\"id\" = $1 ORDER BY"));
        assert!(DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL.ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1"
        ));
        // Missing outcomes: False / 0, no transaction.
        assert_eq!(fx07()["drain_for_runner_by_id_missing"], false);
        assert_eq!(fx07()["drain_pod_by_id_missing"], 0);
    }

    // -- assignment plan ------------------------------------------------------

    #[test]
    fn assignment_plan_orders_set_clauses_and_effect() {
        let facts = AssignmentFacts {
            run_id: Uuid::parse_str("bc03c3d0-2e8b-4601-99be-e84bca719b9c").expect("run"),
            runner_id: Uuid::parse_str("297ebdfd-8d3b-44ae-b295-964880f92013").expect("runner"),
            owner_id: Uuid::parse_str("ce755a5b-0c64-4e83-8f27-6218e639df62").expect("owner"),
            work_item_id: Uuid::parse_str("d36a0a49-a1f3-41de-a09d-f8a74e031f4c").ok(),
            prompt: String::new(),
            run_config: Map::new(),
        };
        let plan = plan_assignment(&facts);
        let columns: Vec<&str> = plan.set_clauses.iter().map(|c| c.column).collect();
        assert_eq!(columns, ["owner_id", "runner_id", "status", "assigned_at"]);
        assert_eq!(
            plan.set_clauses[0].value,
            SetValue::Text("ce755a5b-0c64-4e83-8f27-6218e639df62".to_string())
        );
        assert_eq!(
            plan.set_clauses[1].value,
            SetValue::Text("297ebdfd-8d3b-44ae-b295-964880f92013".to_string())
        );
        assert_eq!(
            plan.set_clauses[2].value,
            SetValue::Text("assigned".to_string())
        );
        assert_eq!(plan.set_clauses[3].value, SetValue::Now);
        // The frame is the shapes sub-issue's builder, byte-equal to
        // the first drain_pod redis payload minus the appended mid.
        let fx = fx07();
        let payload = &fx["drain_pod"]["redis"][1]["args"][1]
            .as_str()
            .expect("fields");
        assert!(payload.contains(&format!("\"run_id\": \"{}\"", facts.run_id)));
        assert_eq!(
            plan.message["run_id"],
            Value::String(facts.run_id.to_string())
        );
        assert_eq!(
            plan.after_commit,
            DrainEffect::SendAssign {
                runner_id: facts.runner_id,
                message: plan.message.clone(),
            }
        );
    }

    // -- log lines --------------------------------------------------------------

    #[test]
    fn log_lines_match_python() {
        let pod = Uuid::parse_str("c63d5b13-d567-45de-932a-6e5e28568cfb").expect("pod");
        assert_eq!(
            drain_pod_log(&pod, 3),
            "drain_pod: pod=c63d5b13-d567-45de-932a-6e5e28568cfb assigned 3 run(s)"
        );
        let runner = Uuid::parse_str("4d026053-5a2e-406e-8e84-ee47c9f6d7ff").expect("runner");
        let run = Uuid::parse_str("20077b45-2263-4eb3-b53a-2b6ab178e067").expect("run");
        assert_eq!(
            drain_for_runner_log(&runner, &run),
            "drain_for_runner: runner=4d026053-5a2e-406e-8e84-ee47c9f6d7ff assigned run=20077b45-2263-4eb3-b53a-2b6ab178e067"
        );
    }
}
