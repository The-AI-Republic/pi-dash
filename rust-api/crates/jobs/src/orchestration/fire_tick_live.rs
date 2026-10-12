#![forbid(unsafe_code)]

//! Live [`FireTickSeam`] for the worker-owned `fire_tick` (PIDASHCONV-861).
//!
//! [`FireTickShim`](super::fire_tick_seam::FireTickShim) cannot run live:
//! its `new_deps: Fn() -> StoreDeps` is sync, but presence
//! (`has_usable_llm_config`, `managed_llm_profile`) is only knowable
//! per fire. [`LiveFireTickSeam`] instead prefetches the creator
//! universe asynchronously on every dispatch (the HTTP path's
//! `dispatch_deps` shape), builds fresh [`StoreDeps`] over a
//! per-call cache, and runs the jobs continuation driver. Sync
//! methods resolve through the boot-loaded state map;
//! `has_active_run` / `has_prior_run` read live SQL.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::orchestration::runs::{active_run_sql, latest_prior_run_sql};
use pidash_services::extensions::CloudAgentToolsetsSeam;

use super::creation_jobs::StoreDeps;
use super::fire_tick_seam::{
    dispatch_continuation_run, emit_logs, shim_is_ticking_state, shim_tick_interval_seconds,
    DispatchJobsError, StateView,
};
use super::live_inputs::{
    creator_universe, fetch_presence, has_key_fn, profile_fn, CallCache, JobsMatcher,
};
use crate::tasks_ticker::fire_tick::{FireTickSeam, ProjectTickCols};

/// Live [`FireTickSeam`]: pool + dispatch settings + the boot-loaded
/// state map + the injected toolsets seam. One value serves every fire;
/// per-fire inputs (presence, cache, `now`) are built inside the async
/// methods.
pub struct LiveFireTickSeam {
    pool: PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    states: Arc<HashMap<Uuid, StateView>>,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
}

impl LiveFireTickSeam {
    pub fn new(
        pool: PgPool,
        cloud: CloudAgentSettings,
        managed: ManagedRunnerSettings,
        states: HashMap<Uuid, StateView>,
        toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    ) -> Self {
        Self {
            pool,
            cloud,
            managed,
            states: Arc::new(states),
            toolsets,
        }
    }

    /// Load the boot-time state cache.
    pub async fn load_states(pool: &PgPool) -> Result<HashMap<Uuid, StateView>, sqlx::Error> {
        super::live_inputs::load_states(pool).await
    }
}

impl FireTickSeam for LiveFireTickSeam {
    fn is_ticking_state(&self, state_id: Option<Uuid>) -> bool {
        let state = state_id.and_then(|id| self.states.get(&id));
        shim_is_ticking_state(state)
    }

    fn tick_interval_seconds(&self, state_id: Option<Uuid>, project: &ProjectTickCols) -> i64 {
        let state = state_id.and_then(|id| self.states.get(&id));
        shim_tick_interval_seconds(state, project)
    }

    fn has_active_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let sql = format!("SELECT EXISTS({})", active_run_sql());
            sqlx::query_scalar(&sql)
                .bind(issue_id)
                .fetch_one(&pool)
                .await
        })
    }

    fn has_prior_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let sql = format!("SELECT EXISTS({})", latest_prior_run_sql());
            sqlx::query_scalar(&sql)
                .bind(issue_id)
                .fetch_one(&pool)
                .await
        })
    }

    fn dispatch_continuation_run(
        &self,
        issue_id: Uuid,
        triggered_by: &str,
        actor: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Uuid>, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        let cloud = self.cloud.clone();
        let managed = self.managed.clone();
        let toolsets = self.toolsets.clone();
        let matcher = JobsMatcher::new(pool.clone());
        let triggered_by = triggered_by.to_owned();
        Box::pin(async move {
            let universe = creator_universe(&pool, actor, issue_id).await?;
            let presence = Arc::new(fetch_presence(&pool, &universe).await?);
            let deps = StoreDeps {
                cloud,
                managed,
                cache: CallCache::default(),
                has_usable_llm_config: has_key_fn(Arc::clone(&presence)),
                toolsets,
                llm_profile: profile_fn(presence),
                now_unix_secs: Utc::now().timestamp(),
            };
            let now = Utc::now();
            // Continuation never retimes a clock (a bounce only
            // disarms), so the jitter draw is unread — 0.0.
            let outcome = dispatch_continuation_run(
                &pool,
                deps,
                &matcher,
                now,
                0.0,
                issue_id,
                &triggered_by,
                actor,
            )
            .await
            .map_err(|error| match error {
                DispatchJobsError::Db(db) => db,
                other => sqlx::Error::Protocol(other.to_string()),
            })?;
            emit_logs(&outcome.outcome.logs);
            Ok(outcome.outcome.run_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::extensions::NoExtraToolsets;

    fn state(id: u128, name: &str, group: &str) -> StateView {
        StateView {
            id: Uuid::from_u128(id),
            name: name.to_owned(),
            group: group.to_owned(),
        }
    }

    fn seam() -> LiveFireTickSeam {
        let pool = PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let states = [
            state(1, "In Progress", "started"),
            state(2, "In Review", "review"),
            state(3, "Done", "completed"),
        ]
        .into_iter()
        .map(|state| (state.id, state))
        .collect();
        let settings = pidash_db::config::Settings::test_defaults();
        LiveFireTickSeam {
            pool,
            cloud: settings.cloud_agent,
            managed: settings.managed_runner,
            states: Arc::new(states),
            toolsets: Arc::new(NoExtraToolsets),
        }
    }

    #[tokio::test]
    async fn sync_methods_resolve_through_cached_states() {
        let seam = seam();
        assert!(seam.is_ticking_state(Some(Uuid::from_u128(1))));
        assert!(seam.is_ticking_state(Some(Uuid::from_u128(2))));
        assert!(!seam.is_ticking_state(Some(Uuid::from_u128(3))));
        assert!(!seam.is_ticking_state(Some(Uuid::from_u128(9))));
        assert!(!seam.is_ticking_state(None));
        let project = ProjectTickCols {
            pool: Some(10),
            ticking_enabled: Some(true),
            interval_default: Some(10800),
            interval_review: Some(7200),
            interval_test: Some(3600),
        };
        assert_eq!(
            seam.tick_interval_seconds(Some(Uuid::from_u128(1)), &project),
            10800
        );
        assert_eq!(
            seam.tick_interval_seconds(Some(Uuid::from_u128(2)), &project),
            7200
        );
        assert_eq!(
            seam.tick_interval_seconds(Some(Uuid::from_u128(9)), &project),
            10800
        );
    }

    #[test]
    fn run_probe_sql_parses() {
        for sql in [
            format!("SELECT EXISTS({})", active_run_sql()),
            format!("SELECT EXISTS({})", latest_prior_run_sql()),
        ] {
            let parsed = sqlparser::parser::Parser::parse_sql(
                &sqlparser::dialect::PostgreSqlDialect {},
                &sql,
            )
            .expect("SQL parses");
            assert_eq!(parsed.len(), 1, "{sql}");
        }
    }
}
