//! The issue's agent runs through SeaORM: the latest run, the active run and
//! the run count that the detail, archive, intake and reads serializers
//! render (`issue.py:1384-1412`).
//!
//! This replaces the hand-written `LATEST/ACTIVE_AGENT_RUN_SQL` join, whose
//! projection carried two `llm_model` columns. sqlx resolves a duplicated
//! output name to the LAST column, so by-name reads got the runner's live
//! model instead of the run's, and positional reads went stale whenever a
//! column was added or dropped. Here the run, the runner's name and the live
//! state come back as separate typed values, so they cannot be confused.

use pidash_db::entity::{agent_run, runner, runner_live_state};
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, DerivePartialModel, EntityTrait, FromQueryResult,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::Value;
use uuid::Uuid;

use super::serializers_detail::ACTIVE_RUN_STATUSES;

/// The `agent_run` columns the serializers render (no prompt or config
/// payloads).
#[derive(Debug, Clone, PartialEq, DerivePartialModel, FromQueryResult)]
#[sea_orm(entity = "agent_run::Entity")]
pub struct AgentRunSummary {
    pub id: Uuid,
    pub status: String,
    pub executor_kind: String,
    pub runner_id: Option<Uuid>,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub assigned_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub started_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub ended_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub done_payload: Option<Value>,
    pub error_details: Value,
    pub llm_model: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

impl AgentRunSummary {
    /// `error_details->>'message'`, or `''` (`COALESCE(.., '')`).
    pub fn error(&self) -> String {
        details_text(&self.error_details, "message")
    }

    /// `error_details->>'code'`, or `''`.
    pub fn error_code(&self) -> String {
        details_text(&self.error_details, "code")
    }
}

/// One run with what the old `LEFT JOIN runner ... LEFT JOIN
/// runner_live_state` carried: the runner's name, and its live state when
/// the runner has one.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRunWithRunner {
    pub run: AgentRunSummary,
    pub runner_name: Option<String>,
    pub live: Option<runner_live_state::Model>,
}

/// The issue's newest run, any status.
pub async fn latest_agent_run(
    db: &DatabaseConnection,
    issue_id: Uuid,
) -> Result<Option<AgentRunWithRunner>, DbErr> {
    newest_run(db, issue_id, false).await
}

/// The issue's newest run in an [`ACTIVE_RUN_STATUSES`] status.
pub async fn active_agent_run(
    db: &DatabaseConnection,
    issue_id: Uuid,
) -> Result<Option<AgentRunWithRunner>, DbErr> {
    newest_run(db, issue_id, true).await
}

/// How many runs the issue has.
pub async fn agent_run_count(db: &DatabaseConnection, issue_id: Uuid) -> Result<i64, DbErr> {
    let count = agent_run::Entity::find()
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
        .count(db)
        .await
        .inspect_err(|error| tracing::error!(%error, %issue_id, "agent run count failed"))?;
    i64::try_from(count).map_err(|_| DbErr::Custom("agent run count overflows i64".to_owned()))
}

async fn newest_run(
    db: &DatabaseConnection,
    issue_id: Uuid,
    active_only: bool,
) -> Result<Option<AgentRunWithRunner>, DbErr> {
    let mut query = agent_run::Entity::find().filter(agent_run::Column::WorkItemId.eq(issue_id));
    if active_only {
        query = query.filter(agent_run::Column::Status.is_in(ACTIVE_RUN_STATUSES));
    }
    let run = query
        .order_by_desc(agent_run::Column::CreatedAt)
        .into_partial_model::<AgentRunSummary>()
        .one(db)
        .await
        .inspect_err(|error| tracing::error!(%error, %issue_id, "agent run read failed"))?;
    let Some(run) = run else {
        return Ok(None);
    };
    let (runner_name, live) = match run.runner_id {
        Some(runner_id) => runner_and_live_state(db, runner_id).await?,
        None => (None, None),
    };
    Ok(Some(AgentRunWithRunner {
        run,
        runner_name,
        live,
    }))
}

/// The runner's name and live state. Both stay `None` when the runner row is
/// gone, as the outer join did.
async fn runner_and_live_state(
    db: &DatabaseConnection,
    runner_id: Uuid,
) -> Result<(Option<String>, Option<runner_live_state::Model>), DbErr> {
    let name: Option<String> = runner::Entity::find_by_id(runner_id)
        .select_only()
        .column(runner::Column::Name)
        .into_tuple()
        .one(db)
        .await
        .inspect_err(|error| tracing::error!(%error, %runner_id, "runner read failed"))?;
    if name.is_none() {
        return Ok((None, None));
    }
    let live = runner_live_state::Entity::find_by_id(runner_id)
        .one(db)
        .await
        .inspect_err(
            |error| tracing::error!(%error, %runner_id, "runner live state read failed"),
        )?;
    Ok((name, live))
}

/// Postgres `jsonb ->> key`: a string as itself, JSON null or a missing key
/// as `''` (the `COALESCE`), any other value as its JSON text.
fn details_text(details: &Value, key: &str) -> String {
    match details.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::test_support::{pool, seed_world};
    use serde_json::json;
    use sqlx::PgPool;

    async fn seed_pod(pool: &PgPool, workspace_id: Uuid, project_id: Uuid) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO pod (id, name, description, is_default, workspace_id, project_id, \
             created_at, updated_at) VALUES ($1, $2, '', false, $3, $4, now(), now())",
        )
        .bind(id)
        .bind(format!("pod-{}", id.simple()))
        .bind(workspace_id)
        .bind(project_id)
        .execute(pool)
        .await
        .expect("seed pod");
        id
    }

    async fn seed_runner(pool: &PgPool, workspace_id: Uuid, owner: Uuid, pod: Uuid) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO runner (id, created_at, updated_at, owner_id, workspace_id, pod_id, \
             name, host_label, provisioning, visibility, refresh_token_hash, \
             refresh_token_fingerprint, refresh_token_generation, \
             previous_refresh_token_hash, access_token_signing_key_version, \
             enrollment_token_hash, enrollment_token_fingerprint, capabilities, status, \
             os, arch, runner_version, dev_metadata, protocol_version, revoked_reason) \
             VALUES ($1, now(), now(), $2, $3, $4, 'macmini-7', '', 'desktop_bundled', 0, '', \
             '', 0, '', 1, '', '', '[]', 'online', '', '', '', '{}', 1, '')",
        )
        .bind(id)
        .bind(owner)
        .bind(workspace_id)
        .bind(pod)
        .execute(pool)
        .await
        .expect("seed runner");
        id
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_run(
        pool: &PgPool,
        world: &crate::read::test_support::World,
        pod: Uuid,
        runner: Option<Uuid>,
        status: &str,
        llm_model: &str,
        error_details: Value,
        age_secs: i64,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, runner_id, \
             work_item_id, status, executor_kind, dispatch_attempts, cancel_reason, tool_plan, \
             prompt, trigger, phase_kind, run_config, required_capabilities, thread_id, \
             agent_metadata, error_details, llm_model, usage, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'local_runner', 0, '', '{}', 'a long prompt', \
             'human', 'work', '{}', '{}', 't', '{}', $8, $9, '{}', \
             now() - make_interval(secs => $10))",
        )
        .bind(id)
        .bind(world.workspace_id)
        .bind(world.user_id)
        .bind(pod)
        .bind(runner)
        .bind(world.issue_id)
        .bind(status)
        .bind(error_details)
        .bind(llm_model)
        .bind(age_secs as f64)
        .execute(pool)
        .await
        .expect("seed run");
        id
    }

    /// The prod case behind the rustpidash 500: the run has a model, the
    /// runner's live state has none. The old by-name read returned the live
    /// NULL for the run's `llm_model` and failed.
    #[tokio::test]
    async fn live_reads_run_model_apart_from_null_live_model() {
        let Some(pool) = pool().await else {
            return;
        };
        let world = seed_world(&pool).await;
        let pod = seed_pod(&pool, world.workspace_id, world.project_id).await;
        let runner = seed_runner(&pool, world.workspace_id, world.user_id, pod).await;
        sqlx::query(
            "INSERT INTO runner_live_state (runner_id, updated_at, usage, llm_model) \
             VALUES ($1, now(), '{}', NULL)",
        )
        .bind(runner)
        .execute(&pool)
        .await
        .expect("seed live state");
        let older = seed_run(
            &pool,
            &world,
            pod,
            Some(runner),
            "running",
            "older-model",
            json!({}),
            60,
        )
        .await;
        let newest = seed_run(
            &pool,
            &world,
            pod,
            Some(runner),
            "completed",
            "deepseek/deepseek-v4.1-flash",
            json!({"message": "boom", "code": "E42"}),
            0,
        )
        .await;
        let db = pidash_db::pool::sea_orm_connection(&pool);

        let latest = latest_agent_run(&db, world.issue_id)
            .await
            .expect("latest")
            .expect("has a run");
        assert_eq!(latest.run.id, newest);
        assert_eq!(latest.run.llm_model, "deepseek/deepseek-v4.1-flash");
        assert_eq!(latest.run.error(), "boom");
        assert_eq!(latest.run.error_code(), "E42");
        assert_eq!(latest.runner_name.as_deref(), Some("macmini-7"));
        let live = latest.live.expect("runner has live state");
        assert_eq!(live.llm_model, None);

        let active = active_agent_run(&db, world.issue_id)
            .await
            .expect("active")
            .expect("the older run is running");
        assert_eq!(active.run.id, older);
        assert_eq!(active.run.error(), "");

        assert_eq!(
            agent_run_count(&db, world.issue_id).await.expect("count"),
            2
        );
    }

    /// A runner-less run carries no runner name and no live state; an issue
    /// without runs reads as `None` and a zero count.
    #[tokio::test]
    async fn live_runner_less_run_and_empty_issue() {
        let Some(pool) = pool().await else {
            return;
        };
        let world = seed_world(&pool).await;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        assert!(latest_agent_run(&db, world.issue_id)
            .await
            .expect("latest")
            .is_none());
        assert_eq!(
            agent_run_count(&db, world.issue_id).await.expect("count"),
            0
        );

        let pod = seed_pod(&pool, world.workspace_id, world.project_id).await;
        seed_run(&pool, &world, pod, None, "queued", "m", json!({}), 0).await;
        let latest = latest_agent_run(&db, world.issue_id)
            .await
            .expect("latest")
            .expect("has a run");
        assert_eq!(latest.runner_name, None);
        assert_eq!(latest.live, None);
        assert!(active_agent_run(&db, world.issue_id)
            .await
            .expect("active")
            .is_some());
    }

    #[test]
    fn details_text_follows_jsonb_text_extraction() {
        let details = json!({"message": "boom", "code": "E1", "n": 3, "nil": null});
        assert_eq!(details_text(&details, "message"), "boom");
        assert_eq!(details_text(&details, "code"), "E1");
        assert_eq!(details_text(&details, "n"), "3");
        assert_eq!(details_text(&details, "nil"), "");
        assert_eq!(details_text(&details, "absent"), "");
        assert_eq!(details_text(&json!({}), "message"), "");
    }
}
