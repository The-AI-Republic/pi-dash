//! The issue's agent runs through SeaORM: the latest run, the active run and
//! the run count that the detail, archive, intake and reads serializers
//! render (`issue.py:1384-1412`).
//!
//! This replaces the hand-written `LATEST/ACTIVE_AGENT_RUN_SQL` join, whose
//! projection carried two `llm_model` columns. sqlx resolves a duplicated
//! output name to the LAST column, so by-name reads got the runner's live
//! model instead of the run's, and positional reads went stale whenever a
//! column was added or dropped. Here the statement is built from the
//! entities, every joined column has its own alias, and rows decode by name
//! into one typed struct (a unit test pins the aliases).

use pidash_db::entity::{agent_run, runner, runner_live_state};
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait, Select,
};
use serde_json::Value;
use uuid::Uuid;

use super::serializers_detail::{
    serialize_iso_datetime, AgentLiveStateRow, AgentRunDetailRow, ACTIVE_RUN_STATUSES,
};

type Moment = sea_orm::prelude::DateTimeWithTimeZone;

/// One row of [`run_query`]: the run's rendered columns, the runner's name
/// and the runner's live state (all `live_*` NULL when it has none). Only
/// the two `error_details` keys the serializers read leave the database.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
struct RunRow {
    id: Uuid,
    status: String,
    executor_kind: String,
    runner_id: Option<Uuid>,
    created_at: Moment,
    assigned_at: Option<Moment>,
    started_at: Option<Moment>,
    ended_at: Option<Moment>,
    done_payload: Option<Value>,
    error_message: Option<Value>,
    error_code: Option<Value>,
    llm_model: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    runner_name: Option<String>,
    live_runner_id: Option<Uuid>,
    live_observed_run_id: Option<Uuid>,
    live_last_event_at: Option<Moment>,
    live_last_event_kind: Option<String>,
    live_last_event_summary: Option<String>,
    live_agent_pid: Option<i32>,
    live_agent_subprocess_alive: Option<bool>,
    live_approvals_pending: Option<i32>,
    live_usage: Option<Value>,
    live_llm_model: Option<String>,
    live_turn_count: Option<i32>,
    live_agent_health: Option<String>,
    live_agent_last_activity_at: Option<Moment>,
    live_updated_at: Option<Moment>,
}

/// A run with its datetimes rendered as plain `isoformat` (`+00:00`, the
/// services convention) and ids as strings: the one owned shape every
/// caller borrows its [`AgentRunDetailRow`] from.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedAgentRun {
    pub id: String,
    pub status: String,
    pub executor_kind: String,
    pub runner_id: Option<String>,
    pub runner_name: Option<String>,
    pub created_at: String,
    pub assigned_at: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub done_payload: Option<Value>,
    pub error: String,
    pub error_code: String,
    pub llm_model: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub live: Option<RenderedLiveState>,
}

/// The runner's `runner_live_state` row, rendered like [`RenderedAgentRun`].
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLiveState {
    pub observed_run_id: Option<String>,
    pub last_event_at: Option<String>,
    pub last_event_kind: Option<String>,
    pub last_event_summary: Option<String>,
    pub agent_pid: Option<i32>,
    pub agent_subprocess_alive: Option<bool>,
    pub approvals_pending: Option<i32>,
    pub usage: Value,
    pub llm_model: Option<String>,
    pub turn_count: Option<i32>,
    pub agent_health: Option<String>,
    pub agent_last_activity_at: Option<String>,
    pub updated_at: String,
}

impl RenderedAgentRun {
    /// The services serializer view over this run.
    pub fn detail_row(&self) -> AgentRunDetailRow<'_> {
        AgentRunDetailRow {
            id: &self.id,
            status: &self.status,
            executor_kind: &self.executor_kind,
            runner_id: self.runner_id.as_deref(),
            runner_name: self.runner_name.as_deref(),
            created_at: &self.created_at,
            assigned_at: self.assigned_at.as_deref(),
            started_at: self.started_at.as_deref(),
            ended_at: self.ended_at.as_deref(),
            done_payload: self.done_payload.as_ref(),
            error: &self.error,
            error_code: &self.error_code,
            llm_model: &self.llm_model,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            total_tokens: self.total_tokens,
            live_state: self.live.as_ref().map(|live| AgentLiveStateRow {
                observed_run_id: live.observed_run_id.as_deref(),
                last_event_at: live.last_event_at.as_deref(),
                last_event_kind: live.last_event_kind.as_deref(),
                last_event_summary: live.last_event_summary.as_deref(),
                agent_pid: live.agent_pid,
                agent_subprocess_alive: live.agent_subprocess_alive,
                approvals_pending: live.approvals_pending,
                usage: &live.usage,
                llm_model: live.llm_model.as_deref(),
                turn_count: live.turn_count,
                agent_health: live.agent_health.as_deref(),
                agent_last_activity_at: live.agent_last_activity_at.as_deref(),
                updated_at: &live.updated_at,
            }),
        }
    }
}

/// The issue's newest run, its newest active run, and its run count.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueAgentRuns {
    pub latest: Option<RenderedAgentRun>,
    pub active: Option<RenderedAgentRun>,
    pub count: i64,
}

/// Latest run, active run and count for one issue, over the request's pool
/// (SeaORM wraps it; no new connection). The reads run one after another on
/// one connection at a time: an issue without runs costs one query; with
/// runs, the count follows, and the active read is skipped when the newest
/// run is itself active (it is then also the newest active run).
pub async fn issue_agent_runs(
    pool: &sqlx::PgPool,
    issue_id: Uuid,
) -> Result<IssueAgentRuns, DbErr> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let Some(latest) = newest_run(&db, issue_id, false).await? else {
        return Ok(IssueAgentRuns {
            latest: None,
            active: None,
            count: 0,
        });
    };
    let count = run_count(&db, issue_id).await?;
    let active = if ACTIVE_RUN_STATUSES.contains(&latest.status.as_str()) {
        Some(latest.clone())
    } else {
        newest_run(&db, issue_id, true).await?
    };
    Ok(IssueAgentRuns {
        latest: Some(latest),
        active,
        count,
    })
}

/// `runs.count()` (`issue.py:1412`): every run of the issue, any status.
async fn run_count(db: &DatabaseConnection, issue_id: Uuid) -> Result<i64, DbErr> {
    let count: Option<i64> = count_query(issue_id)
        .into_tuple()
        .one(db)
        .await
        .inspect_err(|error| tracing::error!(%error, %issue_id, "agent run count failed"))?;
    Ok(count.unwrap_or(0))
}

/// `SELECT COUNT("agent_run"."id") AS "n" ... WHERE work_item_id = $1`.
/// `id` is the primary key, so this is `COUNT(*)`.
fn count_query(issue_id: Uuid) -> Select<agent_run::Entity> {
    agent_run::Entity::find()
        .select_only()
        .column_as(agent_run::Column::Id.count(), "n")
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
}

async fn newest_run(
    db: &DatabaseConnection,
    issue_id: Uuid,
    active_only: bool,
) -> Result<Option<RenderedAgentRun>, DbErr> {
    let row = run_query(issue_id, active_only)
        .into_model::<RunRow>()
        .one(db)
        .await
        .inspect_err(|error| tracing::error!(%error, %issue_id, "agent run read failed"))?;
    Ok(row.map(render))
}

/// `runs.select_related("runner__live_state").order_by("-created_at")`
/// (`issue.py:1385`), narrowed to the active statuses for the active run
/// (`:1386-1402`). No tiebreak: Django emits none either.
fn run_query(issue_id: Uuid, active_only: bool) -> Select<agent_run::Entity> {
    let mut query = agent_run::Entity::find()
        .select_only()
        .columns([
            agent_run::Column::Id,
            agent_run::Column::Status,
            agent_run::Column::ExecutorKind,
            agent_run::Column::RunnerId,
            agent_run::Column::CreatedAt,
            agent_run::Column::AssignedAt,
            agent_run::Column::StartedAt,
            agent_run::Column::EndedAt,
            agent_run::Column::DonePayload,
            agent_run::Column::LlmModel,
            agent_run::Column::InputTokens,
            agent_run::Column::OutputTokens,
            agent_run::Column::TotalTokens,
        ])
        .column_as(error_details_key("message"), "error_message")
        .column_as(error_details_key("code"), "error_code")
        .column_as(runner::Column::Name, "runner_name")
        .column_as(runner_live_state::Column::RunnerId, "live_runner_id")
        .column_as(
            runner_live_state::Column::ObservedRunId,
            "live_observed_run_id",
        )
        .column_as(runner_live_state::Column::LastEventAt, "live_last_event_at")
        .column_as(
            runner_live_state::Column::LastEventKind,
            "live_last_event_kind",
        )
        .column_as(
            runner_live_state::Column::LastEventSummary,
            "live_last_event_summary",
        )
        .column_as(runner_live_state::Column::AgentPid, "live_agent_pid")
        .column_as(
            runner_live_state::Column::AgentSubprocessAlive,
            "live_agent_subprocess_alive",
        )
        .column_as(
            runner_live_state::Column::ApprovalsPending,
            "live_approvals_pending",
        )
        .column_as(runner_live_state::Column::Usage, "live_usage")
        .column_as(runner_live_state::Column::LlmModel, "live_llm_model")
        .column_as(runner_live_state::Column::TurnCount, "live_turn_count")
        .column_as(runner_live_state::Column::AgentHealth, "live_agent_health")
        .column_as(
            runner_live_state::Column::AgentLastActivityAt,
            "live_agent_last_activity_at",
        )
        .column_as(runner_live_state::Column::UpdatedAt, "live_updated_at")
        .join(JoinType::LeftJoin, agent_run::Relation::Runner.def())
        .join(JoinType::LeftJoin, runner::Relation::RunnerLiveState.def())
        .filter(agent_run::Column::WorkItemId.eq(issue_id));
    if active_only {
        query = query.filter(agent_run::Column::Status.is_in(ACTIVE_RUN_STATUSES));
    }
    query.order_by_desc(agent_run::Column::CreatedAt)
}

/// `agent_run.error_details -> key`: only the one value leaves the database.
fn error_details_key(key: &str) -> sea_orm::sea_query::SimpleExpr {
    Expr::col((agent_run::Entity, agent_run::Column::ErrorDetails)).get_json_field(key)
}

fn render(row: RunRow) -> RenderedAgentRun {
    let iso = |moment: Option<Moment>| moment.map(render_moment);
    let live = match (row.live_runner_id, row.live_updated_at) {
        (Some(_), Some(updated_at)) => Some(RenderedLiveState {
            observed_run_id: row.live_observed_run_id.map(|id| id.to_string()),
            last_event_at: iso(row.live_last_event_at),
            last_event_kind: row.live_last_event_kind,
            last_event_summary: row.live_last_event_summary,
            agent_pid: row.live_agent_pid,
            agent_subprocess_alive: row.live_agent_subprocess_alive,
            approvals_pending: row.live_approvals_pending,
            usage: row.live_usage.unwrap_or(Value::Null),
            llm_model: row.live_llm_model,
            turn_count: row.live_turn_count,
            agent_health: row.live_agent_health,
            agent_last_activity_at: iso(row.live_agent_last_activity_at),
            updated_at: render_moment(updated_at),
        }),
        _ => None,
    };
    RenderedAgentRun {
        id: row.id.to_string(),
        status: row.status,
        executor_kind: row.executor_kind,
        runner_id: row.runner_id.map(|id| id.to_string()),
        runner_name: row.runner_name,
        created_at: render_moment(row.created_at),
        assigned_at: iso(row.assigned_at),
        started_at: iso(row.started_at),
        ended_at: iso(row.ended_at),
        done_payload: row.done_payload,
        error: details_text(row.error_message.as_ref()),
        error_code: details_text(row.error_code.as_ref()),
        llm_model: row.llm_model,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        total_tokens: row.total_tokens,
        live,
    }
}

fn render_moment(moment: Moment) -> String {
    serialize_iso_datetime(moment.with_timezone(&chrono::Utc))
}

/// `AgentRun.error` / `.error_code` (`runner/models.py:1079-1092`):
/// `(error_details or {}).get(key) or ""`. A falsy value (missing, null,
/// false, 0, "", [], {}) is `""` and a string is itself, as in Django.
///
/// Tier 3 (rule 13): a truthy non-string value renders as its JSON text.
/// Django has no output to match there: the serializer passes `run.error` to
/// `classify_run_error`, whose `.strip()` raises on a non-string, so the
/// request 500s. Every writer stores strings.
fn details_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) if number.as_f64() == Some(0.0) => String::new(),
        Some(Value::Array(items)) if items.is_empty() => String::new(),
        Some(Value::Object(map)) if map.is_empty() => String::new(),
        Some(other) => other.to_string(),
    }
}

/// The output column names of a built statement's select list, in order.
#[cfg(test)]
fn unique_output_names(sql: &str) -> Vec<String> {
    let select = sql
        .strip_prefix("SELECT ")
        .and_then(|rest| rest.split(" FROM ").next())
        .expect("select list");
    let mut columns = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (at, ch) in select.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                columns.push(select[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    columns.push(select[start..].trim());
    columns
        .into_iter()
        .map(|column| {
            column
                .rsplit(" AS ")
                .next()
                .and_then(|name| name.rsplit('.').next())
                .expect("column name")
                .trim_matches('"')
                .to_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::test_support::{pool, seed_world};
    use sea_orm::{DbBackend, QueryTrait};
    use serde_json::json;
    use sqlx::PgPool;

    fn built(active_only: bool) -> String {
        run_query(Uuid::nil(), active_only)
            .build(DbBackend::Postgres)
            .to_string()
    }

    #[test]
    fn run_query_has_one_name_per_output_column() {
        for active_only in [false, true] {
            let mut names = unique_output_names(&built(active_only));
            assert_eq!(names.len(), 30, "{names:?}");
            names.sort();
            let total = names.len();
            names.dedup();
            assert_eq!(names.len(), total, "duplicate output column: {names:?}");
        }
    }

    #[test]
    fn run_query_pins_joins_filter_and_order() {
        let latest = built(false);
        assert!(
            latest.contains(
                r#"LEFT JOIN "runner" ON "agent_run"."runner_id" = "runner"."id" LEFT JOIN "runner_live_state" ON "runner"."id" = "runner_live_state"."runner_id""#
            ),
            "{latest}"
        );
        assert!(
            latest.contains(
                r#"WHERE "agent_run"."work_item_id" = '00000000-0000-0000-0000-000000000000'"#
            ),
            "{latest}"
        );
        assert!(
            latest.ends_with(r#"ORDER BY "agent_run"."created_at" DESC"#),
            "{latest}"
        );
        assert!(
            !latest.contains(r#""status" IN"#),
            "latest is unfiltered: {latest}"
        );
        assert!(
            latest.contains(r#""agent_run"."error_details" -> 'message' AS "error_message""#),
            "{latest}"
        );
        assert!(!latest.contains("prompt"), "{latest}");
        let active = built(true);
        for status in ACTIVE_RUN_STATUSES {
            assert!(
                active.contains(&format!("'{status}'")),
                "{status}: {active}"
            );
        }
    }

    #[test]
    fn count_query_counts_every_run_of_the_issue() {
        let sql = count_query(Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert_eq!(
            sql,
            r#"SELECT COUNT("agent_run"."id") AS "n" FROM "agent_run" WHERE "agent_run"."work_item_id" = '00000000-0000-0000-0000-000000000000'"#
        );
    }

    #[test]
    fn details_text_follows_the_django_property() {
        let text = |value: Value| details_text(Some(&value));
        assert_eq!(text(json!("boom")), "boom");
        assert_eq!(text(json!("")), "");
        assert_eq!(text(json!(null)), "");
        assert_eq!(text(json!(false)), "");
        assert_eq!(text(json!(0)), "");
        assert_eq!(text(json!([])), "");
        assert_eq!(text(json!({})), "");
        // Tier 3: Django 500s on these (classify_run_error .strip()).
        assert_eq!(text(json!(3)), "3");
        assert_eq!(text(json!(true)), "true");
        assert_eq!(text(json!({"detail": "x"})), r#"{"detail":"x"}"#);
        assert_eq!(details_text(None), "");
    }

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
             name, host_label, provisioning, pod_dispatch, visibility, refresh_token_hash, \
             refresh_token_fingerprint, refresh_token_generation, \
             previous_refresh_token_hash, access_token_signing_key_version, \
             enrollment_token_hash, enrollment_token_fingerprint, capabilities, status, \
             os, arch, runner_version, dev_metadata, protocol_version, revoked_reason) \
             VALUES ($1, now(), now(), $2, $3, $4, 'macmini-7', '', 'desktop_bundled', 'pinned_only', 0, '', \
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

    async fn seed_live_state(pool: &PgPool, runner: Uuid) {
        sqlx::query(
            "INSERT INTO runner_live_state (runner_id, updated_at, usage, llm_model) \
             VALUES ($1, now(), '{}', NULL)",
        )
        .bind(runner)
        .execute(pool)
        .await
        .expect("seed live state");
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
        seed_live_state(&pool, runner).await;
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
            json!({"message": "boom", "code": "E42", "trace": "x".repeat(4096)}),
            0,
        )
        .await;

        let runs = issue_agent_runs(&pool, world.issue_id).await.expect("runs");
        let latest = runs.latest.expect("has a run");
        assert_eq!(latest.id, newest.to_string());
        assert_eq!(latest.llm_model, "deepseek/deepseek-v4.1-flash");
        assert_eq!(latest.error, "boom");
        assert_eq!(latest.error_code, "E42");
        assert_eq!(latest.runner_name.as_deref(), Some("macmini-7"));
        assert_eq!(latest.live.expect("runner has live state").llm_model, None);
        let active = runs.active.expect("the older run is running");
        assert_eq!(active.id, older.to_string());
        assert_eq!(active.error, "");
        assert_eq!(runs.count, 2);
    }

    /// A newest run that is itself active is also the active run.
    #[tokio::test]
    async fn live_active_newest_run_is_the_active_run() {
        let Some(pool) = pool().await else {
            return;
        };
        let world = seed_world(&pool).await;
        let pod = seed_pod(&pool, world.workspace_id, world.project_id).await;
        seed_run(&pool, &world, pod, None, "completed", "m", json!({}), 60).await;
        let newest = seed_run(&pool, &world, pod, None, "running", "m", json!({}), 0).await;
        let runs = issue_agent_runs(&pool, world.issue_id).await.expect("runs");
        assert_eq!(
            runs.latest.as_ref().map(|run| run.id.clone()),
            Some(newest.to_string())
        );
        assert_eq!(runs.active, runs.latest);
        assert_eq!(runs.count, 2);
    }

    /// A runner-less run and a runner with no live-state row both carry no
    /// live state; an issue without runs reads as `None` and a zero count.
    #[tokio::test]
    async fn live_missing_runner_or_live_state_and_empty_issue() {
        let Some(pool) = pool().await else {
            return;
        };
        let world = seed_world(&pool).await;
        let empty = issue_agent_runs(&pool, world.issue_id).await.expect("runs");
        assert_eq!((empty.latest, empty.active, empty.count), (None, None, 0));

        let pod = seed_pod(&pool, world.workspace_id, world.project_id).await;
        seed_run(&pool, &world, pod, None, "queued", "m", json!({}), 10).await;
        let runs = issue_agent_runs(&pool, world.issue_id).await.expect("runs");
        let latest = runs.latest.expect("has a run");
        assert_eq!((latest.runner_name, latest.live), (None, None));

        let runner = seed_runner(&pool, world.workspace_id, world.user_id, pod).await;
        seed_run(
            &pool,
            &world,
            pod,
            Some(runner),
            "completed",
            "m",
            json!({}),
            0,
        )
        .await;
        let runs = issue_agent_runs(&pool, world.issue_id).await.expect("runs");
        let latest = runs.latest.expect("has a run");
        assert_eq!(latest.runner_name.as_deref(), Some("macmini-7"));
        assert_eq!(latest.live, None, "runner without a live-state row");
    }
}
