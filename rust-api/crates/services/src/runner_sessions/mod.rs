#![forbid(unsafe_code)]

//! Runner session services (D-14, stage 5).
//!
//! * [`guards`] — matcher consts + legacy/dispatch-gate queries
//!   (`runner/services/matcher.py:44-81` consts, `:338-465`
//!   legacy/guards; PIDASHCONV-555).
//!
//! This crate has no database handle, so every entry point is pure:
//! status-set consts, SQL text in Django shape (quoted identifiers,
//! `%s` params rendered as Postgres `$N`), and branch predicates
//! over caller-fetched facts. The executing layer is the drain
//! sub-issue (PIDASHCONV-552, same module) and the session handlers
//! (PIDASHCONV-557/558/559).
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
