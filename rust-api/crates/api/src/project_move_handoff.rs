//! Request-path D-12 project-move handoff entry (PIDASHCONV-785).
//!
//! Python fires `complete_project_move_handoff(run_id)` from several
//! post-commit positions: `Runner.revoke` (`models.py:653-674`), the
//! session reaper's `_drain_after_commit` (`session_service.py:277-290`)
//! and the run-lifecycle endpoints. The executor lives in jobs
//! ([`pidash_jobs::orchestration::handoff`], OVL-06 PIDASHCONV-1544);
//! this module only adapts the request state to its explicit deps.
//!
//! [`complete_project_move_handoff`] is the one callable entry.

use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;

/// Api-crate-owned Redis client: `None` when `REDIS_URL` is unset,
/// empty, or unparsable, mirroring `redis_instance()` returning `None`.
fn redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| pidash_db::redis::open_client(url).ok())
}

/// Redis-backed admission cache, unified in services (OVL-06):
/// re-exported here so the direct-create tail and the v1 work-item
/// actions keep their import path.
pub(crate) use pidash_services::dispatch::admission::RedisAdmissionCache;

/// `complete_project_move_handoff(run_id)` (`service.py:644-732`):
/// thin caller of the public jobs executor (OVL-06), which owns the
/// transaction over the moved [`HandoffStore`](pidash_jobs::orchestration::HandoffStore)
/// and the post-commit terminal pairs + pod drains. Isolated (the
/// terminal transition already committed): failures are `Err(())`
/// for the caller to log.
pub(crate) async fn complete_project_move_handoff(
    pool: &PgPool,
    state: &AppState,
    run_id: Uuid,
    terminal_created_by: Option<Uuid>,
) -> Result<Option<Uuid>, ()> {
    let settings = state.settings();
    let deps = pidash_jobs::orchestration::HandoffInlineDeps {
        cloud: settings.cloud_agent.clone(),
        managed: settings.managed_runner.clone(),
        redis: redis_client(state),
        runner: settings.runner.clone(),
        toolsets: state.toolsets().clone(),
        assistant_seam: state.assistant_seam().clone(),
    };
    let ports = crate::runner_runs::LivePorts::new(pool.clone(), state);
    pidash_jobs::orchestration::complete_project_move_handoff_inline(
        pool,
        &deps,
        run_id,
        |effect| {
            crate::runner_runs::run_endpoints::drain_lifecycle_effects(
                pool,
                &ports,
                vec![effect],
                terminal_created_by,
            )
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::EdgeHandle;

    const FIXTURE_HANDOFF: &str =
        include_str!("../../../fixtures/orchestration/fx06_creation/handoff.before_after.json");

    fn cases() -> serde_json::Value {
        let fixture: serde_json::Value =
            serde_json::from_str(FIXTURE_HANDOFF).expect("handoff fixture parses");
        assert_eq!(
            fixture["marker_key"], "_project_move_handoff",
            "the marker key the services driver reads"
        );
        fixture["cases"].clone()
    }

    /// The entry's `Ok(None)` is the fixture's `result: null`: every
    /// early-out case (unknown run, non-terminal source, no marker,
    /// moved again, no target pod) records no replacement.
    #[test]
    fn early_out_cases_record_null_result() {
        let cases = cases();
        for key in [
            "H7a_unknown_run_id",
            "H7b_source_not_terminal",
            "H7c_no_marker",
            "H4_moved_again_suppressed",
            "H6_no_target_pod",
        ] {
            let case = cases
                .get(key)
                .unwrap_or_else(|| panic!("fixture case {key}"));
            assert!(case["result"].is_null(), "{key}: {case}");
        }
        // H7a is the bare shape: nothing but the null result.
        assert_eq!(
            cases["H7a_unknown_run_id"],
            serde_json::json!({"result": null})
        );
    }

    /// The entry's `Ok(Some(id))` is the fixture's non-null `result`: the
    /// created, recorded, or winning active run.
    #[test]
    fn replacement_cases_record_a_run() {
        let cases = cases();
        for key in [
            "H2_happy_path",
            "H3_replacement_id_idempotent",
            "H5_active_run_wins",
            "H9_active_race_returns_existing",
        ] {
            let case = cases
                .get(key)
                .unwrap_or_else(|| panic!("fixture case {key}"));
            assert!(case["result"].is_string(), "{key}: {case}");
        }
    }

    // -- live scratch-DB test (env-gated) --------------------------------
    // Same convention as `runner_runs::runs`: unset DATABASE_URL (plain
    // `cargo test`) skips it.

    /// H7a through the real entry: an unknown run id reads no
    /// `work_item_id`, so the driver returns before any lock and the
    /// entry commits an empty transaction and drains nothing. One
    /// connection, so the temp table is visible to the handoff tx.
    #[tokio::test]
    async fn live_unknown_run_is_the_no_handoff_early_out() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        sqlx::query("CREATE TEMPORARY TABLE agent_run (id UUID PRIMARY KEY, work_item_id UUID)")
            .execute(&pool)
            .await
            .expect("temp agent_run");
        let state = AppState::with_edge("0.1.0", EdgeHandle::for_tests("http://127.0.0.1:1"));
        let outcome = complete_project_move_handoff(&pool, &state, Uuid::new_v4(), None).await;
        assert_eq!(outcome, Ok(None));
        // A run with no work item is the same early-out (`row.0` is NULL).
        let orphan = Uuid::new_v4();
        sqlx::query("INSERT INTO agent_run (id, work_item_id) VALUES ($1, NULL)")
            .bind(orphan)
            .execute(&pool)
            .await
            .expect("insert orphan run");
        let outcome = complete_project_move_handoff(&pool, &state, orphan, None).await;
        assert_eq!(outcome, Ok(None));
    }
}
