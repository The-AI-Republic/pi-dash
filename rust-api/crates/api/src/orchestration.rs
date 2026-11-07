#![forbid(unsafe_code)]

//! API-side D-12 creation runtime: factory + post-commit drain (PIDASHCONV-743).
//!
//! The D-12 creation drivers (`orchestration::creation`) run against the
//! pool [`CreationSeam`][pidash_services::orchestration::creation::CreationSeam]
//! and [`FinalizeAgentRunSeam`][pidash_services::orchestration::creation::FinalizeAgentRunSeam]
//! twin that lives in `project_move_handoff` (the api-side twin of
//! `jobs::LiveCreationStore`). The move domain service creates handoff
//! runs inside its own transaction span; its
//! [`crate::domain_hooks::AppMovePostCommitHook`] drains them through the
//! public [`MoveHook`][pidash_jobs::hooks::MoveHook], which calls back
//! into this module for the api-only legs:
//!
//! - [`SpanCreationDeps::from_state`] builds the runtime facts
//!   (executor settings, admission cache, clock) from the request's
//!   [`AppState`].
//! - [`drain_terminal_effects`] runs finalized runs' terminal publish
//!   pairs after the caller's span commits, inline with the ambient
//!   user.
//!
//! The caller drains creation outboxes before its own post-commit
//! work: the creation registers its `on_commit` entries first (the
//! move's dispatch precedes its cancel send and pod drains,
//! `issue_move.py:353-372`).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use sea_query::{
    Alias, Condition, Expr, JoinType, LockBehavior, LockType, Order, Query, SelectStatement,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::redis::client_from_settings;
use pidash_db::runner_sessions::machine_outbox::{self, MachineOutboxError};
use pidash_db::runner_sessions::outbox as runner_outbox;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::runner_sessions::RunnerSession;
use pidash_services::domain::issues_move::MoveDrainError;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_sessions::drain::{
    drain_pod_log, is_desktop_provisioning, plan_assignment, AssignmentFacts, DrainEffect,
};
use pidash_services::runner_sessions::guards::{alive_threshold, BUSY_STATUSES};
use pidash_services::runner_sessions::pubsub::{send_to_runner, PubsubStore};

use crate::state::AppState;
use pidash_services::dispatch::admission::RedisAdmissionCache;

/// The creation runtime facts a span store is built from: executor
/// settings, the admission cache, and the unix clock for the
/// capacity/admission verdicts.
#[derive(Clone)]
pub(crate) struct SpanCreationDeps {
    pub(crate) cloud: CloudAgentSettings,
    pub(crate) managed: ManagedRunnerSettings,
    pub(crate) cache: RedisAdmissionCache,
    pub(crate) now_unix_secs: i64,
    pub(crate) toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    pub(crate) assistant_seam: Arc<dyn AssistantModelSeam>,
}

impl SpanCreationDeps {
    /// Build the runtime facts from the request state (the
    /// `complete_project_move_handoff` position).
    pub(crate) fn from_state(state: &AppState) -> Self {
        let settings = state.settings();
        Self {
            cloud: settings.cloud_agent.clone(),
            managed: settings.managed_runner.clone(),
            cache: RedisAdmissionCache {
                client: client_from_settings(state.settings()),
            },
            now_unix_secs: unix_now_secs(),
            toolsets: state.toolsets().clone(),
            assistant_seam: state.assistant_seam().clone(),
        }
    }
}

/// Drain finalized runs' terminal publish pairs after the caller's
/// span commits: one emit + inline apply per run, each isolated with
/// its failure log (the old `drain_creation_outboxes` terminal leg;
/// the dispatch + deferred legs moved to the public
/// [`MoveHook`][pidash_jobs::hooks::MoveHook], OVL-16).
/// `terminal_created_by` is the ambient crum request user for the
/// inline terminal effects.
pub(crate) async fn drain_terminal_effects(
    pool: &PgPool,
    state: &AppState,
    run_ids: &[Uuid],
    terminal_created_by: Option<Uuid>,
) {
    if run_ids.is_empty() {
        return;
    }
    let mut pairs = Vec::with_capacity(run_ids.len() * 2);
    for id in run_ids {
        pairs.extend(finalize_kernel::plan_publish_effects(*id));
    }
    let ports = crate::runner_runs::LivePorts::new(pool.clone(), state);
    pidash_jobs::orchestration::drain_publish_effects(pairs, |effect| {
        crate::runner_runs::run_endpoints::drain_lifecycle_effects(
            pool,
            &ports,
            vec![effect],
            terminal_created_by,
        )
    })
    .await;
}

/// `int(time.time())` (the `project_move_handoff` `unix_now_secs` position).
fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// A SeaORM failure as the text the old sqlx read raised: the wrapped
/// sqlx error when there is one (so Postgres codes still classify),
/// otherwise its message (cf. `tools_exec::orm_error`).
fn orm_error_text(error: sea_orm::DbErr) -> String {
    match error {
        sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(inner))
        | sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(inner))
        | sea_orm::DbErr::Conn(sea_orm::RuntimeErr::SqlxError(inner)) => inner.to_string(),
        sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout) => {
            sqlx::Error::PoolTimedOut.to_string()
        }
        sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed) => {
            sqlx::Error::PoolClosed.to_string()
        }
        other => other.to_string(),
    }
}

fn outbox_from_orm(error: sea_orm::DbErr) -> OutboxError {
    OutboxError::from(sqlx::Error::Protocol(orm_error_text(error)))
}

fn drain_from_orm(error: sea_orm::DbErr) -> MoveDrainError {
    MoveDrainError(orm_error_text(error))
}

/// The `CLOSE_ACTIVE_SESSIONS_SQL` read (`pubsub.py:95-99`): active
/// sessions for one runner, newest first — same guards, same order,
/// full-row projection, no `LIMIT`.
fn active_sessions_select(
    runner_id: Uuid,
) -> sea_orm::Select<pidash_db::entity::runner_session::Entity> {
    use pidash_db::entity::runner_session;
    runner_session::Entity::find()
        .filter(runner_session::Column::RunnerId.eq(runner_id))
        .filter(runner_session::Column::RevokedAt.is_null())
        .order_by_desc(runner_session::Column::CreatedAt)
}

fn session_from_model(
    model: pidash_db::entity::runner_session::Model,
) -> Result<RunnerSession, OutboxError> {
    Ok(RunnerSession {
        id: model.id,
        runner_id: model.runner_id,
        protocol_version: model.protocol_version,
        created_at: model.created_at.with_timezone(&Utc),
        last_seen_at: model.last_seen_at.map(|at| at.with_timezone(&Utc)),
        revoked_at: model.revoked_at.map(|at| at.with_timezone(&Utc)),
        revoked_reason: model.revoked_reason,
    })
}

/// The `DRAIN_POD_BY_ID_LOOKUP_SQL` read (`matcher.py:247`):
/// `Pod.objects.filter(pk=…).first()` — the `deleted_at IS NULL`
/// manager scope precedes the pk term; unordered `.first()` keeps
/// `Pod.Meta.ordering` (`-is_default`, `created_at`). Narrow id
/// read: only existence is consumed.
fn pod_lookup_select(pod_id: Uuid) -> sea_orm::Select<pidash_db::entity::pod::Entity> {
    use pidash_db::entity::pod;
    pod::Entity::find()
        .filter(pod::Column::Id.eq(pod_id))
        .filter(pod::Column::DeletedAt.is_null())
        .order_by_desc(pod::Column::IsDefault)
        .order_by_asc(pod::Column::CreatedAt)
        .limit(1)
        .select_only()
}

/// The `DRAIN_POD_IDLE_RUNNERS_SQL` read (`matcher.py:211-223`):
/// the full idle set in drain order — heartbeat-fresh, unpaused,
/// pod-scoped, `online`, with no busy `agent_run`. Narrow id read:
/// only the drain loop's four facts are consumed. `FOR UPDATE SKIP
/// LOCKED`: call inside a transaction.
pub fn drain_pod_idle_list_stmt(threshold: DateTime<Utc>, pod_id: Uuid) -> SelectStatement {
    let busy: Vec<&str> = BUSY_STATUSES.iter().map(|status| status.value()).collect();
    let busy_exists = Query::select()
        .expr(Expr::val(1))
        .from_as("agent_run", Alias::new("U1"))
        .and_where(Expr::col(("U1", "status")).is_in(busy))
        .and_where(Expr::col(("U1", "runner_id")).equals(("runner", "id")))
        .limit(1)
        .to_owned();
    let mut query = Query::select();
    query
        .column(("runner", "id"))
        .column(("runner", "owner_id"))
        .column(("runner", "provisioning"))
        .column(("runner", "visibility"))
        .from("runner")
        .and_where(Expr::col(("runner", "last_heartbeat_at")).gte(threshold))
        .and_where(Expr::col(("runner", "paused_at")).is_null())
        .and_where(Expr::col(("runner", "pod_id")).eq(pod_id))
        .and_where(Expr::col(("runner", "status")).eq("online"))
        .and_where(Expr::exists(busy_exists).not())
        .order_by(("runner", "last_heartbeat_at"), Order::Desc)
        .lock_with_tables_behavior(LockType::Update, ["runner"], LockBehavior::SkipLocked);
    query.to_owned()
}

/// `next_for_runner` (`matcher.py:147-192`): the next `QUEUED` run
/// this runner should take — personal queue first, then the pod
/// general queue, FIFO inside each tier. Returns `None` for a
/// non-private runner (the usability filter answers `qs.none()`,
/// `.first()` returns `None` without issuing a query). Narrow id
/// read: only the claim's four facts are consumed. The pin-rank
/// `CASE` orders the tiers; `FOR UPDATE SKIP LOCKED`: call inside
/// a transaction.
pub fn next_for_runner_stmt(
    provisioning: &str,
    visibility: i32,
    pod_id: Uuid,
    runner_id: Uuid,
    owner_id: Uuid,
) -> Option<SelectStatement> {
    use pidash_auth::permissions::runner::VISIBILITY_PRIVATE;
    use pidash_types::dispatch::AgentExecutorKind;
    if visibility != VISIBILITY_PRIVATE {
        return None;
    }
    let issue_visible = Query::select()
        .expr(Expr::val(1))
        .from_as("issues", Alias::new("U0"))
        .join_as(
            JoinType::LeftJoin,
            "issue_assignees",
            Alias::new("U2"),
            Expr::col(("U0", "id")).equals(("U2", "issue_id")),
        )
        .and_where(Expr::col(("U0", "deleted_at")).is_null())
        .and_where(Expr::col(("U0", "id")).equals(("agent_run", "work_item_id")))
        .cond_where(
            Condition::any()
                .add(Expr::col(("U0", "created_by_id")).eq(owner_id))
                .add(
                    Condition::all()
                        .add(Expr::col(("U2", "assignee_id")).eq(owner_id))
                        .add(Expr::col(("U2", "deleted_at")).is_null()),
                ),
        )
        .limit(1)
        .to_owned();
    let scheduler_visible = Query::select()
        .expr(Expr::val(1))
        .from_as("scheduler_bindings", Alias::new("U0"))
        .and_where(Expr::col(("U0", "deleted_at")).is_null())
        .and_where(Expr::col(("U0", "actor_id")).eq(owner_id))
        .and_where(Expr::col(("U0", "id")).equals(("agent_run", "scheduler_binding_id")))
        .limit(1)
        .to_owned();
    let usability = Condition::any()
        .add(Expr::col(("agent_run", "created_by_id")).eq(owner_id))
        .add(Expr::col(("agent_run", "owner_id")).eq(owner_id))
        .add(Expr::exists(issue_visible))
        .add(Expr::exists(scheduler_visible));
    let split = if is_desktop_provisioning(provisioning) {
        Condition::all()
            .add(
                Expr::col(("agent_run", "executor_kind"))
                    .eq(AgentExecutorKind::ManagedRunner.value()),
            )
            .add(Expr::col(("agent_run", "pinned_runner_id")).eq(runner_id))
    } else {
        Condition::all().add(
            Expr::col(("agent_run", "executor_kind")).eq(AgentExecutorKind::LocalRunner.value()),
        )
    };
    let rank_cond = Expr::col(("agent_run", "pinned_runner_id")).eq(runner_id);
    let rank_for_select = Expr::case(rank_cond.clone(), Expr::val(0)).finally(Expr::val(1));
    let rank_for_order = Expr::case(rank_cond, Expr::val(0)).finally(Expr::val(1));
    let mut query = Query::select();
    query
        .column(("agent_run", "id"))
        .column(("agent_run", "work_item_id"))
        .column(("agent_run", "prompt"))
        .column(("agent_run", "run_config"))
        .expr_as(rank_for_select, Alias::new("_is_mine"))
        .from("agent_run")
        .and_where(Expr::col(("agent_run", "executor_kind")).is_in([
            AgentExecutorKind::LocalRunner.value(),
            AgentExecutorKind::ManagedRunner.value(),
        ]))
        .and_where(Expr::col(("agent_run", "pod_id")).eq(pod_id))
        .and_where(Expr::col(("agent_run", "status")).eq("queued"))
        .cond_where(
            Condition::any()
                .add(Expr::col(("agent_run", "pinned_runner_id")).eq(runner_id))
                .add(Expr::col(("agent_run", "pinned_runner_id")).is_null()),
        )
        .cond_where(usability)
        .cond_where(split)
        .order_by_expr(rank_for_order.into(), Order::Asc)
        .order_by(("agent_run", "created_at"), Order::Asc)
        .limit(1)
        .lock_with_tables_behavior(LockType::Update, ["agent_run"], LockBehavior::SkipLocked);
    Some(query.to_owned())
}

/// One idle-list runner row: the drain loop's four facts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IdleRunner {
    id: Uuid,
    owner_id: Uuid,
    provisioning: String,
    visibility: i32,
}

fn idle_runner_from(row: &sea_orm::QueryResult) -> Result<IdleRunner, sea_orm::DbErr> {
    Ok(IdleRunner {
        id: row.try_get::<Uuid>("", "id")?,
        owner_id: row.try_get::<Uuid>("", "owner_id")?,
        provisioning: row.try_get::<String>("", "provisioning")?,
        visibility: i32::from(row.try_get::<i16>("", "visibility")?),
    })
}

/// One claimed run row: the assign frame's four facts.
#[derive(Debug, Clone, PartialEq)]
struct ClaimedRun {
    id: Uuid,
    work_item_id: Option<Uuid>,
    prompt: String,
    run_config: Option<Value>,
}

fn claimed_run_from(row: &sea_orm::QueryResult) -> Result<ClaimedRun, sea_orm::DbErr> {
    Ok(ClaimedRun {
        id: row.try_get::<Uuid>("", "id")?,
        work_item_id: row.try_get::<Option<Uuid>>("", "work_item_id")?,
        prompt: row.try_get::<String>("", "prompt")?,
        run_config: row.try_get::<Option<Value>>("", "run_config")?,
    })
}

// Shared with both move drivers (app + v1): one copy of the twins that
// lived in each move handler file. Failures answer `MoveDrainError`
// (every site was `Denial::ServerError` in both twins); the move chain
// maps them to its `PodDrain` stop.
// ---------------------------------------------------------------------------
// Move post-commit: cancel send + pod drain
// ---------------------------------------------------------------------------

/// The 552 [`PubsubStore`] over the pool + Redis (the delete-cmds
/// shape): same verbs, same outbox helpers, no runner-enroll
/// dependency.
pub(crate) struct MovePubsub<'p> {
    pub(crate) pool: &'p PgPool,
    pub(crate) redis: Option<&'p redis::Client>,
    pub(crate) runner: &'p RunnerSettings,
}

impl PubsubStore for MovePubsub<'_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError> {
        runner_outbox::enqueue_for_runner(self.redis, self.pool, self.runner, runner_id, message)
            .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, MachineOutboxError> {
        machine_outbox::enqueue_for_machine(
            self.redis,
            self.pool,
            self.runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<RunnerSession>, OutboxError> {
        let db = pidash_db::pool::sea_orm_connection(self.pool);
        let rows = active_sessions_select(runner_id)
            .all(&db)
            .await
            .map_err(outbox_from_orm)?;
        rows.into_iter().map(session_from_model).collect()
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        pidash_services::domain::tokens::revoke_runner_session(
            &pidash_db::pool::sea_orm_connection(self.pool),
            session_id,
            Utc::now(),
            reason,
        )
        .await
        .map_err(OutboxError::from)
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
        runner_outbox::clear_session_marker(self.redis, &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError> {
        runner_outbox::publish_session_eviction(
            self.redis,
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
    }
}

/// The `drain_pod_by_id` executor (`runner/services/matcher.py`,
/// through the 552 recipe): miss → 0 with no transaction; else one
/// transaction over the idle list, one `next_for_runner` claim per
/// runner, the `ASSIGN_RUN_UPDATE_SQL` write per claim — then, past
/// the commit, one `assign` send per claim. Effects dispatch in
/// idle-list order; the log line fires only when claims happened.
///
/// A dispatch failure propagates (the drain contract: the offline
/// error must not be swallowed) — rows already committed, mirroring
/// `on_commit` raising past the commit.
pub(crate) async fn drain_pod_by_id(
    pool: &PgPool,
    pubsub: &MovePubsub<'_>,
    pod_id: Uuid,
) -> Result<(), MoveDrainError> {
    use pidash_db::entity::pod;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let pod = pod_lookup_select(pod_id)
        .columns([pod::Column::Id])
        .into_tuple::<Uuid>()
        .one(&db)
        .await
        .map_err(drain_from_orm)?;
    if pod.is_none() {
        return Ok(());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| MoveDrainError(error.to_string()))?;
    let threshold = alive_threshold(Utc::now());
    let runners = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        conn.query_all(DbBackend::Postgres.build(&drain_pod_idle_list_stmt(threshold, pod_id)))
            .await
            .map_err(drain_from_orm)?
            .iter()
            .map(idle_runner_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(drain_from_orm)?
    };
    let mut effects = Vec::new();
    for runner in &runners {
        let runner_id = runner.id;
        let owner_id = runner.owner_id;
        let provisioning = runner.provisioning.clone();
        let visibility = runner.visibility;
        let Some(next_stmt) =
            next_for_runner_stmt(&provisioning, visibility, pod_id, runner_id, owner_id)
        else {
            continue;
        };
        let run = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            let found = conn
                .query_one(DbBackend::Postgres.build(&next_stmt))
                .await
                .map_err(drain_from_orm)?;
            found
                .as_ref()
                .map(claimed_run_from)
                .transpose()
                .map_err(drain_from_orm)?
        };
        let Some(run) = run else {
            continue;
        };
        let run_id = run.id;
        let work_item_id = run.work_item_id;
        let prompt = run.prompt;
        let run_config = run.run_config;
        let run_config = run_config
            .and_then(|config| config.as_object().cloned())
            .ok_or_else(|| {
                MoveDrainError("move pod drain: run_config is not an object".to_owned())
            })?;
        let plan = plan_assignment(&AssignmentFacts {
            run_id,
            runner_id,
            owner_id,
            work_item_id,
            prompt,
            run_config,
        });
        {
            use pidash_db::entity::agent_run;
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            agent_run::Entity::update_many()
                .col_expr(agent_run::Column::OwnerId, Expr::value(Some(owner_id)))
                .col_expr(agent_run::Column::RunnerId, Expr::value(Some(runner_id)))
                .col_expr(
                    agent_run::Column::Status,
                    Expr::value("assigned".to_owned()),
                )
                .col_expr(agent_run::Column::AssignedAt, Expr::value(Utc::now()))
                .filter(agent_run::Column::Id.eq(run_id))
                .exec(&conn)
                .await
                .map(|_| ())
                .map_err(drain_from_orm)?;
        }
        effects.push(plan.after_commit);
    }
    tx.commit()
        .await
        .map_err(|error| MoveDrainError(error.to_string()))?;
    for effect in &effects {
        let DrainEffect::SendAssign { runner_id, message } = effect;
        send_to_runner(pubsub, *runner_id, message)
            .await
            .map_err(|error| MoveDrainError(error.to_string()))?;
    }
    if !effects.is_empty() {
        tracing::info!("{}", drain_pod_log(&pod_id, effects.len()));
    }
    Ok(())
}

#[cfg(test)]
mod ticker_orch_orm_tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn active_sessions_select_pins_guard_and_order() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let sql = active_sessions_select(id)
            .build(DbBackend::Postgres)
            .to_string();
        let lower = sql.to_ascii_lowercase();
        for fragment in ["runner_session", "revoked_at", "runner_id", "created_at"] {
            assert!(lower.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        assert!(lower.contains("desc"), "desc order:\n{sql}");
        assert!(!lower.contains("limit"), "no limit:\n{sql}");
    }

    #[test]
    fn pod_lookup_select_pins_scope_and_ordering() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let sql = pod_lookup_select(id).build(DbBackend::Postgres).to_string();
        let lower = sql.to_ascii_lowercase();
        for fragment in ["pod", "deleted_at", "is_default"] {
            assert!(lower.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        assert!(lower.contains("limit 1"), "limit:\n{sql}");
    }

    #[test]
    fn idle_list_stmt_pins_busy_exists_order_and_skip_locked() {
        let pod = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let threshold = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let stmt = drain_pod_idle_list_stmt(threshold, pod);
        let shape = stmt.to_string(PostgresQueryBuilder);
        let lower = shape.to_ascii_lowercase();
        for fragment in [
            "from \"runner\"",
            "exists",
            "agent_run",
            "last_heartbeat_at",
            "paused_at",
            "pod_id",
            "'online'",
            "order by",
            "desc",
            "skip locked",
        ] {
            assert!(lower.contains(fragment), "missing {fragment} in:\n{shape}");
        }
        // BUSY_STATUSES order pins the IN list.
        let assigned = lower.find("'assigned'").unwrap();
        let running = lower.find("'running'").unwrap();
        assert!(assigned < running, "busy order:\n{shape}");
        let built = DbBackend::Postgres.build(&stmt);
        assert!(built.sql.contains("$1"), "threshold bind:\n{}", built.sql);
        assert!(built.sql.contains("$2"), "pod bind:\n{}", built.sql);
    }

    #[test]
    fn next_for_runner_stmt_splits_and_ranks() {
        let pod = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let runner = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174001").unwrap();
        let owner = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174002").unwrap();
        // Non-private: no query (matches `next_for_runner_sql`).
        assert!(
            next_for_runner_stmt("managed", 1, pod, runner, owner).is_none(),
            "non-private must be None"
        );
        for (provisioning, split_frag) in [
            ("manual", "local_runner"),
            ("desktop_bundled", "managed_runner"),
        ] {
            let stmt = next_for_runner_stmt(provisioning, 0, pod, runner, owner).expect("stmt");
            let shape = stmt.to_string(PostgresQueryBuilder);
            let lower = shape.to_ascii_lowercase();
            for fragment in [
                "from \"agent_run\"",
                "'queued'",
                "exists",
                "issues",
                "scheduler_bindings",
                "case",
                "_is_mine",
                "skip locked",
                "limit 1",
            ] {
                assert!(
                    lower.contains(fragment),
                    "{provisioning} missing {fragment} in:\n{shape}"
                );
            }
            assert!(
                lower.contains(split_frag),
                "{provisioning} split {split_frag}:\n{shape}"
            );
            // Pinned-first rank ordering.
            assert!(lower.contains("asc"), "rank order:\n{shape}");
        }
    }

    #[test]
    fn orm_error_unwraps_sqlx_text() {
        let err = sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(sqlx::Error::RowNotFound));
        assert_eq!(orm_error_text(err), sqlx::Error::RowNotFound.to_string());
    }
}
