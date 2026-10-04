#![forbid(unsafe_code)]

//! Runner session services (D-14, stage 5).
//!
//! * [`guards`] — matcher consts + legacy/dispatch-gate queries
//!   (`runner/services/matcher.py:44-81` consts, `:338-465`
//!   legacy/guards; PIDASHCONV-555).
//! * [`pubsub`] — send/close/revoke/remove verbs
//!   (`runner/services/pubsub.py:34-176`; PIDASHCONV-553).
//! * [`drain`] — matcher drain: pod + runner dispatch SQL, assignment
//!   plans and post-commit effects (`runner/services/matcher.py:89-303`;
//!   PIDASHCONV-552).
//! * [`session_service`] — hello apply, heartbeat reaper, live-state
//!   upsert, presence marks, project-slug resolve, session-open
//!   redeliver + resume ack
//!   (`runner/services/session_service.py:33-508`; PIDASHCONV-556).
//! * [`usage`] — `normalize_usage` re-exported from D-15 L1
//!   (`runner/services/usage.py:129-148`; PIDASHCONV-556).
//!
//! This crate has no database handle, so the [`guards`], [`drain`]
//! and [`session_service`] entry points are pure: status-set consts,
//! SQL text in Django shape (quoted identifiers, `%s` params rendered
//! as Postgres `$N`), branch predicates over caller-fetched facts,
//! update plans and post-commit effect descriptors. The executing
//! layer is the session handlers (PIDASHCONV-557/558/559) via
//! `pidash_db::tx`.
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
//! [`session_service`] replays FX-RSES-06
//! (`fx-rses-06-session-service.json`); [`usage`] replays its
//! `normalize_usage` section.

pub mod drain;
pub mod guards;
pub mod pubsub;
pub mod session_service;
pub mod usage;

pub use drain::{
    drain_for_runner_log, drain_pod_log, is_desktop_provisioning, next_for_runner_sql,
    plan_assignment, AssignmentFacts, AssignmentPlan, DrainEffect, ASSIGN_RUN_UPDATE_SQL,
    DRAIN_FOR_RUNNER_BY_ID_LOOKUP_SQL, DRAIN_FOR_RUNNER_LOCK_SQL, DRAIN_POD_BY_ID_LOOKUP_SQL,
    DRAIN_POD_IDLE_RUNNERS_SQL, NEXT_FOR_RUNNER_RANK_POSITION, NEXT_QUEUED_RUN_FOR_POD_SQL,
    POD_COLUMNS, SELECT_RUNNER_IN_POD_SQL,
};
pub use pubsub::{
    close_runner_session, send_connection_revoke, send_runner_remove, send_runner_revoke,
    send_to_machine, send_to_runner, PubsubStore, SendOutcome, CLOSE_ACTIVE_SESSIONS_SQL,
    CLOSE_RUNNER_SESSION_DEFAULT_CODE, FORCE_CLOSE_REASON,
};
pub use session_service::{
    agent_capabilities, cancel_barrier_update_sql, cancel_retry_error_line, cancel_retry_message,
    drain_pod_ids, effective_cutoff, invalid_run_id_warning, live_state_update_sql,
    merge_dev_metadata, parse_heartbeat_ts, parse_in_flight_id, parse_optional_uuid, parse_skip_id,
    plan_drain_after_commit, plan_hello_update, plan_live_state_upsert, plan_reap_finalize,
    plan_redeliver, plan_resume_ack, reap_error_detail, reapable_statuses, redeliver_assign_sql,
    redeliver_cancel_sql, resolve_runner_slug_sql, resolve_slug, resume_ack_lookup_sql,
    should_schedule_cancel_retry, should_schedule_drain, stale_cancel_ids_sql,
    stale_cancel_pod_ids_sql, stale_pairs_sql, HeartbeatError, HelloFacts, HelloPlan,
    LiveStateFacts, LiveStatePlan, RedeliverAssignFacts, RedeliverFacts, ResumeAckFacts,
    SessionEffect, SlugFacts, ASSIGN_DELIVERY_GRACE_SECS, CANCEL_REQUESTED_EXISTS_SQL,
    HELLO_UPDATE_NO_CAPABILITIES_SQL, HELLO_UPDATE_SQL, LIVE_STATE_COLUMNS, LIVE_STATE_INSERT_SQL,
    LIVE_STATE_SELECT_SQL, LLM_MODEL_MAX_CHARS, MARK_RUNNER_OFFLINE_SQL, MARK_RUNNER_ONLINE_SQL,
    OFFLINE_GRACE_SECS, REAP_ERROR_CODE, RESUME_ACK_LAST_SEQ_SQL, SNAPSHOT_FIELDS,
};
pub use usage::{coerce_token, normalize_usage, BIGINT_MAX, CANONICAL_USAGE_KEYS};
