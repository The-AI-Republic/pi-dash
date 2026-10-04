#![forbid(unsafe_code)]

//! Runner session services (D-14, stage 5).
//!
//! * [`guards`] — matcher consts + legacy/dispatch-gate queries
//!   (`runner/services/matcher.py:44-81` consts, `:338-465`
//!   legacy/guards; PIDASHCONV-555).
//! * [`pubsub`] — send/close/revoke/remove verbs
//!   (`runner/services/pubsub.py:34-176`; PIDASHCONV-553).
//!
//! This crate has no database handle, so the [`guards`] entry points
//! are pure: status-set consts, SQL text in Django shape (quoted
//! identifiers, `%s` params rendered as Postgres `$N`), and branch
//! predicates over caller-fetched facts. The executing layer is the
//! drain sub-issue (PIDASHCONV-552, same module) and the session
//! handlers (PIDASHCONV-557/558/559).
//!
//! The [`pubsub`] verbs are async drivers over the
//! [`PubsubStore`](pubsub::PubsubStore) seam instead (the
//! `RunCreationStore` / `GitStore` precedent): the failure policy
//! lives here, the Redis + SQL effects land in the api/jobs crates
//! that implement the seam.
//!
//! Reused, never redefined: `AgentRunStatus`
//! (`pidash_types::runner_runs`), `AgentExecutorKind`
//! (`pidash_types::dispatch`), `effective_executor_for_issue`
//! (`crate::dispatch`), and the `Runner` status/provisioning
//! values + `MAX_PER_USER`
//! (`pidash_db::runner_enroll::columns`).
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-07-matcher.json`
//! (FX-RSES-07 `consts`, `select_runner_for_run_legacy`,
//! `count_active`, `pod_has_runner_for_issue_principal`,
//! `eligible_for_assignment`) plus the `Runner` column list in
//! FX-RSES-01 (`fk_contract_column_lists.Runner`). Each section is
//! replayed by the `#[cfg(test)]` suite beside the code.

pub mod guards;
pub mod pubsub;

pub use pubsub::{
    close_runner_session, send_connection_revoke, send_runner_remove, send_runner_revoke,
    send_to_machine, send_to_runner, PubsubStore, SendOutcome, CLOSE_ACTIVE_SESSIONS_SQL,
    CLOSE_RUNNER_SESSION_DEFAULT_CODE, FORCE_CLOSE_REASON,
};
