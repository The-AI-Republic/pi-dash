#![forbid(unsafe_code)]

//! Cloud-side runner/machine delete service (services-C, PIDASHCONV-588).
//!
//! Port of `delete_runner` + `delete_dev_machine`
//! (`runner/services/runner_delete.py:47-143`): frame-before-revoke
//! order, the `runner_removed` vs `user_revoke` reason rule, the atomic
//! revoke + close + hard-delete tail, and the machine variant's token
//! revoke + locked-runner select + enqueue-all-before-revoke order.
//! (`parse_purge_local`, same file, already lives in [`super::purge`].)
//!
//! # Shape: store seam, not inline SQL
//!
//! Like [`super::revoke`], one async method per statement behind
//! [`RunnerDeleteStore`] (a [`RevokeStore`] supertrait, so the nested
//! revoke runs flat on the same store — the
//! 586 precedent: the nested `atomic` is a savepoint with no
//! partial-rollback path, hence unobservable). The drivers keep the full
//! orchestration: frame order, reason rule, the Python-side active
//! filter, and the delete-collector statement sequences below.
//!
//! # The hard-delete tail (Django 4.2 collector, probe-verified)
//!
//! `QuerySet.delete()` is not one `DELETE`: the collector fetches the
//! root rows, collects related rows, then issues fast deletes, field
//! updates, and instance deletes. No `pre_delete`/`post_delete`
//! receivers exist for any model in this graph (repo-wide grep), so no
//! signal effects are ported. Conditional structure, in order:
//!
//! Runner collector (`Runner.objects.filter(...).delete()`):
//!
//! 1. root fetch (full row, no `ORDER BY`); empty → nothing further;
//! 2. chat-id fetch (always); message-id fetch iff chats found
//!    (child order appends after the parent ordering, hence the join);
//! 3. fast deletes: chat event/approval/dedupe (iff chats found — the
//!    collector groups fast deletes per model *after* the relation
//!    loop, so the chat subtree precedes the runner level), then
//!    runner session/force-refresh/live-state (unconditional);
//! 4. `SET_NULL` updates: `agent_run.runner`/`pinned_runner`
//!    (unconditional — registered lazily, no `SELECT`), then
//!    `agent_chat_event.message_id` (iff messages found);
//! 5. instance deletes, deepest first: messages, chats, then the
//!    runner rows (`DELETE ... WHERE id IN (...)`).
//!
//! Machine collector (`DevMachine.objects.filter(pk).delete()`):
//!
//! 1. root fetch; empty → nothing further;
//! 2. `machine_session` fast delete (unconditional);
//! 3. `SET_NULL` updates: `runner.dev_machine_id`, then
//!    `machine_token.dev_machine_id` (token rows SURVIVE with NULL);
//! 4. the machine-row delete.
//!
//! `IN` lists render one `$N` bind per id, in fetch order. Django chunks
//! collection at 100 ids (`get_del_batches`); the port issues one
//! statement per step (same rows — the chunking is a client-side memory
//! optimization, unobservable below 100 ids per list).
//!
//! # Transactions
//!
//! `delete_runner`'s outer `atomic` (frame pre-tx) and
//! `delete_dev_machine`'s single `atomic` (everything inside) both run
//! on the executor's one [`Transaction`][pidash_db::tx::Transaction]:
//! the frame seam methods touch Redis only, so calling them first on an
//! empty transaction is unobservable. `close_runner_session` publishes
//! from inside the transaction (non-transactional by design — the daemon
//! treats the frames as idempotent advisories).
//!
//! # Fixture source of truth
//!
//! D13-F6 `services/flows.golden.json` (`delete_runner`,
//! `delete_dev_machine`: frames, reason rule, order-of-ops); D13-F8
//! `external/wire_pins.json` (frame shapes, close semantics — called,
//! never inlined).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * QUIRK-enqueue-vs-evict (`runner_delete.py:52-60`): the frame goes
//!   out while the session row is still alive; post-revoke it would
//!   divert into an offline buffer that never drains for a deleted row.
//!   The driver calls the frame seam methods before the nested revoke.
//! * QUIRK-machine-tx (`:125`): unlike `delete_runner`, the machine
//!   path holds ONE atomic over everything including the frames.
//! * QUIRK-python-side-active-filter (`:129`): `active` filters
//!   `revoked_at is None` in Python, not SQL — but the collector
//!   deletes runners by machine regardless, so pre-revoked runners
//!   skip frames/revokes yet still lose their rows. Ported verbatim.
//! * QUIRK-collector-no-order: root fetches carry no `ORDER BY`.
//!   Ported verbatim.
//!
//! Ported bugs: none found in this unit on read-through.

use chrono::{DateTime, Utc};
use pidash_db::entity::{
    agent_chat_approval, agent_chat_event, agent_chat_message, agent_chat_session, agent_run,
    chat_message_dedupe, dev_machine, machine_session, runner, runner_force_refresh,
    runner_live_state,
};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, JoinType, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait,
};
use uuid::Uuid;

use super::revoke::{revoke_runner, RevokeError, RevokeOutcome, RevokeStore};

// ---------------------------------------------------------------------------
// Frames + reasons (`runner_delete.py:72-90,130-135`)
// ---------------------------------------------------------------------------

/// `delete_runner` frame reason, both verbs (`:74,76`).
pub const DELETE_RUNNER_FRAME_REASON: &str = "deleted by user";

/// `delete_dev_machine` frame reason, both verbs (`:133,135`).
pub const DELETE_MACHINE_FRAME_REASON: &str = "dev machine deleted";

/// Revoke reason when `purge_local` is set (`:90,130`): canonical, so the
/// daemon falls back to local cleanup if the wire frame was lost.
pub const REVOKE_REASON_PURGE: &str = "runner_removed";

/// Revoke reason when `purge_local` is unset (`:90,130`): deliberately
/// outside the daemon synthesizer's canonical set — clean `RunnerLoop`
/// exit, files kept.
pub const REVOKE_REASON_KEEP: &str = "user_revoke";

/// The reason rule (`:90,130`), shared by both drivers.
pub fn delete_revoke_reason(purge_local: bool) -> &'static str {
    if purge_local {
        REVOKE_REASON_PURGE
    } else {
        REVOKE_REASON_KEEP
    }
}

// ---------------------------------------------------------------------------
// SQL (`runner_delete.py:124-143`, probe-verified on Django 4.2.30)
// ---------------------------------------------------------------------------

// NOTE: the D1 machine-token revoke const lived here; the statement
// moved to `crate::domain::tokens::revoke_tokens_for_dev_machine`
// (PIDASHCONV-1520), whose test carries the pin.

// NOTE: the session fast-delete builder lived here; the statement moved
// to `crate::domain::tokens::delete_runner_sessions_for_runners`
// (PIDASHCONV-1520), whose tests carry the pins.

// NOTE: the machine-token `SET_NULL` const lived here; the statement
// moved to `crate::domain::tokens::clear_token_dev_machines`
// (PIDASHCONV-1520), whose test carries the pin (token rows survive).

// ---------------------------------------------------------------------------
// SeaORM cascade statements (`runner_delete.py:94-143`)
// ---------------------------------------------------------------------------

/// D2 (`:128`): the machine's runners, `Meta.ordering`, `FOR UPDATE`; the
/// driver reads id + `revoked_at` ([`LockedRunner`]).
pub async fn lock_machine_runners(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Vec<LockedRunner>, DbErr> {
    let rows: Vec<(Uuid, Option<DateTime<Utc>>)> = runner::Entity::find()
        .select_only()
        .columns([runner::Column::Id, runner::Column::RevokedAt])
        .filter(runner::Column::DevMachineId.eq(machine_id))
        .order_by_desc(runner::Column::LastHeartbeatAt)
        .order_by_desc(runner::Column::CreatedAt)
        .lock_exclusive()
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, revoked_at)| LockedRunner { id, revoked_at })
        .collect())
}

/// Runner-collector root (`:94`): the runner row exists.
pub async fn runner_exists(db: &impl ConnectionTrait, runner_id: Uuid) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = runner::Entity::find()
        .select_only()
        .column(runner::Column::Id)
        .filter(runner::Column::Id.eq(runner_id))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// Runner-collector root, by machine (`:142`; no ordering).
pub async fn machine_runner_ids(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    runner::Entity::find()
        .select_only()
        .column(runner::Column::Id)
        .filter(runner::Column::DevMachineId.eq(machine_id))
        .into_tuple()
        .all(db)
        .await
}

/// Machine-collector root (`:143`): the machine row exists.
pub async fn machine_exists(db: &impl ConnectionTrait, machine_id: Uuid) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = dev_machine::Entity::find()
        .select_only()
        .column(dev_machine::Column::Id)
        .filter(dev_machine::Column::Id.eq(machine_id))
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// Chat ids under the runners (`agent_chat_session.runner CASCADE`), in
/// the session `Meta.ordering`.
pub async fn chat_ids_for_runners(
    db: &impl ConnectionTrait,
    runner_ids: &[Uuid],
) -> Result<Vec<Uuid>, DbErr> {
    agent_chat_session::Entity::find()
        .select_only()
        .column(agent_chat_session::Column::Id)
        .filter(agent_chat_session::Column::RunnerId.is_in(runner_ids.iter().copied()))
        .order_by_desc(agent_chat_session::Column::LastMessageAt)
        .order_by_desc(agent_chat_session::Column::CreatedAt)
        .into_tuple()
        .all(db)
        .await
}

/// Message ids under the chats; the parent ordering prefixes the child's
/// (`seq ASC`), hence the join.
pub async fn message_ids_for_chats(
    db: &impl ConnectionTrait,
    chat_ids: &[Uuid],
) -> Result<Vec<Uuid>, DbErr> {
    agent_chat_message::Entity::find()
        .select_only()
        .column(agent_chat_message::Column::Id)
        .join(
            JoinType::InnerJoin,
            agent_chat_message::Relation::AgentChatSession.def(),
        )
        .filter(agent_chat_message::Column::SessionId.is_in(chat_ids.iter().copied()))
        .order_by_desc(agent_chat_session::Column::LastMessageAt)
        .order_by_desc(agent_chat_session::Column::CreatedAt)
        .order_by_asc(agent_chat_message::Column::Seq)
        .into_tuple()
        .all(db)
        .await
}

/// Chat-subtree fast delete: events (then approvals, then dedupes).
pub async fn delete_chat_events(db: &impl ConnectionTrait, chat_ids: &[Uuid]) -> Result<(), DbErr> {
    agent_chat_event::Entity::delete_many()
        .filter(agent_chat_event::Column::SessionId.is_in(chat_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// See [`delete_chat_events`].
pub async fn delete_chat_approvals(
    db: &impl ConnectionTrait,
    chat_ids: &[Uuid],
) -> Result<(), DbErr> {
    agent_chat_approval::Entity::delete_many()
        .filter(agent_chat_approval::Column::SessionId.is_in(chat_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// See [`delete_chat_events`].
pub async fn delete_chat_dedupes(
    db: &impl ConnectionTrait,
    chat_ids: &[Uuid],
) -> Result<(), DbErr> {
    chat_message_dedupe::Entity::delete_many()
        .filter(chat_message_dedupe::Column::SessionId.is_in(chat_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// Runner-level fast delete: force-refresh rows.
pub async fn delete_force_refresh(
    db: &impl ConnectionTrait,
    runner_ids: &[Uuid],
) -> Result<(), DbErr> {
    runner_force_refresh::Entity::delete_many()
        .filter(runner_force_refresh::Column::RunnerId.is_in(runner_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// Runner-level fast delete: live-state rows.
pub async fn delete_live_state(
    db: &impl ConnectionTrait,
    runner_ids: &[Uuid],
) -> Result<(), DbErr> {
    runner_live_state::Entity::delete_many()
        .filter(runner_live_state::Column::RunnerId.is_in(runner_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// `SET_NULL`: `agent_run.runner`.
pub async fn null_run_runners(db: &impl ConnectionTrait, runner_ids: &[Uuid]) -> Result<(), DbErr> {
    agent_run::Entity::update_many()
        .col_expr(agent_run::Column::RunnerId, Expr::value(None::<Uuid>))
        .filter(agent_run::Column::RunnerId.is_in(runner_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// `SET_NULL`: `agent_run.pinned_runner`.
pub async fn null_run_pins(db: &impl ConnectionTrait, runner_ids: &[Uuid]) -> Result<(), DbErr> {
    agent_run::Entity::update_many()
        .col_expr(agent_run::Column::PinnedRunnerId, Expr::value(None::<Uuid>))
        .filter(agent_run::Column::PinnedRunnerId.is_in(runner_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// `SET_NULL`: `agent_chat_event.message`.
pub async fn null_event_messages(
    db: &impl ConnectionTrait,
    message_ids: &[Uuid],
) -> Result<(), DbErr> {
    agent_chat_event::Entity::update_many()
        .col_expr(
            agent_chat_event::Column::MessageId,
            Expr::value(None::<Uuid>),
        )
        .filter(agent_chat_event::Column::MessageId.is_in(message_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// Instance delete: chat messages.
pub async fn delete_chat_messages(
    db: &impl ConnectionTrait,
    message_ids: &[Uuid],
) -> Result<(), DbErr> {
    agent_chat_message::Entity::delete_many()
        .filter(agent_chat_message::Column::Id.is_in(message_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// Instance delete: chat sessions.
pub async fn delete_chat_sessions(
    db: &impl ConnectionTrait,
    chat_ids: &[Uuid],
) -> Result<(), DbErr> {
    agent_chat_session::Entity::delete_many()
        .filter(agent_chat_session::Column::Id.is_in(chat_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// Instance delete: runners.
pub async fn delete_runner_rows(
    db: &impl ConnectionTrait,
    runner_ids: &[Uuid],
) -> Result<(), DbErr> {
    runner::Entity::delete_many()
        .filter(runner::Column::Id.is_in(runner_ids.iter().copied()))
        .exec(db)
        .await
        .map(drop)
}

/// Machine fast delete: its sessions.
pub async fn delete_machine_sessions(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<(), DbErr> {
    machine_session::Entity::delete_many()
        .filter(machine_session::Column::DevMachineId.eq(machine_id))
        .exec(db)
        .await
        .map(drop)
}

/// Machine `SET_NULL`: `runner.dev_machine` (the rows survive).
pub async fn null_runner_machines(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<(), DbErr> {
    runner::Entity::update_many()
        .col_expr(runner::Column::DevMachineId, Expr::value(None::<Uuid>))
        .filter(runner::Column::DevMachineId.eq(machine_id))
        .exec(db)
        .await
        .map(drop)
}

/// Machine-row delete.
pub async fn delete_machine_row(db: &impl ConnectionTrait, machine_id: Uuid) -> Result<(), DbErr> {
    dev_machine::Entity::delete_many()
        .filter(dev_machine::Column::Id.eq(machine_id))
        .exec(db)
        .await
        .map(drop)
}

// ---------------------------------------------------------------------------
// Seam + drivers
// ---------------------------------------------------------------------------

/// A D2 row as the driver reads it: id + `revoked_at` for the
/// Python-side active filter (`:129`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockedRunner {
    /// `runner.id`.
    pub id: Uuid,
    /// `runner.revoked_at` (`None` = active).
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Storage seam for the delete service.
///
/// Extends [`RevokeStore`] so the nested revoke runs flat on the same
/// store. Frame/close methods delegate to the D-14 pubsub drivers (see
/// each method); the collector methods execute the adjacent SQL verbatim.
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam.
#[allow(async_fn_in_trait)]
pub trait RunnerDeleteStore: RevokeStore {
    /// Enqueue a `revoke` frame (`pubsub.send_runner_revoke`, D-14).
    /// The pool implementation delegates to that driver and returns
    /// its warnings (every failure is swallowed there, as in Python).
    async fn send_revoke_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, RevokeError>;

    /// Enqueue a `remove_runner` frame (`pubsub.send_runner_remove`,
    /// D-14). Same swallow-into-warnings policy.
    async fn send_remove_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, RevokeError>;

    /// Evict the live session (`pubsub.close_runner_session`, D-14).
    /// Python passes the default `code=4010`
    /// (`CLOSE_RUNNER_SESSION_DEFAULT_CODE`; accepted and ignored
    /// there). Python has no `try` here: the first failure aborts and
    /// propagates.
    async fn close_runner_session(&mut self, runner_id: Uuid) -> Result<(), RevokeError>;

    /// D1: revoke the machine's tokens
    /// (`crate::domain::tokens::revoke_tokens_for_dev_machine`).
    async fn revoke_machine_tokens(
        &mut self,
        machine_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), RevokeError>;

    /// D2: locked machine runners, in
    /// listed order.
    async fn lock_machine_runners(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<LockedRunner>, RevokeError>;

    /// Runner-collector root fetch, single id
    ///: whether the row exists.
    async fn fetch_runner_for_delete(&mut self, runner_id: Uuid) -> Result<bool, RevokeError>;

    /// Runner-collector root fetch, by machine
    ///: ids in fetch order
    /// (no `ORDER BY` — the driver keeps the returned order).
    async fn fetch_machine_runner_ids_for_delete(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<Uuid>, RevokeError>;

    /// Chat-id collection, in listed order.
    async fn fetch_chat_ids(&mut self, runner_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError>;

    /// Message-id collection, in listed
    /// order.
    async fn fetch_message_ids(&mut self, chat_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError>;

    /// Chat-subtree fast deletes.
    async fn delete_chat_events(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Chat-subtree fast deletes.
    async fn delete_chat_approvals(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Chat-subtree fast deletes.
    async fn delete_chat_dedupes(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Runner fast deletes
    /// (`crate::domain::tokens::delete_runner_sessions_for_runners`).
    async fn delete_runner_sessions(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Runner fast deletes.
    async fn delete_force_refresh(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Runner fast deletes.
    async fn delete_live_state(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// `SET_NULL`.
    async fn null_run_runners(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// `SET_NULL`.
    async fn null_run_pins(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// `SET_NULL`.
    async fn null_event_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Instance deletes.
    async fn delete_chat_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Instance deletes.
    async fn delete_chat_sessions(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Instance deletes.
    async fn delete_runner_rows(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Machine-collector root fetch:
    /// whether the row exists.
    async fn fetch_machine_for_delete(&mut self, machine_id: Uuid) -> Result<bool, RevokeError>;

    /// Machine fast delete.
    async fn delete_machine_sessions(&mut self, machine_id: Uuid) -> Result<(), RevokeError>;

    /// Machine `SET_NULL`.
    async fn null_runner_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError>;

    /// Machine `SET_NULL` on token rows
    /// (`crate::domain::tokens::clear_token_dev_machines`).
    async fn null_token_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError>;

    /// Machine-row delete.
    async fn delete_machine_row(&mut self, machine_id: Uuid) -> Result<(), RevokeError>;
}

/// Outcome of [`delete_runner`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteRunnerOutcome {
    /// Echo of the flag.
    pub purge_local: bool,
    /// The nested revoke reason (the reason rule).
    pub revoke_reason: String,
    /// The nested revoke outcome.
    pub revoke: RevokeOutcome,
    /// Frame warnings first, then the nested revoke warnings.
    pub warnings: Vec<String>,
    /// The root row existed (and is now deleted).
    pub deleted: bool,
}

/// Outcome of [`delete_dev_machine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteMachineOutcome {
    /// Echo of the flag.
    pub purge_local: bool,
    /// The nested revoke reason (the reason rule).
    pub revoke_reason: String,
    /// One entry per active runner, in D2 order.
    pub revokes: Vec<(Uuid, RevokeOutcome)>,
    /// All frame warnings first (frame-loop order), then the
    /// per-runner revoke warnings (revoke-loop order).
    pub warnings: Vec<String>,
    /// Collector runner ids, in fetch order (includes pre-revoked).
    pub deleted_runners: Vec<Uuid>,
    /// The machine row existed (and is now deleted).
    pub deleted: bool,
}

/// The runner-collector tail shared by both drivers (steps 2–5 of the
/// module docs). `runner_ids` come from the path-specific root fetch;
/// empty skips everything (the collector returns early on no rows).
async fn delete_collected_runner_rows<S: RunnerDeleteStore>(
    store: &mut S,
    runner_ids: &[Uuid],
) -> Result<(), RevokeError> {
    if runner_ids.is_empty() {
        return Ok(());
    }
    let chat_ids = store.fetch_chat_ids(runner_ids).await?;
    let message_ids = if chat_ids.is_empty() {
        Vec::new()
    } else {
        let message_ids = store.fetch_message_ids(&chat_ids).await?;
        store.delete_chat_events(&chat_ids).await?;
        store.delete_chat_approvals(&chat_ids).await?;
        store.delete_chat_dedupes(&chat_ids).await?;
        message_ids
    };
    // Fast deletes (chat subtree already ran above, runner level here).
    store.delete_runner_sessions(runner_ids).await?;
    store.delete_force_refresh(runner_ids).await?;
    store.delete_live_state(runner_ids).await?;
    // Field updates.
    store.null_run_runners(runner_ids).await?;
    store.null_run_pins(runner_ids).await?;
    if !message_ids.is_empty() {
        store.null_event_messages(&message_ids).await?;
    }
    // Instance deletes, deepest first.
    if !message_ids.is_empty() {
        store.delete_chat_messages(&message_ids).await?;
    }
    if !chat_ids.is_empty() {
        store.delete_chat_sessions(&chat_ids).await?;
    }
    store.delete_runner_rows(runner_ids).await?;
    Ok(())
}

/// `delete_runner` (`runner_delete.py:47-94`).
///
/// `revoked_at` is the instance's pre-read `self.revoked_at` for the
/// nested revoke; `now` is that revoke's S1/S2 instant. The executor
/// runs this on its transaction: the frame goes out first (Redis-only,
/// pre-tx position), then revoke + close + collector run as the atomic
/// tail.
pub async fn delete_runner<S: RunnerDeleteStore>(
    store: &mut S,
    runner_id: Uuid,
    revoked_at: Option<DateTime<Utc>>,
    purge_local: bool,
    now: DateTime<Utc>,
) -> Result<DeleteRunnerOutcome, RevokeError> {
    let mut warnings = if purge_local {
        store
            .send_remove_frame(runner_id, DELETE_RUNNER_FRAME_REASON)
            .await?
    } else {
        store
            .send_revoke_frame(runner_id, DELETE_RUNNER_FRAME_REASON)
            .await?
    };
    let revoke_reason = delete_revoke_reason(purge_local);
    let revoke = revoke_runner(store, runner_id, revoked_at, revoke_reason, now).await?;
    warnings.extend(revoke.warnings.iter().cloned());
    store.close_runner_session(runner_id).await?;
    let deleted = store.fetch_runner_for_delete(runner_id).await?;
    if deleted {
        delete_collected_runner_rows(store, &[runner_id]).await?;
    }
    Ok(DeleteRunnerOutcome {
        purge_local,
        revoke_reason: revoke_reason.to_string(),
        revoke,
        warnings,
        deleted,
    })
}

/// `delete_dev_machine` (`runner_delete.py:97-143`).
///
/// `now` stamps D1; `revoke_clock` stamps each nested revoke's S1/S2
/// (Python evaluates `timezone.now()` per `runner.revoke()` call).
/// Everything runs inside the executor's single transaction.
pub async fn delete_dev_machine<S: RunnerDeleteStore>(
    store: &mut S,
    machine_id: Uuid,
    purge_local: bool,
    now: DateTime<Utc>,
    revoke_clock: &mut impl FnMut() -> DateTime<Utc>,
) -> Result<DeleteMachineOutcome, RevokeError> {
    store.revoke_machine_tokens(machine_id, now).await?;
    let runners = store.lock_machine_runners(machine_id).await?;
    let active: Vec<LockedRunner> = runners
        .iter()
        .copied()
        .filter(|runner| runner.revoked_at.is_none())
        .collect();
    let revoke_reason = delete_revoke_reason(purge_local);
    let mut warnings = Vec::new();
    for runner in &active {
        let frame_warnings = if purge_local {
            store
                .send_remove_frame(runner.id, DELETE_MACHINE_FRAME_REASON)
                .await?
        } else {
            store
                .send_revoke_frame(runner.id, DELETE_MACHINE_FRAME_REASON)
                .await?
        };
        warnings.extend(frame_warnings);
    }
    let mut revokes = Vec::with_capacity(active.len());
    for runner in &active {
        let outcome = revoke_runner(
            store,
            runner.id,
            runner.revoked_at,
            revoke_reason,
            revoke_clock(),
        )
        .await?;
        warnings.extend(outcome.warnings.iter().cloned());
        store.close_runner_session(runner.id).await?;
        revokes.push((runner.id, outcome));
    }
    let runner_ids = store
        .fetch_machine_runner_ids_for_delete(machine_id)
        .await?;
    delete_collected_runner_rows(store, &runner_ids).await?;
    let deleted = store.fetch_machine_for_delete(machine_id).await?;
    if deleted {
        store.delete_machine_sessions(machine_id).await?;
        store.null_runner_machines(machine_id).await?;
        store.null_token_machines(machine_id).await?;
        store.delete_machine_row(machine_id).await?;
    }
    Ok(DeleteMachineOutcome {
        purge_local,
        revoke_reason: revoke_reason.to_string(),
        revokes,
        warnings,
        deleted_runners: runner_ids,
        deleted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn flows_fixture() -> serde_json::Value {
        let text = include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
        serde_json::from_str(text).expect("flows.golden.json parses")
    }

    fn frozen_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, 7, 0, 0).unwrap()
    }

    fn uuid(n: u8) -> Uuid {
        Uuid::parse_str(&format!("00000000-0000-0000-0000-0000000000{n:02x}")).unwrap()
    }

    // -- reasons + frames ---------------------------------------------------

    #[test]
    fn reason_rule_and_frame_reasons() {
        assert_eq!(delete_revoke_reason(true), "runner_removed");
        assert_eq!(delete_revoke_reason(false), "user_revoke");
        assert_eq!(REVOKE_REASON_PURGE, "runner_removed");
        assert_eq!(REVOKE_REASON_KEEP, "user_revoke");
        assert_eq!(DELETE_RUNNER_FRAME_REASON, "deleted by user");
        assert_eq!(DELETE_MACHINE_FRAME_REASON, "dev machine deleted");

        let fixture = flows_fixture();
        assert_eq!(
            fixture["delete_runner"]["source"],
            "runner/services/runner_delete.py:47-94"
        );
        assert_eq!(
            fixture["delete_dev_machine"]["source"],
            "runner/services/runner_delete.py:97-143"
        );
        let prose = fixture["delete_runner"]["revoke_reason"]
            .as_str()
            .expect("prose");
        assert!(prose.contains("'runner_removed' if purge_local"), "{prose}");
        assert!(prose.contains("'user_revoke'"), "{prose}");

        // Both rule outputs stay inside the allow-list (else revoke
        // would warn on every delete).
        use pidash_db::runner_enroll::columns::revoke_reasons::KNOWN_REVOKE_REASONS;
        assert!(KNOWN_REVOKE_REASONS.contains(&REVOKE_REASON_PURGE));
        assert!(KNOWN_REVOKE_REASONS.contains(&REVOKE_REASON_KEEP));
    }

    // -- SQL text -----------------------------------------------------------

    // (The D1 SQL pin moved with the statement to
    // `crate::domain::tokens`.)

    // -- driver order (fake store) ------------------------------------------

    #[derive(Debug, PartialEq)]
    enum Call {
        // frames + close
        RevokeFrame(Uuid, String),
        RemoveFrame(Uuid, String),
        Close(Uuid),
        // revoke cascade (nested)
        Mark(Uuid),
        Sessions(Uuid),
        LockRuns(Uuid),
        Finalize(Uuid),
        PinnedSelect(Uuid),
        Unpin(Uuid),
        Handoff(Uuid),
        Drain(Uuid),
        Cleanup(Uuid),
        // machine reads
        RevokeTokens(Uuid),
        LockRunners(Uuid),
        // collector
        FetchRunner(Uuid),
        FetchMachineRunners(Uuid),
        FetchChats(Vec<Uuid>),
        FetchMessages(Vec<Uuid>),
        DelEvents(Vec<Uuid>),
        DelApprovals(Vec<Uuid>),
        DelDedupes(Vec<Uuid>),
        DelSessions(Vec<Uuid>),
        DelForce(Vec<Uuid>),
        DelLive(Vec<Uuid>),
        NullRunners(Vec<Uuid>),
        NullPins(Vec<Uuid>),
        NullEventMsgs(Vec<Uuid>),
        DelMessages(Vec<Uuid>),
        DelChats(Vec<Uuid>),
        DelRunnerRows(Vec<Uuid>),
        FetchMachine(Uuid),
        DelMachineSessions(Uuid),
        NullRunnerMachines(Uuid),
        NullTokenMachines(Uuid),
        DelMachineRow(Uuid),
    }

    struct Fake {
        calls: Vec<Call>,
        frame_warnings: Vec<String>,
        active_runs: Vec<(Uuid, Option<Uuid>)>,
        pinned_pods: Vec<Option<Uuid>>,
        machine_runners: Vec<LockedRunner>,
        runner_exists: bool,
        machine_runner_ids: Vec<Uuid>,
        machine_exists: bool,
        chat_ids: Vec<Uuid>,
        message_ids: Vec<Uuid>,
    }

    impl Fake {
        fn empty() -> Self {
            Self {
                calls: Vec::new(),
                frame_warnings: Vec::new(),
                active_runs: Vec::new(),
                pinned_pods: Vec::new(),
                machine_runners: Vec::new(),
                runner_exists: true,
                machine_runner_ids: Vec::new(),
                machine_exists: true,
                chat_ids: Vec::new(),
                message_ids: Vec::new(),
            }
        }
    }

    impl RevokeStore for Fake {
        async fn mark_runner_revoked(
            &mut self,
            runner_id: Uuid,
            _now: DateTime<Utc>,
            _stored_reason: &str,
        ) -> Result<(), RevokeError> {
            self.calls.push(Call::Mark(runner_id));
            Ok(())
        }

        async fn revoke_active_sessions(
            &mut self,
            runner_id: Uuid,
            _now: DateTime<Utc>,
            _stored_reason: &str,
        ) -> Result<(), RevokeError> {
            self.calls.push(Call::Sessions(runner_id));
            Ok(())
        }

        async fn lock_active_runs(
            &mut self,
            runner_id: Uuid,
        ) -> Result<Vec<(Uuid, Option<Uuid>)>, RevokeError> {
            self.calls.push(Call::LockRuns(runner_id));
            Ok(self.active_runs.clone())
        }

        async fn finalize_cancelled_run(
            &mut self,
            run_id: Uuid,
            _runner_id: Uuid,
            _stored_reason: &str,
        ) -> Result<(), RevokeError> {
            self.calls.push(Call::Finalize(run_id));
            Ok(())
        }

        async fn pinned_queued_pod_ids(
            &mut self,
            runner_id: Uuid,
        ) -> Result<Vec<Option<Uuid>>, RevokeError> {
            self.calls.push(Call::PinnedSelect(runner_id));
            Ok(self.pinned_pods.clone())
        }

        async fn unpin_queued_runs(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
            self.calls.push(Call::Unpin(runner_id));
            Ok(())
        }

        fn complete_handoff_after_commit(&mut self, run_id: Uuid) {
            self.calls.push(Call::Handoff(run_id));
        }

        fn drain_pod_after_commit(&mut self, pod_id: Uuid) {
            self.calls.push(Call::Drain(pod_id));
        }

        fn schedule_stream_cleanup_after_commit(&mut self, runner_id: Uuid) {
            self.calls.push(Call::Cleanup(runner_id));
        }

        async fn dedicated_issue_ids(
            &mut self,
            _runner_id: Uuid,
        ) -> Result<Vec<Uuid>, RevokeError> {
            Ok(Vec::new())
        }

        async fn apply_lost_runner(
            &mut self,
            _issue_id: Uuid,
            _reason: &str,
        ) -> Result<crate::domain::issue_assignment::LostRunnerOutcome, RevokeError> {
            Ok(crate::domain::issue_assignment::LostRunnerOutcome::default())
        }

        fn bounce_dedicated_after_commit(
            &mut self,
            _issue_id: Uuid,
            _runner_name: Option<&str>,
            _reason: &str,
            _triggered_by: &str,
        ) {
        }

        fn recreate_dedicated_after_commit(
            &mut self,
            _issue_id: Uuid,
            _pod_id: Uuid,
            _trigger: &str,
            _creator: Option<Uuid>,
        ) {
        }
    }

    impl RunnerDeleteStore for Fake {
        async fn send_revoke_frame(
            &mut self,
            runner_id: Uuid,
            reason: &str,
        ) -> Result<Vec<String>, RevokeError> {
            self.calls
                .push(Call::RevokeFrame(runner_id, reason.to_string()));
            Ok(self.frame_warnings.clone())
        }

        async fn send_remove_frame(
            &mut self,
            runner_id: Uuid,
            reason: &str,
        ) -> Result<Vec<String>, RevokeError> {
            self.calls
                .push(Call::RemoveFrame(runner_id, reason.to_string()));
            Ok(self.frame_warnings.clone())
        }

        async fn close_runner_session(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
            self.calls.push(Call::Close(runner_id));
            Ok(())
        }

        async fn revoke_machine_tokens(
            &mut self,
            machine_id: Uuid,
            _now: DateTime<Utc>,
        ) -> Result<(), RevokeError> {
            self.calls.push(Call::RevokeTokens(machine_id));
            Ok(())
        }

        async fn lock_machine_runners(
            &mut self,
            machine_id: Uuid,
        ) -> Result<Vec<LockedRunner>, RevokeError> {
            self.calls.push(Call::LockRunners(machine_id));
            Ok(self.machine_runners.clone())
        }

        async fn fetch_runner_for_delete(&mut self, runner_id: Uuid) -> Result<bool, RevokeError> {
            self.calls.push(Call::FetchRunner(runner_id));
            Ok(self.runner_exists)
        }

        async fn fetch_machine_runner_ids_for_delete(
            &mut self,
            machine_id: Uuid,
        ) -> Result<Vec<Uuid>, RevokeError> {
            self.calls.push(Call::FetchMachineRunners(machine_id));
            Ok(self.machine_runner_ids.clone())
        }

        async fn fetch_chat_ids(&mut self, runner_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError> {
            self.calls.push(Call::FetchChats(runner_ids.to_vec()));
            Ok(self.chat_ids.clone())
        }

        async fn fetch_message_ids(&mut self, chat_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError> {
            self.calls.push(Call::FetchMessages(chat_ids.to_vec()));
            Ok(self.message_ids.clone())
        }

        async fn delete_chat_events(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelEvents(chat_ids.to_vec()));
            Ok(())
        }

        async fn delete_chat_approvals(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelApprovals(chat_ids.to_vec()));
            Ok(())
        }

        async fn delete_chat_dedupes(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelDedupes(chat_ids.to_vec()));
            Ok(())
        }

        async fn delete_runner_sessions(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelSessions(runner_ids.to_vec()));
            Ok(())
        }

        async fn delete_force_refresh(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelForce(runner_ids.to_vec()));
            Ok(())
        }

        async fn delete_live_state(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelLive(runner_ids.to_vec()));
            Ok(())
        }

        async fn null_run_runners(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::NullRunners(runner_ids.to_vec()));
            Ok(())
        }

        async fn null_run_pins(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::NullPins(runner_ids.to_vec()));
            Ok(())
        }

        async fn null_event_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::NullEventMsgs(message_ids.to_vec()));
            Ok(())
        }

        async fn delete_chat_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelMessages(message_ids.to_vec()));
            Ok(())
        }

        async fn delete_chat_sessions(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelChats(chat_ids.to_vec()));
            Ok(())
        }

        async fn delete_runner_rows(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
            self.calls.push(Call::DelRunnerRows(runner_ids.to_vec()));
            Ok(())
        }

        async fn fetch_machine_for_delete(
            &mut self,
            machine_id: Uuid,
        ) -> Result<bool, RevokeError> {
            self.calls.push(Call::FetchMachine(machine_id));
            Ok(self.machine_exists)
        }

        async fn delete_machine_sessions(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
            self.calls.push(Call::DelMachineSessions(machine_id));
            Ok(())
        }

        async fn null_runner_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
            self.calls.push(Call::NullRunnerMachines(machine_id));
            Ok(())
        }

        async fn null_token_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
            self.calls.push(Call::NullTokenMachines(machine_id));
            Ok(())
        }

        async fn delete_machine_row(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
            self.calls.push(Call::DelMachineRow(machine_id));
            Ok(())
        }
    }

    #[tokio::test]
    async fn delete_runner_purge_sends_remove_frame_first_then_tail() {
        let mut store = Fake::empty();
        store.frame_warnings = vec!["frame-warn".to_string()];
        let outcome = delete_runner(&mut store, uuid(7), None, true, frozen_now())
            .await
            .expect("delete");
        assert_eq!(
            store.calls,
            vec![
                Call::RemoveFrame(uuid(7), "deleted by user".to_string()),
                Call::Mark(uuid(7)),
                Call::Sessions(uuid(7)),
                Call::LockRuns(uuid(7)),
                Call::PinnedSelect(uuid(7)),
                Call::Cleanup(uuid(7)),
                Call::Close(uuid(7)),
                Call::FetchRunner(uuid(7)),
                Call::FetchChats(vec![uuid(7)]),
                Call::DelSessions(vec![uuid(7)]),
                Call::DelForce(vec![uuid(7)]),
                Call::DelLive(vec![uuid(7)]),
                Call::NullRunners(vec![uuid(7)]),
                Call::NullPins(vec![uuid(7)]),
                Call::DelRunnerRows(vec![uuid(7)]),
            ]
        );
        assert!(outcome.purge_local);
        assert_eq!(outcome.revoke_reason, "runner_removed");
        assert_eq!(outcome.revoke.stored_reason, "runner_removed");
        assert_eq!(outcome.warnings, vec!["frame-warn".to_string()]);
        assert!(outcome.deleted);

        let loaded_fixture = flows_fixture();
        let order = loaded_fixture["delete_runner"]["order"]
            .as_array()
            .expect("order");
        assert_eq!(order.len(), 6);
        assert!(order[0]
            .as_str()
            .expect("step")
            .contains("control frame FIRST"));
    }

    #[tokio::test]
    async fn delete_runner_keep_uses_revoke_frame_and_user_revoke() {
        let mut store = Fake::empty();
        let chat = uuid(20);
        let message = uuid(21);
        store.chat_ids = vec![chat];
        store.message_ids = vec![message];
        let outcome = delete_runner(&mut store, uuid(7), None, false, frozen_now())
            .await
            .expect("delete");
        assert_eq!(
            store.calls,
            vec![
                Call::RevokeFrame(uuid(7), "deleted by user".to_string()),
                Call::Mark(uuid(7)),
                Call::Sessions(uuid(7)),
                Call::LockRuns(uuid(7)),
                Call::PinnedSelect(uuid(7)),
                Call::Cleanup(uuid(7)),
                Call::Close(uuid(7)),
                Call::FetchRunner(uuid(7)),
                Call::FetchChats(vec![uuid(7)]),
                Call::FetchMessages(vec![chat]),
                Call::DelEvents(vec![chat]),
                Call::DelApprovals(vec![chat]),
                Call::DelDedupes(vec![chat]),
                Call::DelSessions(vec![uuid(7)]),
                Call::DelForce(vec![uuid(7)]),
                Call::DelLive(vec![uuid(7)]),
                Call::NullRunners(vec![uuid(7)]),
                Call::NullPins(vec![uuid(7)]),
                Call::NullEventMsgs(vec![message]),
                Call::DelMessages(vec![message]),
                Call::DelChats(vec![chat]),
                Call::DelRunnerRows(vec![uuid(7)]),
            ]
        );
        assert!(!outcome.purge_local);
        assert_eq!(outcome.revoke_reason, "user_revoke");
        assert!(outcome.deleted);
    }

    #[tokio::test]
    async fn delete_runner_chats_without_messages_still_drop_chat_fast_rows() {
        let mut store = Fake::empty();
        let chat = uuid(20);
        store.chat_ids = vec![chat];
        store.message_ids = Vec::new();
        delete_runner(&mut store, uuid(7), None, true, frozen_now())
            .await
            .expect("delete");
        let tail = &store.calls[8..];
        assert_eq!(
            tail,
            vec![
                Call::FetchChats(vec![uuid(7)]),
                Call::FetchMessages(vec![chat]),
                Call::DelEvents(vec![chat]),
                Call::DelApprovals(vec![chat]),
                Call::DelDedupes(vec![chat]),
                Call::DelSessions(vec![uuid(7)]),
                Call::DelForce(vec![uuid(7)]),
                Call::DelLive(vec![uuid(7)]),
                Call::NullRunners(vec![uuid(7)]),
                Call::NullPins(vec![uuid(7)]),
                Call::DelChats(vec![chat]),
                Call::DelRunnerRows(vec![uuid(7)]),
            ]
        );
    }

    #[tokio::test]
    async fn delete_runner_already_revoked_still_closes_and_deletes() {
        let mut store = Fake::empty();
        let outcome = delete_runner(&mut store, uuid(7), Some(frozen_now()), false, frozen_now())
            .await
            .expect("delete");
        // Nested revoke no-ops (no Mark), but close + collector run on.
        assert!(!store.calls.contains(&Call::Mark(uuid(7))));
        assert!(store.calls.contains(&Call::Close(uuid(7))));
        assert!(store.calls.contains(&Call::DelRunnerRows(vec![uuid(7)])));
        assert!(outcome.revoke.already_revoked);
        assert!(outcome.deleted);
    }

    #[tokio::test]
    async fn delete_runner_missing_root_stops_after_fetch() {
        let mut store = Fake::empty();
        store.runner_exists = false;
        let outcome = delete_runner(&mut store, uuid(7), None, true, frozen_now())
            .await
            .expect("delete");
        assert!(!outcome.deleted);
        assert_eq!(store.calls.last(), Some(&Call::FetchRunner(uuid(7))));
    }

    #[tokio::test]
    async fn delete_machine_frames_all_active_before_any_revoke() {
        let mut store = Fake::empty();
        let live = uuid(30);
        let dead = uuid(31);
        store.machine_runners = vec![
            LockedRunner {
                id: live,
                revoked_at: None,
            },
            LockedRunner {
                id: dead,
                revoked_at: Some(frozen_now()),
            },
        ];
        store.machine_runner_ids = vec![live, dead];
        let mut ticks = 0;
        let mut clock = || {
            ticks += 1;
            frozen_now()
        };
        let outcome = delete_dev_machine(&mut store, uuid(5), false, frozen_now(), &mut clock)
            .await
            .expect("delete");
        assert_eq!(
            store.calls,
            vec![
                Call::RevokeTokens(uuid(5)),
                Call::LockRunners(uuid(5)),
                Call::RevokeFrame(live, "dev machine deleted".to_string()),
                Call::Mark(live),
                Call::Sessions(live),
                Call::LockRuns(live),
                Call::PinnedSelect(live),
                Call::Cleanup(live),
                Call::Close(live),
                Call::FetchMachineRunners(uuid(5)),
                Call::FetchChats(vec![live, dead]),
                Call::DelSessions(vec![live, dead]),
                Call::DelForce(vec![live, dead]),
                Call::DelLive(vec![live, dead]),
                Call::NullRunners(vec![live, dead]),
                Call::NullPins(vec![live, dead]),
                Call::DelRunnerRows(vec![live, dead]),
                Call::FetchMachine(uuid(5)),
                Call::DelMachineSessions(uuid(5)),
                Call::NullRunnerMachines(uuid(5)),
                Call::NullTokenMachines(uuid(5)),
                Call::DelMachineRow(uuid(5)),
            ]
        );
        // The pre-revoked runner skips frames/revokes yet loses its row.
        assert_eq!(outcome.revokes.len(), 1);
        assert_eq!(outcome.revokes[0].0, live);
        assert_eq!(outcome.deleted_runners, vec![live, dead]);
        assert_eq!(outcome.revoke_reason, "user_revoke");
        assert!(outcome.deleted);
        assert_eq!(ticks, 1, "one revoke instant per nested revoke");

        let loaded_fixture = flows_fixture();
        let order = loaded_fixture["delete_dev_machine"]["order"]
            .as_array()
            .expect("order");
        assert_eq!(order.len(), 8);
        assert!(order[3]
            .as_str()
            .expect("step")
            .contains("all frames BEFORE"));
    }

    #[tokio::test]
    async fn delete_machine_purge_uses_remove_frames_and_canonical_reason() {
        let mut store = Fake::empty();
        store.machine_runners = vec![LockedRunner {
            id: uuid(30),
            revoked_at: None,
        }];
        store.machine_runner_ids = vec![uuid(30)];
        let outcome = delete_dev_machine(&mut store, uuid(5), true, frozen_now(), &mut frozen_now)
            .await
            .expect("delete");
        assert_eq!(outcome.revoke_reason, "runner_removed");
        assert_eq!(outcome.revokes[0].1.stored_reason, "runner_removed");
        assert!(matches!(
            store.calls[2],
            Call::RemoveFrame(_, ref reason) if reason == "dev machine deleted"
        ));
    }

    /// The runner / machine delete cascade and the revoke S1/S3/S4
    /// statements against constraint-free temp tables on one transaction.
    #[tokio::test]
    async fn live_cascade_statements() {
        use crate::runner_enroll::revoke;
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        for table in [
            "runner",
            "agent_run",
            "agent_chat_session",
            "agent_chat_message",
            "agent_chat_event",
            "agent_chat_approval",
            "chat_message_dedupe",
            "runner_force_refresh",
            "runner_live_state",
            "machine_session",
            "dev_machine",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} ON COMMIT DROP AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&mut *tx)
            .await
            .expect("temp table");
        }
        let (machine, r1, r2, chat, msg, run, queued, pod) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let active = crate::runner_sessions::guards::NON_TERMINAL_STATUSES[0].value();
        for sql in [
            format!("INSERT INTO dev_machine (id) VALUES ('{machine}')"),
            format!(
                "INSERT INTO runner (id, dev_machine_id, created_at, revoked_at) VALUES \
                 ('{r1}', '{machine}', now(), NULL), ('{r2}', '{machine}', now() - interval '1 hour', now())"
            ),
            format!("INSERT INTO agent_chat_session (id, runner_id, created_at) VALUES ('{chat}', '{r1}', now())"),
            format!("INSERT INTO agent_chat_message (id, session_id, seq) VALUES ('{msg}', '{chat}', 1)"),
            format!("INSERT INTO agent_chat_event (id, session_id, message_id) VALUES (1, '{chat}', '{msg}')"),
            format!("INSERT INTO agent_chat_approval (id, session_id) VALUES (gen_random_uuid(), '{chat}')"),
            format!("INSERT INTO chat_message_dedupe (id, session_id) VALUES (1, '{chat}')"),
            format!("INSERT INTO runner_force_refresh (runner_id, min_rtg) VALUES ('{r1}', 1)"),
            format!("INSERT INTO runner_live_state (runner_id, usage, updated_at) VALUES ('{r1}', '{{}}', now())"),
            format!(
                "INSERT INTO agent_run (id, runner_id, pinned_runner_id, pod_id, status, created_at) VALUES \
                 ('{run}', '{r1}', NULL, '{pod}', '{active}', now()), \
                 ('{queued}', NULL, '{r1}', '{pod}', 'queued', now())"
            ),
            format!("INSERT INTO machine_session (id, dev_machine_id) VALUES (gen_random_uuid(), '{machine}')"),
        ] {
            sqlx::query(&sql).execute(&mut *tx).await.expect("seed");
        }
        let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
        let count = |table: &'static str| {
            let conn = &conn;
            async move {
                let row = conn
                    .query_one(sea_orm::Statement::from_string(
                        sea_orm::DbBackend::Postgres,
                        format!("SELECT COUNT(*) AS n FROM {table}"),
                    ))
                    .await
                    .expect("count")
                    .expect("row");
                row.try_get::<i64>("", "n").expect("n")
            }
        };

        // Revoke S1/S3/S4.
        revoke::stamp_runner_revoked(&conn, r1, chrono::Utc::now(), "manual")
            .await
            .expect("S1");
        assert_eq!(
            revoke::lock_active_runs(&conn, r1).await.expect("S3"),
            vec![(run, Some(pod))]
        );
        assert_eq!(
            revoke::pinned_queued_pod_ids(&conn, r1).await.expect("S4a"),
            vec![Some(pod)]
        );
        revoke::unpin_queued_runs_for_runner(&conn, r1)
            .await
            .expect("S4b");
        assert!(revoke::pinned_queued_pod_ids(&conn, r1)
            .await
            .expect("S4a")
            .is_empty());

        // D2 lock: both runners, newest heartbeat/creation first.
        let locked: Vec<Uuid> = lock_machine_runners(&conn, machine)
            .await
            .expect("D2")
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(locked, vec![r1, r2]);
        assert!(runner_exists(&conn, r1).await.expect("root"));
        assert!(machine_exists(&conn, machine).await.expect("root"));
        assert_eq!(
            machine_runner_ids(&conn, machine).await.expect("ids").len(),
            2
        );

        // Runner cascade, in the driver's order.
        let chats = chat_ids_for_runners(&conn, &[r1]).await.expect("chats");
        assert_eq!(chats, vec![chat]);
        let messages = message_ids_for_chats(&conn, &chats)
            .await
            .expect("messages");
        assert_eq!(messages, vec![msg]);
        delete_chat_events(&conn, &chats).await.expect("events");
        delete_chat_approvals(&conn, &chats)
            .await
            .expect("approvals");
        delete_chat_dedupes(&conn, &chats).await.expect("dedupes");
        delete_force_refresh(&conn, &[r1]).await.expect("ffr");
        delete_live_state(&conn, &[r1]).await.expect("live");
        null_run_runners(&conn, &[r1]).await.expect("null runner");
        null_run_pins(&conn, &[r1]).await.expect("null pin");
        null_event_messages(&conn, &messages)
            .await
            .expect("null msg");
        delete_chat_messages(&conn, &messages)
            .await
            .expect("messages");
        delete_chat_sessions(&conn, &chats).await.expect("sessions");
        delete_runner_rows(&conn, &[r1]).await.expect("runner");
        for table in [
            "agent_chat_event",
            "agent_chat_approval",
            "chat_message_dedupe",
            "runner_force_refresh",
            "runner_live_state",
            "agent_chat_message",
            "agent_chat_session",
        ] {
            assert_eq!(count(table).await, 0, "{table}");
        }
        assert_eq!(count("runner").await, 1);
        // The runs survive with their runner link nulled.
        assert!(revoke::lock_active_runs(&conn, r1)
            .await
            .expect("S3")
            .is_empty());
        assert_eq!(count("agent_run").await, 2);

        // Machine delete: sessions gone, surviving runners detached, row gone.
        delete_machine_sessions(&conn, machine)
            .await
            .expect("sessions");
        null_runner_machines(&conn, machine).await.expect("detach");
        delete_machine_row(&conn, machine).await.expect("row");
        assert_eq!(count("machine_session").await, 0);
        assert!(machine_runner_ids(&conn, machine)
            .await
            .expect("ids")
            .is_empty());
        assert!(!machine_exists(&conn, machine).await.expect("root"));
        assert!(runner_exists(&conn, r2).await.expect("root"));
    }
}
