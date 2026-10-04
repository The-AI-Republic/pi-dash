#![forbid(unsafe_code)]

//! Runner pub/sub verbs: thin send/close/revoke/remove over the two outboxes.
//!
//! Port of `apps/api/pi_dash/runner/services/pubsub.py:34-176`:
//! [`send_to_runner`] (`:45-60`), [`send_to_machine`] (`:63-78`),
//! [`close_runner_session`] (`:81-107`), [`send_runner_revoke`]
//! (`:110-127`), [`send_runner_remove`] (`:130-171`) and the
//! [`send_connection_revoke`] back-compat alias (`:176`), plus the
//! [`runner_group`] legacy name helper (`:34-36`, re-exported from
//! types). `_ensure_envelope` (`:39-42`) is reused from types
//! ([`pidash_types::runner_sessions::ensure_envelope`]), never re-ported.
//!
//! # Layering: a seam, not direct calls
//!
//! This crate carries no `sqlx`/`redis`/`tracing` dependency, so the
//! verbs are generic over the [`PubsubStore`] seam (the
//! `RunCreationStore` / `GitStore` precedent): one method per
//! underlying effect, each naming the `db` call or SQL text the pool
//! implementation executes verbatim. Redis + SQL effects therefore
//! land in the api/jobs crates — D-12 orchestration, D-15 run
//! lifecycle, and the D-14 handlers (PIDASHCONV-557/558/559) consume
//! this module when they land — while the failure policy (which
//! errors propagate, which become log lines) lives here, exactly as
//! in Python.
//!
//! # Failure policy (verbatim from the `try`/`except` sites)
//!
//! | verb | offline error | any other error |
//! |---|---|---|
//! | `send_to_runner` | re-raised (matcher re-queues) | swallowed, one [`SendOutcome::warnings`] line |
//! | `send_to_machine` | propagated | propagated (`None` = not written) |
//! | `close_runner_session` | n/a | propagated (Python has no `try` here) |
//! | `send_runner_revoke` / `send_runner_remove` | swallowed, one warning line | swallowed, one warning line |
//!
//! This crate has no logger, so swallowed failures come back as
//! [`SendOutcome::warnings`] lines in the Python `logger` text (the
//! `SoftDeleteOutcome` precedent); the caller logs each line. Empty
//! `warnings` means every effect succeeded.
//!
//! # Fixtures
//!
//! The tests below replay
//! `rust-api/fixtures/runner_sessions/fx-rses-03-envelopes.json`
//! (FX-RSES-03: `pubsub_frames`, `send_to_machine_live`,
//! `close_runner_session`, `close_runner_session_noop`) against a
//! recording [`PubsubStore`]: same frames in the same order, same
//! swallow/propagate behaviour, same warning lines. The Redis argv
//! shapes those frames produce (`XADD`/`EXPIRE`/`DEL`/`PUBLISH`) are
//! pinned by the db crates' own trace replays (PIDASHCONV-550/551);
//! the eviction body bytes by the types crate (PIDASHCONV-547).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `close_runner_session` accepts `code` and ignores it
//!   (`pubsub.py:81` takes `code: int = 4010`; the body never reads
//!   it). The default survives as [`CLOSE_RUNNER_SESSION_DEFAULT_CODE`].
//! * `send_to_runner` swallows the unknown-type `ValueError` too: the
//!   `except Exception` exempts only `RunnerOfflineError`.
//! * The `revoke` / `remove_runner` offline arms are unreachable in
//!   practice (both types queue offline) but kept, defensively
//!   logging like Python.
//! * Revoke/remove frames carry no `mid`: `_serialize` inside the
//!   outbox mints it (fixture payloads show `mid` appended last).

use pidash_db::runner_sessions::machine_outbox::MachineOutboxError;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::runner_sessions::RunnerSession;
use pidash_types::runner_sessions::{ensure_envelope, remove_runner_frame, revoke_frame};
use serde_json::{Map, Value};
use uuid::Uuid;

/// Legacy Channels group name (`pubsub.py:34-36`), retained for the
/// upgrade-ticket WS: `runner.{rid}` (dot, not colon). Re-exported
/// from types (PIDASHCONV-547 owns the shape); this module keeps the
/// Python surface so callers import it from here as they did from
/// `pubsub`.
pub use pidash_types::runner_sessions::keys::runner::runner_group;

/// Default `code` for [`close_runner_session`] (`pubsub.py:81`).
/// Accepted and ignored, verbatim — the Python body never reads it.
pub const CLOSE_RUNNER_SESSION_DEFAULT_CODE: i32 = 4010;

/// `revoked_reason` stamped by [`close_runner_session`] (`pubsub.py:102`).
pub const FORCE_CLOSE_REASON: &str = "force_close";

/// Active-session list for [`close_runner_session`] (`pubsub.py:95-99`):
/// `filter(runner_id=…, revoked_at__isnull=True)` as a full list —
/// full-row projection, default `-created_at` ordering, no `LIMIT`
/// (unlike the open/delete/poll lookups). `$1` is the runner id. The
/// pool implementation executes this text verbatim and maps rows with
/// [`pidash_db::runner_sessions::models::runner_session::runner_session_from_row`].
pub const CLOSE_ACTIVE_SESSIONS_SQL: &str = "SELECT \"runner_session\".\"id\", \"runner_session\".\"runner_id\", \"runner_session\".\"protocol_version\", \"runner_session\".\"created_at\", \"runner_session\".\"last_seen_at\", \"runner_session\".\"revoked_at\", \"runner_session\".\"revoked_reason\" FROM \"runner_session\" WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $1) ORDER BY \"runner_session\".\"created_at\" DESC";

/// What a swallowing verb did with its failures.
///
/// Every swallowed failure appends one line in the Python `logger`
/// text (`logger.exception` lines carry `: {error}` with the
/// `Display`; `logger.warning` lines carry no error, like Python).
/// The caller logs each line (this crate has no `tracing`
/// dependency); empty `warnings` means every effect succeeded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendOutcome {
    /// Swallowed-failure lines, in effect order.
    pub warnings: Vec<String>,
}

/// Storage seam for the pub/sub verbs.
///
/// Methods mirror the `db` calls in `pubsub.py`, one per effect; the
/// pool implementation calls the named
/// [`pidash_db::runner_sessions`](pidash_db::runner_sessions) function
/// (or executes the named SQL text) verbatim. Uuids arrive typed; the
/// implementation stringifies where the `db` signature takes `&str`
/// (session ids, channel/consumer fragments).
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam (the `GitStore`
/// precedent).
#[allow(async_fn_in_trait)]
pub trait PubsubStore {
    /// `enqueue_for_runner` (`outbox.py:220-253`): live-stream id, or
    /// `None` when buffered offline / Redis is unavailable. The
    /// message arrives already enveloped ([`send_to_runner`] applies
    /// `ensure_envelope`; revoke/remove frames arrive mid-less, as in
    /// Python).
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError>;

    /// `enqueue_for_machine` (`machine_outbox.py:161-193`): same
    /// `Some(id)` / `None` contract. The message arrives enveloped.
    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, MachineOutboxError>;

    /// Active runner sessions, newest first ([`CLOSE_ACTIVE_SESSIONS_SQL`]).
    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<RunnerSession>, OutboxError>;

    /// One session-row revoke: `save(update_fields=["revoked_at",
    /// "revoked_reason"])` (`pubsub.py:101-103`). The implementation
    /// executes
    /// [`REVOKE_SQL`](pidash_db::runner_sessions::models::runner_session::REVOKE_SQL)
    /// binding `Utc::now()` per row (Python calls `timezone.now()`
    /// inside the loop) and `reason` as `$2`.
    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError>;

    /// `clear_session_marker` (`outbox.py:444-449`): delete the
    /// per-session PEL-drained marker.
    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError>;

    /// `publish_session_eviction` (`outbox.py:544-556`): eviction
    /// notice on the runner channel. `old_session_id` is always a
    /// real session here (Python passes `str(session.id)`);
    /// `new_session_id` is always `""` on the close path.
    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError>;
}

/// Best-effort enqueue of a control message for the runner
/// (`pubsub.py:45-60`).
///
/// The message is enveloped (`mid` added when the key is missing),
/// then routed through the outbox: live stream with an active
/// session, offline buffer without. Only [`OutboxError::RunnerOffline`]
/// propagates (the matcher re-queues); every other failure is
/// swallowed into [`SendOutcome::warnings`] — including the
/// unknown-type `ValueError`, whose `except` arm Python does not
/// exempt. The live-stream id is discarded (Python returns `None`).
pub async fn send_to_runner<S: PubsubStore>(
    store: &S,
    runner_id: Uuid,
    message: &Map<String, Value>,
) -> Result<SendOutcome, OutboxError> {
    let fresh_mid = Uuid::new_v4().to_string();
    let envelope = ensure_envelope(message, &fresh_mid);
    match store.enqueue_for_runner(runner_id, &envelope).await {
        Ok(_) => Ok(SendOutcome::default()),
        Err(error @ OutboxError::RunnerOffline { .. }) => Err(error),
        Err(error) => Ok(SendOutcome {
            warnings: vec![format!(
                "send_to_runner enqueue failed for {runner_id}: {error}"
            )],
        }),
    }
}

/// Enqueue a machine-scoped control message (`pubsub.py:63-78`).
///
/// Unlike [`send_to_runner`], delivery failures are NOT swallowed:
/// machine commands are user-initiated from the web UI, and a silent
/// drop would leave the operator polling a "pending" result to
/// timeout. Redis errors propagate; a `None` return means the message
/// was not written to the live stream (Redis unavailable, or buffered
/// offline — for offline-reject types nothing was delivered at all).
pub async fn send_to_machine<S: PubsubStore>(
    store: &S,
    dev_machine_id: Uuid,
    message: &Map<String, Value>,
) -> Result<Option<String>, MachineOutboxError> {
    let fresh_mid = Uuid::new_v4().to_string();
    let envelope = ensure_envelope(message, &fresh_mid);
    store.enqueue_for_machine(dev_machine_id, &envelope).await
}

/// Tell the cloud to evict any active session for this runner
/// (`pubsub.py:81-107`): session-row revoke + Redis eviction signal
/// per active session.
///
/// `code` is accepted and ignored, verbatim (callers pass
/// [`CLOSE_RUNNER_SESSION_DEFAULT_CODE`]); per session, in order:
/// revoke the row ([`FORCE_CLOSE_REASON`]), clear the PEL marker,
/// publish the eviction with `new_sid ""`. No `try` in Python, so the
/// first failure aborts the loop and propagates — earlier sessions in
/// the list stay fully processed.
pub async fn close_runner_session<S: PubsubStore>(
    store: &S,
    runner_id: Uuid,
    code: i32,
) -> Result<(), OutboxError> {
    let _ = code;
    let sessions = store.active_runner_sessions(runner_id).await?;
    for session in &sessions {
        store
            .revoke_runner_session(session.id, FORCE_CLOSE_REASON)
            .await?;
        store.clear_session_marker(session.id).await?;
        store
            .publish_session_eviction(runner_id, session.id, "")
            .await?;
    }
    Ok(())
}

/// Enqueue a `revoke` control frame for the runner
/// (`pubsub.py:110-127`). Callers pass
/// [`REVOKE_DEFAULT_REASON`](pidash_types::runner_sessions::REVOKE_DEFAULT_REASON)
/// for the Python default.
///
/// `revoke` is in the offline-allowed set, so the offline arm cannot
/// fire in practice — it is kept, logging defensively like Python.
/// Every failure is swallowed into [`SendOutcome::warnings`]; the
/// frame carries no `mid` (the outbox mints it).
pub async fn send_runner_revoke<S: PubsubStore>(
    store: &S,
    runner_id: Uuid,
    reason: &str,
) -> SendOutcome {
    let frame = revoke_frame(reason);
    match store.enqueue_for_runner(runner_id, &frame).await {
        Ok(_) => SendOutcome::default(),
        Err(OutboxError::RunnerOffline { .. }) => SendOutcome {
            warnings: vec![format!(
                "revoke enqueue rejected as offline for {runner_id}"
            )],
        },
        Err(error) => SendOutcome {
            warnings: vec![format!(
                "send_runner_revoke failed for {runner_id}: {error}"
            )],
        },
    }
}

/// Enqueue a `remove_runner` control frame for the runner
/// (`pubsub.py:130-171`): the cascade-delete verb (the daemon cancels
/// the in-flight run, drops the runner from its live maps, deletes
/// the data dir and strips the `config.toml` block). Callers pass
/// [`REMOVE_RUNNER_DEFAULT_REASON`](pidash_types::runner_sessions::REMOVE_RUNNER_DEFAULT_REASON)
/// for the Python default. For a cloud-only delete that leaves the
/// local install in place, call [`send_runner_revoke`] instead.
///
/// Same swallow-everything policy as [`send_runner_revoke`], with the
/// offline arm kept defensively; the frame carries no `mid`.
pub async fn send_runner_remove<S: PubsubStore>(
    store: &S,
    runner_id: Uuid,
    reason: &str,
) -> SendOutcome {
    let frame = remove_runner_frame(&runner_id.to_string(), reason);
    match store.enqueue_for_runner(runner_id, &frame).await {
        Ok(_) => SendOutcome::default(),
        Err(OutboxError::RunnerOffline { .. }) => SendOutcome {
            warnings: vec![format!(
                "remove_runner enqueue rejected as offline for {runner_id}"
            )],
        },
        Err(error) => SendOutcome {
            warnings: vec![format!(
                "send_runner_remove failed for {runner_id}: {error}"
            )],
        },
    }
}

/// Backwards-compatibility alias for [`send_runner_revoke`]
/// (`pubsub.py:176`). New code calls `send_runner_revoke` directly.
pub async fn send_connection_revoke<S: PubsubStore>(
    store: &S,
    runner_id: Uuid,
    reason: &str,
) -> SendOutcome {
    send_runner_revoke(store, runner_id, reason).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-03-envelopes.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn session(id: Uuid, runner_id: Uuid) -> RunnerSession {
        let at = chrono::DateTime::from_timestamp(0, 0).expect("epoch");
        RunnerSession {
            id,
            runner_id,
            protocol_version: 4,
            created_at: at,
            last_seen_at: Some(at),
            revoked_at: None,
            revoked_reason: String::new(),
        }
    }

    /// Render a message the way the call log records it: compact JSON,
    /// insertion order (the `preserve_order` Map keeps frame key order).
    fn logged(message: &Map<String, Value>) -> String {
        Value::Object(message.clone()).to_string()
    }

    /// Replace the single `'...'::uuid` literal in fixture SQL with `$1`
    /// (the db `dollarize_param` shape, local: that helper is
    /// `pub(crate)` to `pidash-db`).
    fn dollarize_param(sql: &str) -> String {
        let start = sql.find('\'').expect("fixture SQL carries a literal");
        let tail = &sql[start..];
        let end = tail.find("::uuid").expect("uuid-cast literal") + "::uuid".len();
        format!("{}${}{}", &sql[..start], 1, &sql[start + end..])
    }

    /// Every `'...'` span in fixture SQL, without quotes.
    fn quoted_spans(sql: &str) -> Vec<&str> {
        let mut spans = Vec::new();
        let mut rest = sql;
        while let Some(start) = rest.find('\'') {
            rest = &rest[start + 1..];
            let Some(end) = rest.find('\'') else {
                break;
            };
            spans.push(&rest[..end]);
            rest = &rest[end + 1..];
        }
        spans
    }

    /// Configurable [`PubsubStore`] recording every call (the
    /// `FakeStore` precedent: `RefCell` log, `&self` seam).
    struct FakeStore {
        sessions: Vec<RunnerSession>,
        runner_script: RefCell<VecDeque<Result<Option<String>, OutboxError>>>,
        machine_script: RefCell<VecDeque<Result<Option<String>, MachineOutboxError>>>,
        fail_list_with: RefCell<Option<OutboxError>>,
        fail_revoke_with: RefCell<Option<OutboxError>>,
        /// 1-based revoke call on which `fail_revoke_with` fires (0 = never).
        fail_revoke_on_call: usize,
        revoke_calls: RefCell<usize>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeStore {
        fn new(sessions: Vec<RunnerSession>) -> Self {
            Self {
                sessions,
                runner_script: RefCell::new(VecDeque::new()),
                machine_script: RefCell::new(VecDeque::new()),
                fail_list_with: RefCell::new(None),
                fail_revoke_with: RefCell::new(None),
                fail_revoke_on_call: 0,
                revoke_calls: RefCell::new(0),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl PubsubStore for FakeStore {
        async fn enqueue_for_runner(
            &self,
            runner_id: Uuid,
            message: &Map<String, Value>,
        ) -> Result<Option<String>, OutboxError> {
            self.calls.borrow_mut().push(format!(
                "enqueue_runner rid={runner_id} msg={}",
                logged(message)
            ));
            self.runner_script
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(None))
        }

        async fn enqueue_for_machine(
            &self,
            dev_machine_id: Uuid,
            message: &Map<String, Value>,
        ) -> Result<Option<String>, MachineOutboxError> {
            self.calls.borrow_mut().push(format!(
                "enqueue_machine mid={dev_machine_id} msg={}",
                logged(message)
            ));
            self.machine_script
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(None))
        }

        async fn active_runner_sessions(
            &self,
            runner_id: Uuid,
        ) -> Result<Vec<RunnerSession>, OutboxError> {
            self.calls
                .borrow_mut()
                .push(format!("list rid={runner_id}"));
            if let Some(error) = self.fail_list_with.borrow_mut().take() {
                return Err(error);
            }
            Ok(self.sessions.clone())
        }

        async fn revoke_runner_session(
            &self,
            session_id: Uuid,
            reason: &str,
        ) -> Result<(), OutboxError> {
            self.calls
                .borrow_mut()
                .push(format!("revoke sid={session_id} reason={reason}"));
            *self.revoke_calls.borrow_mut() += 1;
            if *self.revoke_calls.borrow() == self.fail_revoke_on_call
                && self.fail_revoke_on_call != 0
            {
                if let Some(error) = self.fail_revoke_with.borrow_mut().take() {
                    return Err(error);
                }
            }
            Ok(())
        }

        async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
            self.calls
                .borrow_mut()
                .push(format!("clear sid={session_id}"));
            Ok(())
        }

        async fn publish_session_eviction(
            &self,
            runner_id: Uuid,
            old_session_id: Uuid,
            new_session_id: &str,
        ) -> Result<(), OutboxError> {
            self.calls.borrow_mut().push(format!(
                "evict rid={runner_id} old={old_session_id} new={new_session_id}"
            ));
            Ok(())
        }
    }

    #[test]
    fn close_sql_replays_fixture_select() {
        let fx = fixture();
        let django = fx["close_runner_session"]["sql"][0]
            .as_str()
            .expect("close sql[0]");
        assert_eq!(dollarize_param(django), CLOSE_ACTIVE_SESSIONS_SQL);
        // Full-row projection, default ordering, no LIMIT: `list()`
        // fetches every active row (unlike the LIMIT 1 open lookup).
        assert!(!CLOSE_ACTIVE_SESSIONS_SQL.contains("LIMIT"));
        assert!(CLOSE_ACTIVE_SESSIONS_SQL.contains("ORDER BY"));
    }

    #[test]
    fn close_default_code_and_reason_match_python() {
        assert_eq!(CLOSE_RUNNER_SESSION_DEFAULT_CODE, 4010);
        // The UPDATE the fixture captured stamps this reason verbatim.
        let fx = fixture();
        let update = fx["close_runner_session"]["sql"][1]
            .as_str()
            .expect("close sql[1]");
        let spans = quoted_spans(update);
        assert_eq!(spans[1], FORCE_CLOSE_REASON);
    }

    #[tokio::test]
    async fn close_replays_fixture_row_marker_eviction() {
        // Ids straight out of the fixture SQL (uuid hex renders
        // dashless there; `Uuid` parses both spellings).
        let fx = fixture();
        let select = fx["close_runner_session"]["sql"][0].as_str().expect("sql");
        let update = fx["close_runner_session"]["sql"][1].as_str().expect("sql");
        let rid = Uuid::parse_str(quoted_spans(select)[0]).expect("runner uuid");
        let sid = Uuid::parse_str(quoted_spans(update)[2]).expect("session uuid");
        let store = FakeStore::new(vec![session(sid, rid)]);

        close_runner_session(&store, rid, CLOSE_RUNNER_SESSION_DEFAULT_CODE)
            .await
            .expect("close succeeds");
        assert_eq!(
            store.calls(),
            vec![
                format!("list rid={rid}"),
                format!("revoke sid={sid} reason=force_close"),
                format!("clear sid={sid}"),
                format!("evict rid={rid} old={sid} new="),
            ]
        );
        // The fixture's Redis trace shows exactly this per-row pair:
        // `DEL session_pel_drained:{sid}` then `PUBLISH
        // session_eviction:{rid} {"old_sid": …, "new_sid": ""}`.
        assert_eq!(
            fx["close_runner_session"]["row_after"]["reason"],
            "force_close"
        );
    }

    #[tokio::test]
    async fn close_ignores_code_and_orders_sessions() {
        let rid = uuid(1);
        let s1 = uuid(11);
        let s2 = uuid(12);
        for code in [CLOSE_RUNNER_SESSION_DEFAULT_CODE, 1008] {
            let store = FakeStore::new(vec![session(s1, rid), session(s2, rid)]);
            close_runner_session(&store, rid, code)
                .await
                .expect("close succeeds");
            // Revoke → clear → publish per session, in list order.
            assert_eq!(
                store.calls(),
                vec![
                    format!("list rid={rid}"),
                    format!("revoke sid={s1} reason=force_close"),
                    format!("clear sid={s1}"),
                    format!("evict rid={rid} old={s1} new="),
                    format!("revoke sid={s2} reason=force_close"),
                    format!("clear sid={s2}"),
                    format!("evict rid={rid} old={s2} new="),
                ],
                "code {code} behaves identically"
            );
        }
    }

    #[tokio::test]
    async fn close_noop_lists_only() {
        let rid = uuid(1);
        let store = FakeStore::new(Vec::new());
        close_runner_session(&store, rid, CLOSE_RUNNER_SESSION_DEFAULT_CODE)
            .await
            .expect("noop close succeeds");
        assert_eq!(store.calls(), vec![format!("list rid={rid}")]);
        assert!(fixture()["close_runner_session_noop"]["redis"]
            .as_array()
            .expect("noop redis")
            .is_empty());
    }

    #[tokio::test]
    async fn close_propagates_and_aborts_on_first_failure() {
        // Stand-in generic error: `OutboxError::Db`/`Redis` are
        // unconstructible here (this crate has neither dependency),
        // but `close` propagates every variant identically through
        // `?`, so any one proves the abort.
        let rid = uuid(1);
        let s1 = uuid(11);
        let s2 = uuid(12);
        let mut store = FakeStore::new(vec![session(s1, rid), session(s2, rid)]);
        store.fail_revoke_on_call = 2;
        *store.fail_revoke_with.borrow_mut() =
            Some(OutboxError::UnknownMessageType("stand-in".to_string()));
        let error = close_runner_session(&store, rid, CLOSE_RUNNER_SESSION_DEFAULT_CODE)
            .await
            .expect_err("revoke failure propagates");
        assert!(matches!(error, OutboxError::UnknownMessageType(_)));
        // First session fully processed, second attempted then aborted:
        // no clear/publish past the failure, matching the untried loop.
        assert_eq!(
            store.calls(),
            vec![
                format!("list rid={rid}"),
                format!("revoke sid={s1} reason=force_close"),
                format!("clear sid={s1}"),
                format!("evict rid={rid} old={s1} new="),
                format!("revoke sid={s2} reason=force_close"),
            ]
        );
    }

    #[tokio::test]
    async fn close_list_failure_propagates_with_no_effects() {
        let rid = uuid(1);
        let store = FakeStore::new(vec![session(uuid(11), rid)]);
        *store.fail_list_with.borrow_mut() =
            Some(OutboxError::UnknownMessageType("stand-in".to_string()));
        close_runner_session(&store, rid, CLOSE_RUNNER_SESSION_DEFAULT_CODE)
            .await
            .expect_err("list failure propagates");
        assert_eq!(store.calls(), vec![format!("list rid={rid}")]);
    }

    /// The recorded `msg={…}` JSON of the nth `enqueue_runner` call.
    fn runner_message(calls: &[String], n: usize) -> Value {
        let call = calls
            .iter()
            .filter(|c| c.starts_with("enqueue_runner"))
            .nth(n)
            .expect("nth enqueue");
        let json = call.split_once(" msg=").expect("msg part").1;
        serde_json::from_str(json).expect("message parses")
    }

    #[tokio::test]
    async fn send_to_runner_envelopes_mid() {
        let rid = uuid(1);
        let store = FakeStore::new(Vec::new());
        let mut message = Map::new();
        message.insert("type".to_string(), Value::String("cancel".to_string()));
        let outcome = send_to_runner(&store, rid, &message)
            .await
            .expect("send succeeds");
        assert!(outcome.warnings.is_empty());
        let sent = runner_message(&store.calls(), 0);
        assert_eq!(sent["type"], "cancel");
        let mid = sent["mid"].as_str().expect("mid added");
        let parsed = Uuid::parse_str(mid).expect("mid is a uuid");
        assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
        // Input untouched: the envelope is a copy (`dict(message)`).
        assert!(!message.contains_key("mid"));
    }

    #[tokio::test]
    async fn send_to_runner_keeps_existing_mid() {
        let rid = uuid(1);
        let store = FakeStore::new(Vec::new());
        let mut message = Map::new();
        message.insert("type".to_string(), Value::String("cancel".to_string()));
        message.insert("mid".to_string(), Value::String("keep-me".to_string()));
        send_to_runner(&store, rid, &message)
            .await
            .expect("send succeeds");
        let sent = runner_message(&store.calls(), 0);
        assert_eq!(sent["mid"], "keep-me");
        assert_eq!(
            fixture()["ensure_envelope"]["keeps_existing_mid"]["mid"],
            "keep-me"
        );
    }

    #[tokio::test]
    async fn send_to_runner_offline_propagates() {
        let rid = uuid(1);
        let store = FakeStore::new(Vec::new());
        store
            .runner_script
            .borrow_mut()
            .push_back(Err(OutboxError::RunnerOffline {
                runner_id: rid.to_string(),
                message_type: "assign".to_string(),
            }));
        let mut message = Map::new();
        message.insert("type".to_string(), Value::String("assign".to_string()));
        let error = send_to_runner(&store, rid, &message)
            .await
            .expect_err("offline re-raised");
        match error {
            OutboxError::RunnerOffline {
                runner_id,
                message_type,
            } => {
                assert_eq!(runner_id, rid.to_string());
                assert_eq!(message_type, "assign");
            }
            other => panic!("wrong variant: {other}"),
        }
        // The offline-assign probe in the fixture raises exactly this.
        assert!(fixture()["pubsub_frames"]["offline_assign_raises"]
            .as_str()
            .expect("probe")
            .starts_with("RunnerOfflineError:"));
    }

    #[tokio::test]
    async fn send_to_runner_swallows_generic_errors() {
        let rid = uuid(1);
        let store = FakeStore::new(Vec::new());
        store
            .runner_script
            .borrow_mut()
            .push_back(Err(OutboxError::UnknownMessageType("bogus".to_string())));
        let mut message = Map::new();
        message.insert("type".to_string(), Value::String("bogus".to_string()));
        let outcome = send_to_runner(&store, rid, &message)
            .await
            .expect("generic swallowed");
        assert_eq!(
            outcome.warnings,
            vec![format!(
                "send_to_runner enqueue failed for {rid}: unknown message type 'bogus'"
            )]
        );
        // The redis-None probe never raises either (the db layer's
        // `Ok(None)` path, also swallowed here by construction).
        assert!(
            fixture()["pubsub_frames"]["send_to_runner_redis_none_no_raise"]
                .as_bool()
                .expect("probe")
        );
    }

    #[tokio::test]
    async fn send_to_machine_passes_id_through() {
        let fx = fixture();
        let mid = uuid(2);
        let store = FakeStore::new(Vec::new());
        let live_id = fx["send_to_machine_live"]["returned"]
            .as_str()
            .expect("live id");
        store
            .machine_script
            .borrow_mut()
            .push_back(Ok(Some(live_id.to_string())));
        let mut message = Map::new();
        message.insert("type".to_string(), Value::String("ping".to_string()));
        let got = send_to_machine(&store, mid, &message)
            .await
            .expect("machine send ok");
        assert_eq!(got.as_deref(), Some(live_id));
        let call = &store.calls()[0];
        assert!(
            call.starts_with(&format!("enqueue_machine mid={mid} msg=")),
            "{call}"
        );
        let sent: Value =
            serde_json::from_str(call.split_once(" msg=").expect("msg").1).expect("parses");
        assert_eq!(sent["type"], "ping");
        Uuid::parse_str(sent["mid"].as_str().expect("mid")).expect("uuid mid");
    }

    #[tokio::test]
    async fn send_to_machine_none_means_not_written() {
        let mid = uuid(2);
        let store = FakeStore::new(Vec::new());
        let mut message = Map::new();
        message.insert("type".to_string(), Value::String("ping".to_string()));
        let got = send_to_machine(&store, mid, &message)
            .await
            .expect("none ok");
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn send_to_machine_propagates_everything() {
        let mid = uuid(2);
        let store = FakeStore::new(Vec::new());
        store
            .machine_script
            .borrow_mut()
            .push_back(Err(MachineOutboxError::MachineOffline {
                dev_machine_id: mid.to_string(),
                message_type: "create_runner".to_string(),
            }));
        let mut message = Map::new();
        message.insert(
            "type".to_string(),
            Value::String("create_runner".to_string()),
        );
        let error = send_to_machine(&store, mid, &message)
            .await
            .expect_err("offline propagates");
        assert_eq!(
            error.to_string(),
            format!("dev machine {mid} is offline; type 'create_runner' cannot queue")
        );
    }

    #[tokio::test]
    async fn pubsub_frames_replay_in_fixture_order() {
        // The four offline-buffer payloads the fixture recorded, in
        // order: revoke, revoke, remove_runner, revoke (alias). Each
        // payload is the post-`_serialize` form (`mid` appended last);
        // the verbs hand the pre-serialize frame to the store.
        let fx = fixture();
        let rid = Uuid::parse_str("7b1bf059-c24d-4a55-ae1a-20b8b05a160b").expect("rid");
        let store = FakeStore::new(Vec::new());
        let default_revoke = pidash_types::runner_sessions::REVOKE_DEFAULT_REASON;
        let default_remove = pidash_types::runner_sessions::REMOVE_RUNNER_DEFAULT_REASON;
        assert!(send_runner_revoke(&store, rid, default_revoke)
            .await
            .warnings
            .is_empty());
        assert!(send_runner_revoke(&store, rid, default_revoke)
            .await
            .warnings
            .is_empty());
        assert!(send_runner_remove(&store, rid, default_remove)
            .await
            .warnings
            .is_empty());
        assert!(send_connection_revoke(&store, rid, "alias-check")
            .await
            .warnings
            .is_empty());

        let payloads = fx["pubsub_frames"]["offline_buffer_payloads"]
            .as_array()
            .expect("payloads");
        assert_eq!(payloads.len(), 4);
        let calls = store.calls();
        for (n, entry) in payloads.iter().enumerate() {
            let mut payload: Map<String, Value> =
                serde_json::from_str(entry["payload"].as_str().expect("payload"))
                    .expect("payload parses");
            payload.shift_remove("mid");
            let sent = runner_message(&calls, n);
            assert_eq!(sent, Value::Object(payload), "frame {n}");
        }
        // Key order is part of the shape: type first, then reason
        // (revoke) / runner_id, reason (remove).
        assert_eq!(
            runner_message(&calls, 0).to_string(),
            r#"{"type":"revoke","reason":"runner revoked"}"#
        );
        assert_eq!(
            runner_message(&calls, 2).to_string(),
            format!(r#"{{"type":"remove_runner","runner_id":"{rid}","reason":"deleted by user"}}"#)
        );
    }

    #[tokio::test]
    async fn revoke_warns_on_offline_and_generic() {
        let rid = uuid(1);
        let store = FakeStore::new(Vec::new());
        store
            .runner_script
            .borrow_mut()
            .push_back(Err(OutboxError::RunnerOffline {
                runner_id: rid.to_string(),
                message_type: "revoke".to_string(),
            }));
        store
            .runner_script
            .borrow_mut()
            .push_back(Err(OutboxError::UnknownMessageType("bogus".to_string())));
        let offline = send_runner_revoke(&store, rid, "runner revoked").await;
        assert_eq!(
            offline.warnings,
            vec![format!("revoke enqueue rejected as offline for {rid}")]
        );
        let generic = send_runner_revoke(&store, rid, "runner revoked").await;
        assert_eq!(
            generic.warnings,
            vec![format!(
                "send_runner_revoke failed for {rid}: unknown message type 'bogus'"
            )]
        );
    }

    #[tokio::test]
    async fn remove_warns_on_offline_and_generic() {
        let rid = uuid(1);
        let store = FakeStore::new(Vec::new());
        store
            .runner_script
            .borrow_mut()
            .push_back(Err(OutboxError::RunnerOffline {
                runner_id: rid.to_string(),
                message_type: "remove_runner".to_string(),
            }));
        store
            .runner_script
            .borrow_mut()
            .push_back(Err(OutboxError::UnknownMessageType("bogus".to_string())));
        let offline = send_runner_remove(&store, rid, "deleted by user").await;
        assert_eq!(
            offline.warnings,
            vec![format!(
                "remove_runner enqueue rejected as offline for {rid}"
            )]
        );
        let generic = send_runner_remove(&store, rid, "deleted by user").await;
        assert_eq!(
            generic.warnings,
            vec![format!(
                "send_runner_remove failed for {rid}: unknown message type 'bogus'"
            )]
        );
    }

    #[test]
    fn runner_group_matches_legacy_name() {
        assert_eq!(runner_group("abc"), "runner.abc");
    }
}
