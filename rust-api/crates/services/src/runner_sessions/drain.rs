#![forbid(unsafe_code)]

//! Matcher drain: pod + runner dispatch (D-14, stage 5).
//!
//! Port of the dispatch half of
//! `apps/api/pi_dash/runner/services/matcher.py`:
//!
//! * `select_runner_in_pod` (`:89-121`) → [`select_runner_in_pod_stmt`].
//! * `next_queued_run_for_pod` (`:123-145`) → [`NEXT_QUEUED_RUN_FOR_POD_SQL`].
//! * `next_for_runner` (`:147-192`) → [`next_for_runner_stmt`].
//! * `drain_pod` (`:194-240`) → [`drain_pod_idle_list_stmt`] +
//!   [`plan_assignment`] + the executor's SeaORM assignment write +
//!   [`DrainEffect::SendAssign`], run per the executor recipe below.
//! * `drain_pod_by_id` (`:242-251`) → a SeaORM `pod` find (executors).
//! * `drain_for_runner` (`:253-296`) → [`drain_for_runner_lock_stmt`] +
//!   [`next_for_runner_stmt`] + the executor's SeaORM assignment
//!   write + [`plan_assignment`] + [`DrainEffect::SendAssign`].
//! * `drain_for_runner_by_id` (`:298-304`) → a SeaORM `runner` find
//!   (executors).
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
//! idle list ([`drain_pod_idle_list_stmt`],
//! [`alive_threshold`](super::guards::alive_threshold) evaluated
//! once, pod id); for each runner row in list order (freshest
//! heartbeat first), run [`next_for_runner_stmt`] for its
//! dispatch facts and, when a run row comes back, write the
//! assignment (`owner_id`, `runner_id`, `status='assigned'`,
//! `assigned_at=timezone.now()` per assignment) and record
//! [`plan_assignment`]; commit; then queue one
//! [`DrainEffect::SendAssign`] per assignment, in loop order, on the
//! post-commit queue. Returns the assignment count; logs
//! [`drain_pod_log`] when non-empty. The loop is runner-first: each
//! idle runner takes its personal-queue-first pick, so a pinned
//! head-of-line run never blocks other runners' unpinned work.
//!
//! `drain_for_runner(runner_id)` (`:253-296`): open a transaction;
//! re-lock the runner ([`drain_for_runner_lock_stmt`], threshold,
//! runner id) — a miss returns `false` with no further query; run
//! [`next_for_runner_stmt`] — a miss returns
//! `false`; write the assignment; commit; queue the
//! single [`DrainEffect::SendAssign`]. Returns `true` and logs
//! [`drain_for_runner_log`].
//!
//! `drain_pod_by_id` (`:242-251`): look the pod up (a SeaORM find
//! carrying the `Pod.objects` soft-delete scope) — a miss returns
//! `0`; else `drain_pod`. `drain_for_runner_by_id` (`:298-304`):
//! look the runner up (a SeaORM find) — a miss returns `false`;
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
//! * The `next_for_runner` statement is a sea-query
//!   `SelectStatement`: `pod_id` / `runner_id` / `owner_id` are bound
//!   as values wherever they occur (the D-15 predicate takes the
//!   owner id, so the engine gate rides the same binding).
//! * The `pod` SELECTs inside the fixture traces are ORM laziness
//!   (`pod=runner.pod` resolves the FK descriptor per runner); Rust
//!   binds `pod_id` from the already-fetched runner row, so no pod
//!   fetch runs on the drain path. `SAVEPOINT` / `RELEASE` /
//!   `BEGIN` / `COMMIT` lines are harness artifacts, not ported.
//! * Django orders by the `_is_mine` annotation's positional index
//!   (`ORDER BY 42`); the sea-query port orders by the annotation
//!   name, which selects identically.
//! * A non-private runner makes `filter_runs_usable_by_runner`
//!   return `qs.none()`, whose `.first()` answers `None` without
//!   touching the database — [`next_for_runner_stmt`] returns `None`
//!   and the executor issues no query (the D-15 `qs.none()`
//!   precedent).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * The `drain_pod` idle list carries **no** `pod_dispatch`
//!   exclusion (unlike `select_runner_in_pod` `:104`): `pinned_only`
//!   engines are listed, then can only take managed runs pinned to
//!   them. Ported as written.
//! * The pin-rank `CASE` (`THEN 0 ELSE 1`) sorts pinned-to-me before
//!   unpinned with FIFO inside each tier; the eligibility arms then
//!   narrow engines to managed pins (+ pod work while they take it)
//!   and own-agent runners to `local_runner` — the `IN
//!   (MACHINE_EXECUTORS)` term stays in the query next to the
//!   narrower equalities. Ported as written.
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

use chrono::{DateTime, Utc};
use pidash_db::runner_runs::agent_run::COLUMNS as AGENT_RUN_COLUMNS;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::AgentRunStatus;
use pidash_types::runner_sessions::envelopes::build_assign_msg;
use sea_query::{
    Alias, Condition, Expr, ExprTrait, LockBehavior, LockType, Order, Query, SelectStatement,
    SimpleExpr,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::runner_runs::guards::{
    runner_visible_issue_exists, runner_visible_scheduler_exists, runs_usable_by_runner_predicate,
};
use crate::runner_runs::{SetClause, SetValue};

use super::guards::{BUSY_STATUSES, RUNNER_COLUMNS};

// ---------------------------------------------------------------------------
// Pod projection (FX-RSES-07 `drain_pod.sql[2]` SELECT list)
// ---------------------------------------------------------------------------

/// `pod` columns in Django `_meta` order, pinned against the
/// fixture's pod SELECT list by the tests below.
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

/// Caller-fetched dispatch facts for [`next_for_runner_stmt`]: the
/// runner's own eligibility shape. `paused` is
/// `Runner.engine_paused` (engine whose machine's engine is paused);
/// `pod_member` is `pod_dispatch == 'pod_member'`; `team_runs` is
/// `DevMachine.engine_team_runs` (any value for own-agent runners).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchFacts {
    pub is_engine: bool,
    pub pod_member: bool,
    pub paused: bool,
    pub team_runs: bool,
    /// §10.2 strict 1:1 alternation: the runner's most recent
    /// assigned run (by `assigned_at`) had `pin_kind = dedicated`
    /// pinned to it. An idle runner's last run is terminal or
    /// paused, so this never reorders an active lane.
    pub last_dedicated: bool,
}

fn col(table: &'static str, column: &'static str) -> SimpleExpr {
    Expr::col((Alias::new(table), column)).into()
}

/// The `.exclude(agent_runs__status__in=BUSY_STATUSES)` reverse join:
/// `NOT EXISTS` a serving run on this runner (alias `U1`).
fn busy_not_exists() -> SimpleExpr {
    let statuses: Vec<SimpleExpr> = BUSY_STATUSES
        .iter()
        .map(|status| status.value().into())
        .collect();
    let sub = Query::select()
        .expr_as(
            SimpleExpr::Value(sea_query::Value::Int(Some(1))),
            Alias::new("a"),
        )
        .from_as(Alias::new("agent_run"), Alias::new("U1"))
        .and_where(col("U1", "status").is_in(statuses))
        .and_where(col("U1", "runner_id").eq(col("runner", "id")))
        .limit(1u64)
        .to_owned();
    Expr::exists(sub)
}

/// The `assignable()` engine-pause exclusion (`engine_paused_q`,
/// #2903): a paused engine is treated as offline for matching. The
/// machine test is an `IN (subquery)` rather than a join on purpose —
/// every caller carries `FOR UPDATE SKIP LOCKED`, and Postgres
/// refuses it on the nullable side of an outer join.
fn engine_paused_excluded() -> Condition {
    let paused = Query::select()
        .column("id")
        .from_as(Alias::new("dev_machine"), Alias::new("U0"))
        .cond_where(
            Condition::any()
                .add(col("U0", "engine_paused_by_owner").eq(true))
                .add(col("U0", "engine_auto_pause_reason").gt("")),
        )
        .to_owned();
    Condition::all()
        .add(col("runner", "dev_machine_id").in_subquery(paused))
        .add(col("runner", "dev_machine_id").is_not_null())
        .add(col("runner", "provisioning").eq("desktop_bundled"))
}

/// The shared `assignable()` base for the three runner selects:
/// projection in [`RUNNER_COLUMNS`] order, heartbeat freshness +
/// operator pause. Callers add the pod/runner target, the pod-work
/// exclusion (one select only), the busy exclusion, and the tail.
fn runner_base(threshold: DateTime<Utc>) -> SelectStatement {
    let mut query = Query::select().to_owned();
    for column in RUNNER_COLUMNS {
        query.column((Alias::new("runner"), column));
    }
    query
        .from(Alias::new("runner"))
        .and_where(col("runner", "last_heartbeat_at").gte(threshold))
        .and_where(col("runner", "paused_at").is_null())
        .and_where(col("runner", "status").eq("online"))
        .cond_where(engine_paused_excluded().not())
        .cond_where(Condition::all().add(busy_not_exists()).not());
    query
}

/// `select_runner_in_pod` (`:89-121`): one online, heartbeat-fresh,
/// unpaused, idle pod member in the pod, freshest heartbeat first.
/// Engines rank equally (PO-4): only `pinned_only` runners are
/// excluded, not `desktop_bundled`. Call inside a transaction
/// (`FOR UPDATE SKIP LOCKED`).
pub fn select_runner_in_pod_stmt(threshold: DateTime<Utc>, pod_id: Uuid) -> SelectStatement {
    let mut query = runner_base(threshold);
    query
        .and_where(col("runner", "pod_id").eq(pod_id))
        .cond_where(
            Condition::all()
                .add(col("runner", "pod_dispatch").eq("pinned_only"))
                .not(),
        )
        .order_by(("runner", "last_heartbeat_at"), Order::Desc)
        .limit(1u64)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    query
}

/// `drain_pod` idle list (`:211-223`): same filter as
/// [`select_runner_in_pod_stmt`] **without** the `pod_dispatch`
/// exclusion and **without** `LIMIT 1` — the full idle set, in drain
/// order (paused runners excluded by the `assignable()` arm, but
/// `pinned_only` engines stay listed: they can take their managed
/// pins). See the ported-bugs note.
pub fn drain_pod_idle_list_stmt(threshold: DateTime<Utc>, pod_id: Uuid) -> SelectStatement {
    let mut query = runner_base(threshold);
    query
        .and_where(col("runner", "pod_id").eq(pod_id))
        .order_by(("runner", "last_heartbeat_at"), Order::Desc)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    query
}

/// `drain_for_runner` re-lock (`:267-279`): the same row back under
/// `FOR UPDATE SKIP LOCKED` when it is still online, fresh, unpaused
/// and idle. No explicit `order_by`, so `Runner.Meta.ordering`
/// applies (`-last_heartbeat_at`, `-created_at`). A miss means:
/// return `false`, no further query.
pub fn drain_for_runner_lock_stmt(threshold: DateTime<Utc>, runner_id: Uuid) -> SelectStatement {
    let mut query = runner_base(threshold);
    query
        .and_where(col("runner", "id").eq(runner_id))
        .order_by(("runner", "last_heartbeat_at"), Order::Desc)
        .order_by(("runner", "created_at"), Order::Desc)
        .limit(1u64)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    query
}

// ---------------------------------------------------------------------------
// Run selects (`matcher.py:123-192`)
// ---------------------------------------------------------------------------

/// `next_queued_run_for_pod` (`:123-145`): the oldest unpinned
/// `QUEUED` machine-executor run in the pod, locked. `$1` is the
/// pod id. Pinned runs are excluded (`IS NULL`); the `IN` list is
/// `MACHINE_EXECUTORS` in tuple order. `FOR UPDATE SKIP LOCKED`:
/// call inside a transaction.
pub const NEXT_QUEUED_RUN_FOR_POD_SQL: &str = "SELECT \"agent_run\".\"id\", \"agent_run\".\"workspace_id\", \"agent_run\".\"owner_id\", \"agent_run\".\"created_by_id\", \"agent_run\".\"pod_id\", \"agent_run\".\"runner_id\", \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"pin_kind\", \"agent_run\".\"work_item_id\", \"agent_run\".\"scheduler_binding_id\", \"agent_run\".\"parent_run_id\", \"agent_run\".\"status\", \"agent_run\".\"executor_kind\", \"agent_run\".\"dispatch_attempts\", \"agent_run\".\"cancel_requested_at\", \"agent_run\".\"cancel_reason\", \"agent_run\".\"tool_plan\", \"agent_run\".\"terminal_hooks_applied_at\", \"agent_run\".\"terminal_capacity_released_at\", \"agent_run\".\"prompt\", \"agent_run\".\"trigger\", \"agent_run\".\"prompt_manifest\", \"agent_run\".\"phase_kind\", \"agent_run\".\"run_config\", \"agent_run\".\"required_capabilities\", \"agent_run\".\"thread_id\", \"agent_run\".\"agent_metadata\", \"agent_run\".\"lease_expires_at\", \"agent_run\".\"done_payload\", \"agent_run\".\"error_details\", \"agent_run\".\"refusal_category\", \"agent_run\".\"llm_model\", \"agent_run\".\"usage\", \"agent_run\".\"input_tokens\", \"agent_run\".\"output_tokens\", \"agent_run\".\"total_tokens\", \"agent_run\".\"created_at\", \"agent_run\".\"assigned_at\", \"agent_run\".\"started_at\", \"agent_run\".\"ended_at\" FROM \"agent_run\" WHERE (\"agent_run\".\"executor_kind\" IN ('local_runner', 'managed_runner') AND \"agent_run\".\"pinned_runner_id\" IS NULL AND \"agent_run\".\"pod_id\" = $1 AND \"agent_run\".\"status\" = 'queued') ORDER BY \"agent_run\".\"created_at\" ASC LIMIT 1 FOR UPDATE SKIP LOCKED";

/// The `next_for_runner` eligibility arms (`:169-178`): what a runner
/// may take, as one `Condition`. Own-agent runners take `local_runner`
/// runs pinned to them plus (as a `pod_member`) the unpinned pod queue;
/// engines take legacy `managed_runner` runs pinned to them always,
/// plus pins and the pod queue while they take pod work
/// (`pod_member` and unpaused). The old `desktop_bundled` split is gone.
fn eligible_condition(runner_id: Uuid, facts: &DispatchFacts) -> Condition {
    let local = AgentExecutorKind::LocalRunner.value();
    let managed = AgentExecutorKind::ManagedRunner.value();
    let pinned = col("agent_run", "pinned_runner_id");
    if facts.is_engine {
        let managed_arm = Condition::all()
            .add(col("agent_run", "executor_kind").eq(managed))
            .add(pinned.clone().eq(runner_id));
        // §23.1: an engine, pinned_only or not, always takes its
        // dedicated pins — pod_dispatch does not gate them.
        let dedicated_arm = Condition::all()
            .add(col("agent_run", "executor_kind").eq(local))
            .add(pinned.clone().eq(runner_id))
            .add(col("agent_run", "pin_kind").eq("dedicated"));
        if facts.pod_member && !facts.paused {
            let local_arm = Condition::all()
                .add(col("agent_run", "executor_kind").eq(local))
                .add(
                    Condition::any()
                        .add(pinned.clone().eq(runner_id))
                        .add(pinned.is_null()),
                );
            return Condition::any()
                .add(managed_arm)
                .add(dedicated_arm)
                .add(local_arm);
        }
        return Condition::any().add(managed_arm).add(dedicated_arm);
    }
    let mine_or_pod = if facts.pod_member {
        Condition::any()
            .add(pinned.clone().eq(runner_id))
            .add(pinned.is_null())
    } else {
        Condition::all().add(pinned.eq(runner_id))
    };
    Condition::all()
        .add(col("agent_run", "executor_kind").eq(local))
        .add(mine_or_pod)
}

/// The `_is_mine` rank (`matcher.py:181-192`): today pinned-to-me is
/// 0 and shared is 1. Under §10.2 strict 1:1 alternation — after a
/// dedicated run — locality pins stay 0, dedicated pins demote to 2,
/// shared stays 1. Locality pins and `pod_once` runs count as shared
/// (a `pod_once` run has no pin, so it lands in the default arm).
fn rank_expr(runner_id: Uuid, last_dedicated: bool) -> SimpleExpr {
    let pinned = col("agent_run", "pinned_runner_id");
    if !last_dedicated {
        return Expr::case(pinned.eq(runner_id), 0).finally(1).into();
    }
    Expr::case(
        Condition::all()
            .add(pinned.clone().eq(runner_id))
            .add(col("agent_run", "pin_kind").eq("locality")),
        0,
    )
    .case(
        Condition::all()
            .add(pinned.eq(runner_id))
            .add(col("agent_run", "pin_kind").eq("dedicated")),
        2,
    )
    .finally(1)
    .into()
}

/// `next_for_runner` (`:147-192`): the next `QUEUED` run this runner
/// should take — personal queue first, then the pod general queue,
/// FIFO inside each tier.
///
/// Returns `None` for a non-private runner: the usability filter
/// answers `qs.none()`, whose `.first()` returns `None` without
/// issuing a query. The executor runs no SQL in that case.
///
/// Otherwise the full `SELECT … FOR UPDATE SKIP LOCKED` statement:
/// the 39-column run projection plus the two `EXISTS` annotations the
/// usability filter contributes, the pin-rank `CASE`
/// (`pinned_runner_id = runner THEN 0 ELSE 1`), and the eligibility
/// arms in [`eligible_condition`]. The usability `OR` is
/// [`runs_usable_by_runner_predicate`]
/// (D-15 owns the filter; this module only calls it).
pub fn next_for_runner_stmt(
    visibility: i32,
    pod_id: Uuid,
    runner_id: Uuid,
    owner_id: Uuid,
    facts: &DispatchFacts,
) -> Option<SelectStatement> {
    let usability = runs_usable_by_runner_predicate(
        visibility,
        owner_id,
        "agent_run",
        facts.is_engine,
        facts.team_runs,
    )?;
    let mut query = Query::select().to_owned();
    for column in AGENT_RUN_COLUMNS {
        query.column((Alias::new("agent_run"), *column));
    }
    query
        .expr_as(
            runner_visible_issue_exists(owner_id, "agent_run"),
            Alias::new("_runner_visible_issue"),
        )
        .expr_as(
            runner_visible_scheduler_exists(owner_id, "agent_run"),
            Alias::new("_runner_visible_scheduler"),
        )
        .expr_as(
            rank_expr(runner_id, facts.last_dedicated),
            Alias::new("_is_mine"),
        )
        .from(Alias::new("agent_run"))
        .and_where(col("agent_run", "executor_kind").is_in([
            AgentExecutorKind::LocalRunner.value(),
            AgentExecutorKind::ManagedRunner.value(),
        ]))
        .and_where(col("agent_run", "pod_id").eq(pod_id))
        .and_where(col("agent_run", "status").eq(AgentRunStatus::Queued.value()))
        .cond_where(eligible_condition(runner_id, facts))
        .cond_where(usability)
        .order_by(Alias::new("_is_mine"), Order::Asc)
        .order_by(("agent_run", "created_at"), Order::Asc)
        .limit(1u64)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    Some(query)
}

// ---------------------------------------------------------------------------
// By-id lookups (`matcher.py:242-251`, `:298-304`)
// ---------------------------------------------------------------------------
//
// The lookups run through SeaORM entity finds in the executors (`pod`
// with the `deleted_at IS NULL` manager scope and
// `(-is_default, created_at)` ordering; plain runner by id). The pod
// projection is pinned against the fixture's pod SELECT in `by_id`
// test below.

// ---------------------------------------------------------------------------
// Assignment plan + post-commit effect
// ---------------------------------------------------------------------------

/// Caller-fetched facts for one assignment: the locked run row's
/// payload fields plus the assignee's ids. `owner_id` is the
/// runner's owner (`run.owner_id = runner.owner_id`, `:229`).
pub struct AssignmentFacts {
    /// The locked run id (the assignment write's target).
    pub run_id: Uuid,
    /// The assignee runner id.
    pub runner_id: Uuid,
    /// The runner's owner id.
    pub owner_id: Uuid,
    /// `run.work_item_id` for the assign frame (`None` renders null).
    pub work_item_id: Option<Uuid>,
    /// `run.prompt` for the assign frame.
    pub prompt: String,
    /// `run.run_config` for the assign frame (`dict.get` lookups).
    pub run_config: Map<String, Value>,
}

/// One committed assignment: the ordered `SET` plan plus the
/// post-commit dispatch. The executor writes the assignment inside
/// its transaction, commits, then queues
/// [`DrainEffect::SendAssign`] via `pidash_db::tx`.
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
    use crate::runner_runs::guards::runs_usable_by_runner_predicate;
    use crate::runner_sessions::guards::{BUSY_STATUSES, RUNNER_COLUMNS};
    use chrono::TimeZone;
    use pidash_auth::permissions::runner::VISIBILITY_PRIVATE;
    use pidash_types::dispatch::MACHINE_EXECUTORS;
    use sea_query::PostgresQueryBuilder;
    use serde_json::Value;

    static FX07: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-07-matcher.json");

    fn fx07() -> Value {
        serde_json::from_str(FX07).expect("FX-RSES-07 parses")
    }

    const POD_ID: &str = "5bd71e42-032f-47b7-987a-c407e6465809";
    const RUNNER_ID: &str = "c1b285f2-b1b2-4fd2-8fff-0a18bd04b83d";
    const OWNER_ID: &str = "8a4219aa-7cfa-475b-92a2-8fcc6029008a";

    fn render(stmt: &SelectStatement) -> String {
        stmt.to_string(PostgresQueryBuilder)
    }

    fn threshold() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 6, 7, 36, 52).unwrap()
    }

    fn pod() -> Uuid {
        Uuid::parse_str(POD_ID).unwrap()
    }

    fn runner() -> Uuid {
        Uuid::parse_str(RUNNER_ID).unwrap()
    }

    fn owner() -> Uuid {
        Uuid::parse_str(OWNER_ID).unwrap()
    }

    /// The `SELECT` projection of a rendered statement, as columns.
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

    /// The `SELECT` projection of a run statement: the 39 run columns
    /// come first, then the three annotations.
    fn run_projection(sql: &str) -> (Vec<&str>, Vec<&str>) {
        let select = sql
            .strip_prefix("SELECT ")
            .expect("SELECT prefix")
            .split_once(" FROM \"agent_run\"")
            .expect("FROM agent_run")
            .0;
        let raw: Vec<&str> = select.split(", ").collect();
        let cols: Vec<&str> = raw
            .iter()
            .filter_map(|c| {
                c.strip_prefix("\"agent_run\".\"")
                    .map(|rest| rest[..rest.len() - 1].trim_end_matches('"'))
            })
            .collect();
        let rest = raw[cols.len()..].to_vec();
        (cols, rest)
    }

    fn runner_stmt_sql(stmt: &SelectStatement) -> String {
        render(stmt)
    }

    // -- select_runner_in_pod --------------------------------------------

    #[test]
    fn select_runner_in_pod_stmt_matches_fixture() {
        let sql = runner_stmt_sql(&select_runner_in_pod_stmt(threshold(), pod()));
        let fx = fx07();
        // Oracle outcome: the pod-member engine is freshest and picked
        // (PO-4: engines rank equally — the `desktop_bundled`
        // exclusion is gone).
        assert_eq!(
            fx["select_runner_in_pod"]["picked"].as_str(),
            Some("eng-member")
        );
        assert_eq!(
            fx["select_runner_in_pod"]["all_busy_picked"],
            Value::Null,
            "no eligible runner answers None"
        );
        assert_eq!(runner_projection(&sql), RUNNER_COLUMNS);
        // The fixture's Django WHERE terms survive in our statement,
        // with params bound: pod id, pinned-only exclusion,
        // engine-pause IN-subquery, busy NOT EXISTS.
        for term in [
            "last_heartbeat_at",
            "\"paused_at\" IS NULL",
            "\"status\" = 'online'",
            "IN (SELECT \"id\" FROM \"dev_machine\" AS \"U0\"",
            "engine_paused_by_owner",
            "engine_auto_pause_reason",
            "pod_dispatch",
            "pinned_only",
            "NOT EXISTS",
        ] {
            assert!(sql.contains(term), "term present: {term}");
        }
        assert!(sql.contains(POD_ID), "pod bound");
        assert!(
            sql.contains("provisioning"),
            "projection carries provisioning"
        );
        assert!(sql.ends_with(
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
            runner_stmt_sql(&select_runner_in_pod_stmt(threshold(), pod())),
            runner_stmt_sql(&drain_pod_idle_list_stmt(threshold(), pod())),
            runner_stmt_sql(&drain_for_runner_lock_stmt(threshold(), runner())),
        ] {
            assert!(
                sql.contains(&format!("\"status\" IN ({list})")),
                "NOT EXISTS arm pins BUSY_STATUSES order"
            );
        }
    }

    #[test]
    fn maker_base_excludes_neither_pod_work_nor_paused_handling() {
        // drain_pod idle: assignable() arms but NO pod_dispatch term.
        let idle = runner_stmt_sql(&drain_pod_idle_list_stmt(threshold(), pod()));
        let where_only = idle
            .split_once(" WHERE ")
            .expect("WHERE")
            .1
            .split_once(" ORDER BY ")
            .expect("ORDER BY")
            .0;
        assert!(
            !where_only.contains("pod_dispatch"),
            "idle list carries no pod_dispatch exclusion (ported as written)"
        );
        assert!(
            idle.ends_with("ORDER BY \"runner\".\"last_heartbeat_at\" DESC FOR UPDATE SKIP LOCKED")
        );
        assert_eq!(idle.matches("LIMIT").count(), 1, "only the EXISTS LIMIT");
        // select_runner_in_pod DOES carry it.
        let one = runner_stmt_sql(&select_runner_in_pod_stmt(threshold(), pod()));
        let one_where = one
            .split_once(" WHERE ")
            .expect("WHERE")
            .1
            .split_once(" ORDER BY ")
            .expect("ORDER BY")
            .0;
        assert!(
            one_where.contains("pod_dispatch"),
            "select excludes pinned_only"
        );
    }

    // -- drain_for_runner lock --------------------------------------------

    #[test]
    fn drain_for_runner_lock_stmt_matches_fixture() {
        let fx = fx07();
        assert_eq!(fx["drain_for_runner"]["assigned"], true);
        let sql = runner_stmt_sql(&drain_for_runner_lock_stmt(threshold(), runner()));
        assert_eq!(runner_projection(&sql), RUNNER_COLUMNS);
        assert!(sql.ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED"
        ));
        assert!(sql.contains("dev_machine_id"));
        assert!(sql.contains("engine_paused_by_owner"));
    }

    // -- next_queued_run_for_pod ------------------------------------------

    #[test]
    fn next_queued_run_for_pod_sql_matches_fixture() {
        let fx = fx07();
        // Oracle: the oldest unpinned queued run (q1) is next.
        assert_eq!(
            fx["next_queued_run_for_pod"]["picked_prompt"].as_str(),
            Some("p-q1")
        );
        // The Django text binds the fixture pod id where our const
        // binds `$1` (unchanged by #2903).
        let django = fx["next_queued_run_for_pod"]["sql"]
            .as_array()
            .expect("sql")
            .iter()
            .find_map(|entry| {
                let text = entry.as_str().expect("sql");
                text.contains("FROM \"agent_run\"").then_some(text)
            })
            .expect("run SELECT");
        let django = django.replace("5bd71e42032f47b7987ac407e6465809", POD_ID);
        let django = django.replace(&format!("'{POD_ID}'::uuid"), "$1");
        assert_eq!(django.trim_end(), NEXT_QUEUED_RUN_FOR_POD_SQL);
        let projected: Vec<&str> = NEXT_QUEUED_RUN_FOR_POD_SQL
            .strip_prefix("SELECT ")
            .expect("SELECT")
            .split_once(" FROM \"agent_run\"")
            .expect("FROM")
            .0
            .split(", ")
            .map(|c| {
                c.strip_prefix("\"agent_run\".\"")
                    .expect("qualified")
                    .strip_suffix('"')
                    .expect("quoted")
            })
            .collect();
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
    }

    // -- next_for_runner --------------------------------------------------

    fn manual_facts() -> DispatchFacts {
        DispatchFacts {
            is_engine: false,
            pod_member: true,
            paused: false,
            team_runs: true,
            last_dedicated: false,
        }
    }

    fn engine_facts(pod_member: bool, paused: bool) -> DispatchFacts {
        DispatchFacts {
            is_engine: true,
            pod_member,
            paused,
            team_runs: false,
            last_dedicated: false,
        }
    }

    #[test]
    fn next_for_runner_manual_matches_fixture() {
        let sql = render(
            &next_for_runner_stmt(
                VISIBILITY_PRIVATE,
                pod(),
                runner(),
                owner(),
                &manual_facts(),
            )
            .expect("manual arm"),
        );
        assert_eq!(
            fx07()["next_for_runner"]["manual_idle"]["picked_prompt"].as_str(),
            Some("p-q1")
        );
        let (cols, rest) = run_projection(&sql);
        assert_eq!(cols, AGENT_RUN_COLUMNS);
        assert_eq!(cols.len(), 40);
        assert_eq!(rest.len(), 3);
        assert!(sql.contains("_is_mine"));
        assert!(sql.ends_with(
            "ORDER BY \"_is_mine\" ASC, \"agent_run\".\"created_at\" ASC LIMIT 1 FOR UPDATE SKIP LOCKED"
        ));
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'local_runner'"));
        assert!(
            !sql.contains("managed_runner")
                || sql.contains("executor_kind\" IN ('local_runner', 'managed_runner')")
        );
        // No trigger gate for own-agent runners.
        assert!(!sql.contains("NOT IN"));
    }

    #[test]
    fn next_for_runner_engine_member_matches_fixture() {
        let sql = render(
            &next_for_runner_stmt(
                VISIBILITY_PRIVATE,
                pod(),
                runner(),
                owner(),
                &engine_facts(true, false),
            )
            .expect("engine arm"),
        );
        assert_eq!(
            fx07()["next_for_runner"]["engine_member"]["picked_prompt"].as_str(),
            Some("p-q-managed")
        );
        // Managed pins AND the pod-member local arm.
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'managed_runner'"));
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'local_runner'"));
        // Narrow PO-3 usability: no owner_id arm in the predicate, trigger gate present.
        // (The 39-column projection always names owner_id; check WHERE only.)
        let where_clause = sql.split(" WHERE ").nth(1).expect("WHERE present");
        assert!(!where_clause.contains("\"agent_run\".\"owner_id\""));
        assert!(sql.contains("NOT IN"));
    }

    #[test]
    fn next_for_runner_engine_pinned_only_matches_fixture() {
        let sql = render(
            &next_for_runner_stmt(
                VISIBILITY_PRIVATE,
                pod(),
                runner(),
                owner(),
                &engine_facts(false, false),
            )
            .expect("pinned engine"),
        );
        assert_eq!(
            fx07()["next_for_runner"]["engine_pinned_only"]["picked_prompt"].as_str(),
            Some("p-q-managed-pinned")
        );
        // §23.1: managed pins plus dedicated pins (a pinned_only
        // engine still takes its dedicated runs); the shared local
        // arm stays out.
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'managed_runner'"));
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'local_runner'"));
        assert!(sql.contains("\"agent_run\".\"pin_kind\" = 'dedicated'"));
    }

    #[test]
    fn next_for_runner_engine_paused_matches_fixture() {
        let sql = render(
            &next_for_runner_stmt(
                VISIBILITY_PRIVATE,
                pod(),
                runner(),
                owner(),
                &engine_facts(true, true),
            )
            .expect("paused engine"),
        );
        assert_eq!(
            fx07()["next_for_runner"]["engine_paused"]["picked_prompt"].as_str(),
            None
        );
        // §23.1: a paused engine still serves its dedicated pins;
        // the shared local arm stays out.
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'managed_runner'"));
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'local_runner'"));
        assert!(sql.contains("\"agent_run\".\"pin_kind\" = 'dedicated'"));
    }

    #[test]
    fn next_for_runner_alternation_rank() {
        // §10.2 strict 1:1: after a dedicated run, dedicated pins
        // demote to 2, locality stays 0, shared stays 1.
        let mut facts = manual_facts();
        facts.last_dedicated = true;
        let sql = render(
            &next_for_runner_stmt(VISIBILITY_PRIVATE, pod(), runner(), owner(), &facts)
                .expect("alternation"),
        );
        assert!(sql.contains("\"pin_kind\" = 'locality'"));
        assert!(sql.contains("\"pin_kind\" = 'dedicated'"));
        // and without the alternation the CASE is the today rank
        // (no pin_kind arm — the column still projects).
        let sql = render(
            &next_for_runner_stmt(
                VISIBILITY_PRIVATE,
                pod(),
                runner(),
                owner(),
                &manual_facts(),
            )
            .expect("today rank"),
        );
        assert!(!sql.contains("\"pin_kind\" = 'locality'"));
        assert!(!sql.contains("\"pin_kind\" = 'dedicated'"));
    }

    #[test]
    fn next_for_runner_team_runs_restores_ordinary_rule() {
        let team = DispatchFacts {
            is_engine: true,
            pod_member: true,
            paused: false,
            team_runs: true,
            last_dedicated: false,
        };
        let sql = render(
            &next_for_runner_stmt(VISIBILITY_PRIVATE, pod(), runner(), owner(), &team)
                .expect("team engine"),
        );
        assert!(sql.contains("\"agent_run\".\"owner_id\""));
        assert!(!sql.contains("NOT IN"));
        assert!(sql.contains("\"agent_run\".\"executor_kind\" = 'local_runner'"));
    }

    #[test]
    fn next_for_runner_non_private_selects_nothing() {
        assert!(next_for_runner_stmt(99, pod(), runner(), owner(), &manual_facts()).is_none());
        assert!(
            next_for_runner_stmt(1, pod(), runner(), owner(), &engine_facts(true, false)).is_none()
        );
    }

    #[test]
    fn usability_arms_match_d15_predicate() {
        let sql = render(
            &next_for_runner_stmt(
                VISIBILITY_PRIVATE,
                pod(),
                runner(),
                owner(),
                &manual_facts(),
            )
            .expect("manual"),
        );
        let cond =
            runs_usable_by_runner_predicate(VISIBILITY_PRIVATE, owner(), "agent_run", false, true)
                .expect("private");
        let predicate = Query::select()
            .from(Alias::new("agent_run"))
            .cond_where(cond)
            .to_owned()
            .to_string(PostgresQueryBuilder);
        // Every arm of the D-15 predicate appears in the statement.
        for arm in [
            "\"agent_run\".\"created_by_id\"",
            "\"agent_run\".\"owner_id\"",
            "FROM \"issues\" AS \"U0\"",
            "FROM \"scheduler_bindings\" AS \"U0\"",
        ] {
            assert!(sql.contains(arm), "arm present: {arm}");
            assert!(
                predicate.contains(arm.trim_start_matches('"').trim_end_matches('"'))
                    || predicate.contains(arm),
                "predicate carries arm: {arm}"
            );
        }
        // The SELECT annotations carry the EXISTS arms.
        assert!(sql.contains("AS \"_runner_visible_issue\""));
        assert!(sql.contains("AS \"_runner_visible_scheduler\""));
    }

    // -- drain_pod ----------------------------------------------------------

    #[test]
    fn drain_pod_trace_replays() {
        let trace = &fx07()["drain_pod"];
        assert_eq!(trace["assigned"], 5);
        // Every runner picked exactly one run in loop order:
        // eng-member (managed pin), idle1 (q1), shiny (q2),
        // idle2 (q-pin), eng-pinned (managed pinned-only pin).
        let prompts: Vec<&str> = trace["sent_frames"]
            .as_array()
            .expect("frames")
            .iter()
            .map(|f| f["prompt"].as_str().expect("prompt"))
            .collect();
        assert_eq!(
            prompts,
            [
                "p-q-managed",
                "p-q1",
                "p-q2",
                "p-q-pin",
                "p-q-managed-pinned"
            ]
        );
        assert_eq!(trace["q1_after_status"], "assigned");
        assert_eq!(trace["q2_after_status"], "assigned");
        assert_eq!(trace["q_pin_after_status"], "assigned");
        assert_eq!(trace["q_managed_after_status"], "assigned");
        assert_eq!(trace["q_managed_pinned_after_status"], "assigned");
        // Stranger work no pod member may take (usage rule).
        assert_eq!(trace["q_stranger_human_after_status"], "queued");
        assert_eq!(trace["q_stranger_auto_after_status"], "queued");
    }

    #[test]
    fn assign_frames_match_redis_payloads() {
        let frames = fx07()["drain_pod"]["sent_frames"]
            .as_array()
            .expect("frames")
            .clone();
        assert_eq!(frames.len(), 5);
        for frame in &frames {
            let rebuilt: Map<String, Value> = frame
                .as_object()
                .expect("frame object")
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
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
        }
    }

    // -- drain_for_runner ---------------------------------------------------

    #[test]
    fn drain_for_runner_trace_replays() {
        let trace = &fx07()["drain_for_runner"];
        assert_eq!(trace["assigned"], true);
        assert_eq!(trace["q_pin_after_status"], "assigned");
        assert_eq!(trace["sent_frames"].as_array().expect("frames").len(), 1);
        let frame = &trace["sent_frames"][0];
        assert_eq!(frame["prompt"].as_str(), Some("p-q-pin"));
        assert_eq!(frame["v"].as_i64(), Some(1));
        assert_eq!(frame["type"].as_str(), Some("assign"));
    }

    #[test]
    fn sessionless_keeps_assigned_row_and_raises() {
        let trace = &fx07()["drain_for_runner_sessionless"];
        assert_eq!(trace["assigned_return"], true);
        assert_eq!(trace["q1_after_status"], "assigned");
        assert_eq!(
            trace["raised"].as_str(),
            Some("RunnerOfflineError: fake offline")
        );
    }

    // -- by-id lookups ------------------------------------------------------

    #[test]
    fn by_id_pod_projection_pins_fixture() {
        // Pod projection pins the Django pod SELECT shape (the by-id
        // lookups run through SeaORM entity finds in the executors).
        let fx = fx07();
        let pod_select = fx["drain_pod"]["sql"]
            .as_array()
            .expect("sql")
            .iter()
            .find_map(|entry| {
                let text = entry.as_str().expect("sql");
                text.contains("FROM \"pod\"").then_some(text)
            })
            .expect("pod SELECT");
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
        // Missing outcomes: False / 0, no transaction.
        assert_eq!(fx07()["drain_for_runner_by_id_missing"], false);
        assert_eq!(fx07()["drain_pod_by_id_missing"], 0);
    }

    // -- assignment plan ----------------------------------------------------

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
        // The frame matches the drained assign shape.
        let frame = &fx07()["drain_pod"]["sent_frames"][0];
        assert_eq!(
            plan.message["run_id"],
            Value::String(facts.run_id.to_string())
        );
        assert_eq!(frame["v"].as_i64(), Some(1));
        assert_eq!(
            plan.after_commit,
            DrainEffect::SendAssign {
                runner_id: facts.runner_id,
                message: plan.message.clone(),
            }
        );
    }

    // -- log lines ----------------------------------------------------------

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
