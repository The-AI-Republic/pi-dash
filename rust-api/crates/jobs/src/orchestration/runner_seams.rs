#![forbid(unsafe_code)]

//! Live D-12 seam executors for the jobs `RunnerRunsSeams` provider
//! (PIDASHCONV-862).
//!
//! The bin owns the `RunnerRunsSeams` provider but cannot build the
//! jobs [`StoreDeps`] inputs: the api `AdmissionCache` impl is
//! `pub(crate)` and the `H`/`E`/`P` closures have no production
//! factory. This module closes the gap with public executors over
//! (`pool`, settings, `run_id`) that build their own inputs per call:
//!
//! * [`complete_project_move_handoff_live`]: prefetches the source
//!   run's creator presence, builds [`StoreDeps`] over the shared
//!   [`RedisAdmissionCache`][crate::admission::RedisAdmissionCache],
//!   and runs the jobs handoff driver.
//! * [`maybe_apply_deferred_pause_live`]: same shape for the
//!   deferred cap-hit pause (the pause path never reads presence,
//!   so the map stays empty).
//! * [`disarm_on_terminal_signal`]: loads the run / ticker / issue
//!   rows, runs the pure clock planner
//!   ([`maybe_disarm_on_terminal_signal`][clock::maybe_disarm_on_terminal_signal]),
//!   and executes the write — the executor the planner's docs call
//!   for.
//!
//! Admission counts against the shared Redis buckets (PIDASHCONV-1487),
//! the same buckets the HTTP plane and the Django worker use. Presence
//! reads `assistant_user_llm_config` the way the assistant gate reads it.
//!
//! Post-commit terminal effects the drivers return pending are
//! enqueued into `rust_job_queue` here (the D-15 pipeline entry),
//! and the pause's collected log lines emit via `tracing` — so the
//! bin provider maps each executor to `()` with no further work.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::assistant::models::has_secret;
use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings, RunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus;
use pidash_db::tasks_ticker::jitter_seconds;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction;
use pidash_services::assistant::seams::{agent_model_profile_for_user, has_usable_llm_config};
use pidash_services::dispatch::LlmProfile;
#[cfg(test)]
use pidash_services::extensions::NoExtraToolsets;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::orchestration::clock::{self as clock, ClockIssue, ClockWrite, RunEndedRef};
use pidash_services::runner_sessions::live_drains::{drain_pod_by_id, DrainError};
use pidash_types::orchestration::{StateRef, TickerEventKind};

use super::creation_jobs::StoreDeps;
use super::creation_jobs::{complete_project_move_handoff as drive_handoff, CreationJobsError};
use super::fire_tick_seam::{
    create_ticker_sql, emit_logs, lock_ticker_by_issue_sql, map_policy, map_ticker,
    maybe_apply_deferred_pause as drive_pause, save_clock_sql, DispatchJobsError,
};
use crate::admission::RedisAdmissionCache;
use crate::dispatch::DispatchDecision;
use crate::queue::enqueue;
use crate::runner_runs::sweeps_chat::terminal_effects_job;

// ---------------------------------------------------------------------------
// Presence prefetch (the 861 live-inputs shape, scoped to one run)
// ---------------------------------------------------------------------------

/// LLM-key presence for `user_ids`, read from
/// `assistant_user_llm_config` the way the assistant gate reads it.
/// Missing rows answer keyless.
async fn fetch_presence(
    pool: &PgPool,
    user_ids: &[Uuid],
) -> Result<HashMap<Uuid, bool>, sqlx::Error> {
    let mut presence: HashMap<Uuid, bool> = HashMap::with_capacity(user_ids.len());
    if user_ids.is_empty() {
        return Ok(presence);
    }
    let rows: Vec<(Uuid, Option<Vec<u8>>)> =
        sqlx::query_as(r#"SELECT "user_id", "api_key_encrypted" FROM "assistant_user_llm_config" WHERE "user_id" = ANY($1)"#)
            .bind(user_ids)
            .fetch_all(pool)
            .await?;
    for id in user_ids {
        presence.insert(*id, false);
    }
    for (id, key) in rows {
        presence.insert(id, has_secret(&key));
    }
    Ok(presence)
}

/// The `has_usable_llm_config` closure over pre-fetched presence. A
/// user outside the map answers keyless, the same as a missing
/// config row. Only the pause executor still uses this: the pause
/// path creates no runs, so its universe stays empty and the closure
/// is unread (identical under any seam).
fn has_key_fn(presence: Arc<HashMap<Uuid, bool>>) -> impl Fn(Uuid) -> bool + Clone + Send + Sync {
    move |uid: Uuid| -> bool { has_usable_llm_config(presence.get(&uid).copied().unwrap_or(false)) }
}

/// The `has_usable_llm_config` verdicts for the pre-fetched
/// universe: one installed-seam consult per user over the key bits
/// (`user_has_llm_config`, `agent_execution.py:69-80`). The CE default
/// answers the key bit; the overlay routes OpenHub/Codex/BYOK through
/// the same single switch point Pi Dash AI uses (PIDASHCONV-1051,
/// installed worker-side in PIDASHCONV-1054). The worker closures are
/// sync, so the async consult happens here at prefetch time and the
/// driver reads the verdict map.
async fn gate_verdicts(
    seam: &Arc<dyn AssistantModelSeam>,
    presence: &HashMap<Uuid, bool>,
) -> HashMap<Uuid, bool> {
    let mut verdicts = HashMap::with_capacity(presence.len());
    for (uid, has_key) in presence {
        verdicts.insert(*uid, seam.has_usable_llm_config(*uid, *has_key).await);
    }
    verdicts
}

/// The `has_usable_llm_config` closure over pre-fetched verdicts. A
/// user outside the pre-fetched universe answers closed, the same as
/// a missing config row.
fn gate_fn(verdicts: Arc<HashMap<Uuid, bool>>) -> impl Fn(Uuid) -> bool + Clone + Send + Sync {
    move |uid: Uuid| -> bool { verdicts.get(&uid).copied().unwrap_or(false) }
}

/// The `managed_llm_profile` closure over pre-fetched presence.
fn profile_fn(
    presence: Arc<HashMap<Uuid, bool>>,
) -> impl Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync {
    move |uid: Option<Uuid>| -> LlmProfile {
        let key = uid
            .and_then(|uid| presence.get(&uid).copied())
            .unwrap_or(false);
        let profile = agent_model_profile_for_user(key);
        LlmProfile {
            available: profile.available,
            reason_code: profile.reason_code,
        }
    }
}

/// The source run's creator (the handoff's `ExecutionRequest` actor).
/// A missing run narrows to `None` — the driver's own reads surface
/// the missing row as designed.
async fn run_creator(pool: &PgPool, run_id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(r#"SELECT "created_by_id" FROM "agent_run" WHERE "id" = $1"#)
        .bind(run_id)
        .fetch_optional(pool)
        .await
        .map(|row: Option<Uuid>| row)
}

/// Enqueue the drivers' pending terminal effects into `rust_job_queue`
/// (the D-15 pipeline entry): one `apply_agent_run_terminal_effects`
/// job per id, in the shared [`terminal_effects_job`] wire shape.
async fn drain_terminal_effects(
    pool: &PgPool,
    terminal_effects: &[Uuid],
) -> Result<(), sqlx::Error> {
    for run_id in terminal_effects {
        enqueue(pool, &terminal_effects_job(run_id)).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handoff + pause (StoreDeps built per call)
// ---------------------------------------------------------------------------

/// `complete_project_move_handoff(run_id)` with live inputs: the
/// source run's creator presence, the installed assistant seam's
/// llm-gate verdicts over it, the shared [`RedisAdmissionCache`],
/// and `now`. The shared Redis client and runner settings feed the
/// replacement run's pod drain. Answers the replacement run id
/// (`None` on every early-out: unknown run, non-terminal source, no
/// marker, moved again, no pod).
#[allow(clippy::too_many_arguments)]
pub async fn complete_project_move_handoff_live(
    pool: &PgPool,
    cloud: &CloudAgentSettings,
    managed: &ManagedRunnerSettings,
    redis: Option<&redis::Client>,
    runner: &RunnerSettings,
    run_id: Uuid,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn AssistantModelSeam>,
) -> Result<Option<Uuid>, CreationJobsError> {
    let actor = run_creator(pool, run_id).await?;
    let universe = actor.into_iter().collect::<Vec<_>>();
    let presence = Arc::new(fetch_presence(pool, &universe).await?);
    let verdicts = Arc::new(gate_verdicts(&assistant_seam, &presence).await);
    let deps = StoreDeps {
        cloud: cloud.clone(),
        managed: managed.clone(),
        cache: RedisAdmissionCache::from_env(),
        has_usable_llm_config: gate_fn(verdicts),
        toolsets,
        llm_profile: profile_fn(presence),
        now_unix_secs: Utc::now().timestamp(),
    };
    let outcome = drive_handoff(pool, deps, run_id, Utc::now()).await?;
    // The driver's `dispatch_agent_run` only *returns* the decision
    // (`dispatch.py:61-70` drains the pod inline), so a `PodDrain`
    // must be acted on here or the replacement stays QUEUED
    // (PIDASHCONV-1494). The target-pod drain lands before the
    // caller's source drains, as in `finalization.py`.
    for (_, decision) in &outcome.dispatch_decisions {
        if let DispatchDecision::PodDrain { pod_id } = decision {
            drain_pod_by_id(pool, redis, runner, *pod_id)
                .await
                .map_err(|error| match error {
                    DrainError::Db(db) => CreationJobsError::Db(db),
                    other => CreationJobsError::Db(sqlx::Error::Protocol(other.to_string())),
                })?;
        }
    }
    drain_terminal_effects(pool, &outcome.terminal_effects).await?;
    Ok(outcome.outcome.map(|run| run.id))
}

/// `maybe_apply_deferred_pause(run_id)` with live inputs. The pause
/// path creates no runs, so no presence is prefetched (the closures
/// are unread; an outside-map user would answer keyless anyway).
/// Answers whether the issue moved to Paused. The jitter draw is
/// unread on this path (`left_bucket` never retimes) — `0.0`.
pub async fn maybe_apply_deferred_pause_live(
    pool: &PgPool,
    cloud: &CloudAgentSettings,
    managed: &ManagedRunnerSettings,
    run_id: Uuid,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
) -> Result<bool, DispatchJobsError> {
    let presence: Arc<HashMap<Uuid, bool>> = Arc::new(HashMap::new());
    let deps = StoreDeps {
        cloud: cloud.clone(),
        managed: managed.clone(),
        cache: RedisAdmissionCache::from_env(),
        has_usable_llm_config: has_key_fn(Arc::clone(&presence)),
        toolsets,
        llm_profile: profile_fn(presence),
        now_unix_secs: Utc::now().timestamp(),
    };
    let outcome = drive_pause(pool, deps, Utc::now(), 0.0, run_id).await?;
    emit_logs(&outcome.outcome.logs);
    drain_terminal_effects(pool, &outcome.terminal_effects)
        .await
        .map_err(DispatchJobsError::Db)?;
    Ok(outcome.outcome.applied)
}

// ---------------------------------------------------------------------------
// Disarm (row loading + clock planner + writes)
// ---------------------------------------------------------------------------

/// Every failure [`disarm_on_terminal_signal`] reports.
#[derive(Debug, thiserror::Error)]
pub enum DisarmError {
    /// A database failure (`sqlx::Error`).
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    /// A row the planner cannot consume (unknown enum text).
    #[error("unparseable row value: {0}")]
    Decode(&'static str),
}

/// `maybe_disarm_on_terminal_signal(run_id)` (`scheduling.py:631-641`,
/// `3869a226`) with live row loading: send `RUN_ENDED` for the run;
/// `true` iff the clock was stopped (only an explicit
/// `done_payload.stop_ticking` stops it). A missing run, a run with no
/// work item, or a missing issue answers `false` without a transaction —
/// the planner's `(false, None)` shape.
pub async fn disarm_on_terminal_signal(pool: &PgPool, run_id: Uuid) -> Result<bool, DisarmError> {
    let run: Option<(Option<Uuid>, String, String, Option<Value>)> = sqlx::query_as(
        r#"SELECT "work_item_id", "status", "phase_kind", "done_payload" FROM "agent_run" WHERE "id" = $1"#,
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await?;
    let Some((work_item_id, status, phase_kind, done_payload)) = run else {
        return Ok(false);
    };
    let Some(work_item_id) = work_item_id else {
        return Ok(false);
    };
    let status =
        AgentRunStatus::from_value(&status).ok_or(DisarmError::Decode("agent_run.status"))?;
    let issue: Option<(Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as(r#"SELECT "state_id", "project_id" FROM "issues" WHERE "id" = $1"#)
            .bind(work_item_id)
            .fetch_optional(pool)
            .await?;
    let Some((state_id, project_id)) = issue else {
        return Ok(false);
    };
    let state: Option<(String, String)> = match state_id {
        Some(state_id) => {
            sqlx::query_as(r#"SELECT "name", "group" FROM "states" WHERE "id" = $1"#)
                .bind(state_id)
                .fetch_optional(pool)
                .await?
        }
        None => None,
    };
    let policy = match project_id {
        Some(project_id) => {
            let row: Option<sqlx::postgres::PgRow> = sqlx::query(
                r#"SELECT "agent_ticking_enabled", "agent_default_max_ticks", "agent_default_interval_seconds", "agent_review_default_interval_seconds", "agent_test_default_interval_seconds" FROM "projects" WHERE "id" = $1"#,
            )
            .bind(project_id)
            .fetch_optional(pool)
            .await?;
            match row {
                Some(row) => map_policy(&row).map_err(|_| DisarmError::Decode("project policy"))?,
                None => Default::default(),
            }
        }
        None => Default::default(),
    };
    let state_ref = state.as_ref().map(|(name, group)| StateRef {
        group: group.as_str(),
        name: name.as_str(),
    });
    let now = Utc::now();
    let interval = clock::effective_interval_seconds(state_ref.as_ref(), &policy);
    let jitter = jitter_seconds(interval, &mut rand08::thread_rng());
    let mut tx = Transaction::begin(pool).await?;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&lock_ticker_by_issue_sql())
        .bind(work_item_id)
        .fetch_optional(&mut **tx.inner())
        .await?;
    let mut ticker: Option<IssueAgentTicker> = row
        .map(|row| map_ticker(&row))
        .transpose()
        .map_err(|_| DisarmError::Decode("issue_agent_ticker"))?;
    let done_payload_ref = done_payload.as_ref().and_then(|payload| match payload {
        Value::Null => None,
        value => Some(value),
    });
    let (stopped, outcome) = clock::maybe_disarm_on_terminal_signal(
        &mut ticker,
        &ClockIssue {
            issue_id: work_item_id,
            state: state_ref,
            policy,
        },
        &RunEndedRef {
            status,
            phase_kind: &phase_kind,
            done_payload: done_payload_ref,
        },
        Some(work_item_id),
        now,
        jitter,
    );
    if let Some(outcome) = &outcome {
        match outcome.write {
            ClockWrite::None => {}
            ClockWrite::Update => {
                let ticker = ticker.as_ref().expect("update has a post-lock row");
                // SAVE_CLOCK_PARAMS order: updated_at, granted,
                // next_run_at, enabled, disarm_reason, pending_entry,
                // pending_entry_free, pending_entry_actor_id,
                // pending_entry_trigger, resume_parent_run_id, id.
                sqlx::query(&save_clock_sql())
                    .bind(now)
                    .bind(ticker.granted)
                    .bind(ticker.next_run_at)
                    .bind(ticker.enabled)
                    .bind(ticker.disarm_reason.as_str())
                    .bind(ticker.pending_entry)
                    .bind(ticker.pending_entry_free)
                    .bind(ticker.pending_entry_actor_id)
                    .bind(ticker.pending_entry_trigger.as_str())
                    .bind(ticker.resume_parent_run_id)
                    .bind(ticker.id)
                    .execute(&mut **tx.inner())
                    .await?;
            }
            ClockWrite::Insert => {
                // Unreachable for `RUN_ENDED` (it never creates), kept
                // so a future create-kind reuse cannot silently drop
                // the row.
                let ticker = ticker.as_ref().expect("insert has a post-lock row");
                sqlx::query(&create_ticker_sql())
                    .bind(ticker.created_at)
                    .bind(ticker.created_by_id)
                    .bind(ticker.id)
                    .bind(ticker.issue_id)
                    .bind(ticker.next_run_at)
                    .bind(ticker.enabled)
                    .bind(ticker.disarm_reason.as_str())
                    .bind(ticker.pending_entry)
                    .bind(ticker.pending_entry_free)
                    .bind(ticker.pending_entry_actor_id)
                    .bind(ticker.pending_entry_trigger.as_str())
                    .bind(ticker.resume_parent_run_id)
                    .execute(&mut **tx.inner())
                    .await?;
            }
        }
    }
    tx.commit().await?;
    if let Some(outcome) = &outcome {
        tracing::info!(
            "{}",
            clock::reconcile_log_line(&work_item_id, TickerEventKind::RUN_ENDED, &outcome.decision)
        );
    }
    Ok(stopped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse one statement with the Postgres dialect (the scan-suite
    /// precedent): new SQL must at least parse offline.
    fn parse(sql: &str) -> sqlparser::ast::Statement {
        sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
            .expect("SQL parses")
            .pop()
            .expect("one statement")
    }

    #[test]
    fn key_and_profile_closures_answer_keyless_outside_map() {
        let present = Uuid::from_u128(7);
        let presence: Arc<HashMap<Uuid, bool>> = Arc::new([(present, true)].into_iter().collect());
        let has_key = has_key_fn(Arc::clone(&presence));
        assert!(has_key(present));
        assert!(!has_key(Uuid::from_u128(8)));
        let profile = profile_fn(presence);
        let keyed = profile(Some(present));
        assert!(!keyed.available);
        assert!(!keyed.reason_code.is_empty());
        let keyless = profile(None);
        assert!(!keyless.available);
    }

    /// Overlay-shaped seam: a keyless user still counts as configured
    /// (the gateway lane, not BYOK).
    #[derive(Debug)]
    struct GatewaySeam;

    impl AssistantModelSeam for GatewaySeam {
        fn has_usable_llm_config(
            &self,
            user_id: Uuid,
            has_api_key: bool,
        ) -> pidash_services::extensions::SeamFuture<'_, bool> {
            let _ = (user_id, has_api_key);
            Box::pin(async move { true })
        }

        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("seam title".to_owned()) })
        }
    }

    /// The trait defaults with a canned title: the CE behaviour lives
    /// in the defaults (the api crate's `CeAssistantSeam` only adds
    /// the title transport), so this pins the default surface.
    #[derive(Debug)]
    struct DefaultSeam;

    impl AssistantModelSeam for DefaultSeam {
        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("canned".to_owned()) })
        }
    }

    #[tokio::test]
    async fn handoff_verdicts_use_the_installed_seam() {
        let seam: Arc<dyn AssistantModelSeam> = Arc::new(GatewaySeam);
        let keyless = Uuid::new_v4();
        let presence = HashMap::from([(keyless, false)]);
        let verdicts = gate_verdicts(&seam, &presence).await;
        assert_eq!(
            verdicts.get(&keyless),
            Some(&true),
            "an installed seam that accepts the user must open the gate without a key"
        );
        let gate = gate_fn(Arc::new(verdicts));
        assert!(gate(keyless));
        assert!(
            !gate(Uuid::new_v4()),
            "a user outside the universe still answers closed"
        );
    }

    #[tokio::test]
    async fn handoff_verdicts_keep_the_ce_default() {
        // No installed seam: the CE default answers each key bit
        // exactly like the pre-seam free function did.
        let seam: Arc<dyn AssistantModelSeam> = Arc::new(DefaultSeam);
        let keyed = Uuid::new_v4();
        let keyless = Uuid::new_v4();
        let presence = HashMap::from([(keyed, true), (keyless, false)]);
        let verdicts = gate_verdicts(&seam, &presence).await;
        assert_eq!(verdicts.get(&keyed), Some(&true));
        assert_eq!(verdicts.get(&keyless), Some(&false));
        let gate = gate_fn(Arc::new(verdicts));
        assert!(gate(keyed));
        assert!(!gate(keyless));
        assert!(!gate(Uuid::new_v4()));
    }

    #[test]
    fn seam_sql_parses() {
        for sql in [
            r#"SELECT "created_by_id" FROM "agent_run" WHERE "id" = $1"#,
            r#"SELECT "user_id", "api_key_encrypted" FROM "assistant_user_llm_config" WHERE "user_id" = ANY($1)"#,
            r#"SELECT "work_item_id", "status", "phase_kind", "done_payload" FROM "agent_run" WHERE "id" = $1"#,
            r#"SELECT "state_id", "project_id" FROM "issues" WHERE "id" = $1"#,
            r#"SELECT "name", "group" FROM "states" WHERE "id" = $1"#,
            r#"SELECT "agent_ticking_enabled", "agent_default_max_ticks", "agent_default_interval_seconds", "agent_review_default_interval_seconds", "agent_test_default_interval_seconds" FROM "projects" WHERE "id" = $1"#,
        ] {
            assert!(
                matches!(parse(sql), sqlparser::ast::Statement::Query(_)),
                "{sql}"
            );
        }
    }

    /// Error rendering the bin maps to the seam string.
    #[test]
    fn disarm_error_decode_renders_verbatim() {
        let error = DisarmError::Decode("agent_run.status");
        assert_eq!(error.to_string(), "unparseable row value: agent_run.status");
    }

    async fn pool() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("export DATABASE_URL for live seam tests");
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("pool connects")
    }

    fn settings() -> (CloudAgentSettings, ManagedRunnerSettings) {
        (
            CloudAgentSettings {
                enabled: false,
                writes_enabled: false,
                github_tools_enabled: true,
                disabled_tools: Vec::new(),
                reconcile_interval_secs: 30,
                model_request_timeout_secs: 60,
                execution_timeout_secs: 285,
                run_soft_limit_secs: 300,
                run_hard_limit_secs: 330,
                stale_grace_secs: 60,
                dispatch_lease_secs: 60,
                dispatch_backoff_secs: 10,
                dispatch_scan_interval_secs: 10,
                sweep_interval_secs: 30,
                dispatch_scan_batch: 100,
                max_queue_age_secs: 900,
                model_request_limit: 25,
                tool_call_limit: 20,
                write_call_limit: 3,
                input_token_limit: 144_000,
                output_token_limit: 16_000,
                total_token_limit: 160_000,
                max_output_tokens_per_request: 4096,
                max_queued_per_workspace: 20,
                max_running_per_workspace: 2,
                user_creation_rate_per_minute: 6,
                workspace_creation_rate_per_minute: 30,
                tool_timeout_secs: 20,
                max_tool_result_bytes: 65536,
                max_prompt_bytes: 262_144,
                max_final_result_bytes: 65536,
                max_events: 500,
                block_private_urls: true,
            },
            ManagedRunnerSettings {
                enabled: false,
                max_per_user_project: 1,
                queued_max_age_secs: 43200,
                graceful_stop_secs: 30,
                sweep_interval_secs: 300,
                desktop_min_version: String::new(),
            },
        )
    }

    /// Unknown run answers `Ok(None)` (the driver's unknown-run
    /// early-out), proving the live inputs build without a seed row.
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_handoff_unknown_run_is_none() {
        let pool = pool().await;
        let (cloud, managed) = settings();
        let runner = pidash_db::config::Settings::test_defaults().runner;
        let replacement = complete_project_move_handoff_live(
            &pool,
            &cloud,
            &managed,
            None,
            &runner,
            Uuid::new_v4(),
            Arc::new(NoExtraToolsets),
            Arc::new(DefaultSeam),
        )
        .await
        .expect("unknown run is Ok");
        assert!(replacement.is_none());
    }

    /// Unknown run answers `Ok(false)` (no work item, no pause).
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_pause_unknown_run_is_false() {
        let pool = pool().await;
        let (cloud, managed) = settings();
        let applied = maybe_apply_deferred_pause_live(
            &pool,
            &cloud,
            &managed,
            Uuid::new_v4(),
            Arc::new(NoExtraToolsets),
        )
        .await
        .expect("unknown run is Ok");
        assert!(!applied);
    }

    /// Unknown run answers `Ok(false)` with no transaction.
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_disarm_unknown_run_is_false() {
        let pool = pool().await;
        let stopped = disarm_on_terminal_signal(&pool, Uuid::new_v4())
            .await
            .expect("unknown run is Ok");
        assert!(!stopped);
    }
}
