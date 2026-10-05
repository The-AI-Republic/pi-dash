#![forbid(unsafe_code)]

//! Orchestration entry points: transition + comment + scheduler + signals (D-12 L7, stage 5).
//!
//! Port of the entries half of `apps/api/pi_dash/orchestration/service.py` plus
//! `orchestration/signals.py` as explicit calls:
//!
//! * `_is_delegation_trigger` (`service.py:170-177`) → [`is_delegation_trigger`].
//! * `handle_issue_state_transition` (`:180-300`) → [`handle_issue_state_transition`].
//! * `handle_issue_comment` (`:349-440`) → [`handle_issue_comment`]; the eligible
//!   groups (`:341-346`) are the reused L1 [`CONTINUATION_ELIGIBLE_GROUPS`].
//! * `dispatch_scheduler_run` (`:846-1001`) → [`dispatch_scheduler_run`].
//! * `capture_prior_state` (`signals.py:61-71`) → [`capture_prior_state`].
//! * `fire_state_transition` (`:74-107`) → [`fire_state_transition`].
//! * `_lookup_state` (`:110-116`) — folded into [`fire_state_transition`]
//!   (missing id → `None`, like the `DoesNotExist` catch).
//! * `apps.py:ready()` is Django wiring only (imports the signals module) —
//!   no port; the receivers above are the explicit calls D-26 save paths invoke.
//!
//! The drivers are async over [`EntriesSeam`] (which extends L6's
//! [`CreationSeam`](super::creation::CreationSeam) — the `cloud_agent.creation`
//! calls stay behind it, so Python's control flow stays services-side in
//! verbatim order — plus L6's
//! [`FinalizeAgentRunSeam`](super::creation::FinalizeAgentRunSeam) for the
//! FAILED paths) and over [`PreflightSeam`] for the L8
//! `preflight_eligibility_or_bounce` call. L8 (PIDASHCONV-597) is unmerged,
//! so the trait lives here and L8 implements it later — the same one-method
//! seam precedent L6 set for D-15's finalizer. The jobs-side live
//! [`EntriesSeam`] implementation is future work for the D-26 save paths that
//! will call [`fire_state_transition`]; the SQL text each method must execute
//! verbatim lives in the adjacent `*_SQL` consts.
//!
//! Read seams (reused, never re-ported): L1 ticking registry / events /
//! decisions / outcomes (`pidash_types::orchestration`), L5 [`reconcile`](super::clock::reconcile)
//! (pure — the drivers lock, call, and save around it), L6 builders /
//! parenting / resolvers, the F-06 [`check_project_role`](pidash_auth::permissions::membership::check_project_role)
//! kernel over L2 role-fact SQL, D-11 L3
//! [`user_has_llm_config`](crate::dispatch::policy::user_has_llm_config), and
//! the merged prompting scheduler composer. `run_config` for scheduler runs
//! is the literal `{}` (`service.py:963`); scheduler rows stamp no
//! `phase_kind` (the model default `""`).
//!
//! Fixture: FX-ORCH-07 (`rust-api/fixtures/orchestration/fx07_entries/`:
//! `transition.matrix.json`, `comment.matrix.json`,
//! `scheduler_dispatch.before_after.json`, `signals.golden.json`). The replays
//! below run the real L5/L6 drivers through an in-memory [`EntriesSeam`] with
//! scripted jobs-side effects — the same shape the fixture generator used
//! (real reconcile + preflight + builders + composer except where a case
//! stubs a seam). Timestamps pin exact port math at fixed caller-drawn jitter
//! (L5 quirk-5: no Rust RNG reproduces CPython's stream, so the requests carry
//! `jitter_secs`); prompt bodies pin render wiring (non-empty, marker
//! substrings, manifest shape), not fixture bytes — byte fidelity of the
//! composer is L6's proven unit.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Existing quirks ported as-is (translation, don't redesign):
//!
//! 1. Reason strings verbatim — the oracle and callers match on them
//!    (`not-a-trigger-state`, `dispatch-deferred-to-caller`, `pool-spent`,
//!    `entry-queued`, `no-dispatch`, `active-run-exists`, `no-creator`,
//!    `no-pod-available`, `no-eligible-runner`, plus the comment set).
//! 2. The T7 active-race guard is a true race net: reconcile said
//!    `dispatch_now` while a run exists. Unreachable through one
//!    synchronous pass (a busy issue always queues) — the replay scripts
//!    the two `_active_run_for` reads around an interleaved insert.
//! 3. The T6b bare `no-dispatch` fallback is stub-only: every real
//!    enter/move reconcile path sets a reason (all five
//!    `on_enter_or_move` branches do). The `or "no-dispatch"` expression
//!    is kept verbatim.
//! 4. The scheduler outcome-mode refusal compares the *execution's*
//!    executor kind (post-`execution_fields`), not the project default.
//! 5. Admission/render failures return the FAILED run with a `None` error
//!    (`(run, None)`), so the Beat loop records `last_run` and skips
//!    `last_error`; only the short-circuits return `(None, reason)`.
//! 6. `capture_prior_state` reads through `all_objects` (soft-deleted rows
//!    included) and answers `None` for new instances and missing rows.
//! 7. `fire_state_transition` never lets the *handler* crash the save
//!    (counter + verbatim log line); a *lookup* storage failure still
//!    propagates, as in Python where only the handler call is in `try`.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use pidash_auth::permissions::membership::{check_project_role, ProjectRoleFacts};
use pidash_db::app_project::models::state::StateGroup;
use pidash_db::dispatch::status::{AgentRunStatus, AgentRunTrigger};
use pidash_db::orchestration::querysets::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_db::orchestration::runs::ACTIVE_STATUSES;
use pidash_db::orchestration::workpad::AgentUserCollisionError;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::orchestration::{
    is_ticking_state, ContinuationOutcome, StateRef, TickerDecision, TickerEvent,
    TransitionOutcome, CONTINUATION_ELIGIBLE_GROUPS,
};

use crate::dispatch::policy::{user_has_llm_config, UserFlags};
use crate::prompting::composer::OverrideRow;
use crate::prompting::context::SchedulerContextInput;
use crate::prompting::{composer, context};

use super::clock::ClockError;
use super::clock::{self, ClockIssue, ClockWrite, ProjectClockPolicy};
use super::creation::{
    create_and_dispatch_run, create_continuation_run, parent_for_next_run,
    resolve_fallback_creator, resolve_pod_for_issue, ActorRequest, ContinuationRequest,
    CreateDispatchRequest, CreationError, CreationSeam, ExecutionError, ExecutionRequest,
    FinalizeAgentRunSeam, PodView, ProjectView, RenderedTurn, RunView, StateView,
    ERROR_CODE_PROMPT_BUILD_FAILED, RUN_VIEW_COLUMNS,
};

// ---------------------------------------------------------------------------
// Reasons, error strings, signal attributes (all verbatim)
// ---------------------------------------------------------------------------

/// Leave-bucket and non-trigger answers (`service.py:223,226`).
pub const REASON_NOT_A_TRIGGER_STATE: &str = "not-a-trigger-state";
/// `dispatch_immediate=False`: the clock was updated, the caller dispatches (`:245`).
pub const REASON_DISPATCH_DEFERRED: &str = "dispatch-deferred-to-caller";
/// An agent moved the issue while the pool is spent (`:247`).
pub const REASON_POOL_SPENT: &str = "pool-spent";
/// The clock queued the entry (`:249`, comment `:423`).
pub const REASON_ENTRY_QUEUED: &str = "entry-queued";
/// Bare fallback when a no-dispatch decision carries no reason (`:251`).
/// Stub-only: every real enter/move path sets a reason (quirk 3).
pub const REASON_NO_DISPATCH: &str = "no-dispatch";
/// The post-reconcile race net tripped (`:262`).
pub const REASON_ACTIVE_RUN_EXISTS: &str = "active-run-exists";
/// No actor and no fallback creator (`:274`).
pub const REASON_NO_CREATOR: &str = "no-creator";
/// Neither assigned nor default pod exists (`:283`, comment `:405`).
pub const REASON_NO_POD_AVAILABLE: &str = "no-pod-available";
/// Preflight bounced the issue (`:291`).
pub const REASON_NO_ELIGIBLE_RUNNER: &str = "no-eligible-runner";
/// Comment without an actor (`service.py:369`).
pub const REASON_NO_ACTOR: &str = "no-actor";
/// Comment authored by a bot (`:372`).
pub const REASON_BOT_COMMENT: &str = "bot-comment";
/// Comment on an issue outside [`CONTINUATION_ELIGIBLE_GROUPS`] (`:382`).
pub const REASON_STATE_NOT_ELIGIBLE: &str = "state-not-eligible";
/// Comment on an issue with no prior run (`:387`).
pub const REASON_NO_PRIOR_RUN: &str = "no-prior-run";
/// A QUEUED follow-up already exists; the new comment rides along (`:396`).
pub const REASON_COALESCED: &str = "coalesced";
/// The prior run is still in flight; the clock write rolls back (`:429`).
pub const REASON_PRIOR_RUN_ACTIVE: &str = "prior-run-active";

/// Cloud scheduler with no usable human principal (`service.py:924`).
pub const SCHEDULER_NO_CREATOR: &str = "no current human execution principal";
/// Cloud scheduler refusing a non-`create_issue` binding (`:942`).
pub const SCHEDULER_OUTCOME_MODE_REFUSAL: &str =
    "Cloud Agent scheduler runs support only the create_issue outcome mode";
/// The only outcome mode a Cloud Agent scheduler run supports (`:938`).
pub const OUTCOME_MODE_CREATE_ISSUE: &str = "create_issue";

/// `f"no default pod for project {binding.project_id}"` (`service.py:889`).
/// A `None` project renders `None`, as Python's `str()` does.
pub fn no_default_pod_message(project_id: Option<Uuid>) -> String {
    match project_id {
        Some(id) => format!("no default pod for project {id}"),
        None => "no default pod for project None".to_owned(),
    }
}

/// `signals.py:37` — the pre-save snapshot attribute. Kept as a const for
/// the golden; in Rust the value travels as [`FireRequest::prev_state_id`].
pub const PREVIOUS_STATE_ATTR: &str = "_orchestration_prev_state_id";
/// `signals.py:43` — the per-instance immediate-dispatch opt-out. Kept for
/// the golden; in Rust it is [`FireRequest::dispatch_immediate`].
pub const DISPATCH_IMMEDIATE_ATTR: &str = "_orchestration_dispatch_immediate";
/// `signals.py:52` — the per-instance moved-by-run carrier. Kept for the
/// golden; in Rust it is [`FireRequest::moved_by_run`].
pub const MOVED_BY_RUN_ATTR: &str = "_orchestration_moved_by_run";

/// The roles the scheduler creator chain passes to `check_project_role`
/// (`service.py:909`): `[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST]`.
pub const SCHEDULER_ALLOWED_ROLES: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

// ---------------------------------------------------------------------------
// SQL: entries flow (executed verbatim by the jobs-side store)
// ---------------------------------------------------------------------------

/// `capture_prior_state` (`signals.py:67`):
/// `Issue.all_objects.only("state_id").get(pk)` — soft-deleted rows
/// included (no `deleted_at` guard), pk plus `state_id` only. Django
/// renders `... WHERE "issues"."id" = X ORDER BY "issues"."created_at"
/// DESC LIMIT 21`; the ordering and page size are unobservable on a pk
/// lookup, so the const keeps the L6 pk-read shape. `$1` is the issue id.
pub const PRIOR_STATE_SELECT_SQL: &str = "SELECT id, state_id FROM issues WHERE id = $1";

/// The coalesce lookup (`service.py:392-394`):
/// `AgentRun.objects.filter(work_item=issue, status=QUEUED).order_by("-created_at").first()`
/// over [`RUN_VIEW_COLUMNS`]. `$1` is the work-item id.
pub fn queued_follow_up_sql() -> String {
    format!(
        "SELECT {} FROM agent_run WHERE work_item_id = $1 AND status = 'queued' \
         ORDER BY created_at DESC LIMIT 1",
        RUN_VIEW_COLUMNS.join(", ")
    )
}

/// The scheduler pod-override read (`service.py:876-880`):
/// `Pod.objects.filter(pk, deleted_at__isnull=True, project_id).first()`,
/// projected to the consumed columns. Django renders the `deleted_at`
/// guard twice (the default manager's plus the explicit filter's) —
/// kept verbatim. `$1` is the pod id, `$2` the binding's project id
/// (`= NULL` never matches, mirroring `filter(project_id=None)`).
pub const SCHEDULER_POD_OVERRIDE_SQL: &str = "SELECT id, project_id FROM pod \
    WHERE deleted_at IS NULL AND deleted_at IS NULL AND id = $1 AND project_id = $2";

/// The consumed `scheduler_bindings` projection. A plain pk read (no
/// deleted guard): the Beat loop passes the live binding object, and the
/// read must return that same row. `$1` is the binding id.
pub const BINDING_SELECT_SQL: &str = "SELECT id, workspace_id, project_id, scheduler_id, \
    outcome_mode, extra_context, actor_id, pod_id FROM scheduler_bindings WHERE id = $1";

/// The consumed `schedulers` projection for the `binding.scheduler` FK
/// follow (`context.py:675`): the default manager scope applies, so a
/// soft-deleted scheduler misses (and the driver raises, as the FK
/// follow would). `$1` is the scheduler id.
pub const SCHEDULER_SELECT_SQL: &str =
    "SELECT id, slug, name, description, prompt FROM schedulers WHERE id = $1 AND deleted_at IS NULL";

/// The project clock policy for [`reconcile`](clock::reconcile) — the one
/// project read the entries drivers add beyond L6's [`ProjectView`]
/// (which lacks `agent_ticking_enabled`). A seam-supporting read: Python
/// reaches these fields through the cached `issue.project`. `$1` is the
/// project id.
pub const CLOCK_POLICY_SQL: &str = "SELECT agent_ticking_enabled, agent_default_max_ticks, \
    agent_default_interval_seconds, agent_review_default_interval_seconds, \
    agent_test_default_interval_seconds FROM projects WHERE id = $1";

/// The scheduler full-row INSERT (`AgentRun.objects.create`,
/// `service.py:953-965`): the same 38 non-generated columns in model
/// order as L6's `RUN_INSERT_SQL`, with the scheduler VALUES shape —
/// `work_item` NULL, `scheduler_binding` bound, `parent_run` NULL, the
/// `'scheduler'` trigger and `'{}'` run config as literals (fixed by the
/// call path, like `'queued'`), no `phase_kind` stamp (the model default
/// `""`). `$1..$10` carry the caller-supplied fields in
/// [`NewSchedulerRun`] order.
pub const SCHEDULER_RUN_INSERT_SQL: &str =
    "INSERT INTO agent_run (id, workspace_id, owner_id, created_by_id, \
    pod_id, runner_id, pinned_runner_id, work_item_id, scheduler_binding_id, parent_run_id, \
    status, executor_kind, dispatch_attempts, cancel_requested_at, cancel_reason, error_code, \
    tool_plan, terminal_hooks_applied_at, terminal_capacity_released_at, prompt, trigger, \
    prompt_manifest, phase_kind, run_config, required_capabilities, thread_id, agent_metadata, \
    lease_expires_at, done_payload, error, refusal_category, llm_model, usage, created_at, \
    assigned_at, queue_position, started_at, ended_at) \
    VALUES ($1, $2, NULL, $3, $4, NULL, $5, NULL, $6, NULL, 'queued', $7, 0, NULL, '', $8, $9, \
    NULL, NULL, '', 'scheduler', NULL, '', '{}', '[]', '', '{}', NULL, NULL, '', '', '', '{}', $10, \
    NULL, NULL, NULL, NULL)";

/// The scheduler INSERT as executed: [`SCHEDULER_RUN_INSERT_SQL`] plus
/// `RETURNING` the [`RUN_VIEW_COLUMNS`] projection, like `objects.create`.
pub fn scheduler_run_insert_returning_sql() -> String {
    format!(
        "{SCHEDULER_RUN_INSERT_SQL} RETURNING {}",
        RUN_VIEW_COLUMNS.join(", ")
    )
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// The consumed `issue_comments` projection. Python is handed the comment
/// object (no query), so the driver takes this view as input instead of
/// inventing a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentView {
    pub id: Uuid,
    pub issue_id: Uuid,
    /// `None` is the system-authored comment (`service.py:367`).
    pub actor_id: Option<Uuid>,
}

/// The consumed `scheduler_bindings` projection ([`BINDING_SELECT_SQL`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// `None` only for the pathological project-less binding (the base
    /// model allows it; the dispatcher answers the no-pod error then).
    pub project_id: Option<Uuid>,
    pub scheduler_id: Uuid,
    pub outcome_mode: String,
    pub extra_context: String,
    pub actor_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
}

/// The consumed `schedulers` projection ([`SCHEDULER_SELECT_SQL`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerView {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub prompt: String,
}

/// The consumed `workspaces` projection (`BUNDLE_WORKSPACE_SQL`, reused —
/// never re-ported).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceView {
    pub slug: String,
    pub name: String,
}

/// One scheduler `agent_run` INSERT ([`SCHEDULER_RUN_INSERT_SQL`]): the
/// caller-supplied half of the full-row write, in `$1..$10` order.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSchedulerRun {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub created_by_id: Uuid,
    pub pod_id: Uuid,
    /// The execution pin merged over no computed pin (there is no parent
    /// run to inherit affinity from).
    pub pinned_runner_id: Option<Uuid>,
    pub binding_id: Uuid,
    pub executor_kind: AgentExecutorKind,
    /// The `error_code` splat (`""` except the managed waiting path).
    pub error_code: String,
    pub tool_plan: Value,
    /// `created_at` (`auto_now_add`, supplied explicitly).
    pub now: DateTime<Utc>,
}

/// What `dispatch_scheduler_run` decided: the run (if one was produced)
/// and the short-circuit error (if none was). At most one is `Some` —
/// a FAILED run still answers `(run, None)` (quirk 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerOutcome {
    pub run: Option<Uuid>,
    pub error: Option<String>,
}

/// What [`fire_state_transition`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FireOutcome {
    /// No state change — the handler was not called.
    NoTransition,
    /// The handler ran; its outcome.
    Called(TransitionOutcome),
    /// The handler raised; the save survives with the counter bumped and
    /// this verbatim log line for the handler to emit.
    Failed {
        log_line: String,
        error: String,
        total_errors: u64,
    },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure the entries drivers report.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EntriesError {
    /// Any database failure (the store stringifies its `sqlx::Error` —
    /// this crate carries no `sqlx`), or a dangling-FK follow.
    #[error(transparent)]
    Store(#[from] CreationError),
    /// `reconcile` on an unknown event kind — unreachable from these
    /// drivers (they only construct known kinds), kept fallible rather
    /// than panicking.
    #[error(transparent)]
    Clock(#[from] ClockError),
    /// The reserved agent username belongs to a human account — the
    /// verbatim L2 [`AgentUserCollisionError`], propagated uncaught as
    /// in Python.
    #[error(transparent)]
    AgentUserCollision(#[from] AgentUserCollisionError),
}

// ---------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------

/// Storage + executor seam for the entries drivers.
///
/// Extends L6's [`CreationSeam`] (storage reads, executor calls, prompt
/// bundle) and [`FinalizeAgentRunSeam`] (the FAILED paths) with the reads
/// and writes the entries units add. Methods mirror the Django ORM calls,
/// one per query shape; the SQL text for each lives in the adjacent
/// `*_SQL` consts, which the jobs-side store must execute verbatim.
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam.
#[allow(async_fn_in_trait)]
pub trait EntriesSeam: CreationSeam + FinalizeAgentRunSeam {
    /// The pre-save `state_id` snapshot ([`PRIOR_STATE_SELECT_SQL`]):
    /// `None` covers both a NULL pointer and a missing row (the
    /// `DoesNotExist` catch).
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError>;
    /// The newest QUEUED follow-up ([`queued_follow_up_sql`]).
    async fn queued_follow_up(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError>;
    /// The issue's ticker row, locked (`clock::lock_ticker_sql`).
    async fn lock_ticker(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError>;
    /// Persist the reconciled row: `clock::create_ticker_sql` on
    /// [`ClockWrite::Insert`], `clock::SAVE_CLOCK_SQL` on
    /// [`ClockWrite::Update`].
    async fn save_ticker(
        &mut self,
        row: &IssueAgentTicker,
        write: ClockWrite,
    ) -> Result<(), CreationError>;
    /// `transaction.set_rollback(True)`: discard this driver's
    /// transaction scope. The live store rolls back; nothing executes
    /// after except the outcome return.
    fn set_rollback(&mut self);
    /// The project's clock policy ([`CLOCK_POLICY_SQL`]).
    async fn clock_policy(&mut self, project_id: Uuid)
        -> Result<ProjectClockPolicy, CreationError>;
    /// The scheduler binding ([`BINDING_SELECT_SQL`]); a set id with no
    /// row is [`CreationError::MissingRow`].
    async fn binding(&mut self, binding_id: Uuid) -> Result<BindingView, CreationError>;
    /// The live override pod of the binding's project
    /// ([`SCHEDULER_POD_OVERRIDE_SQL`]).
    async fn scheduler_override_pod(
        &mut self,
        pod_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError>;
    /// The workspace slug + name (`BUNDLE_WORKSPACE_SQL`).
    async fn workspace(&mut self, workspace_id: Uuid) -> Result<WorkspaceView, CreationError>;
    /// The scheduler row ([`SCHEDULER_SELECT_SQL`]).
    async fn scheduler_row(&mut self, scheduler_id: Uuid) -> Result<SchedulerView, CreationError>;
    /// Active prompt overrides for the scheduler compose
    /// (`BUNDLE_OVERRIDES_SQL` with `$2 = NULL`: workspace rows only —
    /// scheduler runs are always automatic).
    async fn scheduler_override_rows(
        &mut self,
        workspace_id: Uuid,
    ) -> Result<Vec<OverrideRow>, CreationError>;
    /// The `check_project_role` EXISTS trio for the candidate
    /// (`project_has_allowed_role_sql(3)` with
    /// [`SCHEDULER_ALLOWED_ROLES`], `PROJECT_IS_MEMBER_SQL`,
    /// `WORKSPACE_ADMIN_BY_SLUG_SQL`).
    async fn project_role_facts(
        &mut self,
        user_id: Uuid,
        workspace_slug: &str,
        project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError>;
    /// The EE-overlayable `has_usable_llm_config` seam for the
    /// candidate (CE: BYOK key presence). Called only after the flag
    /// and role gates pass, as in Python.
    async fn has_usable_llm_config(&mut self, user_id: Uuid) -> Result<bool, CreationError>;
    /// `get_agent_system_user`: the dedicated bot user, created on first
    /// call ([`AGENT_USER_SELECT_SQL`](pidash_db::orchestration::workpad::AGENT_USER_SELECT_SQL),
    /// `AGENT_USER_INSERT_SQL`, `AGENT_USER_PASSWORD_SQL`). The outer
    /// error is a store failure; the inner is the verbatim collision.
    async fn agent_system_user(
        &mut self,
    ) -> Result<Result<Uuid, AgentUserCollisionError>, CreationError>;
    /// The scheduler full-row INSERT ([`SCHEDULER_RUN_INSERT_SQL`] +
    /// `RETURNING` the [`RUN_VIEW_COLUMNS`] projection, like
    /// `objects.create`).
    async fn insert_scheduler_run(
        &mut self,
        row: &NewSchedulerRun,
    ) -> Result<RunView, CreationError>;
    /// Render the scheduler prompt through the merged composer
    /// (`build_scheduler_turn`, `composer.py:426-456`): `context` is the
    /// `build_scheduler_context` value, `index` the override index, the
    /// rest the composer's scope inputs. The live store delegates to the
    /// merged `prompting` composer and maps the manifest (`to_json`, or
    /// `Null` when absent); the call is a seam — rather than a direct
    /// call like L6's issue render — because no scheduler input can fail
    /// the composer (all six recipe sections are locked, so override
    /// rows never resolve), and the render-failed path must stay
    /// drivable (fixture S8 stubs this exact call). `Err` is the
    /// composer message for the `render-failed` path.
    fn compose_scheduler_turn(
        &mut self,
        context: &Value,
        index: &composer::OverrideIndex,
        workspace_id: Option<&str>,
        executor_kind: Option<&str>,
        tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String>;
}

/// The one-method seam for the `preflight_eligibility_or_bounce` call in
/// the transition driver (`scheduling.py:955-1026`). Preflight is L8
/// (PIDASHCONV-597, unmerged): L8 implements this trait later, and the
/// D-26 save paths that call [`fire_state_transition`] supply the
/// implementation — the D-10 `FireTickSeam` / L6 `FinalizeAgentRunSeam`
/// precedent. `triggered_by` is the `AgentRunTrigger` value the bounce
/// comment records (`"state_transition"` from this driver).
#[allow(async_fn_in_trait)]
pub trait PreflightSeam {
    /// `True` when dispatch may proceed; `False` when the issue was
    /// bounced (the seam owns the Backlog move + notice comment).
    async fn preflight_eligibility_or_bounce(
        &mut self,
        issue_id: Uuid,
        creator_id: Uuid,
        pod_id: Uuid,
        triggered_by: &str,
    ) -> Result<bool, CreationError>;
}

// ---------------------------------------------------------------------------
// Pure units
// ---------------------------------------------------------------------------

/// A state transition triggers a run when `to_state` is one of the
/// registered ticking states (`_is_delegation_trigger`,
/// `service.py:170-177`).
pub fn is_delegation_trigger(to_state: Option<&StateRef<'_>>) -> bool {
    is_ticking_state(to_state)
}

/// `AgentRun.is_active` (`runner/models.py:1146-1160`): active runs occupy
/// the single-active-run slot per issue — the same 7 statuses as
/// [`ACTIVE_STATUSES`].
pub fn is_active_status(status: AgentRunStatus) -> bool {
    ACTIVE_STATUSES.contains(&status)
}

/// The `:100-106` post-commit log line, byte-verbatim: `orchestration.error:
/// handle_issue_state_transition failed for issue=<uuid> from_state=<uuid>
/// to_state=<uuid> (total_errors=<n>)`. A `None` id renders `None`, as
/// Python's `%s` does; the exception itself travels out-of-band (Python's
/// `exc_info`), in [`FireOutcome::Failed::error`].
pub fn fire_error_log_line(
    issue_pk: &Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
    total_errors: u64,
) -> String {
    format!(
        "orchestration.error: handle_issue_state_transition failed \
         for issue={issue_pk} from_state={} to_state={} (total_errors={total_errors})",
        opt_uuid(prev_state_id),
        opt_uuid(current_state_id),
    )
}

/// Python `str()` of an optional id for log lines.
fn opt_uuid(id: Option<Uuid>) -> String {
    id.map_or_else(|| "None".to_owned(), |id| id.to_string())
}

// ---------------------------------------------------------------------------
// Drivers (Python control flow, verbatim order)
// ---------------------------------------------------------------------------

/// One `handle_issue_state_transition` call (`service.py:180-300`).
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionRequest {
    pub issue_id: Uuid,
    /// The resolved states (Python is handed the objects).
    pub from_state: Option<StateView>,
    pub to_state: Option<StateView>,
    pub actor: Option<Uuid>,
    pub dispatch_immediate: bool,
    pub moved_by_run: Option<Uuid>,
    /// The clock for `next_run_at` / row stamps (frozen in tests).
    pub now: DateTime<Utc>,
    /// Caller-drawn jitter for the reconcile retime (L5 quirk-5).
    pub jitter_secs: f64,
    /// The crum user for the ticker create-shape, or `None` on system paths.
    pub created_by: Option<Uuid>,
}

/// One `handle_issue_comment` call (`service.py:349-440`).
#[derive(Debug, Clone, PartialEq)]
pub struct CommentRequest {
    pub comment: CommentView,
    /// The clock for `next_run_at` / row stamps (frozen in tests).
    pub now: DateTime<Utc>,
    /// Caller-drawn jitter for the reconcile retime (L5 quirk-5).
    pub jitter_secs: f64,
    /// The crum user for the ticker create-shape, or `None` on system paths.
    pub created_by: Option<Uuid>,
}

/// One `dispatch_scheduler_run` call (`service.py:846-848`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerRequest {
    pub binding_id: Uuid,
    /// The clock for row stamps (frozen in tests).
    pub now: DateTime<Utc>,
}

/// One `fire_state_transition` call (`signals.py:74-107`).
#[derive(Debug, Clone, PartialEq)]
pub struct FireRequest {
    pub issue_id: Uuid,
    /// The [`capture_prior_state`] snapshot vs the saved row.
    pub prev_state_id: Option<Uuid>,
    pub current_state_id: Option<Uuid>,
    pub dispatch_immediate: bool,
    pub moved_by_run: Option<Uuid>,
    /// The clock for `next_run_at` / row stamps (frozen in tests).
    pub now: DateTime<Utc>,
    /// Caller-drawn jitter for the reconcile retime (L5 quirk-5).
    pub jitter_secs: f64,
    /// The crum user for the ticker create-shape, or `None` on system paths.
    pub created_by: Option<Uuid>,
}

/// Lock the ticker, apply one clock event, and persist the write — the
/// `:374-397` `reconcile` body as the drivers call it. `state` is the
/// issue's current (post-save) stage; a project-less issue reconciles
/// under the default policy (Python's `getattr(..., True)` rows).
async fn reconcile_clock<S: EntriesSeam>(
    seam: &mut S,
    issue_id: Uuid,
    state: Option<&StateView>,
    event: &TickerEvent,
    now: DateTime<Utc>,
    jitter_secs: f64,
    created_by: Option<Uuid>,
) -> Result<TickerDecision, EntriesError> {
    let issue = seam.issue(issue_id).await?;
    let policy = match issue.project_id {
        Some(project_id) => seam.clock_policy(project_id).await?,
        None => ProjectClockPolicy::default(),
    };
    let mut ticker = seam.lock_ticker(issue_id).await?;
    let has_active_run = seam.active_run_for(issue_id).await?.is_some();
    let state_ref = state.map(|state| StateRef {
        group: &state.group,
        name: &state.name,
    });
    let clock_issue = ClockIssue {
        issue_id,
        state: state_ref,
        policy,
    };
    let outcome = clock::reconcile(
        &mut ticker,
        &clock_issue,
        event,
        None,
        has_active_run,
        now,
        jitter_secs,
        created_by,
    )?;
    match outcome.write {
        ClockWrite::None => {}
        write => {
            // A write always has its row: `Insert` created it, `Update`
            // mutated the locked one (the L5 `expect` precedent).
            let row = ticker
                .as_ref()
                .expect("reconcile write always has a post-lock row");
            seam.save_ticker(row, write).await?;
        }
    }
    Ok(outcome.decision)
}

/// React to an issue state change (`handle_issue_state_transition`,
/// `service.py:180-300`).
///
/// Classifies the move against the ticking bucket, sends the matching
/// event to the one clock, and creates the entry run when the clock says
/// the issue is free. Handler contract (the two `:386`-style commit
/// scopes, which only the executing store can do): run the reconcile
/// scope on one transaction and commit it before the builder scope —
/// Python commits `reconcile`'s atomic before the race guard, so a later
/// storage failure must not roll the clock write back.
pub async fn handle_issue_state_transition<S: EntriesSeam, P: PreflightSeam>(
    seam: &mut S,
    preflight: &mut P,
    req: &TransitionRequest,
) -> Result<TransitionOutcome, EntriesError> {
    let from_ref = req.from_state.as_ref().map(|state| StateRef {
        group: &state.group,
        name: &state.name,
    });
    let to_ref = req.to_state.as_ref().map(|state| StateRef {
        group: &state.group,
        name: &state.name,
    });
    let from_ticking = is_ticking_state(from_ref.as_ref());
    let to_ticking = is_ticking_state(to_ref.as_ref());
    let cross_stage = match (&req.from_state, &req.to_state) {
        (Some(from), Some(to)) => from_ticking && to_ticking && from.group != to.group,
        _ => false,
    };

    // Leaving In Progress (to any room): remember the latest
    // implementation run so a later hand-back parents off the
    // implementation lineage. Read *before* the clock update.
    let mut resume_parent: Option<Uuid> = None;
    if cross_stage
        && req
            .from_state
            .as_ref()
            .is_some_and(|from| from.group == StateGroup::Started.as_str())
    {
        resume_parent = seam.latest_prior_run(req.issue_id).await?.map(|run| run.id);
    }

    if from_ticking && !to_ticking {
        let event = TickerEvent::left_bucket();
        reconcile_clock(
            seam,
            req.issue_id,
            req.to_state.as_ref(),
            &event,
            req.now,
            req.jitter_secs,
            req.created_by,
        )
        .await?;
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_NOT_A_TRIGGER_STATE.to_owned(),
        });
    }

    if !is_delegation_trigger(to_ref.as_ref()) {
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_NOT_A_TRIGGER_STATE.to_owned(),
        });
    }

    let event = if from_ticking {
        TickerEvent::moved_stage(
            req.moved_by_run,
            resume_parent,
            req.dispatch_immediate,
            req.actor,
        )
    } else {
        TickerEvent::entered_bucket(
            req.moved_by_run,
            resume_parent,
            req.dispatch_immediate,
            req.actor,
        )
    };
    let decision = reconcile_clock(
        seam,
        req.issue_id,
        req.to_state.as_ref(),
        &event,
        req.now,
        req.jitter_secs,
        req.created_by,
    )
    .await?;

    if !req.dispatch_immediate {
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_DISPATCH_DEFERRED.to_owned(),
        });
    }
    if decision.parked {
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_POOL_SPENT.to_owned(),
        });
    }
    if decision.queued {
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_ENTRY_QUEUED.to_owned(),
        });
    }
    if !decision.dispatch_now {
        let reason = if decision.reason.is_empty() {
            REASON_NO_DISPATCH
        } else {
            decision.reason.as_str()
        };
        return Ok(TransitionOutcome {
            created_run: None,
            reason: reason.to_owned(),
        });
    }

    if seam.active_run_for(req.issue_id).await?.is_some() {
        // Raced with a run created between reconcile's check and here;
        // the clock is armed, so the next tick covers it.
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_ACTIVE_RUN_EXISTS.to_owned(),
        });
    }

    // Parent + session shape follow the shared stage rule so a
    // dispatch-now entry and a queued entry are built the same way.
    let to_state_id = req.to_state.as_ref().map(|state| state.id);
    let (parent, fresh_session) =
        parent_for_next_run(seam, req.issue_id, to_state_id, Some(cross_stage)).await?;

    let creator = match req.actor {
        Some(actor) => Some(actor),
        None => resolve_fallback_creator(seam, req.issue_id).await?,
    };
    let Some(creator) = creator else {
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_NO_CREATOR.to_owned(),
        });
    };

    let pod = resolve_pod_for_issue(seam, req.issue_id).await?;
    let Some(pod) = pod else {
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_NO_POD_AVAILABLE.to_owned(),
        });
    };

    if !preflight
        .preflight_eligibility_or_bounce(
            req.issue_id,
            creator,
            pod,
            AgentRunTrigger::StateTransition.value(),
        )
        .await?
    {
        return Ok(TransitionOutcome {
            created_run: None,
            reason: REASON_NO_ELIGIBLE_RUNNER.to_owned(),
        });
    }

    let outcome = create_and_dispatch_run(
        seam,
        &CreateDispatchRequest {
            issue_id: req.issue_id,
            parent,
            creator_id: creator,
            pod_id: pod,
            fresh_session,
            trigger: AgentRunTrigger::StateTransition,
            now: req.now,
        },
    )
    .await?;
    Ok(outcome)
}

/// React to a comment on an issue, maybe waking the agent
/// (`handle_issue_comment`, `service.py:349-440`). Handler contract (the
/// `:414` `transaction.atomic()`, which only the executing store can
/// do): run the reconcile check, the in-flight check, and the
/// continuation build on ONE transaction, so a dispatch that produces
/// no run leaves the clock exactly as it was.
pub async fn handle_issue_comment<S: EntriesSeam>(
    seam: &mut S,
    req: &CommentRequest,
) -> Result<ContinuationOutcome, EntriesError> {
    let Some(actor_id) = req.comment.actor_id else {
        return Ok(ContinuationOutcome {
            created_run: None,
            coalesced_into: None,
            reason: REASON_NO_ACTOR.to_owned(),
        });
    };
    if seam.user_flags(actor_id).await?.is_bot {
        return Ok(ContinuationOutcome {
            created_run: None,
            coalesced_into: None,
            reason: REASON_BOT_COMMENT.to_owned(),
        });
    }

    let issue = seam.issue(req.comment.issue_id).await?;
    let state = seam.state(issue.state_id).await?;
    let eligible = state
        .as_ref()
        .is_some_and(|state| CONTINUATION_ELIGIBLE_GROUPS.contains(&state.group.as_str()));
    if !eligible {
        return Ok(ContinuationOutcome {
            created_run: None,
            coalesced_into: None,
            reason: REASON_STATE_NOT_ELIGIBLE.to_owned(),
        });
    }

    let prior = seam.latest_prior_run(issue.id).await?;
    let Some(prior) = prior else {
        return Ok(ContinuationOutcome {
            created_run: None,
            coalesced_into: None,
            reason: REASON_NO_PRIOR_RUN.to_owned(),
        });
    };

    // Coalesce against an already-queued follow-up: the agent reads the
    // comment thread fresh on dispatch, so the queued run picks the new
    // comment up without a separate queued run per comment.
    if let Some(queued) = seam.queued_follow_up(issue.id).await? {
        return Ok(ContinuationOutcome {
            created_run: None,
            coalesced_into: Some(queued.id),
            reason: REASON_COALESCED.to_owned(),
        });
    }

    let pod = resolve_pod_for_issue(seam, issue.id).await?;
    let Some(pod) = pod else {
        return Ok(ContinuationOutcome {
            created_run: None,
            coalesced_into: None,
            reason: REASON_NO_POD_AVAILABLE.to_owned(),
        });
    };

    // A human comment is engagement: one free run. If a run is in
    // flight, the clock queues the follow-up and fires it as soon as
    // the issue is free; otherwise re-time the clock and dispatch now —
    // in one transaction, so a dispatch that produces no run leaves
    // the clock exactly as it was.
    let state_ref = state.as_ref().map(|state| StateRef {
        group: &state.group,
        name: &state.name,
    });
    if is_ticking_state(state_ref.as_ref()) {
        let event = TickerEvent::human_run_requested(
            true,
            Some(actor_id),
            AgentRunTrigger::CommentAndRun.value(),
        );
        let decision = reconcile_clock(
            seam,
            issue.id,
            state.as_ref(),
            &event,
            req.now,
            req.jitter_secs,
            req.created_by,
        )
        .await?;
        if decision.queued {
            return Ok(ContinuationOutcome {
                created_run: None,
                coalesced_into: None,
                reason: REASON_ENTRY_QUEUED.to_owned(),
            });
        }
    }

    // Don't wake while a run is already in flight; the queued entry (or
    // the terminate sweep) handles it.
    if is_active_status(prior.status) {
        seam.set_rollback();
        return Ok(ContinuationOutcome {
            created_run: None,
            coalesced_into: None,
            reason: REASON_PRIOR_RUN_ACTIVE.to_owned(),
        });
    }

    let outcome = create_continuation_run(
        seam,
        &ContinuationRequest {
            issue_id: issue.id,
            parent: prior,
            creator_id: actor_id,
            pod_id: pod,
            trigger: AgentRunTrigger::CommentAndRun,
            now: req.now,
        },
    )
    .await?;
    if outcome.created_run.is_none() {
        seam.set_rollback();
    }
    Ok(outcome)
}

/// Whether the scheduler candidate has a usable LLM config
/// (`agent_execution.py:69-80`) via the merged D-11 L3 policy. The flag
/// short-circuit runs first so the EE seam is never consulted for an
/// inactive or bot candidate, as in Python.
async fn candidate_has_llm_config<S: EntriesSeam>(
    seam: &mut S,
    candidate: Uuid,
    flags: &UserFlags,
) -> Result<bool, EntriesError> {
    if !flags.is_active || flags.is_bot {
        return Ok(false);
    }
    let has = seam.has_usable_llm_config(candidate).await?;
    Ok(user_has_llm_config(Some(flags), || has))
}

/// Create a fresh `AgentRun` for one scheduler-binding tick
/// (`dispatch_scheduler_run`, `service.py:846-1001`). Handler contract
/// (the `:944` `transaction.atomic()`, which only the executing store
/// can do): run the capacity lock, the INSERT, and the FAILED
/// finalization on ONE transaction.
pub async fn dispatch_scheduler_run<S: EntriesSeam>(
    seam: &mut S,
    req: &SchedulerRequest,
) -> Result<SchedulerOutcome, EntriesError> {
    let binding = seam.binding(req.binding_id).await?;

    // Pod resolution (late bound): prefer the binding's explicit pod
    // override, but only when it is still active and belongs to this
    // project — otherwise fall back to the project default so a stale
    // override never silently dispatches into the wrong/dead pod.
    let mut pod: Option<PodView> = None;
    if let Some(override_id) = binding.pod_id {
        pod = seam
            .scheduler_override_pod(override_id, binding.project_id)
            .await?;
    }
    if pod.is_none() {
        if let Some(project_id) = binding.project_id {
            pod = seam.default_pod_for_project(project_id).await?;
        }
    }
    let Some(pod) = pod else {
        return Ok(SchedulerOutcome {
            run: None,
            error: Some(no_default_pod_message(binding.project_id)),
        });
    };
    // A resolved pod implies a project (neither the override filter nor
    // the default lookup can match a NULL project id); the error is
    // unreachable from a live store.
    let project_id = binding
        .project_id
        .ok_or_else(|| CreationError::MissingRow("binding has no project".to_owned()))?;

    let project = seam.project(project_id).await?;
    let workspace = seam.workspace(binding.workspace_id).await?;
    let (creator, creator_valid): (Option<Uuid>, bool) =
        if project.default_agent_executor == AgentExecutorKind::CloudAgent.value() {
            // Cloud runs execute against the creator's BYOK LLM config, so
            // the execution principal must also have a usable provider key.
            let mut creator = None;
            for candidate in [binding.actor_id, project.project_lead_id]
                .into_iter()
                .flatten()
            {
                let flags = seam.user_flags(candidate).await?;
                if !flags.is_active || flags.is_bot {
                    continue;
                }
                let facts = seam
                    .project_role_facts(candidate, &workspace.slug, project_id)
                    .await?;
                if !check_project_role(&facts, true) {
                    continue;
                }
                if !candidate_has_llm_config(seam, candidate, &flags).await? {
                    continue;
                }
                creator = Some(candidate);
                break;
            }
            (creator, creator.is_some())
        } else {
            let mut creator = binding.actor_id;
            if creator.is_none() {
                creator = Some(seam.agent_system_user().await??);
            }
            (creator, creator.is_some())
        };
    let creator = match creator {
        Some(creator) if creator_valid => creator,
        _ => {
            return Ok(SchedulerOutcome {
                run: None,
                error: Some(SCHEDULER_NO_CREATOR.to_owned()),
            });
        }
    };

    let flags = seam.user_flags(creator).await?;
    let execution = match seam
        .execution_fields(&ExecutionRequest {
            project_id: project.id,
            workspace_id: project.workspace_id,
            default_agent_executor: project.default_agent_executor.clone(),
            run_kind: "scheduler".to_owned(),
            has_issue: false,
            actor: Some(ActorRequest { id: creator, flags }),
            automatic: true,
            requested: None,
        })
        .await
    {
        Ok(execution) => execution,
        Err(ExecutionError::Refused(message)) => {
            return Ok(SchedulerOutcome {
                run: None,
                error: Some(message),
            });
        }
        Err(ExecutionError::Store(error)) => return Err(error.into()),
    };
    let mut admission_error = execution.cloud_admission_error.clone();

    // Permanent misconfiguration — refuse BEFORE creating a run so a
    // broken binding writes last_error once instead of minting a FAILED
    // AgentRun on every scheduler firing. Note the execution's executor
    // kind, not the project default.
    if execution.executor_kind == AgentExecutorKind::CloudAgent
        && binding.outcome_mode != OUTCOME_MODE_CREATE_ISSUE
    {
        return Ok(SchedulerOutcome {
            run: None,
            error: Some(SCHEDULER_OUTCOME_MODE_REFUSAL.to_owned()),
        });
    }

    admission_error = seam
        .lock_cloud_creation_capacity(project.workspace_id, execution.executor_kind, true)
        .await?
        .or(admission_error);
    let run_id = Uuid::new_v4();
    let run = seam
        .insert_scheduler_run(&NewSchedulerRun {
            id: run_id,
            workspace_id: binding.workspace_id,
            created_by_id: creator,
            pod_id: pod.id,
            pinned_runner_id: execution.merge_pin(None),
            binding_id: binding.id,
            executor_kind: execution.executor_kind,
            error_code: execution.error_code.clone().unwrap_or_default(),
            tool_plan: execution.tool_plan.clone(),
            now: req.now,
        })
        .await?;
    if let Some(admission) = admission_error {
        let failed = seam
            .finalize_failed_run(run.id, &admission.code, &admission.detail, req.now)
            .await?;
        return Ok(SchedulerOutcome {
            run: Some(failed.id),
            error: None,
        });
    }
    let turn = match render_scheduler_turn(seam, &binding, &project, &workspace, &run).await? {
        Ok(turn) => turn,
        Err(message) => {
            let failed = seam
                .finalize_failed_run(
                    run.id,
                    ERROR_CODE_PROMPT_BUILD_FAILED,
                    &format!("prompt build failed: {message}"),
                    req.now,
                )
                .await?;
            // A render failure DID produce a run — return it (not a
            // short-circuit None) so the Beat loop records it as
            // last_run and does not write binding.last_error.
            return Ok(SchedulerOutcome {
                run: Some(failed.id),
                error: None,
            });
        }
    };
    seam.save_prompt(run.id, &turn.text, &turn.manifest).await?;
    seam.dispatch_after_commit(run.id);
    Ok(SchedulerOutcome {
        run: Some(run.id),
        error: None,
    })
}

/// Render the new scheduler run's prompt (`run.prompt =
/// build_scheduler_turn(binding, run)`, `service.py:977`). The outer
/// error is a store failure (propagates, like an ORM error outside the
/// `_PROMPT_BUILD_ERRORS` catch); the inner `Err` is the composer
/// message for the render-failed path.
async fn render_scheduler_turn<S: EntriesSeam>(
    seam: &mut S,
    binding: &BindingView,
    project: &ProjectView,
    workspace: &WorkspaceView,
    run: &RunView,
) -> Result<Result<RenderedTurn, String>, EntriesError> {
    let scheduler = seam.scheduler_row(binding.scheduler_id).await?;
    let override_rows = seam.scheduler_override_rows(binding.workspace_id).await?;
    let tool_plan = &run.tool_plan;
    let extra_enabled = tool_plan
        .get("extra_toolsets")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let schema_tool = if extra_enabled {
        seam.extra_toolsets_schema_tool()
    } else {
        String::new()
    };
    let input = SchedulerContextInput {
        workspace_slug: workspace.slug.clone(),
        workspace_name: workspace.name.clone(),
        project_id: Some(project.id.to_string()),
        project_identifier: project.identifier.clone(),
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        scheduler_slug: scheduler.slug.clone(),
        scheduler_name: scheduler.name.clone(),
        scheduler_description: Some(scheduler.description.clone()),
        run_id: run.id.to_string(),
        executor_kind: run.executor_kind.value().to_owned(),
        available_tools: tool_plan.get("tools").cloned().unwrap_or(Value::Null),
        unavailable_capabilities: tool_plan
            .get("unavailable_capabilities")
            .cloned()
            .unwrap_or(Value::Null),
        extra_toolsets_enabled: extra_enabled,
        extra_toolsets_schema_tool: schema_tool,
        limits: tool_plan.get("limits").cloned().unwrap_or(Value::Null),
        scheduler_prompt: scheduler.prompt.clone(),
        binding_extra_context: binding.extra_context.clone(),
        outcome_directive: context::outcome_mode_directive(&binding.outcome_mode).to_owned(),
    };
    let context_value = context::build_scheduler_context(&input);
    let workspace_id = binding.workspace_id.to_string();
    let index = composer::build_override_index(Some(workspace_id.as_str()), None, &override_rows);
    let catalog_version = tool_plan
        .get("catalog_version")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    Ok(seam.compose_scheduler_turn(
        &context_value,
        &index,
        Some(workspace_id.as_str()),
        Some(run.executor_kind.value()),
        catalog_version,
    ))
}

// ---------------------------------------------------------------------------
// Signals as explicit calls
// ---------------------------------------------------------------------------

/// Process-local counter of swallowed orchestration errors
/// (`signals.py:58`): [`fire_state_transition`] bumps it whenever the
/// handler raises, so a broken trigger can't crash the save but never
/// fails silently either. Ops dashboards and tests assert on it.
static ORCHESTRATION_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

/// Read [`ORCHESTRATION_ERROR_COUNT`].
pub fn orchestration_error_count() -> u64 {
    ORCHESTRATION_ERROR_COUNT.load(Ordering::Relaxed)
}

/// Bump the counter; answers the new total for the log line.
fn bump_orchestration_error_count() -> u64 {
    ORCHESTRATION_ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1
}

/// Snapshot the pre-save `state_id` (`capture_prior_state`,
/// `signals.py:61-71`): `None` for a new instance (`issue_pk` is
/// `None`) or a missing row, else the stored `state_id`. D-26 save
/// paths call this before the write and pass the snapshot to
/// [`fire_state_transition`] after.
pub async fn capture_prior_state<S: EntriesSeam>(
    seam: &mut S,
    issue_pk: Option<Uuid>,
) -> Result<Option<Uuid>, EntriesError> {
    let Some(issue_pk) = issue_pk else {
        return Ok(None);
    };
    Ok(seam.prior_state_id(issue_pk).await?)
}

/// Build the transition call for a fired save (`signals.py:89-97`): the
/// resolved states, `actor=None` (the signal never carries an actor),
/// and the per-instance flags.
pub fn fire_transition_request(
    req: &FireRequest,
    from_state: Option<StateView>,
    to_state: Option<StateView>,
) -> TransitionRequest {
    TransitionRequest {
        issue_id: req.issue_id,
        from_state,
        to_state,
        actor: None,
        dispatch_immediate: req.dispatch_immediate,
        moved_by_run: req.moved_by_run,
        now: req.now,
        jitter_secs: req.jitter_secs,
        created_by: req.created_by,
    }
}

/// Fire the state-transition handler after an issue save
/// (`fire_state_transition`, `signals.py:74-107`).
///
/// A non-transition answers [`FireOutcome::NoTransition`] without I/O.
/// Otherwise the states resolve (`_lookup_state`: a missing `from` id
/// answers `None`; any other storage failure propagates, as in Python
/// where only the handler call is in `try`) and the transition driver
/// runs with `actor=None`. A handler failure answers
/// [`FireOutcome::Failed`] with the counter bumped and the verbatim log
/// line — the save itself never crashes.
pub async fn fire_state_transition<S: EntriesSeam, P: PreflightSeam>(
    seam: &mut S,
    preflight: &mut P,
    req: &FireRequest,
) -> Result<FireOutcome, EntriesError> {
    if req.prev_state_id == req.current_state_id {
        return Ok(FireOutcome::NoTransition);
    }
    let from_state = match seam.state(req.prev_state_id).await {
        Ok(state) => state,
        Err(CreationError::MissingRow(_)) => None,
        Err(error) => return Err(error.into()),
    };
    let to_state = seam.state(req.current_state_id).await?;
    let transition = fire_transition_request(req, from_state, to_state);
    match handle_issue_state_transition(seam, preflight, &transition).await {
        Ok(outcome) => Ok(FireOutcome::Called(outcome)),
        Err(error) => {
            let total_errors = bump_orchestration_error_count();
            Ok(FireOutcome::Failed {
                log_line: fire_error_log_line(
                    &req.issue_id,
                    req.prev_state_id,
                    req.current_state_id,
                    total_errors,
                ),
                error: error.to_string(),
                total_errors,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{DateTime, Utc};
    use serde_json::{json, Value};
    use uuid::Uuid;

    use pidash_auth::permissions::membership::ProjectRoleFacts;
    use pidash_db::dispatch::status::{AgentRunStatus, AgentRunTrigger};
    use pidash_db::orchestration::workpad::AgentUserCollisionError;
    use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
    use pidash_types::dispatch::AgentExecutorKind;
    use pidash_types::orchestration::StateRef;

    use super::*;
    use crate::dispatch::UserFlags;
    use crate::orchestration::clock::{ClockWrite, ProjectClockPolicy};
    use crate::orchestration::creation::{
        AdmissionError, ExecutionError, ExecutionFields, ExecutionRequest, IssueView, NewAgentRun,
        PodView, ProjectView, RenderBundle, RunView, RunnerView, StateView,
    };
    use crate::orchestration::creation::{CreationError, CreationSeam, FinalizeAgentRunSeam};
    use crate::prompting::composer::OverrideRow;
    use crate::prompting::{composer, context};

    static TRANSITION_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx07_entries/transition.matrix.json");
    static COMMENT_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx07_entries/comment.matrix.json");
    static SCHEDULER_FIXTURE: &str = include_str!(
        "../../../../fixtures/orchestration/fx07_entries/scheduler_dispatch.before_after.json"
    );
    static SIGNALS_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx07_entries/signals.golden.json");

    /// Serializes the counter-sensitive tests: the process-local
    /// [`orchestration_error_count`] is shared across test threads.
    /// Async-aware so the guard may span the drivers' await points.
    static COUNTER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn transition_case(name: &str) -> Value {
        let fixture: Value = serde_json::from_str(TRANSITION_FIXTURE).expect("fixture parses");
        fixture["cases"][name].clone()
    }

    fn comment_case(name: &str) -> Value {
        let fixture: Value = serde_json::from_str(COMMENT_FIXTURE).expect("fixture parses");
        fixture["cases"][name].clone()
    }

    fn scheduler_case(name: &str) -> Value {
        let fixture: Value = serde_json::from_str(SCHEDULER_FIXTURE).expect("fixture parses");
        fixture["cases"][name].clone()
    }

    fn signals_cases() -> Value {
        let fixture: Value = serde_json::from_str(SIGNALS_FIXTURE).expect("fixture parses");
        fixture["cases"].clone()
    }

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn frozen_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
            .expect("frozen clock parses")
            .to_utc()
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
            name: Some("FX7 issue".to_owned()),
            description_stripped: Some(String::new()),
            priority: Some("none".to_owned()),
            sequence_id: 10,
            target_date: None,
        }
    }

    fn project_view() -> ProjectView {
        ProjectView {
            id: uid(0x20),
            workspace_id: uid(0x10),
            identifier: "FX7".to_owned(),
            name: "FX7".to_owned(),
            description: None,
            repo_url: Some("https://example.com/fx7.git".to_owned()),
            base_branch: Some("main".to_owned()),
            default_agent_executor: AgentExecutorKind::LocalRunner.value().to_owned(),
            project_lead_id: Some(uid(0x41)),
            default_assignee_id: None,
            pool: 10,
            interval_impl: 10_800,
            interval_review: 10_800,
            interval_test: 10_800,
        }
    }

    /// The FX7 project clock policy: cadences unified at 3h (PDASHOSS01-167),
    /// so every stage retimes to `now + 10800s` at zero jitter.
    fn clock_policy() -> ProjectClockPolicy {
        ProjectClockPolicy {
            agent_ticking_enabled: Some(true),
            agent_default_max_ticks: Some(10),
            agent_default_interval_seconds: Some(10_800),
            agent_review_default_interval_seconds: Some(10_800),
            agent_test_default_interval_seconds: Some(10_800),
        }
    }

    fn state_view(name: &str, group: &str) -> StateView {
        StateView {
            id: uid(0x30),
            name: name.to_owned(),
            group: group.to_owned(),
        }
    }

    fn run_view(id: Uuid, status: AgentRunStatus) -> RunView {
        RunView {
            id,
            workspace_id: uid(0x10),
            created_by_id: uid(0x40),
            pod_id: uid(0x50),
            runner_id: None,
            pinned_runner_id: None,
            parent_run_id: None,
            work_item_id: Some(uid(0x01)),
            status,
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

    fn ticker_row(issue_id: Uuid, now: DateTime<Utc>) -> IssueAgentTicker {
        IssueAgentTicker {
            id: uid(0xA0),
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id,
            used: 0,
            granted: 0,
            waited: 0,
            user_disabled: false,
            next_run_at: None,
            last_tick_at: None,
            enabled: false,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
            resume_parent_run_id: None,
        }
    }

    fn minimal_bundle(
        issue: &IssueView,
        project: &ProjectView,
        state: Option<StateView>,
    ) -> RenderBundle {
        RenderBundle {
            issue: issue.clone(),
            project: project.clone(),
            workspace_slug: "fx7-ws".to_owned(),
            workspace_name: "fx7-ws".to_owned(),
            state,
            labels: Vec::new(),
            assignees: Vec::new(),
            project_states: Vec::new(),
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

    fn human_flags() -> UserFlags {
        UserFlags {
            is_active: true,
            is_bot: false,
        }
    }

    /// Scalar ticker fields pinned to the fixture's after-row. Actor /
    /// resume / `next_run_at` need uuid / time mapping, so each test
    /// asserts those explicitly.
    fn assert_ticker_scalars(ticker_json: &Value, row: &IssueAgentTicker) {
        assert_eq!(
            i64::from(row.used),
            ticker_json["used"].as_i64().unwrap(),
            "used"
        );
        assert_eq!(
            i64::from(row.granted),
            ticker_json["granted"].as_i64().unwrap(),
            "granted"
        );
        assert_eq!(
            i64::from(row.waited),
            ticker_json["waited"].as_i64().unwrap(),
            "waited"
        );
        assert_eq!(
            row.user_disabled,
            ticker_json["user_disabled"].as_bool().unwrap(),
            "user_disabled"
        );
        assert_eq!(
            row.enabled,
            ticker_json["enabled"].as_bool().unwrap(),
            "enabled"
        );
        assert_eq!(
            row.disarm_reason,
            ticker_json["disarm_reason"].as_str().unwrap(),
            "disarm_reason"
        );
        assert_eq!(row.last_tick_at, None, "last_tick_at");
        assert!(
            ticker_json["last_tick_at"].is_null(),
            "fixture last_tick_at"
        );
        assert_eq!(
            row.pending_entry,
            ticker_json["pending_entry"].as_bool().unwrap(),
            "pending_entry"
        );
        assert_eq!(
            row.pending_entry_free,
            ticker_json["pending_entry_free"].as_bool().unwrap(),
            "pending_entry_free"
        );
        assert_eq!(
            row.pending_entry_trigger,
            ticker_json["pending_entry_trigger"].as_str().unwrap(),
            "pending_entry_trigger"
        );
    }

    #[derive(Default)]
    struct FakeEntriesSeam {
        issues: HashMap<Uuid, IssueView>,
        projects: HashMap<Uuid, ProjectView>,
        states: HashMap<Uuid, StateView>,
        state_calls: Vec<Option<Uuid>>,
        runs: HashMap<Uuid, RunView>,
        runners: HashMap<Uuid, RunnerView>,
        pods: HashMap<Uuid, PodView>,
        default_pods: HashMap<Uuid, Uuid>,
        latest: HashMap<Uuid, Uuid>,
        active: HashMap<Uuid, Uuid>,
        race_active: Option<Uuid>,
        active_calls: usize,
        resume: HashMap<Uuid, Uuid>,
        users: HashMap<Uuid, UserFlags>,
        execution: Option<Result<ExecutionFields, ExecutionError>>,
        execution_requests: Vec<ExecutionRequest>,
        lock: Option<Result<Option<AdmissionError>, CreationError>>,
        bundle: Option<RenderBundle>,
        dispatched: Vec<Uuid>,
        inserted: Vec<NewAgentRun>,
        saved_prompts: HashMap<Uuid, (String, Value)>,
        finalized: Vec<(Uuid, String, String)>,
        tickers: HashMap<Uuid, IssueAgentTicker>,
        ticker_snapshot: Option<HashMap<Uuid, IssueAgentTicker>>,
        ticker_saves: Vec<(Uuid, ClockWrite)>,
        rollbacks: usize,
        policies: HashMap<Uuid, ProjectClockPolicy>,
        bindings: HashMap<Uuid, BindingView>,
        schedulers: HashMap<Uuid, SchedulerView>,
        workspaces: HashMap<Uuid, WorkspaceView>,
        role_facts: HashMap<Uuid, (bool, bool, bool, bool)>,
        role_calls: Vec<(Uuid, String, Uuid)>,
        llm: HashMap<Uuid, bool>,
        llm_calls: Vec<Uuid>,
        agent_user: Option<Result<Uuid, AgentUserCollisionError>>,
        scheduler_overrides: Vec<OverrideRow>,
        scheduler_runs: Vec<NewSchedulerRun>,
        compose_script: Option<Result<RenderedTurn, String>>,
        compose_calls: usize,
        queued: HashMap<Uuid, Uuid>,
        prior_states: HashMap<Uuid, Option<Uuid>>,
        prior_state_calls: Vec<Uuid>,
    }

    impl FakeEntriesSeam {
        fn minimal() -> Self {
            let mut seam = Self::default();
            seam.issues.insert(uid(0x01), issue_view());
            seam.projects.insert(uid(0x20), project_view());
            seam.policies.insert(uid(0x20), clock_policy());
            seam.states
                .insert(uid(0x30), state_view("In Progress", "started"));
            seam.states
                .insert(uid(0x31), state_view("Todo", "unstarted"));
            seam.states
                .insert(uid(0x32), state_view("In Review", "review"));
            seam.states
                .insert(uid(0x33), state_view("Done", "completed"));
            seam.pods.insert(
                uid(0x50),
                PodView {
                    id: uid(0x50),
                    project_id: uid(0x20),
                },
            );
            seam.default_pods.insert(uid(0x20), uid(0x50));
            seam.users.insert(uid(0x40), human_flags());
            seam.users.insert(uid(0x41), human_flags());
            seam.execution = Some(Ok(ExecutionFields::local(json!({}))));
            seam.lock = Some(Ok(None));
            seam
        }

        fn with_bundle(self) -> Self {
            self.with_bundle_state(uid(0x30))
        }

        fn with_bundle_state(mut self, state_id: Uuid) -> Self {
            let bundle = minimal_bundle(
                &self.issues[&uid(0x01)],
                &self.projects[&uid(0x20)],
                Some(self.states[&state_id].clone()),
            );
            self.bundle = Some(bundle);
            self
        }
    }

    impl CreationSeam for FakeEntriesSeam {
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
            self.state_calls.push(state_id);
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
                .and_then(|id| self.runs.get(id).cloned()))
        }

        async fn active_run_for(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            // T7 race scripting: the first read (reconcile's) sees a free
            // issue; later reads (the race guard's) see the interleaved run.
            if let Some(race_id) = self.race_active {
                self.active_calls += 1;
                if self.active_calls == 1 {
                    return Ok(None);
                }
                return Ok(self.runs.get(&race_id).cloned());
            }
            Ok(self
                .active
                .get(&issue_id)
                .and_then(|id| self.runs.get(id).cloned()))
        }

        async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
            Ok(self.runs.get(&run_id).cloned())
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
                .default_pods
                .get(&project_id)
                .and_then(|id| self.pods.get(id).cloned()))
        }

        async fn resume_parent_run_id(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<Uuid>, CreationError> {
            Ok(self.resume.get(&issue_id).copied())
        }

        async fn work_item_id_for_run(
            &mut self,
            _run_id: Uuid,
        ) -> Result<Option<Uuid>, CreationError> {
            Ok(None)
        }

        async fn lock_issue_for_handoff(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<crate::orchestration::creation::LockedIssue>, CreationError> {
            Ok(None)
        }

        async fn lock_run_for_handoff(
            &mut self,
            _run_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            Ok(None)
        }

        async fn user_flags(&mut self, user_id: Uuid) -> Result<UserFlags, CreationError> {
            self.users
                .get(&user_id)
                .copied()
                .ok_or_else(|| CreationError::Db(format!("no user {user_id}")))
        }

        async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
            self.inserted.push(row.clone());
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
            self.runs.insert(row.id, run.clone());
            Ok(run)
        }

        async fn save_prompt(
            &mut self,
            run_id: Uuid,
            prompt: &str,
            manifest: &Value,
        ) -> Result<(), CreationError> {
            self.saved_prompts
                .insert(run_id, (prompt.to_owned(), manifest.clone()));
            if let Some(run) = self.runs.get_mut(&run_id) {
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
            if let Some(run) = self.runs.get_mut(&run_id) {
                run.run_config = config.clone();
            }
            Ok(())
        }

        async fn execution_fields(
            &mut self,
            req: &ExecutionRequest,
        ) -> Result<ExecutionFields, ExecutionError> {
            self.execution_requests.push(req.clone());
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
            self.bundle
                .clone()
                .ok_or_else(|| CreationError::Db("no bundle".to_owned()))
        }

        fn extra_toolsets_schema_tool(&self) -> String {
            String::new()
        }
    }

    impl FinalizeAgentRunSeam for FakeEntriesSeam {
        async fn finalize_failed_run(
            &mut self,
            run_id: Uuid,
            error_code: &str,
            error: &str,
            now: DateTime<Utc>,
        ) -> Result<RunView, CreationError> {
            self.finalized
                .push((run_id, error_code.to_owned(), error.to_owned()));
            let run = self.runs.get_mut(&run_id).expect("run exists");
            run.status = AgentRunStatus::Failed;
            run.error_code = error_code.to_owned();
            run.error = error.to_owned();
            run.ended_at = Some(now);
            Ok(run.clone())
        }
    }

    impl EntriesSeam for FakeEntriesSeam {
        async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
            self.prior_state_calls.push(issue_id);
            Ok(self.prior_states.get(&issue_id).copied().flatten())
        }

        async fn queued_follow_up(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            Ok(self
                .queued
                .get(&issue_id)
                .and_then(|id| self.runs.get(id).cloned()))
        }

        async fn lock_ticker(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<IssueAgentTicker>, CreationError> {
            // Snapshot the transaction scope: `set_rollback` restores it,
            // mirroring the live store's rollback.
            self.ticker_snapshot = Some(self.tickers.clone());
            Ok(self.tickers.get(&issue_id).cloned())
        }

        async fn save_ticker(
            &mut self,
            row: &IssueAgentTicker,
            write: ClockWrite,
        ) -> Result<(), CreationError> {
            self.ticker_saves.push((row.id, write));
            self.tickers.insert(row.issue_id, row.clone());
            Ok(())
        }

        fn set_rollback(&mut self) {
            self.rollbacks += 1;
            if let Some(snapshot) = self.ticker_snapshot.take() {
                self.tickers = snapshot;
            }
        }

        async fn clock_policy(
            &mut self,
            project_id: Uuid,
        ) -> Result<ProjectClockPolicy, CreationError> {
            self.policies
                .get(&project_id)
                .copied()
                .ok_or_else(|| CreationError::Db(format!("no clock policy {project_id}")))
        }

        async fn binding(&mut self, binding_id: Uuid) -> Result<BindingView, CreationError> {
            self.bindings
                .get(&binding_id)
                .cloned()
                .ok_or_else(|| CreationError::MissingRow(format!("no binding {binding_id}")))
        }

        async fn scheduler_override_pod(
            &mut self,
            pod_id: Uuid,
            project_id: Option<Uuid>,
        ) -> Result<Option<PodView>, CreationError> {
            let Some(project_id) = project_id else {
                return Ok(None);
            };
            Ok(self
                .pods
                .get(&pod_id)
                .filter(|pod| pod.project_id == project_id)
                .cloned())
        }

        async fn workspace(&mut self, workspace_id: Uuid) -> Result<WorkspaceView, CreationError> {
            self.workspaces
                .get(&workspace_id)
                .cloned()
                .ok_or_else(|| CreationError::Db(format!("no workspace {workspace_id}")))
        }

        async fn scheduler_row(
            &mut self,
            scheduler_id: Uuid,
        ) -> Result<SchedulerView, CreationError> {
            self.schedulers
                .get(&scheduler_id)
                .cloned()
                .ok_or_else(|| CreationError::MissingRow(format!("no scheduler {scheduler_id}")))
        }

        async fn scheduler_override_rows(
            &mut self,
            _workspace_id: Uuid,
        ) -> Result<Vec<OverrideRow>, CreationError> {
            Ok(self.scheduler_overrides.clone())
        }

        async fn project_role_facts(
            &mut self,
            user_id: Uuid,
            workspace_slug: &str,
            project_id: Uuid,
        ) -> Result<ProjectRoleFacts, CreationError> {
            self.role_calls
                .push((user_id, workspace_slug.to_owned(), project_id));
            let (authenticated, has_allowed_role, is_project_member, is_workspace_admin) = self
                .role_facts
                .get(&user_id)
                .copied()
                .ok_or_else(|| CreationError::Db(format!("no role facts {user_id}")))?;
            Ok(ProjectRoleFacts {
                authenticated,
                has_allowed_role,
                is_project_member,
                is_workspace_admin,
            })
        }

        async fn has_usable_llm_config(&mut self, user_id: Uuid) -> Result<bool, CreationError> {
            self.llm_calls.push(user_id);
            self.llm
                .get(&user_id)
                .copied()
                .ok_or_else(|| CreationError::Db(format!("no llm verdict {user_id}")))
        }

        async fn agent_system_user(
            &mut self,
        ) -> Result<Result<Uuid, AgentUserCollisionError>, CreationError> {
            Ok(self.agent_user.clone().expect("script agent user"))
        }

        async fn insert_scheduler_run(
            &mut self,
            row: &NewSchedulerRun,
        ) -> Result<RunView, CreationError> {
            self.scheduler_runs.push(row.clone());
            let run = RunView {
                id: row.id,
                workspace_id: row.workspace_id,
                created_by_id: row.created_by_id,
                pod_id: row.pod_id,
                runner_id: None,
                pinned_runner_id: row.pinned_runner_id,
                parent_run_id: None,
                work_item_id: None,
                status: AgentRunStatus::Queued,
                trigger: AgentRunTrigger::Scheduler.value().to_owned(),
                executor_kind: row.executor_kind,
                phase_kind: String::new(),
                run_config: json!({}),
                tool_plan: row.tool_plan.clone(),
                error_code: row.error_code.clone(),
                error: String::new(),
                prompt: String::new(),
                prompt_manifest: None,
                ended_at: None,
            };
            self.runs.insert(row.id, run.clone());
            Ok(run)
        }

        fn compose_scheduler_turn(
            &mut self,
            ctx: &Value,
            index: &composer::OverrideIndex,
            workspace_id: Option<&str>,
            executor_kind: Option<&str>,
            tool_catalog_version: i64,
        ) -> Result<RenderedTurn, String> {
            self.compose_calls += 1;
            if let Some(scripted) = self.compose_script.clone() {
                return scripted;
            }
            // Unscripted: delegate to the merged composer, exactly as the
            // live store will.
            match context::build_scheduler_turn(
                ctx,
                index,
                workspace_id,
                executor_kind,
                tool_catalog_version,
            ) {
                Ok(turn) => Ok(RenderedTurn {
                    text: turn.text,
                    manifest: turn
                        .manifest
                        .map(|manifest| manifest.to_json())
                        .unwrap_or(Value::Null),
                }),
                Err(error) => Err(error.message().to_owned()),
            }
        }
    }

    /// Scripted L8 preflight, standing in for the unmerged
    /// `preflight_eligibility_or_bounce` implementation.
    struct FakePreflight {
        verdict: Option<bool>,
        calls: Vec<(Uuid, Uuid, Uuid, String)>,
    }

    impl FakePreflight {
        fn allow() -> Self {
            Self {
                verdict: Some(true),
                calls: Vec::new(),
            }
        }

        fn deny() -> Self {
            Self {
                verdict: Some(false),
                calls: Vec::new(),
            }
        }
    }

    impl PreflightSeam for FakePreflight {
        async fn preflight_eligibility_or_bounce(
            &mut self,
            issue_id: Uuid,
            creator_id: Uuid,
            pod_id: Uuid,
            triggered_by: &str,
        ) -> Result<bool, CreationError> {
            self.calls
                .push((issue_id, creator_id, pod_id, triggered_by.to_owned()));
            self.verdict
                .ok_or_else(|| CreationError::Db("preflight not scripted".to_owned()))
        }
    }

    // -- transition matrix (FX-ORCH-07 T1-T11b) --------------------------

    fn transition_req(
        from: Option<StateView>,
        to: Option<StateView>,
        now: DateTime<Utc>,
    ) -> TransitionRequest {
        TransitionRequest {
            issue_id: uid(0x01),
            from_state: from,
            to_state: to,
            actor: None,
            dispatch_immediate: true,
            moved_by_run: None,
            now,
            jitter_secs: 0.0,
            created_by: None,
        }
    }

    /// Post-save truth: the issue row carries `to_state` (the fixture
    /// generator sets it before calling).
    fn set_issue_state(seam: &mut FakeEntriesSeam, state_id: Uuid) {
        seam.issues.get_mut(&uid(0x01)).expect("issue").state_id = Some(state_id);
    }

    #[tokio::test]
    async fn t1_leave_bucket_goes_dormant() {
        let case = transition_case("T1_leave_bucket");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        set_issue_state(&mut seam, uid(0x33));
        let mut ticker = ticker_row(uid(0x01), now);
        ticker.used = 2;
        ticker.enabled = true;
        ticker.next_run_at = Some(now);
        seam.tickers.insert(uid(0x01), ticker);

        let from = seam.states[&uid(0x30)].clone();
        let to = seam.states[&uid(0x33)].clone();
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_NOT_A_TRIGGER_STATE);
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker kept");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(row.used, 2);
        assert_eq!(row.next_run_at, None);
        assert!(seam.dispatched.is_empty());
        assert_eq!(seam.rollbacks, 0);
    }

    #[tokio::test]
    async fn t2_non_trigger_touches_nothing() {
        let case = transition_case("T2_non_trigger");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        set_issue_state(&mut seam, uid(0x33));

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x33)].clone();
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert!(outcome.created_run.is_none());
        assert!(seam.tickers.is_empty());
        assert!(seam.ticker_saves.is_empty());
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn t3_dispatch_deferred_updates_clock_only() {
        let case = transition_case("T3_dispatch_deferred");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut req = transition_req(Some(from), Some(to), now);
        req.dispatch_immediate = false;
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(&mut seam, &mut preflight, &req)
            .await
            .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_DISPATCH_DEFERRED);
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(
            row.next_run_at,
            Some(now + chrono::Duration::seconds(10_800))
        );
        assert_eq!(seam.ticker_saves.len(), 1);
        assert_eq!(seam.ticker_saves[0].1, ClockWrite::Insert);
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn t4_agent_move_into_spent_pool_parks() {
        let case = transition_case("T4_pool_spent");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        set_issue_state(&mut seam, uid(0x32));
        let mover = uid(0x60);
        seam.runs
            .insert(mover, run_view(mover, AgentRunStatus::Running));
        seam.latest.insert(uid(0x01), mover);
        let mut ticker = ticker_row(uid(0x01), now);
        ticker.used = 20;
        ticker.enabled = true;
        ticker.next_run_at = Some(now);
        seam.tickers.insert(uid(0x01), ticker);

        let from = seam.states[&uid(0x30)].clone();
        let to = seam.states[&uid(0x32)].clone();
        let mut req = transition_req(Some(from), Some(to), now);
        req.moved_by_run = Some(mover);
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(&mut seam, &mut preflight, &req)
            .await
            .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_POOL_SPENT);
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker kept");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(row.next_run_at, Some(now));
        assert_eq!(row.resume_parent_run_id, Some(mover));
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn t5_busy_human_move_queues_free_entry_with_actor() {
        let case = transition_case("T5_entry_queued_human");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let active = uid(0x61);
        seam.runs
            .insert(active, run_view(active, AgentRunStatus::Running));
        seam.active.insert(uid(0x01), active);

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut req = transition_req(Some(from), Some(to), now);
        req.actor = Some(uid(0x40));
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(&mut seam, &mut preflight, &req)
            .await
            .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_ENTRY_QUEUED);
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(row.next_run_at, Some(now));
        assert_eq!(row.pending_entry_actor_id, Some(uid(0x40)));
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn t5b_agent_move_queues_counting_entry() {
        let case = transition_case("T5b_entry_queued_agent");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        set_issue_state(&mut seam, uid(0x32));
        let mover = uid(0x62);
        seam.runs
            .insert(mover, run_view(mover, AgentRunStatus::Running));
        seam.latest.insert(uid(0x01), mover);
        let mut ticker = ticker_row(uid(0x01), now);
        ticker.used = 3;
        ticker.granted = 10;
        ticker.enabled = true;
        ticker.next_run_at = Some(now);
        seam.tickers.insert(uid(0x01), ticker);

        let from = seam.states[&uid(0x30)].clone();
        let to = seam.states[&uid(0x32)].clone();
        let mut req = transition_req(Some(from), Some(to), now);
        req.moved_by_run = Some(mover);
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(&mut seam, &mut preflight, &req)
            .await
            .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_ENTRY_QUEUED);
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker kept");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(row.next_run_at, Some(now));
        assert_eq!(row.pending_entry_actor_id, None);
        assert_eq!(row.resume_parent_run_id, Some(mover));
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn t6a_disabled_clock_surfaces_reconcile_reason() {
        let case = transition_case("T6a_no_dispatch_custom_reason");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        set_issue_state(&mut seam, uid(0x32));
        let mover = uid(0x63);
        seam.runs
            .insert(mover, run_view(mover, AgentRunStatus::Running));
        seam.latest.insert(uid(0x01), mover);
        let mut ticker = ticker_row(uid(0x01), now);
        ticker.user_disabled = true;
        seam.tickers.insert(uid(0x01), ticker);

        let from = seam.states[&uid(0x30)].clone();
        let to = seam.states[&uid(0x32)].clone();
        let mut req = transition_req(Some(from), Some(to), now);
        req.moved_by_run = Some(mover);
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(&mut seam, &mut preflight, &req)
            .await
            .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, "ticking-disabled");
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker kept");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(row.next_run_at, None);
        assert_eq!(row.resume_parent_run_id, Some(mover));
        assert!(seam.dispatched.is_empty());
    }

    #[test]
    fn t6b_bare_no_dispatch_fallback_is_verbatim() {
        // Stub-only path: every real enter/move reconcile branch sets a
        // reason, so `decision.reason or "no-dispatch"` only falls back
        // under a stubbed reconcile (T6b's seam). Pin the fallback const
        // to the fixture's reason.
        let case = transition_case("T6b_no_dispatch_bare");
        assert_eq!(case["reason"].as_str().unwrap(), REASON_NO_DISPATCH);
        assert_eq!(REASON_NO_DISPATCH, "no-dispatch");
    }

    #[tokio::test]
    async fn t7_interleaved_run_trips_race_guard() {
        let case = transition_case("T7_active_race");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let racer = uid(0x64);
        seam.runs
            .insert(racer, run_view(racer, AgentRunStatus::Running));
        seam.race_active = Some(racer);

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_ACTIVE_RUN_EXISTS);
        assert!(outcome.created_run.is_none());
        // Reconcile saw a free issue and armed the clock before the guard
        // tripped — the next tick covers the entry.
        let row = seam.tickers.get(&uid(0x01)).expect("ticker armed");
        assert!(row.enabled);
        assert_eq!(
            row.next_run_at,
            Some(now + chrono::Duration::seconds(10_800))
        );
        assert!(seam.dispatched.is_empty());
        assert!(preflight.calls.is_empty());
    }

    #[tokio::test]
    async fn t8_missing_creator_chain_answers_no_creator() {
        let case = transition_case("T8_no_creator");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        seam.issues
            .get_mut(&uid(0x01))
            .expect("issue")
            .created_by_id = None;
        seam.projects
            .get_mut(&uid(0x20))
            .expect("project")
            .project_lead_id = None;

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_NO_CREATOR);
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(
            row.next_run_at,
            Some(now + chrono::Duration::seconds(10_800))
        );
        assert!(seam.dispatched.is_empty());
        assert!(preflight.calls.is_empty());
    }

    #[tokio::test]
    async fn t9_missing_pods_answer_no_pod_available() {
        let case = transition_case("T9_no_pod");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        seam.default_pods.remove(&uid(0x20));

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_NO_POD_AVAILABLE);
        assert!(outcome.created_run.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(
            row.next_run_at,
            Some(now + chrono::Duration::seconds(10_800))
        );
        assert!(seam.dispatched.is_empty());
        assert!(preflight.calls.is_empty());
    }

    #[tokio::test]
    async fn t10_preflight_refusal_answers_no_eligible_runner() {
        let case = transition_case("T10_no_eligible_runner");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut preflight = FakePreflight::deny();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_NO_ELIGIBLE_RUNNER);
        assert!(outcome.created_run.is_none());
        // The seam owns the bounce (Backlog move + notice); entries only
        // pins the reason, the empty run set, and the preflight call.
        assert_eq!(
            preflight.calls,
            vec![(
                uid(0x01),
                uid(0x40),
                uid(0x50),
                AgentRunTrigger::StateTransition.value().to_owned()
            )]
        );
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert!(seam.dispatched.is_empty());
        assert!(seam.inserted.is_empty());
    }

    #[tokio::test]
    async fn t11a_free_entry_creates_run() {
        let case = transition_case("T11a_created");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal().with_bundle();

        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, "created");
        let run_id = outcome.created_run.expect("run created");
        let run = seam.runs.get(&run_id).expect("run stored").clone();
        let golden = &case["after"]["run"];
        assert_eq!(run.status.value(), golden["status"].as_str().unwrap());
        assert_eq!(run.trigger, golden["trigger"].as_str().unwrap());
        assert_eq!(run.phase_kind, golden["phase_kind"].as_str().unwrap());
        assert_eq!(run.parent_run_id, None);
        assert!(golden["parent_run"].is_null());
        assert_eq!(run.pinned_runner_id, None);
        assert!(golden["pinned_runner"].is_null());
        assert!(golden["owner"].is_null());
        assert_eq!(run.created_by_id, uid(0x40));
        assert_eq!(run.pod_id, uid(0x50));
        assert_eq!(run.work_item_id, Some(uid(0x01)));
        assert_eq!(
            run.executor_kind.value(),
            golden["executor_kind"].as_str().unwrap()
        );
        assert_eq!(run.tool_plan, golden["tool_plan"]);
        assert_eq!(run.error_code, golden["error_code"].as_str().unwrap());
        assert_eq!(run.error, golden["error"].as_str().unwrap());
        assert_eq!(run.ended_at, None);
        assert!(golden["ended_at"].is_null());
        assert_eq!(run.run_config, golden["run_config"]);
        assert!(!run.prompt.is_empty());
        assert!(run.prompt.contains("FX7-10"));
        assert!(run.prompt_manifest.is_some());
        assert_eq!(seam.dispatched, vec![run_id]);
        assert!(seam.finalized.is_empty());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(
            row.next_run_at,
            Some(now + chrono::Duration::seconds(10_800))
        );
        assert_eq!(seam.ticker_saves.len(), 1);
        assert_eq!(seam.ticker_saves[0].1, ClockWrite::Insert);
    }

    #[tokio::test]
    async fn t11b_cross_stage_captures_resume_parent() {
        let case = transition_case("T11b_cross_stage_resume_capture");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        set_issue_state(&mut seam, uid(0x32));
        let mut seam = seam.with_bundle_state(uid(0x32));
        let prior = uid(0x65);
        let mut prior_run = run_view(prior, AgentRunStatus::Completed);
        prior_run.phase_kind = "coding-task".to_owned();
        seam.runs.insert(prior, prior_run);
        seam.latest.insert(uid(0x01), prior);

        let from = seam.states[&uid(0x30)].clone();
        let to = seam.states[&uid(0x32)].clone();
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(
            &mut seam,
            &mut preflight,
            &transition_req(Some(from), Some(to), now),
        )
        .await
        .expect("transition runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        let run_id = outcome.created_run.expect("run created");
        let run = seam.runs.get(&run_id).expect("run stored").clone();
        let golden = &case["after"]["run"];
        assert_eq!(run.phase_kind, golden["phase_kind"].as_str().unwrap());
        assert_eq!(run.phase_kind, "review");
        assert_eq!(run.parent_run_id, None);
        assert!(golden["parent_run"].is_null());
        assert_eq!(run.pinned_runner_id, None);
        assert_eq!(seam.dispatched, vec![run_id]);
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(row.resume_parent_run_id, Some(prior));
        assert_eq!(
            row.next_run_at,
            Some(now + chrono::Duration::seconds(10_800))
        );
    }

    // -- comment matrix (FX-ORCH-07 M1-M10) ------------------------------

    fn comment_req(actor_id: Option<Uuid>, now: DateTime<Utc>) -> CommentRequest {
        CommentRequest {
            comment: CommentView {
                id: uid(0x70),
                issue_id: uid(0x01),
                actor_id,
            },
            now,
            jitter_secs: 0.0,
            created_by: None,
        }
    }

    #[tokio::test]
    async fn m1_system_comment_answers_no_actor() {
        let case = comment_case("M1_no_actor");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let outcome = handle_issue_comment(&mut seam, &comment_req(None, now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_NO_ACTOR);
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        assert!(seam.tickers.is_empty());
    }

    #[tokio::test]
    async fn m2_bot_comment_answers_bot_comment() {
        let case = comment_case("M2_bot_comment");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        seam.users.insert(
            uid(0x42),
            UserFlags {
                is_active: true,
                is_bot: true,
            },
        );
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x42)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_BOT_COMMENT);
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        assert!(seam.tickers.is_empty());
    }

    #[tokio::test]
    async fn m3_ineligible_group_answers_state_not_eligible() {
        let case = comment_case("M3_state_not_eligible");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        set_issue_state(&mut seam, uid(0x31));
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_STATE_NOT_ELIGIBLE);
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        assert!(seam.tickers.is_empty());
    }

    #[tokio::test]
    async fn m4_missing_prior_run_answers_no_prior_run() {
        let case = comment_case("M4_no_prior_run");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_NO_PRIOR_RUN);
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        assert!(seam.tickers.is_empty());
    }

    #[tokio::test]
    async fn m5_queued_follow_up_coalesces() {
        let case = comment_case("M5_coalesced");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let prior = uid(0x66);
        seam.runs
            .insert(prior, run_view(prior, AgentRunStatus::Completed));
        seam.latest.insert(uid(0x01), prior);
        let queued = uid(0x67);
        seam.runs
            .insert(queued, run_view(queued, AgentRunStatus::Queued));
        seam.queued.insert(uid(0x01), queued);
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_COALESCED);
        assert!(outcome.created_run.is_none());
        assert_eq!(outcome.coalesced_into, Some(queued));
        assert!(seam.tickers.is_empty());
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn m6_missing_pods_answer_no_pod_available() {
        let case = comment_case("M6_no_pod");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        seam.default_pods.remove(&uid(0x20));
        let prior = uid(0x66);
        seam.runs
            .insert(prior, run_view(prior, AgentRunStatus::Completed));
        seam.latest.insert(uid(0x01), prior);
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_NO_POD_AVAILABLE);
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        assert!(seam.tickers.is_empty());
    }

    #[tokio::test]
    async fn m7_in_flight_run_queues_free_entry() {
        let case = comment_case("M7_entry_queued");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let active = uid(0x68);
        seam.runs
            .insert(active, run_view(active, AgentRunStatus::Running));
        seam.latest.insert(uid(0x01), active);
        seam.active.insert(uid(0x01), active);
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_ENTRY_QUEUED);
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(row.next_run_at, Some(now));
        assert_eq!(row.pending_entry_actor_id, Some(uid(0x40)));
        assert_eq!(seam.rollbacks, 0);
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn m8_active_prior_rolls_back() {
        let case = comment_case("M8_prior_active_rollback");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        // Eligible group, non-ticking name: reconcile is skipped, then the
        // in-flight check rolls the (empty) scope back.
        let mut grooming = state_view("Grooming", "started");
        grooming.id = uid(0x34);
        seam.states.insert(uid(0x34), grooming);
        set_issue_state(&mut seam, uid(0x34));
        let active = uid(0x69);
        seam.runs
            .insert(active, run_view(active, AgentRunStatus::Running));
        seam.latest.insert(uid(0x01), active);
        let mut ticker = ticker_row(uid(0x01), now);
        ticker.used = 4;
        ticker.enabled = true;
        ticker.next_run_at = Some(now);
        seam.tickers.insert(uid(0x01), ticker);
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, REASON_PRIOR_RUN_ACTIVE);
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        assert_eq!(seam.rollbacks, 1);
        let row = seam.tickers.get(&uid(0x01)).expect("ticker kept");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_ticker_scalars(&case["before"]["ticker"], row);
        assert_eq!(row.used, 4);
        assert_eq!(row.next_run_at, Some(now));
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn m9_free_comment_creates_continuation() {
        let case = comment_case("M9_created");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        seam.issues.get_mut(&uid(0x01)).expect("issue").sequence_id = 20;
        let mut seam = seam.with_bundle();
        let prior = uid(0x6A);
        let mut prior_run = run_view(prior, AgentRunStatus::Completed);
        prior_run.runner_id = Some(uid(0x55));
        seam.runs.insert(prior, prior_run);
        seam.latest.insert(uid(0x01), prior);
        seam.runners.insert(
            uid(0x55),
            RunnerView {
                id: uid(0x55),
                pod_id: uid(0x50),
                status: "online".to_owned(),
            },
        );
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, "created");
        assert!(outcome.coalesced_into.is_none());
        let run_id = outcome.created_run.expect("run created");
        let run = seam.runs.get(&run_id).expect("run stored").clone();
        let golden = &case["after"]["run"];
        assert_eq!(run.status.value(), golden["status"].as_str().unwrap());
        assert_eq!(run.trigger, golden["trigger"].as_str().unwrap());
        assert_eq!(run.phase_kind, golden["phase_kind"].as_str().unwrap());
        assert_eq!(run.parent_run_id, Some(prior));
        assert_eq!(run.pinned_runner_id, Some(uid(0x55)));
        assert!(golden["owner"].is_null());
        assert_eq!(run.created_by_id, uid(0x40));
        assert_eq!(run.pod_id, uid(0x50));
        assert_eq!(run.work_item_id, Some(uid(0x01)));
        assert!(!run.prompt.is_empty());
        assert!(run.prompt.contains("FX7-20"));
        assert!(run.prompt_manifest.is_some());
        assert_eq!(seam.dispatched, vec![run_id]);
        assert_eq!(seam.rollbacks, 0);
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_eq!(
            row.next_run_at,
            Some(now + chrono::Duration::seconds(10_800))
        );
    }

    #[tokio::test]
    async fn m10_builder_failure_rolls_clock_back() {
        let case = comment_case("M10_no_run_rollback");
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        seam.execution = Some(Err(ExecutionError::Refused("fx7-no-executor".to_owned())));
        let prior = uid(0x6B);
        seam.runs
            .insert(prior, run_view(prior, AgentRunStatus::Completed));
        seam.latest.insert(uid(0x01), prior);
        let mut ticker = ticker_row(uid(0x01), now);
        ticker.used = 1;
        ticker.enabled = true;
        ticker.next_run_at = Some(now);
        seam.tickers.insert(uid(0x01), ticker);
        let outcome = handle_issue_comment(&mut seam, &comment_req(Some(uid(0x40)), now))
            .await
            .expect("comment runs");
        assert_eq!(outcome.reason, case["reason"].as_str().unwrap());
        assert_eq!(outcome.reason, "fx7-no-executor");
        assert!(outcome.created_run.is_none());
        assert!(outcome.coalesced_into.is_none());
        assert_eq!(seam.rollbacks, 1);
        // The reconcile retime landed, then the rollback discarded it.
        assert_eq!(seam.ticker_saves.len(), 1);
        let row = seam.tickers.get(&uid(0x01)).expect("ticker kept");
        assert_ticker_scalars(&case["after"]["ticker"], row);
        assert_ticker_scalars(&case["before"]["ticker"], row);
        assert_eq!(row.used, 1);
        assert_eq!(row.next_run_at, Some(now));
        assert!(seam.dispatched.is_empty());
        assert!(seam.inserted.is_empty());
    }

    // -- scheduler matrix (FX-ORCH-07 S1-S9) ------------------------------

    fn binding_view() -> BindingView {
        BindingView {
            id: uid(0x80),
            workspace_id: uid(0x10),
            project_id: Some(uid(0x20)),
            scheduler_id: uid(0x81),
            outcome_mode: OUTCOME_MODE_CREATE_ISSUE.to_owned(),
            extra_context: String::new(),
            actor_id: Some(uid(0x40)),
            pod_id: None,
        }
    }

    fn scheduler_view() -> SchedulerView {
        SchedulerView {
            id: uid(0x81),
            slug: "fx7-sched".to_owned(),
            name: "FX7 Sched".to_owned(),
            description: String::new(),
            prompt: "Do the scheduled thing.".to_owned(),
        }
    }

    fn workspace_view() -> WorkspaceView {
        WorkspaceView {
            slug: "fx7-ws".to_owned(),
            name: "fx7-ws".to_owned(),
        }
    }

    fn scheduler_seam() -> FakeEntriesSeam {
        let mut seam = FakeEntriesSeam::minimal();
        seam.bindings.insert(uid(0x80), binding_view());
        seam.schedulers.insert(uid(0x81), scheduler_view());
        seam.workspaces.insert(uid(0x10), workspace_view());
        seam
    }

    fn cloud_project(seam: &mut FakeEntriesSeam) {
        seam.projects
            .get_mut(&uid(0x20))
            .expect("project")
            .default_agent_executor = AgentExecutorKind::CloudAgent.value().to_owned();
    }

    #[tokio::test]
    async fn s1_default_pod_creates_project_scoped_run() {
        let case = scheduler_case("S1_created_default_pod");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.error.is_none());
        let run_id = outcome.run.expect("run returned");
        let run = seam.runs.get(&run_id).expect("run stored").clone();
        let golden = &case["after"];
        assert_eq!(run.status.value(), golden["status"].as_str().unwrap());
        assert_eq!(run.trigger, golden["trigger"].as_str().unwrap());
        assert_eq!(run.phase_kind, golden["phase_kind"].as_str().unwrap());
        assert_eq!(run.parent_run_id, None);
        assert!(golden["parent_run"].is_null());
        assert_eq!(run.pinned_runner_id, None);
        assert!(golden["pinned_runner"].is_null());
        assert!(golden["owner"].is_null());
        assert_eq!(run.created_by_id, uid(0x40));
        assert_eq!(run.pod_id, uid(0x50));
        assert_eq!(run.work_item_id, None);
        assert!(golden["work_item"].is_null());
        assert_eq!(run.run_config, golden["run_config"]);
        assert_eq!(
            run.executor_kind.value(),
            golden["executor_kind"].as_str().unwrap()
        );
        assert_eq!(run.tool_plan, golden["tool_plan"]);
        assert_eq!(run.error_code, golden["error_code"].as_str().unwrap());
        assert_eq!(run.error, golden["error"].as_str().unwrap());
        assert_eq!(run.ended_at, None);
        assert!(golden["ended_at"].is_null());
        assert_eq!(seam.scheduler_runs.len(), 1);
        assert_eq!(seam.scheduler_runs[0].binding_id, uid(0x80));
        assert!(!run.prompt.is_empty());
        assert!(run.prompt.contains("FX7 Sched"));
        assert!(run.prompt.contains(&run_id.to_string()));
        let manifest = run.prompt_manifest.expect("manifest stamped");
        assert!(manifest.is_array());
        assert_eq!(manifest.as_array().expect("bare list").len(), 6);
        assert_eq!(seam.dispatched, vec![run_id]);
        assert!(seam.finalized.is_empty());
        assert_eq!(seam.compose_calls, 1);
    }

    #[tokio::test]
    async fn s1b_live_override_wins_over_default() {
        let now = frozen_now();
        let mut seam = scheduler_seam();
        seam.pods.insert(
            uid(0x51),
            PodView {
                id: uid(0x51),
                project_id: uid(0x20),
            },
        );
        seam.bindings.get_mut(&uid(0x80)).expect("binding").pod_id = Some(uid(0x51));
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        let run_id = outcome.run.expect("run returned");
        let run = seam.runs.get(&run_id).expect("run stored");
        assert_eq!(run.pod_id, uid(0x51));
    }

    #[tokio::test]
    async fn s2a_soft_deleted_override_falls_back_to_default() {
        let now = frozen_now();
        let mut seam = scheduler_seam();
        // The dead pod is absent from the live-pod map.
        seam.bindings.get_mut(&uid(0x80)).expect("binding").pod_id = Some(uid(0x52));
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        let run_id = outcome.run.expect("run returned");
        let run = seam.runs.get(&run_id).expect("run stored");
        assert_eq!(run.pod_id, uid(0x50));
    }

    #[tokio::test]
    async fn s2b_cross_project_override_falls_back_to_default() {
        let now = frozen_now();
        let mut seam = scheduler_seam();
        seam.pods.insert(
            uid(0x53),
            PodView {
                id: uid(0x53),
                project_id: uid(0x21),
            },
        );
        seam.bindings.get_mut(&uid(0x80)).expect("binding").pod_id = Some(uid(0x53));
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        let run_id = outcome.run.expect("run returned");
        let run = seam.runs.get(&run_id).expect("run stored");
        assert_eq!(run.pod_id, uid(0x50));
    }

    #[tokio::test]
    async fn s3_no_pod_short_circuits_with_verbatim_error() {
        let case = scheduler_case("S3_no_pod");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        seam.default_pods.remove(&uid(0x20));
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        let error = outcome.error.expect("error returned");
        assert_eq!(error, no_default_pod_message(Some(uid(0x20))));
        // Same shape as the fixture's verbatim string.
        let fixture_error = case["returned"]["error"].as_str().unwrap();
        assert!(fixture_error.starts_with("no default pod for project "));
        assert!(error.starts_with("no default pod for project "));
        assert!(seam.scheduler_runs.is_empty());
        assert!(seam.dispatched.is_empty());
    }

    #[test]
    fn s3_no_pod_message_matches_fixture_byte_for_byte() {
        let case = scheduler_case("S3_no_pod");
        let project_id: Uuid = "77777777-aaaa-bbbb-cccc-000000000707"
            .parse()
            .expect("uuid parses");
        assert_eq!(
            no_default_pod_message(Some(project_id)),
            case["returned"]["error"].as_str().unwrap()
        );
    }

    fn stop_after_creator(seam: &mut FakeEntriesSeam) {
        seam.execution = Some(Err(ExecutionError::Refused(
            "fx7-stop-after-creator".to_owned(),
        )));
    }

    fn role_pass(seam: &mut FakeEntriesSeam, user: Uuid) {
        seam.role_facts.insert(user, (true, true, false, false));
    }

    #[tokio::test]
    async fn s4a_cloud_actor_wins_chain() {
        let case = scheduler_case("S4a_cloud_actor_wins");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        cloud_project(&mut seam);
        role_pass(&mut seam, uid(0x40));
        role_pass(&mut seam, uid(0x41));
        seam.llm.insert(uid(0x40), true);
        seam.llm.insert(uid(0x41), true);
        stop_after_creator(&mut seam);
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        assert_eq!(
            outcome.error.as_deref(),
            Some(case["returned_error"].as_str().unwrap())
        );
        assert_eq!(seam.execution_requests.len(), 1);
        let req = &seam.execution_requests[0];
        assert_eq!(req.actor.expect("actor").id, uid(0x40));
        assert_eq!(req.run_kind, "scheduler");
        assert!(!req.has_issue);
        assert!(req.automatic);
        assert_eq!(req.requested, None);
        // First match wins: the lead is never consulted.
        assert_eq!(seam.role_calls.len(), 1);
        assert_eq!(seam.role_calls[0].0, uid(0x40));
        assert_eq!(seam.llm_calls, vec![uid(0x40)]);
    }

    #[tokio::test]
    async fn s4b_cloud_bot_actor_skipped() {
        let now = frozen_now();
        let mut seam = scheduler_seam();
        cloud_project(&mut seam);
        seam.bindings.get_mut(&uid(0x80)).expect("binding").actor_id = Some(uid(0x42));
        seam.users.insert(
            uid(0x42),
            UserFlags {
                is_active: true,
                is_bot: true,
            },
        );
        role_pass(&mut seam, uid(0x41));
        seam.llm.insert(uid(0x41), true);
        stop_after_creator(&mut seam);
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        assert_eq!(seam.execution_requests.len(), 1);
        assert_eq!(
            seam.execution_requests[0].actor.expect("actor").id,
            uid(0x41)
        );
        // The bot short-circuits before the role and LLM seams.
        assert!(seam.role_calls.iter().all(|call| call.0 != uid(0x42)));
        assert_eq!(seam.llm_calls, vec![uid(0x41)]);
    }

    #[tokio::test]
    async fn s4c_cloud_actor_without_llm_skipped() {
        let now = frozen_now();
        let mut seam = scheduler_seam();
        cloud_project(&mut seam);
        role_pass(&mut seam, uid(0x40));
        role_pass(&mut seam, uid(0x41));
        seam.llm.insert(uid(0x40), false);
        seam.llm.insert(uid(0x41), true);
        stop_after_creator(&mut seam);
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        assert_eq!(seam.execution_requests.len(), 1);
        assert_eq!(
            seam.execution_requests[0].actor.expect("actor").id,
            uid(0x41)
        );
        assert_eq!(seam.llm_calls, vec![uid(0x40), uid(0x41)]);
    }

    #[tokio::test]
    async fn s4d_cloud_actor_without_role_skipped() {
        let now = frozen_now();
        let mut seam = scheduler_seam();
        cloud_project(&mut seam);
        seam.role_facts
            .insert(uid(0x40), (true, false, false, false));
        role_pass(&mut seam, uid(0x41));
        seam.llm.insert(uid(0x41), true);
        stop_after_creator(&mut seam);
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        assert_eq!(seam.execution_requests.len(), 1);
        assert_eq!(
            seam.execution_requests[0].actor.expect("actor").id,
            uid(0x41)
        );
        // The role refusal short-circuits before the LLM seam.
        assert_eq!(seam.role_calls.len(), 2);
        assert_eq!(seam.llm_calls, vec![uid(0x41)]);
    }

    #[tokio::test]
    async fn s5_cloud_without_candidates_answers_no_creator() {
        let case = scheduler_case("S5_cloud_no_creator");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        cloud_project(&mut seam);
        seam.bindings.get_mut(&uid(0x80)).expect("binding").actor_id = None;
        seam.projects
            .get_mut(&uid(0x20))
            .expect("project")
            .project_lead_id = None;
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        assert_eq!(outcome.error.as_deref(), case["returned"]["error"].as_str());
        assert_eq!(outcome.error.as_deref(), Some(SCHEDULER_NO_CREATOR));
        assert!(seam.scheduler_runs.is_empty());
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn s5b_local_without_actor_uses_agent_system_user() {
        let now = frozen_now();
        let mut seam = scheduler_seam();
        seam.bindings.get_mut(&uid(0x80)).expect("binding").actor_id = None;
        seam.agent_user = Some(Ok(uid(0x43)));
        seam.users.insert(uid(0x43), human_flags());
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.error.is_none());
        let run_id = outcome.run.expect("run returned");
        let run = seam.runs.get(&run_id).expect("run stored");
        assert_eq!(run.created_by_id, uid(0x43));
        assert_eq!(seam.dispatched, vec![run_id]);
    }

    #[tokio::test]
    async fn s6_cloud_outcome_mode_refusal_before_insert() {
        let case = scheduler_case("S6_outcome_mode_refusal");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        cloud_project(&mut seam);
        role_pass(&mut seam, uid(0x40));
        seam.llm.insert(uid(0x40), true);
        seam.execution = Some(Ok(ExecutionFields {
            executor_kind: AgentExecutorKind::CloudAgent,
            tool_plan: json!({}),
            pinned_runner_entry: Some(None),
            error_code: None,
            cloud_admission_error: None,
        }));
        seam.bindings
            .get_mut(&uid(0x80))
            .expect("binding")
            .outcome_mode = "apply_fix".to_owned();
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        assert_eq!(outcome.error.as_deref(), case["returned"]["error"].as_str());
        assert_eq!(
            outcome.error.as_deref(),
            Some(SCHEDULER_OUTCOME_MODE_REFUSAL)
        );
        assert!(seam.scheduler_runs.is_empty());
        assert!(seam.dispatched.is_empty());
    }

    #[tokio::test]
    async fn s7_admission_failure_returns_failed_run() {
        let case = scheduler_case("S7_admission_failed_returned");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        seam.lock = Some(Ok(Some(AdmissionError {
            code: "run_quota_exceeded".to_owned(),
            detail: "Cloud Agent queue is full for this workspace".to_owned(),
        })));
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.error.is_none());
        let run_id = outcome.run.expect("failed run returned");
        let run = seam.runs.get(&run_id).expect("run stored").clone();
        let golden = &case["after"];
        assert_eq!(run.status.value(), golden["status"].as_str().unwrap());
        assert_eq!(run.trigger, golden["trigger"].as_str().unwrap());
        assert_eq!(run.phase_kind, golden["phase_kind"].as_str().unwrap());
        assert_eq!(run.parent_run_id, None);
        assert_eq!(run.pinned_runner_id, None);
        assert_eq!(run.created_by_id, uid(0x40));
        assert_eq!(run.pod_id, uid(0x50));
        assert_eq!(run.work_item_id, None);
        assert_eq!(run.prompt, golden["prompt"].as_str().unwrap());
        assert!(run.prompt_manifest.is_none());
        assert!(golden["prompt_manifest"].is_null());
        assert_eq!(run.run_config, golden["run_config"]);
        assert_eq!(
            run.executor_kind.value(),
            golden["executor_kind"].as_str().unwrap()
        );
        assert_eq!(run.tool_plan, golden["tool_plan"]);
        assert_eq!(run.error_code, golden["error_code"].as_str().unwrap());
        assert_eq!(run.error, golden["error"].as_str().unwrap());
        assert_eq!(run.ended_at, Some(now));
        assert_eq!(
            seam.finalized,
            vec![(
                run_id,
                "run_quota_exceeded".to_owned(),
                "Cloud Agent queue is full for this workspace".to_owned()
            )]
        );
        assert!(seam.dispatched.is_empty());
        assert_eq!(seam.compose_calls, 0);
    }

    #[tokio::test]
    async fn s8_render_failure_returns_failed_run() {
        let case = scheduler_case("S8_render_failed_returned");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        seam.compose_script = Some(Err("fx7-sched-boom".to_owned()));
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.error.is_none());
        let run_id = outcome.run.expect("failed run returned");
        let run = seam.runs.get(&run_id).expect("run stored").clone();
        let golden = &case["after"];
        assert_eq!(run.status.value(), golden["status"].as_str().unwrap());
        assert_eq!(run.error_code, golden["error_code"].as_str().unwrap());
        assert_eq!(run.error_code, "prompt_build_failed");
        assert_eq!(run.error, golden["error"].as_str().unwrap());
        assert_eq!(run.ended_at, Some(now));
        assert_eq!(run.prompt, golden["prompt"].as_str().unwrap());
        assert!(run.prompt_manifest.is_none());
        assert_eq!(
            seam.finalized,
            vec![(
                run_id,
                "prompt_build_failed".to_owned(),
                "prompt build failed: fx7-sched-boom".to_owned()
            )]
        );
        assert!(seam.dispatched.is_empty());
        assert_eq!(seam.compose_calls, 1);
    }

    #[tokio::test]
    async fn s9_executor_refusal_short_circuits() {
        let case = scheduler_case("S9_executor_unavailable");
        let now = frozen_now();
        let mut seam = scheduler_seam();
        seam.execution = Some(Err(ExecutionError::Refused("fx7-no-executor".to_owned())));
        let outcome = dispatch_scheduler_run(
            &mut seam,
            &SchedulerRequest {
                binding_id: uid(0x80),
                now,
            },
        )
        .await
        .expect("scheduler runs");
        assert!(outcome.run.is_none());
        assert_eq!(outcome.error.as_deref(), case["returned"]["error"].as_str());
        assert!(seam.scheduler_runs.is_empty());
        assert!(seam.dispatched.is_empty());
    }

    // -- signals goldens (FX-ORCH-07 G1-G7) -------------------------------

    fn fire_req(prev: Option<Uuid>, current: Option<Uuid>, now: DateTime<Utc>) -> FireRequest {
        FireRequest {
            issue_id: uid(0x01),
            prev_state_id: prev,
            current_state_id: current,
            dispatch_immediate: true,
            moved_by_run: None,
            now,
            jitter_secs: 0.0,
            created_by: None,
        }
    }

    #[tokio::test]
    async fn g1_capture_new_instance_answers_none_without_read() {
        let cases = signals_cases();
        let mut seam = FakeEntriesSeam::minimal();
        let snapshot = capture_prior_state(&mut seam, None)
            .await
            .expect("capture runs");
        assert!(snapshot.is_none());
        assert!(cases["G1_capture_new_instance"]["snapshot"].is_null());
        assert!(seam.prior_state_calls.is_empty());
    }

    #[tokio::test]
    async fn g2_capture_existing_snapshots_stored_state() {
        let cases = signals_cases();
        let mut seam = FakeEntriesSeam::minimal();
        seam.prior_states.insert(uid(0x01), Some(uid(0x30)));
        let snapshot = capture_prior_state(&mut seam, Some(uid(0x01)))
            .await
            .expect("capture runs");
        assert_eq!(snapshot, Some(uid(0x30)));
        assert!(!cases["G2_capture_existing"]["prev_state_attr"].is_null());
        assert_eq!(seam.prior_state_calls, vec![uid(0x01)]);
    }

    #[tokio::test]
    async fn g3_capture_missing_row_answers_none() {
        let cases = signals_cases();
        let mut seam = FakeEntriesSeam::minimal();
        let snapshot = capture_prior_state(&mut seam, Some(uid(0x01)))
            .await
            .expect("capture runs");
        assert!(snapshot.is_none());
        assert!(cases["G3_capture_missing_row"]["snapshot"].is_null());
        assert_eq!(seam.prior_state_calls, vec![uid(0x01)]);
    }

    #[tokio::test]
    async fn g4_equal_states_skip_handler_without_io() {
        let cases = signals_cases();
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let mut preflight = FakePreflight::allow();
        let outcome = fire_state_transition(
            &mut seam,
            &mut preflight,
            &fire_req(Some(uid(0x30)), Some(uid(0x30)), now),
        )
        .await
        .expect("fire runs");
        assert_eq!(outcome, FireOutcome::NoTransition);
        assert!(!cases["G4_no_transition_noop"]["handler_called"]
            .as_bool()
            .unwrap());
        assert!(seam.state_calls.is_empty());
        assert!(seam.tickers.is_empty());
        assert!(preflight.calls.is_empty());
    }

    #[test]
    fn g5_fire_builds_exact_handler_args() {
        let now = frozen_now();
        let from = state_view("Todo", "unstarted");
        let to = state_view("In Progress", "started");
        let req = fire_req(Some(uid(0x31)), Some(uid(0x30)), now);
        let transition = fire_transition_request(&req, Some(from.clone()), Some(to.clone()));
        assert_eq!(transition.issue_id, uid(0x01));
        assert_eq!(transition.from_state, Some(from));
        assert_eq!(transition.to_state, Some(to));
        assert_eq!(transition.actor, None);
        assert!(transition.dispatch_immediate);
        assert_eq!(transition.moved_by_run, None);
        assert_eq!(transition.now, now);
        assert_eq!(transition.jitter_secs, 0.0);
        assert_eq!(transition.created_by, None);
    }

    #[test]
    fn g5b_fire_passes_flag_overrides_through() {
        let now = frozen_now();
        let mover = uid(0x6C);
        let mut req = fire_req(Some(uid(0x31)), Some(uid(0x30)), now);
        req.dispatch_immediate = false;
        req.moved_by_run = Some(mover);
        let transition = fire_transition_request(&req, None, None);
        assert!(!transition.dispatch_immediate);
        assert_eq!(transition.moved_by_run, Some(mover));
        assert_eq!(transition.actor, None);
    }

    #[tokio::test]
    async fn g5_fire_end_to_end_calls_handler() {
        let _guard = COUNTER_LOCK.lock().await;
        let before = orchestration_error_count();
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal().with_bundle();
        let mut preflight = FakePreflight::allow();
        let outcome = fire_state_transition(
            &mut seam,
            &mut preflight,
            &fire_req(Some(uid(0x31)), Some(uid(0x30)), now),
        )
        .await
        .expect("fire runs");
        match outcome {
            FireOutcome::Called(transition) => {
                assert_eq!(transition.reason, "created");
                assert!(transition.created_run.is_some());
            }
            other => panic!("expected Called, got {other:?}"),
        }
        assert_eq!(preflight.calls.len(), 1);
        assert_eq!(seam.dispatched.len(), 1);
        assert_eq!(orchestration_error_count(), before);
    }

    #[tokio::test]
    async fn g5c_fire_to_deleted_state_answers_not_a_trigger() {
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let mut preflight = FakePreflight::allow();
        let outcome = fire_state_transition(
            &mut seam,
            &mut preflight,
            &fire_req(Some(uid(0x30)), None, now),
        )
        .await
        .expect("fire runs");
        match outcome {
            FireOutcome::Called(transition) => {
                assert_eq!(transition.reason, REASON_NOT_A_TRIGGER_STATE);
                assert!(transition.created_run.is_none());
            }
            other => panic!("expected Called, got {other:?}"),
        }
        assert!(seam.tickers.is_empty());
    }

    #[tokio::test]
    async fn g6_handler_error_is_swallowed_with_counter_and_log() {
        let _guard = COUNTER_LOCK.lock().await;
        let before = orchestration_error_count();
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        // No issue row: the handler's first read fails inside the catch.
        seam.issues.remove(&uid(0x01));
        let mut preflight = FakePreflight::allow();
        let req = fire_req(Some(uid(0x31)), Some(uid(0x30)), now);
        let outcome = fire_state_transition(&mut seam, &mut preflight, &req)
            .await
            .expect("fire never raises handler errors");
        match outcome {
            FireOutcome::Failed {
                log_line,
                error,
                total_errors,
            } => {
                assert_eq!(total_errors, before + 1);
                assert_eq!(
                    log_line,
                    fire_error_log_line(&uid(0x01), Some(uid(0x31)), Some(uid(0x30)), before + 1)
                );
                assert!(error.contains("no issue"), "error carries cause: {error}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(orchestration_error_count(), before + 1);
    }

    #[test]
    fn g6_log_line_matches_fixture_byte_for_byte() {
        let cases = signals_cases();
        let issue: Uuid = "77777777-aaaa-bbbb-cccc-000000007128"
            .parse()
            .expect("uuid parses");
        let from: Uuid = "77777777-aaaa-bbbb-cccc-000000007401"
            .parse()
            .expect("uuid parses");
        let to: Uuid = "77777777-aaaa-bbbb-cccc-000000007403"
            .parse()
            .expect("uuid parses");
        let golden = &cases["G6_error_swallowed"];
        assert_eq!(
            fire_error_log_line(&issue, Some(from), Some(to), 1),
            golden["log_records"][0]["message"].as_str().unwrap()
        );
        assert_eq!(golden["log_records"][0]["level"].as_str().unwrap(), "ERROR");
        assert_eq!(
            golden["log_records"][0]["exc"].as_str().unwrap(),
            "RuntimeError: fx7-boom"
        );
        assert_eq!(golden["counter_after"].as_u64().unwrap(), 1);
    }

    #[tokio::test]
    async fn g7_missing_from_state_resolves_to_none() {
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal().with_bundle();
        let mut preflight = FakePreflight::allow();
        // 0x39 is absent: the lookup answers None (the DoesNotExist
        // catch), so the move reads as an entry and dispatches.
        let outcome = fire_state_transition(
            &mut seam,
            &mut preflight,
            &fire_req(Some(uid(0x39)), Some(uid(0x30)), now),
        )
        .await
        .expect("fire runs");
        match outcome {
            FireOutcome::Called(transition) => {
                assert_eq!(transition.reason, "created");
                assert!(transition.created_run.is_some());
            }
            other => panic!("expected Called, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn g7_none_from_state_resolves_to_none() {
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal().with_bundle();
        let mut preflight = FakePreflight::allow();
        let outcome = fire_state_transition(
            &mut seam,
            &mut preflight,
            &fire_req(None, Some(uid(0x30)), now),
        )
        .await
        .expect("fire runs");
        match outcome {
            FireOutcome::Called(transition) => {
                assert_eq!(transition.reason, "created");
            }
            other => panic!("expected Called, got {other:?}"),
        }
        assert_eq!(seam.state_calls[0], None);
    }

    // -- vocabulary, SQL, pure units ------------------------------------

    #[test]
    fn reason_consts_match_fixture_matrices() {
        let transitions: Value = serde_json::from_str(TRANSITION_FIXTURE).expect("fixture parses");
        let expected = [
            ("T1_leave_bucket", REASON_NOT_A_TRIGGER_STATE),
            ("T2_non_trigger", REASON_NOT_A_TRIGGER_STATE),
            ("T3_dispatch_deferred", REASON_DISPATCH_DEFERRED),
            ("T4_pool_spent", REASON_POOL_SPENT),
            ("T5_entry_queued_human", REASON_ENTRY_QUEUED),
            ("T5b_entry_queued_agent", REASON_ENTRY_QUEUED),
            ("T6a_no_dispatch_custom_reason", "ticking-disabled"),
            ("T6b_no_dispatch_bare", REASON_NO_DISPATCH),
            ("T7_active_race", REASON_ACTIVE_RUN_EXISTS),
            ("T8_no_creator", REASON_NO_CREATOR),
            ("T9_no_pod", REASON_NO_POD_AVAILABLE),
            ("T10_no_eligible_runner", REASON_NO_ELIGIBLE_RUNNER),
            ("T11a_created", "created"),
            ("T11b_cross_stage_resume_capture", "created"),
        ];
        for (name, reason) in expected {
            assert_eq!(
                transitions["cases"][name]["reason"].as_str().unwrap(),
                reason,
                "{name}"
            );
        }
        let comments: Value = serde_json::from_str(COMMENT_FIXTURE).expect("fixture parses");
        let expected = [
            ("M1_no_actor", REASON_NO_ACTOR),
            ("M2_bot_comment", REASON_BOT_COMMENT),
            ("M3_state_not_eligible", REASON_STATE_NOT_ELIGIBLE),
            ("M4_no_prior_run", REASON_NO_PRIOR_RUN),
            ("M5_coalesced", REASON_COALESCED),
            ("M6_no_pod", REASON_NO_POD_AVAILABLE),
            ("M7_entry_queued", REASON_ENTRY_QUEUED),
            ("M8_prior_active_rollback", REASON_PRIOR_RUN_ACTIVE),
            ("M9_created", "created"),
            ("M10_no_run_rollback", "fx7-no-executor"),
        ];
        for (name, reason) in expected {
            assert_eq!(
                comments["cases"][name]["reason"].as_str().unwrap(),
                reason,
                "{name}"
            );
        }
    }

    #[test]
    fn signal_attr_consts_match_golden() {
        let cases = signals_cases();
        let constants = &cases["constants"];
        assert_eq!(
            constants["PREVIOUS_STATE_ATTR"].as_str().unwrap(),
            PREVIOUS_STATE_ATTR
        );
        assert_eq!(
            constants["DISPATCH_IMMEDIATE_ATTR"].as_str().unwrap(),
            DISPATCH_IMMEDIATE_ATTR
        );
        assert_eq!(
            constants["MOVED_BY_RUN_ATTR"].as_str().unwrap(),
            MOVED_BY_RUN_ATTR
        );
    }

    #[test]
    fn scheduler_consts_match_fixture() {
        assert_eq!(OUTCOME_MODE_CREATE_ISSUE, "create_issue");
        assert_eq!(SCHEDULER_ALLOWED_ROLES, &[20, 15, 5]);
        assert_eq!(
            SCHEDULER_NO_CREATOR,
            scheduler_case("S5_cloud_no_creator")["returned"]["error"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            SCHEDULER_OUTCOME_MODE_REFUSAL,
            scheduler_case("S6_outcome_mode_refusal")["returned"]["error"]
                .as_str()
                .unwrap()
        );
    }

    #[test]
    fn sql_shapes_match_django_rendering() {
        assert_eq!(
            PRIOR_STATE_SELECT_SQL,
            "SELECT id, state_id FROM issues WHERE id = $1"
        );
        assert_eq!(
            queued_follow_up_sql(),
            format!(
                "SELECT {} FROM agent_run WHERE work_item_id = $1 AND status = 'queued' \
                 ORDER BY created_at DESC LIMIT 1",
                RUN_VIEW_COLUMNS.join(", ")
            )
        );
        assert_eq!(
            SCHEDULER_POD_OVERRIDE_SQL,
            "SELECT id, project_id FROM pod WHERE deleted_at IS NULL AND deleted_at IS NULL AND id = $1 AND project_id = $2"
        );
        assert_eq!(
            BINDING_SELECT_SQL,
            "SELECT id, workspace_id, project_id, scheduler_id, outcome_mode, extra_context, actor_id, pod_id FROM scheduler_bindings WHERE id = $1"
        );
        assert_eq!(
            SCHEDULER_SELECT_SQL,
            "SELECT id, slug, name, description, prompt FROM schedulers WHERE id = $1 AND deleted_at IS NULL"
        );
        assert_eq!(
            CLOCK_POLICY_SQL,
            "SELECT agent_ticking_enabled, agent_default_max_ticks, agent_default_interval_seconds, agent_review_default_interval_seconds, agent_test_default_interval_seconds FROM projects WHERE id = $1"
        );
    }

    #[test]
    fn scheduler_insert_shares_l6_column_order() {
        // Same 38 non-generated columns in model order as RUN_INSERT_SQL;
        // only the VALUES shape differs.
        let head = |sql: &str| sql.split(") VALUES").next().expect("head").to_owned();
        assert_eq!(
            head(SCHEDULER_RUN_INSERT_SQL),
            head(crate::orchestration::creation::RUN_INSERT_SQL)
        );
        let values = SCHEDULER_RUN_INSERT_SQL
            .split("VALUES")
            .nth(1)
            .expect("values");
        for bind in ["$1", "$2", "$3", "$4", "$5", "$6", "$7", "$8", "$9", "$10"] {
            assert!(values.contains(bind), "{bind} bound");
        }
        assert!(!values.contains("$11"), "no $11");
        assert!(
            values.contains("NULL, $6, NULL, 'queued'"),
            "work_item NULL, binding bound, parent NULL"
        );
        assert!(values.contains("'scheduler'"), "scheduler trigger literal");
        assert!(
            values.contains("'', '{}'"),
            "empty phase kind, empty run config"
        );
        assert_eq!(
            scheduler_run_insert_returning_sql(),
            format!(
                "{SCHEDULER_RUN_INSERT_SQL} RETURNING {}",
                RUN_VIEW_COLUMNS.join(", ")
            )
        );
    }

    #[test]
    fn delegation_trigger_matches_ticking_registry() {
        let ticking = StateRef {
            group: "started",
            name: "In Progress",
        };
        assert!(is_delegation_trigger(Some(&ticking)));
        let review = StateRef {
            group: "review",
            name: "In Review",
        };
        assert!(is_delegation_trigger(Some(&review)));
        let todo = StateRef {
            group: "unstarted",
            name: "Todo",
        };
        assert!(!is_delegation_trigger(Some(&todo)));
        let grooming = StateRef {
            group: "started",
            name: "Grooming",
        };
        assert!(!is_delegation_trigger(Some(&grooming)));
        let done = StateRef {
            group: "completed",
            name: "Done",
        };
        assert!(!is_delegation_trigger(Some(&done)));
        assert!(!is_delegation_trigger(None));
    }

    #[test]
    fn active_status_matches_python_is_active() {
        // `runner/models.py:1146-1160`: QUEUED occupies the slot, BLOCKED
        // does not — the same 7 as L2's ACTIVE_STATUSES.
        for status in [
            AgentRunStatus::Queued,
            AgentRunStatus::Assigned,
            AgentRunStatus::WaitingForWorktree,
            AgentRunStatus::Running,
            AgentRunStatus::CancelRequested,
            AgentRunStatus::AwaitingApproval,
            AgentRunStatus::AwaitingReauth,
        ] {
            assert!(is_active_status(status), "{status:?}");
        }
        for status in [
            AgentRunStatus::Blocked,
            AgentRunStatus::Completed,
            AgentRunStatus::Failed,
            AgentRunStatus::Cancelled,
            AgentRunStatus::Refused,
            AgentRunStatus::PausedAwaitingInput,
        ] {
            assert!(!is_active_status(status), "{status:?}");
        }
    }

    #[tokio::test]
    async fn nonzero_jitter_threads_into_retime_exactly() {
        let now = frozen_now();
        let mut seam = FakeEntriesSeam::minimal();
        let from = seam.states[&uid(0x31)].clone();
        let to = seam.states[&uid(0x30)].clone();
        let mut req = transition_req(Some(from), Some(to), now);
        req.dispatch_immediate = false;
        req.jitter_secs = 45.5;
        let mut preflight = FakePreflight::allow();
        let outcome = handle_issue_state_transition(&mut seam, &mut preflight, &req)
            .await
            .expect("transition runs");
        assert_eq!(outcome.reason, REASON_DISPATCH_DEFERRED);
        let row = seam.tickers.get(&uid(0x01)).expect("ticker created");
        assert_eq!(
            row.next_run_at,
            Some(
                now + chrono::Duration::seconds(10_800)
                    + chrono::Duration::microseconds(45_500_000)
            )
        );
    }
}
