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

/// D1 — revoke the machine's tokens (`:127`).
///
/// `$1` = now, `$2` = machine.
pub const REVOKE_MACHINE_TOKENS_SQL: &str = "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL)";

/// D2 — locked full rows for the machine (`:128`).
///
/// `$1` = machine. Django materializes full instances; the driver reads
/// only id + `revoked_at` ([`LockedRunner`]).
pub const LOCK_MACHINE_RUNNERS_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE \"runner\".\"dev_machine_id\" = $1 ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC FOR UPDATE";

/// Runner-collector root fetch, single-id form (`:94`; no `ORDER BY`).
///
/// `$1` = runner.
pub const COLLECT_RUNNER_BY_ID_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE \"runner\".\"id\" = $1";

/// Runner-collector root fetch, by-machine form (`:142`; no `ORDER BY`).
///
/// `$1` = machine.
pub const COLLECT_RUNNERS_BY_MACHINE_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE \"runner\".\"dev_machine_id\" = $1";

/// Machine-collector root fetch (`:143`; no `ORDER BY`).
///
/// `$1` = machine.
pub const COLLECT_MACHINE_BY_ID_SQL: &str = "SELECT \"dev_machine\".\"id\", \"dev_machine\".\"owner_id\", \"dev_machine\".\"host_label\", \"dev_machine\".\"label\", \"dev_machine\".\"visibility\", \"dev_machine\".\"provisioning\", \"dev_machine\".\"last_seen_at\", \"dev_machine\".\"revoked_at\", \"dev_machine\".\"created_at\", \"dev_machine\".\"updated_at\" FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1";

/// `$1, $2, ...` bind list for an `IN (...)` of `count` ids, in order.
fn in_placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Chat-id collection (`agent_chat_session.runner CASCADE`, non-fast:
/// messages hang off it). `$1..$N` = runner ids in fetch order.
pub fn collect_chat_ids_sql(runner_count: usize) -> String {
    format!(
        "SELECT \"agent_chat_session\".\"id\" FROM \"agent_chat_session\" WHERE \"agent_chat_session\".\"runner_id\" IN ({}) ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC",
        in_placeholders(runner_count),
    )
}

/// Message-id collection. `$1..$N` = chat ids. The parent ordering
/// prefixes the child's (`seq ASC`), hence the join.
pub fn collect_message_ids_sql(chat_count: usize) -> String {
    format!(
        "SELECT \"agent_chat_message\".\"id\" FROM \"agent_chat_message\" INNER JOIN \"agent_chat_session\" ON (\"agent_chat_message\".\"session_id\" = \"agent_chat_session\".\"id\") WHERE \"agent_chat_message\".\"session_id\" IN ({}) ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC, \"agent_chat_message\".\"seq\" ASC",
        in_placeholders(chat_count),
    )
}

/// Chat-subtree fast deletes (chat ids). Each takes `$1..$N` = chat ids.
pub fn delete_chat_events_sql(chat_count: usize) -> String {
    format!(
        "DELETE FROM \"agent_chat_event\" WHERE \"agent_chat_event\".\"session_id\" IN ({})",
        in_placeholders(chat_count),
    )
}

/// See [`delete_chat_events_sql`].
pub fn delete_chat_approvals_sql(chat_count: usize) -> String {
    format!(
        "DELETE FROM \"agent_chat_approval\" WHERE \"agent_chat_approval\".\"session_id\" IN ({})",
        in_placeholders(chat_count),
    )
}

/// See [`delete_chat_events_sql`].
pub fn delete_chat_dedupes_sql(chat_count: usize) -> String {
    format!(
        "DELETE FROM \"chat_message_dedupe\" WHERE \"chat_message_dedupe\".\"session_id\" IN ({})",
        in_placeholders(chat_count),
    )
}

/// Runner-level fast deletes (runner ids). Each takes `$1..$N` = runner
/// ids in fetch order.
pub fn delete_runner_sessions_sql(runner_count: usize) -> String {
    format!(
        "DELETE FROM \"runner_session\" WHERE \"runner_session\".\"runner_id\" IN ({})",
        in_placeholders(runner_count),
    )
}

/// See [`delete_runner_sessions_sql`].
pub fn delete_force_refresh_sql(runner_count: usize) -> String {
    format!(
        "DELETE FROM \"runner_force_refresh\" WHERE \"runner_force_refresh\".\"runner_id\" IN ({})",
        in_placeholders(runner_count),
    )
}

/// See [`delete_runner_sessions_sql`].
pub fn delete_live_state_sql(runner_count: usize) -> String {
    format!(
        "DELETE FROM \"runner_live_state\" WHERE \"runner_live_state\".\"runner_id\" IN ({})",
        in_placeholders(runner_count),
    )
}

/// `SET_NULL` updates (runner ids). Each takes `$1..$N` = runner ids.
pub fn null_run_runners_sql(runner_count: usize) -> String {
    format!(
        "UPDATE \"agent_run\" SET \"runner_id\" = NULL WHERE \"agent_run\".\"runner_id\" IN ({})",
        in_placeholders(runner_count),
    )
}

/// See [`null_run_runners_sql`].
pub fn null_run_pins_sql(runner_count: usize) -> String {
    format!(
        "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE \"agent_run\".\"pinned_runner_id\" IN ({})",
        in_placeholders(runner_count),
    )
}

/// `agent_chat_event.message_id` nulling. `$1..$N` = message ids.
pub fn null_event_messages_sql(message_count: usize) -> String {
    format!(
        "UPDATE \"agent_chat_event\" SET \"message_id\" = NULL WHERE \"agent_chat_event\".\"message_id\" IN ({})",
        in_placeholders(message_count),
    )
}

/// Instance deletes, deepest first. `$1..$N` = row ids in fetch order.
pub fn delete_chat_messages_sql(message_count: usize) -> String {
    format!(
        "DELETE FROM \"agent_chat_message\" WHERE \"agent_chat_message\".\"id\" IN ({})",
        in_placeholders(message_count),
    )
}

/// See [`delete_chat_messages_sql`] (`$1..$N` = chat ids).
pub fn delete_chat_sessions_sql(chat_count: usize) -> String {
    format!(
        "DELETE FROM \"agent_chat_session\" WHERE \"agent_chat_session\".\"id\" IN ({})",
        in_placeholders(chat_count),
    )
}

/// See [`delete_chat_messages_sql`] (`$1..$N` = runner ids).
pub fn delete_runner_rows_sql(runner_count: usize) -> String {
    format!(
        "DELETE FROM \"runner\" WHERE \"runner\".\"id\" IN ({})",
        in_placeholders(runner_count),
    )
}

/// Machine fast delete: `$1` = machine.
pub const DELETE_MACHINE_SESSIONS_SQL: &str =
    "DELETE FROM \"machine_session\" WHERE \"machine_session\".\"dev_machine_id\" IN ($1)";

/// Machine `SET_NULL` updates: `$1` = machine.
pub const NULL_RUNNER_MACHINES_SQL: &str =
    "UPDATE \"runner\" SET \"dev_machine_id\" = NULL WHERE \"runner\".\"dev_machine_id\" IN ($1)";

/// Machine `SET_NULL` updates: `$1` = machine (token rows survive).
pub const NULL_TOKEN_MACHINES_SQL: &str =
    "UPDATE \"machine_token\" SET \"dev_machine_id\" = NULL WHERE \"machine_token\".\"dev_machine_id\" IN ($1)";

/// Machine-row delete: `$1` = machine.
pub const DELETE_MACHINE_ROW_SQL: &str =
    "DELETE FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" IN ($1)";

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

    /// D1: revoke the machine's tokens ([`REVOKE_MACHINE_TOKENS_SQL`]).
    async fn revoke_machine_tokens(
        &mut self,
        machine_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), RevokeError>;

    /// D2: locked machine runners ([`LOCK_MACHINE_RUNNERS_SQL`]), in
    /// listed order.
    async fn lock_machine_runners(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<LockedRunner>, RevokeError>;

    /// Runner-collector root fetch, single id
    /// ([`COLLECT_RUNNER_BY_ID_SQL`]): whether the row exists.
    async fn fetch_runner_for_delete(&mut self, runner_id: Uuid) -> Result<bool, RevokeError>;

    /// Runner-collector root fetch, by machine
    /// ([`COLLECT_RUNNERS_BY_MACHINE_SQL`]): ids in fetch order
    /// (no `ORDER BY` — the driver keeps the returned order).
    async fn fetch_machine_runner_ids_for_delete(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<Uuid>, RevokeError>;

    /// Chat-id collection ([`collect_chat_ids_sql`]), in listed order.
    async fn fetch_chat_ids(&mut self, runner_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError>;

    /// Message-id collection ([`collect_message_ids_sql`]), in listed
    /// order.
    async fn fetch_message_ids(&mut self, chat_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError>;

    /// Chat-subtree fast deletes ([`delete_chat_events_sql`]).
    async fn delete_chat_events(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Chat-subtree fast deletes ([`delete_chat_approvals_sql`]).
    async fn delete_chat_approvals(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Chat-subtree fast deletes ([`delete_chat_dedupes_sql`]).
    async fn delete_chat_dedupes(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Runner fast deletes ([`delete_runner_sessions_sql`]).
    async fn delete_runner_sessions(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Runner fast deletes ([`delete_force_refresh_sql`]).
    async fn delete_force_refresh(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Runner fast deletes ([`delete_live_state_sql`]).
    async fn delete_live_state(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// `SET_NULL` ([`null_run_runners_sql`]).
    async fn null_run_runners(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// `SET_NULL` ([`null_run_pins_sql`]).
    async fn null_run_pins(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// `SET_NULL` ([`null_event_messages_sql`]).
    async fn null_event_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Instance deletes ([`delete_chat_messages_sql`]).
    async fn delete_chat_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Instance deletes ([`delete_chat_sessions_sql`]).
    async fn delete_chat_sessions(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Instance deletes ([`delete_runner_rows_sql`]).
    async fn delete_runner_rows(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError>;

    /// Machine-collector root fetch ([`COLLECT_MACHINE_BY_ID_SQL`]):
    /// whether the row exists.
    async fn fetch_machine_for_delete(&mut self, machine_id: Uuid) -> Result<bool, RevokeError>;

    /// Machine fast delete ([`DELETE_MACHINE_SESSIONS_SQL`]).
    async fn delete_machine_sessions(&mut self, machine_id: Uuid) -> Result<(), RevokeError>;

    /// Machine `SET_NULL` ([`NULL_RUNNER_MACHINES_SQL`]).
    async fn null_runner_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError>;

    /// Machine `SET_NULL` ([`NULL_TOKEN_MACHINES_SQL`]).
    async fn null_token_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError>;

    /// Machine-row delete ([`DELETE_MACHINE_ROW_SQL`]).
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
    use pidash_db::runner_enroll::columns::{dev_machine as dm_cols, runner as r_cols};

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

    fn select_prefix(table: &str, columns: &[&str]) -> String {
        let list = columns
            .iter()
            .map(|column| format!("\"{table}\".\"{column}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("SELECT {list} FROM \"{table}\"")
    }

    #[test]
    fn d1_revokes_machine_tokens() {
        assert_eq!(
            REVOKE_MACHINE_TOKENS_SQL,
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE (\"machine_token\".\"dev_machine_id\" = $2 AND \"machine_token\".\"revoked_at\" IS NULL)"
        );
    }

    #[test]
    fn d2_locks_full_rows_in_model_order() {
        let expected = format!(
            "{} WHERE \"runner\".\"dev_machine_id\" = $1 ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC FOR UPDATE",
            select_prefix(r_cols::TABLE, r_cols::COLUMNS),
        );
        assert_eq!(LOCK_MACHINE_RUNNERS_SQL, expected);
        assert_eq!(r_cols::COLUMNS.len(), 30);
    }

    #[test]
    fn collector_roots_have_no_ordering() {
        for sql in [COLLECT_RUNNER_BY_ID_SQL, COLLECT_RUNNERS_BY_MACHINE_SQL] {
            assert!(
                sql.starts_with(&select_prefix(r_cols::TABLE, r_cols::COLUMNS)),
                "{sql}"
            );
            assert!(!sql.contains("ORDER BY"), "{sql}");
        }
        assert_eq!(
            COLLECT_RUNNER_BY_ID_SQL,
            format!(
                "{} WHERE \"runner\".\"id\" = $1",
                select_prefix(r_cols::TABLE, r_cols::COLUMNS),
            )
        );
        assert_eq!(
            COLLECT_RUNNERS_BY_MACHINE_SQL,
            format!(
                "{} WHERE \"runner\".\"dev_machine_id\" = $1",
                select_prefix(r_cols::TABLE, r_cols::COLUMNS),
            )
        );
        assert_eq!(
            COLLECT_MACHINE_BY_ID_SQL,
            format!(
                "{} WHERE \"dev_machine\".\"id\" = $1",
                select_prefix(dm_cols::TABLE, dm_cols::COLUMNS),
            )
        );
        assert_eq!(dm_cols::COLUMNS.len(), 10);
    }

    #[test]
    fn collector_collection_selects() {
        assert_eq!(
            collect_chat_ids_sql(1),
            "SELECT \"agent_chat_session\".\"id\" FROM \"agent_chat_session\" WHERE \"agent_chat_session\".\"runner_id\" IN ($1) ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC"
        );
        assert_eq!(
            collect_chat_ids_sql(2),
            "SELECT \"agent_chat_session\".\"id\" FROM \"agent_chat_session\" WHERE \"agent_chat_session\".\"runner_id\" IN ($1, $2) ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC"
        );
        assert_eq!(
            collect_message_ids_sql(1),
            "SELECT \"agent_chat_message\".\"id\" FROM \"agent_chat_message\" INNER JOIN \"agent_chat_session\" ON (\"agent_chat_message\".\"session_id\" = \"agent_chat_session\".\"id\") WHERE \"agent_chat_message\".\"session_id\" IN ($1) ORDER BY \"agent_chat_session\".\"last_message_at\" DESC, \"agent_chat_session\".\"created_at\" DESC, \"agent_chat_message\".\"seq\" ASC"
        );
    }

    #[test]
    fn collector_fast_deletes_set_nulls_and_instance_deletes() {
        assert_eq!(
            delete_chat_events_sql(1),
            "DELETE FROM \"agent_chat_event\" WHERE \"agent_chat_event\".\"session_id\" IN ($1)"
        );
        assert_eq!(
            delete_chat_approvals_sql(2),
            "DELETE FROM \"agent_chat_approval\" WHERE \"agent_chat_approval\".\"session_id\" IN ($1, $2)"
        );
        assert_eq!(
            delete_chat_dedupes_sql(1),
            "DELETE FROM \"chat_message_dedupe\" WHERE \"chat_message_dedupe\".\"session_id\" IN ($1)"
        );
        assert_eq!(
            delete_runner_sessions_sql(1),
            "DELETE FROM \"runner_session\" WHERE \"runner_session\".\"runner_id\" IN ($1)"
        );
        assert_eq!(
            delete_force_refresh_sql(1),
            "DELETE FROM \"runner_force_refresh\" WHERE \"runner_force_refresh\".\"runner_id\" IN ($1)"
        );
        assert_eq!(
            delete_live_state_sql(1),
            "DELETE FROM \"runner_live_state\" WHERE \"runner_live_state\".\"runner_id\" IN ($1)"
        );
        assert_eq!(
            null_run_runners_sql(1),
            "UPDATE \"agent_run\" SET \"runner_id\" = NULL WHERE \"agent_run\".\"runner_id\" IN ($1)"
        );
        assert_eq!(
            null_run_pins_sql(1),
            "UPDATE \"agent_run\" SET \"pinned_runner_id\" = NULL WHERE \"agent_run\".\"pinned_runner_id\" IN ($1)"
        );
        assert_eq!(
            null_event_messages_sql(1),
            "UPDATE \"agent_chat_event\" SET \"message_id\" = NULL WHERE \"agent_chat_event\".\"message_id\" IN ($1)"
        );
        assert_eq!(
            delete_chat_messages_sql(1),
            "DELETE FROM \"agent_chat_message\" WHERE \"agent_chat_message\".\"id\" IN ($1)"
        );
        assert_eq!(
            delete_chat_sessions_sql(1),
            "DELETE FROM \"agent_chat_session\" WHERE \"agent_chat_session\".\"id\" IN ($1)"
        );
        assert_eq!(
            delete_runner_rows_sql(1),
            "DELETE FROM \"runner\" WHERE \"runner\".\"id\" IN ($1)"
        );
        assert_eq!(
            delete_runner_rows_sql(3),
            "DELETE FROM \"runner\" WHERE \"runner\".\"id\" IN ($1, $2, $3)"
        );
    }

    #[test]
    fn machine_collector_tail() {
        assert_eq!(
            DELETE_MACHINE_SESSIONS_SQL,
            "DELETE FROM \"machine_session\" WHERE \"machine_session\".\"dev_machine_id\" IN ($1)"
        );
        assert_eq!(
            NULL_RUNNER_MACHINES_SQL,
            "UPDATE \"runner\" SET \"dev_machine_id\" = NULL WHERE \"runner\".\"dev_machine_id\" IN ($1)"
        );
        assert_eq!(
            NULL_TOKEN_MACHINES_SQL,
            "UPDATE \"machine_token\" SET \"dev_machine_id\" = NULL WHERE \"machine_token\".\"dev_machine_id\" IN ($1)"
        );
        assert_eq!(
            DELETE_MACHINE_ROW_SQL,
            "DELETE FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" IN ($1)"
        );
    }

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
}
