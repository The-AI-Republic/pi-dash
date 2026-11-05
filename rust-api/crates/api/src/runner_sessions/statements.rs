//! SeaORM statements for the runner session endpoints.
//!
//! The reads and writes [`super::runner_open`] and [`super::runner_poll`]
//! run, as SeaORM / sea-query builders over the kernels' planned values
//! (`pidash_services::runner_sessions::session_service` keeps the
//! planning; this module only spells the statements). Each builder
//! keeps the Django statement's predicates, ordering, `LIMIT` and lock;
//! the text twins the kernel still exports document the captured SQL.
//!
//! The chat-release builders mirror the SeaORM chat kernel that lands
//! with pi-dash#2953 (`pidash_services::runner_runs::chat`), name for
//! name, so the swap is mechanical once it merges.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, Keyword, SimpleExpr};
use sea_orm::{
    ColumnTrait, Condition, DbErr, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait, Select, SelectGetableTuple, SelectModel, Selector, UpdateMany,
};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::entity::{
    agent_chat_event, agent_chat_message, agent_chat_session, agent_run, agent_run_event, pod,
    projects, runner, runner_live_state,
};
use pidash_db::runner_runs::{AgentChatMessage, AgentChatSession};
use pidash_services::runner_runs::chat::{FailTurnPlan, MessageStatusUpdate, NewEventInputs};
use pidash_services::runner_runs::{SetClause, SetValue};
use pidash_types::runner_runs::{
    AgentChatMessageRole, AgentChatMessageStatus, AgentChatSessionStatus, AgentRunStatus,
};

// ---------------------------------------------------------------------------
// Runner row writes (`apply_hello`, presence, heartbeat)
// ---------------------------------------------------------------------------

/// The hello `UPDATE` (`HELLO_UPDATE_SQL` / the capabilities-less
/// twin): the planned `SET` clauses in order, `Now` as `now`, `WHERE
/// pk`. An unknown column is a kernel drift (`DbErr::Custom`).
pub(super) fn hello_update_statement(
    clauses: &[SetClause],
    runner_id: Uuid,
    now: DateTime<Utc>,
) -> Result<UpdateMany<runner::Entity>, DbErr> {
    let mut update = runner::Entity::update_many();
    for clause in clauses {
        let column: runner::Column = clause
            .column
            .parse()
            .map_err(|_| DbErr::Custom(format!("unknown runner column {}", clause.column)))?;
        update = update.col_expr(column, set_value_expr(&clause.value, now));
    }
    Ok(update.filter(runner::Column::Id.eq(runner_id)))
}

/// A planned `SET` value as an expression: `Now` is the statement's
/// single timestamp, `Null` the SQL keyword, text / JSON bind.
pub(super) fn set_value_expr(value: &SetValue, now: DateTime<Utc>) -> SimpleExpr {
    match value {
        SetValue::Null => SimpleExpr::Keyword(Keyword::Null),
        SetValue::Now => Expr::val(now).into(),
        SetValue::Text(text) => Expr::val(text.clone()).into(),
        SetValue::Json(json) => Expr::val(json.clone()).into(),
    }
}

/// `MARK_RUNNER_OFFLINE_SQL`: `.exclude(status=REVOKED).update(status
/// = 'offline')` — a revoked row stays revoked.
pub(super) fn mark_runner_offline_statement(runner_id: Uuid) -> UpdateMany<runner::Entity> {
    runner::Entity::update_many()
        .col_expr(
            runner::Column::Status,
            Expr::value(pidash_db::runner_enroll::columns::enums::RUNNER_STATUS_OFFLINE),
        )
        .filter(runner::Column::Id.eq(runner_id))
        .filter(
            runner::Column::Status
                .ne(pidash_db::runner_enroll::columns::enums::RUNNER_STATUS_REVOKED),
        )
}

/// The prior-heartbeat snapshot (`HEARTBEAT_SNAPSHOT_SQL`):
/// `select_for_update().values("last_heartbeat_at", "status").get()`
/// — `FOR UPDATE` on the runner row inside the heartbeat transaction.
pub(super) fn heartbeat_snapshot_select(
    runner_id: Uuid,
) -> Selector<SelectGetableTuple<(Option<DateTime<Utc>>, String)>> {
    runner::Entity::find()
        .select_only()
        .columns([runner::Column::LastHeartbeatAt, runner::Column::Status])
        .filter(runner::Column::Id.eq(runner_id))
        .lock_exclusive()
        .into_tuple()
}

/// The heartbeat write (`HEARTBEAT_UPDATE_SQL`): `.update(last_heartbeat_at,
/// status)` in call order.
pub(super) fn heartbeat_update_statement(
    runner_id: Uuid,
    now: DateTime<Utc>,
    status: &str,
) -> UpdateMany<runner::Entity> {
    runner::Entity::update_many()
        .col_expr(runner::Column::LastHeartbeatAt, Expr::value(now))
        .col_expr(runner::Column::Status, Expr::value(status.to_owned()))
        .filter(runner::Column::Id.eq(runner_id))
}

// ---------------------------------------------------------------------------
// Heartbeat reaper scans (`reap_stale_busy_runs`)
// ---------------------------------------------------------------------------

/// The stale core (`stale_where`): `assigned_at < cutoff AND runner_id
/// = runner AND status IN (statuses)`, minus the in-flight run when
/// the daemon reported one.
fn stale_core(
    cutoff: DateTime<Utc>,
    runner_id: Uuid,
    statuses: &[AgentRunStatus],
    in_flight_id: Option<Uuid>,
) -> Select<agent_run::Entity> {
    let mut select = agent_run::Entity::find()
        .select_only()
        .filter(agent_run::Column::AssignedAt.lt(cutoff))
        .filter(agent_run::Column::RunnerId.eq(runner_id))
        .filter(agent_run::Column::Status.is_in(statuses.iter().map(AgentRunStatus::value)));
    if let Some(id) = in_flight_id {
        select = select.filter(agent_run::Column::Id.ne(id));
    }
    select
}

/// Stopped-cancel barrier id scan (`stale_cancel_ids_sql`): the stale
/// core plus the (redundant, kept) `status = 'cancel_requested'`,
/// `ORDER BY created_at DESC`.
pub(super) fn stale_cancel_ids_select(
    cutoff: DateTime<Utc>,
    runner_id: Uuid,
    statuses: &[AgentRunStatus],
    in_flight_id: Option<Uuid>,
) -> Selector<SelectGetableTuple<Uuid>> {
    stale_core(cutoff, runner_id, statuses, in_flight_id)
        .column(agent_run::Column::Id)
        .filter(agent_run::Column::Status.eq(AgentRunStatus::CancelRequested.value()))
        .order_by_desc(agent_run::Column::CreatedAt)
        .into_tuple()
}

/// Stopped-cancel barrier pod-id scan (`stale_cancel_pod_ids_sql`):
/// same `WHERE`, `pod_id` projection.
pub(super) fn stale_cancel_pod_ids_select(
    cutoff: DateTime<Utc>,
    runner_id: Uuid,
    statuses: &[AgentRunStatus],
    in_flight_id: Option<Uuid>,
) -> Selector<SelectGetableTuple<Option<Uuid>>> {
    stale_core(cutoff, runner_id, statuses, in_flight_id)
        .column(agent_run::Column::PodId)
        .filter(agent_run::Column::Status.eq(AgentRunStatus::CancelRequested.value()))
        .order_by_desc(agent_run::Column::CreatedAt)
        .into_tuple()
}

/// Reap candidate scan (`stale_pairs_sql`): `(id, pod_id)` over the
/// stale core, minus the barriered ids.
pub(super) fn stale_pairs_select(
    cutoff: DateTime<Utc>,
    runner_id: Uuid,
    statuses: &[AgentRunStatus],
    in_flight_id: Option<Uuid>,
    excluded_ids: &[Uuid],
) -> Selector<SelectGetableTuple<(Uuid, Option<Uuid>)>> {
    let mut select = stale_core(cutoff, runner_id, statuses, in_flight_id)
        .columns([agent_run::Column::Id, agent_run::Column::PodId]);
    if !excluded_ids.is_empty() {
        select = select.filter(agent_run::Column::Id.is_not_in(excluded_ids.iter().copied()));
    }
    select
        .order_by_desc(agent_run::Column::CreatedAt)
        .into_tuple()
}

/// Stopped-cancel barrier write (`cancel_barrier_update_sql`):
/// `.update(status='cancelled', ended_at=now)` over the ids. Call only
/// with a non-empty list (the source guards the write).
pub(super) fn cancel_barrier_update_statement(
    now: DateTime<Utc>,
    ids: &[Uuid],
) -> UpdateMany<agent_run::Entity> {
    debug_assert!(!ids.is_empty(), "barrier write needs stopped-cancel ids");
    agent_run::Entity::update_many()
        .col_expr(
            agent_run::Column::Status,
            Expr::value(AgentRunStatus::Cancelled.value()),
        )
        .col_expr(agent_run::Column::EndedAt, Expr::value(now))
        .filter(agent_run::Column::Id.is_in(ids.iter().copied()))
}

/// In-flight claim probe (`IN_FLIGHT_CLAIM_SQL`): the reported run's
/// `status` and `error_details` by `(id, runner)`, newest first.
pub(super) fn in_flight_claim_select(
    run_id: Uuid,
    runner_id: Uuid,
) -> Selector<SelectGetableTuple<(String, Value)>> {
    agent_run::Entity::find()
        .select_only()
        .columns([agent_run::Column::Status, agent_run::Column::ErrorDetails])
        .filter(agent_run::Column::Id.eq(run_id))
        .filter(agent_run::Column::RunnerId.eq(runner_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .limit(1)
        .into_tuple()
}

// ---------------------------------------------------------------------------
// Live state (`upsert_runner_live_state`)
// ---------------------------------------------------------------------------

/// The `get_or_create` select (`LIVE_STATE_SELECT_SQL`), narrowed to
/// the one column the plan reads: `Some(observed)` when the row exists.
pub(super) fn live_state_observed_select(
    runner_id: Uuid,
) -> Selector<SelectGetableTuple<Option<Uuid>>> {
    runner_live_state::Entity::find()
        .select_only()
        .column(runner_live_state::Column::ObservedRunId)
        .filter(runner_live_state::Column::RunnerId.eq(runner_id))
        .into_tuple()
}

/// The `get_or_create` insert (`LIVE_STATE_INSERT_SQL`): every
/// snapshot field null, `usage` the `{}` default, `updated_at` now.
pub(super) fn live_state_insert_row(
    runner_id: Uuid,
    now: DateTime<Utc>,
) -> runner_live_state::ActiveModel {
    use sea_orm::Set;
    runner_live_state::ActiveModel {
        runner_id: Set(runner_id),
        observed_run_id: Set(None),
        last_event_at: Set(None),
        last_event_kind: Set(None),
        last_event_summary: Set(None),
        agent_pid: Set(None),
        agent_subprocess_alive: Set(None),
        approvals_pending: Set(None),
        turn_count: Set(None),
        updated_at: Set(now.into()),
        llm_model: Set(None),
        usage: Set(Value::Object(Default::default())),
        agent_health: Set(None),
        agent_last_activity_at: Set(None),
    }
}

/// The live-state `UPDATE` (`live_state_update_sql`): the typed `SET`
/// expressions in plan order, then `updated_at = now`, `WHERE
/// runner_id`.
pub(super) fn live_state_update_statement(
    sets: Vec<(runner_live_state::Column, SimpleExpr)>,
    runner_id: Uuid,
    now: DateTime<Utc>,
) -> UpdateMany<runner_live_state::Entity> {
    let mut update = runner_live_state::Entity::update_many();
    for (column, expr) in sets {
        update = update.col_expr(column, expr);
    }
    update
        .col_expr(runner_live_state::Column::UpdatedAt, Expr::value(now))
        .filter(runner_live_state::Column::RunnerId.eq(runner_id))
}

// ---------------------------------------------------------------------------
// Session-open lookups (slug, resume ack, redeliver)
// ---------------------------------------------------------------------------

/// `runner.pod_id -> pod.id` (trimmed from the entity).
fn runner_pod_rel() -> sea_orm::RelationDef {
    runner::Entity::belongs_to(pod::Entity)
        .from(runner::Column::PodId)
        .to(pod::Column::Id)
        .into()
}

/// `resolve_runner_project_slug` (`resolve_runner_slug_sql`):
/// `select_related("pod", "pod__project")` inner-joins both tables, so
/// a podless runner (or a pod without a project) yields no row; the
/// `Runner` default ordering, first row. Reads the two cells the
/// kernel consumes: `pod_id` and the project `identifier`.
pub(super) fn resolve_runner_slug_select(
    runner_id: Uuid,
) -> Selector<SelectGetableTuple<(Uuid, Option<String>)>> {
    runner::Entity::find()
        .select_only()
        .column(runner::Column::PodId)
        .column_as(projects::Column::Identifier, "identifier")
        .join(JoinType::InnerJoin, runner_pod_rel())
        .join(JoinType::InnerJoin, pod::Relation::Projects.def())
        .filter(runner::Column::Id.eq(runner_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .limit(1)
        .into_tuple()
}

/// `build_resume_ack` lookup (`resume_ack_lookup_sql`): the run by
/// `(id, runner)`, newest first — the `status` and `thread_id` cells
/// the kernel consumes.
pub(super) fn resume_ack_lookup_select(
    run_id: Uuid,
    runner_id: Uuid,
) -> Selector<SelectGetableTuple<(String, String)>> {
    agent_run::Entity::find()
        .select_only()
        .columns([agent_run::Column::Status, agent_run::Column::ThreadId])
        .filter(agent_run::Column::Id.eq(run_id))
        .filter(agent_run::Column::RunnerId.eq(runner_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .limit(1)
        .into_tuple()
}

/// Resume-ack `last_seq` (`RESUME_ACK_LAST_SEQ_SQL`): the newest event
/// seq for the run, `None` without events.
pub(super) fn last_event_seq_select(run_id: Uuid) -> Selector<SelectGetableTuple<i32>> {
    agent_run_event::Entity::find()
        .select_only()
        .column(agent_run_event::Column::Seq)
        .filter(agent_run_event::Column::AgentRunId.eq(run_id))
        .order_by_desc(agent_run_event::Column::Seq)
        .limit(1)
        .into_tuple()
}

/// The redeliver scans' shared tail: the runner's runs, minus the
/// skipped in-flight id, oldest assignment first.
fn redeliver_core(runner_id: Uuid, skip_id: Option<Uuid>) -> Select<agent_run::Entity> {
    let mut select = agent_run::Entity::find()
        .select_only()
        .filter(agent_run::Column::RunnerId.eq(runner_id));
    if let Some(id) = skip_id {
        select = select.filter(agent_run::Column::Id.ne(id));
    }
    select
        .order_by_asc(agent_run::Column::AssignedAt)
        .order_by_asc(agent_run::Column::CreatedAt)
        .limit(1)
}

/// Cancel redeliver scan (`redeliver_cancel_sql`): the oldest
/// `CANCEL_REQUESTED` run on the runner — its id.
pub(super) fn redeliver_cancel_select(
    runner_id: Uuid,
    skip_id: Option<Uuid>,
) -> Selector<SelectGetableTuple<Uuid>> {
    redeliver_core(runner_id, skip_id)
        .column(agent_run::Column::Id)
        .filter(agent_run::Column::Status.eq(AgentRunStatus::CancelRequested.value()))
        .into_tuple()
}

/// The assign redeliver row: the frame fields.
#[derive(Debug, Clone, PartialEq, sea_orm::FromQueryResult)]
pub(super) struct RedeliverAssignRow {
    pub id: Uuid,
    pub work_item_id: Option<Uuid>,
    pub prompt: String,
    pub run_config: Value,
}

/// Assign redeliver scan (`redeliver_assign_sql`): the oldest
/// `ASSIGNED` / `WAITING_FOR_WORKTREE` run on the runner (`RUNNING`
/// runs are the reaper's business).
pub(super) fn redeliver_assign_select(
    runner_id: Uuid,
    skip_id: Option<Uuid>,
) -> Selector<SelectModel<RedeliverAssignRow>> {
    redeliver_core(runner_id, skip_id)
        .columns([
            agent_run::Column::Id,
            agent_run::Column::WorkItemId,
            agent_run::Column::Prompt,
            agent_run::Column::RunConfig,
        ])
        .filter(agent_run::Column::Status.is_in([
            AgentRunStatus::Assigned.value(),
            AgentRunStatus::WaitingForWorktree.value(),
        ]))
        .into_model()
}

// ---------------------------------------------------------------------------
// Chat release (`release_active_chats_for_runner`, `chat.py:451-480`)
// ---------------------------------------------------------------------------

/// The active-turn predicate (`ACTIVE_TURN_PREDICATE_SQL`):
/// `active_message_id IS NOT NULL OR active_turn_id <> ''`.
pub(super) fn active_turn_condition() -> Condition {
    Condition::any()
        .add(agent_chat_session::Column::ActiveMessageId.is_not_null())
        .add(agent_chat_session::Column::ActiveTurnId.ne(""))
}

/// The release id select (`chat.py:451-457`): the runner's OPEN
/// sessions with an active turn, default ordering.
pub(super) fn active_chat_session_ids_select(
    runner_id: Uuid,
) -> Selector<SelectGetableTuple<Uuid>> {
    agent_chat_session::Entity::find()
        .select_only()
        .column(agent_chat_session::Column::Id)
        .filter(agent_chat_session::Column::RunnerId.eq(runner_id))
        .filter(agent_chat_session::Column::Status.eq(AgentChatSessionStatus::Open.value()))
        .filter(active_turn_condition())
        .order_by_desc(agent_chat_session::Column::LastMessageAt)
        .order_by_desc(agent_chat_session::Column::CreatedAt)
        .into_tuple()
}

/// Lock an OPEN session row by pk (release rows `:462`): the full row
/// behind the `status = 'open'` guard, default ordering, `LIMIT 1 FOR
/// UPDATE`.
pub(super) fn session_lock_open_select(session_id: Uuid) -> Select<agent_chat_session::Entity> {
    agent_chat_session::Entity::find()
        .filter(agent_chat_session::Column::Id.eq(session_id))
        .filter(agent_chat_session::Column::Status.eq(AgentChatSessionStatus::Open.value()))
        .order_by_desc(agent_chat_session::Column::LastMessageAt)
        .order_by_desc(agent_chat_session::Column::CreatedAt)
        .limit(1)
        .lock_exclusive()
}

/// The explicit finalize lookup (`chat.py:345`): the target message
/// scoped to the session, default ordering (the session join carries
/// the `-last_message_at, -created_at` half), first row.
pub(super) fn finalize_explicit_lookup_select(
    session_id: Uuid,
    message_id: Uuid,
) -> Select<agent_chat_message::Entity> {
    agent_chat_message::Entity::find()
        .join(
            JoinType::InnerJoin,
            agent_chat_message::Relation::AgentChatSession.def(),
        )
        .filter(agent_chat_message::Column::Id.eq(message_id))
        .filter(agent_chat_message::Column::SessionId.eq(session_id))
        .order_by_desc(agent_chat_session::Column::LastMessageAt)
        .order_by_desc(agent_chat_session::Column::CreatedAt)
        .order_by_asc(agent_chat_message::Column::Seq)
        .limit(1)
}

/// The turn-scoped assistant probe (`chat.py:329`): STREAMING
/// assistants of the session on this `local_turn_id`, newest first.
pub(super) fn assistant_turn_scoped_select(
    session_id: Uuid,
    local_turn_id: &str,
) -> Select<agent_chat_message::Entity> {
    assistant_fallback_select(session_id)
        .filter(agent_chat_message::Column::LocalTurnId.eq(local_turn_id))
}

/// The unscoped fallback probe (`chat.py:332`): the newest STREAMING
/// assistant of the session, whatever its turn.
pub(super) fn assistant_fallback_select(session_id: Uuid) -> Select<agent_chat_message::Entity> {
    agent_chat_message::Entity::find()
        .filter(agent_chat_message::Column::Role.eq(AgentChatMessageRole::Assistant.value()))
        .filter(agent_chat_message::Column::SessionId.eq(session_id))
        .filter(agent_chat_message::Column::Status.eq(AgentChatMessageStatus::Streaming.value()))
        .order_by_desc(agent_chat_message::Column::CreatedAt)
        .limit(1)
}

/// The message status write: `SET status, completed_at WHERE pk`.
pub(super) fn message_status_update_statement(
    update: &MessageStatusUpdate,
) -> UpdateMany<agent_chat_message::Entity> {
    agent_chat_message::Entity::update_many()
        .col_expr(
            agent_chat_message::Column::Status,
            Expr::value(update.status.value()),
        )
        .col_expr(
            agent_chat_message::Column::CompletedAt,
            Expr::value(update.completed_at),
        )
        .filter(agent_chat_message::Column::Id.eq(update.id))
}

/// The fail-flow session write (`save(update_fields)`, `chat.py:471`):
/// clears the active turn, stores the planned error, closes when
/// planned (never on the release path); `updated_at` is `auto_now`.
pub(super) fn fail_session_update_statement(
    session_id: Uuid,
    plan: &FailTurnPlan,
    updated_at: DateTime<Utc>,
) -> UpdateMany<agent_chat_session::Entity> {
    let mut update = agent_chat_session::Entity::update_many();
    if plan.close_session {
        let closed_at = plan.closed_at.expect("closed_at when closing");
        update = update
            .col_expr(
                agent_chat_session::Column::Status,
                Expr::value(AgentChatSessionStatus::Closed.value()),
            )
            .col_expr(agent_chat_session::Column::ClosedAt, Expr::value(closed_at));
    }
    update
        .col_expr(
            agent_chat_session::Column::ActiveTurnId,
            Expr::value(String::new()),
        )
        .col_expr(
            agent_chat_session::Column::ActiveMessageId,
            SimpleExpr::Keyword(Keyword::Null),
        )
        .col_expr(
            agent_chat_session::Column::Error,
            Expr::value(plan.session_error.clone()),
        )
        .col_expr(
            agent_chat_session::Column::UpdatedAt,
            Expr::value(updated_at),
        )
        .filter(agent_chat_session::Column::Id.eq(session_id))
}

/// `next_event_seq_locked`: `MAX(seq)` over the session's events
/// (`None` for an empty set; the caller adds one).
pub(super) fn next_event_seq_select(session_id: Uuid) -> Selector<SelectGetableTuple<Option<i32>>> {
    agent_chat_event::Entity::find()
        .select_only()
        .column_as(
            Expr::col((agent_chat_event::Entity, agent_chat_event::Column::Seq)).max(),
            "seq__max",
        )
        .filter(agent_chat_event::Column::SessionId.eq(session_id))
        .into_tuple()
}

/// The event insert: every concrete column, the `BigAutoField` pk from
/// the database (`last_insert_id`).
pub(super) fn insert_event_statement(
    session_id: Uuid,
    seq: i32,
    inputs: &NewEventInputs,
    created_at: DateTime<Utc>,
) -> sea_orm::Insert<agent_chat_event::ActiveModel> {
    use sea_orm::{NotSet, Set};
    agent_chat_event::Entity::insert(agent_chat_event::ActiveModel {
        id: NotSet,
        seq: Set(seq),
        source_key: Set(inputs.source_key.clone()),
        kind: Set(inputs.kind.clone()),
        payload: Set(inputs.payload.clone()),
        created_at: Set(created_at.into()),
        message_id: Set(inputs.message_id),
        session_id: Set(session_id),
    })
}

/// A locked chat-session row as the L2 struct. An unknown status is
/// `None` (unreachable here — the lock already filters `open`).
pub(super) fn chat_session_from_model(
    model: agent_chat_session::Model,
) -> Option<AgentChatSession> {
    let status = AgentChatSessionStatus::from_value(&model.status)?;
    Some(AgentChatSession {
        id: model.id,
        workspace_id: model.workspace_id,
        runner_id: model.runner_id,
        created_by_id: model.created_by_id,
        pod_id: model.pod_id,
        status,
        agent_kind: model.agent_kind,
        local_thread_id: model.local_thread_id,
        local_session_id: model.local_session_id,
        cwd: model.cwd,
        model: model.model,
        active_turn_id: model.active_turn_id,
        active_message_id: model.active_message_id,
        close_requested: model.close_requested,
        last_message_at: model.last_message_at.map(|at| at.with_timezone(&Utc)),
        closed_at: model.closed_at.map(|at| at.with_timezone(&Utc)),
        error: model.error,
        created_at: model.created_at.with_timezone(&Utc),
        updated_at: model.updated_at.with_timezone(&Utc),
    })
}

/// A chat-message row as the L2 struct. An unknown role or status is
/// `None`.
pub(super) fn chat_message_from_model(
    model: agent_chat_message::Model,
) -> Option<AgentChatMessage> {
    let role = AgentChatMessageRole::from_value(&model.role)?;
    let status = AgentChatMessageStatus::from_value(&model.status)?;
    Some(AgentChatMessage {
        id: model.id,
        session_id: model.session_id,
        role,
        content: model.content,
        content_parts: model.content_parts,
        status,
        local_item_id: model.local_item_id,
        local_turn_id: model.local_turn_id,
        seq: model.seq,
        created_at: model.created_at.with_timezone(&Utc),
        completed_at: model.completed_at.map(|at| at.with_timezone(&Utc)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    fn pg_select<E: EntityTrait>(select: Select<E>) -> String {
        select.build(DbBackend::Postgres).to_string()
    }

    fn pg_selector<S: sea_orm::SelectorTrait>(selector: Selector<S>) -> String {
        selector.into_statement(DbBackend::Postgres).to_string()
    }

    fn pg_update<E: EntityTrait>(update: UpdateMany<E>) -> String {
        update.build(DbBackend::Postgres).to_string()
    }

    fn at() -> DateTime<Utc> {
        "2026-01-02T03:04:05Z".parse().expect("ts")
    }

    #[test]
    fn hello_update_shape() {
        let clauses = [
            SetClause {
                column: "capabilities",
                value: SetValue::Json(serde_json::json!(["a"])),
            },
            SetClause {
                column: "os",
                value: SetValue::Text("linux".to_owned()),
            },
            SetClause {
                column: "last_heartbeat_at",
                value: SetValue::Now,
            },
        ];
        let sql = pg_update(hello_update_statement(&clauses, Uuid::nil(), at()).expect("stmt"));
        assert!(
            sql.starts_with("UPDATE \"runner\" SET \"capabilities\" = "),
            "{sql}"
        );
        assert!(sql.contains("\"os\" = 'linux'"), "{sql}");
        assert!(sql.contains("\"last_heartbeat_at\" = '2026-01-02 03:04:05.000000 +00:00'"));
        assert!(sql.ends_with("WHERE \"runner\".\"id\" = '00000000-0000-0000-0000-000000000000'"));
        assert!(hello_update_statement(
            &[SetClause {
                column: "nope",
                value: SetValue::Now
            }],
            Uuid::nil(),
            at()
        )
        .is_err());
    }

    #[test]
    fn presence_and_heartbeat_shapes() {
        let offline = pg_update(mark_runner_offline_statement(Uuid::nil()));
        assert_eq!(
            offline,
            "UPDATE \"runner\" SET \"status\" = 'offline' WHERE \"runner\".\"id\" = \
             '00000000-0000-0000-0000-000000000000' AND \"runner\".\"status\" <> 'revoked'"
        );
        let snapshot = pg_selector(heartbeat_snapshot_select(Uuid::nil()));
        assert_eq!(
            snapshot,
            "SELECT \"runner\".\"last_heartbeat_at\", \"runner\".\"status\" FROM \"runner\" WHERE \
             \"runner\".\"id\" = '00000000-0000-0000-0000-000000000000' LIMIT 1 FOR UPDATE"
        );
        let update = pg_update(heartbeat_update_statement(Uuid::nil(), at(), "busy"));
        assert!(update.starts_with(
            "UPDATE \"runner\" SET \"last_heartbeat_at\" = '2026-01-02 03:04:05.000000 +00:00', \
             \"status\" = 'busy' WHERE"
        ));
    }

    #[test]
    fn reap_scan_shapes() {
        let statuses = [AgentRunStatus::Running, AgentRunStatus::CancelRequested];
        let ids = pg_selector(stale_cancel_ids_select(at(), Uuid::nil(), &statuses, None));
        assert!(ids.starts_with("SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE"));
        assert!(ids.contains("\"agent_run\".\"assigned_at\" < '2026-01-02 03:04:05.000000 +00:00'"));
        assert!(ids.contains("\"agent_run\".\"status\" IN ('running', 'cancel_requested')"));
        assert!(ids.contains("AND \"agent_run\".\"status\" = 'cancel_requested'"));
        assert!(ids.ends_with("ORDER BY \"agent_run\".\"created_at\" DESC"));
        assert!(!ids.contains("<>"));
        let skip = Uuid::new_v4();
        let pods = pg_selector(stale_cancel_pod_ids_select(
            at(),
            Uuid::nil(),
            &statuses,
            Some(skip),
        ));
        assert!(pods.starts_with("SELECT \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE"));
        assert!(pods.contains(&format!("\"agent_run\".\"id\" <> '{skip}'")));
        let pairs = pg_selector(stale_pairs_select(
            at(),
            Uuid::nil(),
            &statuses,
            None,
            &[skip],
        ));
        assert!(pairs.starts_with(
            "SELECT \"agent_run\".\"id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE"
        ));
        assert!(pairs.contains(&format!("\"agent_run\".\"id\" NOT IN ('{skip}')")));
        assert!(
            !pg_selector(stale_pairs_select(at(), Uuid::nil(), &statuses, None, &[]))
                .contains("NOT IN")
        );
        let barrier = pg_update(cancel_barrier_update_statement(at(), &[skip]));
        assert!(barrier.starts_with(
            "UPDATE \"agent_run\" SET \"status\" = 'cancelled', \"ended_at\" = \
             '2026-01-02 03:04:05.000000 +00:00' WHERE \"agent_run\".\"id\" IN ("
        ));
        let claim = pg_selector(in_flight_claim_select(skip, Uuid::nil()));
        assert!(claim.starts_with(
            "SELECT \"agent_run\".\"status\", \"agent_run\".\"error_details\" FROM \"agent_run\" WHERE"
        ));
        assert!(claim.ends_with("ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1"));
    }

    #[test]
    fn live_state_shapes() {
        let select = pg_selector(live_state_observed_select(Uuid::nil()));
        assert_eq!(
            select,
            "SELECT \"runner_live_state\".\"observed_run_id\" FROM \"runner_live_state\" WHERE \
             \"runner_live_state\".\"runner_id\" = '00000000-0000-0000-0000-000000000000'"
        );
        let insert = runner_live_state::Entity::insert(live_state_insert_row(Uuid::nil(), at()))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(insert.starts_with("INSERT INTO \"runner_live_state\" (\"runner_id\", "));
        assert!(insert.contains("'{}'"), "{insert}");
        let update = pg_update(live_state_update_statement(
            vec![
                (runner_live_state::Column::TurnCount, Expr::val(3i64).into()),
                (
                    runner_live_state::Column::LlmModel,
                    SimpleExpr::Keyword(Keyword::Null),
                ),
            ],
            Uuid::nil(),
            at(),
        ));
        assert_eq!(
            update,
            "UPDATE \"runner_live_state\" SET \"turn_count\" = 3, \"llm_model\" = NULL, \
             \"updated_at\" = '2026-01-02 03:04:05.000000 +00:00' WHERE \
             \"runner_live_state\".\"runner_id\" = '00000000-0000-0000-0000-000000000000'"
        );
    }

    #[test]
    fn open_lookup_shapes() {
        let slug = pg_selector(resolve_runner_slug_select(Uuid::nil()));
        assert!(
            slug.starts_with(
                "SELECT \"runner\".\"pod_id\", \"projects\".\"identifier\" AS \"identifier\" FROM \
             \"runner\" INNER JOIN \"pod\" ON \"runner\".\"pod_id\" = \"pod\".\"id\" INNER JOIN \
             \"projects\" ON \"pod\".\"project_id\" = \"projects\".\"id\" WHERE"
            ),
            "{slug}"
        );
        assert!(slug.ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1"
        ));
        let ack = pg_selector(resume_ack_lookup_select(Uuid::nil(), Uuid::nil()));
        assert!(ack.starts_with(
            "SELECT \"agent_run\".\"status\", \"agent_run\".\"thread_id\" FROM \"agent_run\" WHERE"
        ));
        let seq = pg_selector(last_event_seq_select(Uuid::nil()));
        assert!(seq.ends_with("ORDER BY \"agent_run_event\".\"seq\" DESC LIMIT 1"));
        let cancel = pg_selector(redeliver_cancel_select(Uuid::nil(), None));
        assert!(cancel.contains("\"agent_run\".\"status\" = 'cancel_requested'"));
        assert!(cancel.ends_with(
            "ORDER BY \"agent_run\".\"assigned_at\" ASC, \"agent_run\".\"created_at\" ASC LIMIT 1"
        ));
        assert!(!cancel.contains("<>"));
        let skip = Uuid::new_v4();
        let assign = pg_selector(redeliver_assign_select(Uuid::nil(), Some(skip)));
        assert!(assign.starts_with(
            "SELECT \"agent_run\".\"id\", \"agent_run\".\"work_item_id\", \"agent_run\".\"prompt\", \
             \"agent_run\".\"run_config\" FROM \"agent_run\" WHERE"
        ));
        assert!(assign.contains(&format!("\"agent_run\".\"id\" <> '{skip}'")));
        assert!(assign.contains("\"agent_run\".\"status\" IN ('assigned', 'waiting_for_worktree')"));
    }

    #[test]
    fn chat_release_shapes() {
        let ids = pg_selector(active_chat_session_ids_select(Uuid::nil()));
        assert!(ids
            .starts_with("SELECT \"agent_chat_session\".\"id\" FROM \"agent_chat_session\" WHERE"));
        assert!(ids.contains("\"agent_chat_session\".\"status\" = 'open'"));
        assert!(ids.contains(
            "(\"agent_chat_session\".\"active_message_id\" IS NOT NULL OR \
             \"agent_chat_session\".\"active_turn_id\" <> '')"
        ));
        assert!(ids.ends_with(
            "ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \
             \"agent_chat_session\".\"created_at\" DESC"
        ));
        let lock = pg_select(session_lock_open_select(Uuid::nil()));
        assert!(lock.ends_with(
            "ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \
             \"agent_chat_session\".\"created_at\" DESC LIMIT 1 FOR UPDATE"
        ));
        let explicit = pg_select(finalize_explicit_lookup_select(Uuid::nil(), Uuid::nil()));
        assert!(
            explicit.contains(
                "INNER JOIN \"agent_chat_session\" ON \"agent_chat_message\".\"session_id\" = \
             \"agent_chat_session\".\"id\""
            ),
            "{explicit}"
        );
        assert!(explicit.ends_with(
            "ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \
             \"agent_chat_session\".\"created_at\" DESC, \"agent_chat_message\".\"seq\" ASC LIMIT 1"
        ));
        let scoped = pg_select(assistant_turn_scoped_select(Uuid::nil(), "t1"));
        assert!(scoped.contains("\"agent_chat_message\".\"role\" = 'assistant'"));
        assert!(scoped.contains("\"agent_chat_message\".\"status\" = 'streaming'"));
        assert!(scoped.contains("\"agent_chat_message\".\"local_turn_id\" = 't1'"));
        assert!(scoped.ends_with("ORDER BY \"agent_chat_message\".\"created_at\" DESC LIMIT 1"));
        assert!(!pg_select(assistant_fallback_select(Uuid::nil())).contains("local_turn_id"));
        let seq = pg_selector(next_event_seq_select(Uuid::nil()));
        assert!(seq.starts_with(
            "SELECT MAX(\"agent_chat_event\".\"seq\") AS \"seq__max\" FROM \"agent_chat_event\" WHERE"
        ), "{seq}");
    }
}
