#![forbid(unsafe_code)]

//! Run lookups + done-signal ingest (D-12 L2, stage 5).
//!
//! Ports `orchestration/service.py:84-107` (`_active_run_for`,
//! `_latest_prior_run`) and `orchestration/done_signal.py:141-181`
//! (`ingest_into_run`):
//!
//! * [`active_run_for`] — newest run for the work item whose status is
//!   one of the 7 active values, or `None`.
//! * [`latest_prior_run`] — newest run for the work item, any status.
//! * [`ingest_into_run`] — parse the agent's terminal text with the L1
//!   parser and persist the outcome onto the run with a single `UPDATE`.
//!
//! Rows reuse the D-11 [`AgentRun`] read shape and [`AgentRunStatus`]
//! (`pidash_db::dispatch`, never re-ported): the lookup `SELECT`s
//! project [`READ_COLUMNS`] (22 columns) where Django selects the full
//! 41-column row — same row-selection semantics, narrower projection.
//! The fixture (`active_run.sql`) pins the 41-column Django text; the
//! tests pin our projection against [`READ_COLUMNS`] and the
//! `WHERE`/`ORDER BY`/`LIMIT` tails against the fixture.
//!
//! [`AgentRun`]: crate::dispatch::agent_run::AgentRun
//! [`AgentRunStatus`]: crate::dispatch::status::AgentRunStatus
//! [`READ_COLUMNS`]: crate::dispatch::agent_run::READ_COLUMNS
//!
//! Fixture: `rust-api/fixtures/orchestration/fx02_reads/` (FX-ORCH-02:
//! `active_run.sql`, `active_run.rows.json`, `latest_prior_run.sql`,
//! `ingest.before_after.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::Row;

use crate::dispatch::agent_run::{AgentRun, READ_COLUMNS};
use crate::dispatch::status::{AgentRunStatus, AgentRunTrigger};
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::orchestration::{parse, DoneSignal};

/// The 7 statuses that occupy the single-active-run slot
/// (`service.py:89-99`, fixture order).
pub const ACTIVE_STATUSES: &[AgentRunStatus; 7] = &[
    AgentRunStatus::Queued,
    AgentRunStatus::Assigned,
    AgentRunStatus::WaitingForWorktree,
    AgentRunStatus::Running,
    AgentRunStatus::CancelRequested,
    AgentRunStatus::AwaitingApproval,
    AgentRunStatus::AwaitingReauth,
];

/// `AgentRun.is_terminal` (`runner/models.py:1136-1145`) over the
/// dispatch status enum. The dispatch port deliberately left
/// `is_terminal`/`is_active` for the layer that needs them
/// (`dispatch/status.rs` docs); the ingest error path is that layer.
/// `PAUSED_AWAITING_INPUT` is not terminal — a follow-up run resumes it.
pub fn is_terminal_status(status: AgentRunStatus) -> bool {
    matches!(
        status,
        AgentRunStatus::Completed
            | AgentRunStatus::Failed
            | AgentRunStatus::Cancelled
            | AgentRunStatus::Blocked
            | AgentRunStatus::Refused
    )
}

/// `_active_run_for` (`service.py:84-103`): the dispatch projection of
/// the newest active run for the work item. The status set is a literal
/// `IN` list, as Django renders it; `$1` is the work-item id.
pub fn active_run_sql() -> String {
    let statuses: Vec<String> = ACTIVE_STATUSES
        .iter()
        .map(|status| format!("'{status}'"))
        .collect();
    format!(
        "SELECT {} FROM agent_run WHERE work_item_id = $1 AND status IN ({}) ORDER BY created_at DESC LIMIT 1",
        READ_COLUMNS.join(", "),
        statuses.join(", ")
    )
}

/// `_latest_prior_run` (`service.py:106-107`): the dispatch projection
/// of the newest run for the work item, any status. `$1` is the
/// work-item id.
pub fn latest_prior_run_sql() -> String {
    format!(
        "SELECT {} FROM agent_run WHERE work_item_id = $1 ORDER BY created_at DESC LIMIT 1",
        READ_COLUMNS.join(", ")
    )
}

fn decode_error(column: &str, value: &str) -> sqlx::Error {
    sqlx::Error::Decode(format!("unknown agent_run.{column} {value:?}").into())
}

/// Map one lookup row onto the dispatch read shape.
pub fn map_agent_run(row: &PgRow) -> Result<AgentRun, sqlx::Error> {
    let status: String = row.try_get("status")?;
    let status =
        AgentRunStatus::from_value(&status).ok_or_else(|| decode_error("status", &status))?;
    let trigger: String = row.try_get("trigger")?;
    let trigger =
        AgentRunTrigger::from_value(&trigger).ok_or_else(|| decode_error("trigger", &trigger))?;
    let executor_kind: String = row.try_get("executor_kind")?;
    let executor_kind = AgentExecutorKind::from_value(&executor_kind)
        .ok_or_else(|| decode_error("executor_kind", &executor_kind))?;
    Ok(AgentRun {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        created_by_id: row.try_get("created_by_id")?,
        pod_id: row.try_get("pod_id")?,
        pinned_runner_id: row.try_get("pinned_runner_id")?,
        work_item_id: row.try_get("work_item_id")?,
        scheduler_binding_id: row.try_get("scheduler_binding_id")?,
        status,
        executor_kind,
        dispatch_attempts: row.try_get("dispatch_attempts")?,
        cancel_requested_at: row.try_get("cancel_requested_at")?,
        cancel_reason: row.try_get("cancel_reason")?,
        error_code: row.try_get("error_code")?,
        tool_plan: row.try_get("tool_plan")?,
        prompt: row.try_get("prompt")?,
        trigger,
        lease_expires_at: row.try_get("lease_expires_at")?,
        started_at: row.try_get("started_at")?,
        llm_model: row.try_get("llm_model")?,
        usage: row.try_get("usage")?,
        done_payload: row.try_get("done_payload")?,
        error: row.try_get("error")?,
    })
}

/// Newest active run for the work item, or `None`
/// (`service.py:84-103`).
pub async fn active_run_for<'e, E>(
    ex: E,
    work_item_id: uuid::Uuid,
) -> Result<Option<AgentRun>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&active_run_sql())
        .bind(work_item_id)
        .fetch_optional(ex)
        .await?;
    row.map(|row| map_agent_run(&row)).transpose()
}

/// Newest run for the work item regardless of status, or `None`
/// (`service.py:106-107`).
pub async fn latest_prior_run<'e, E>(
    ex: E,
    work_item_id: uuid::Uuid,
) -> Result<Option<AgentRun>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&latest_prior_run_sql())
        .bind(work_item_id)
        .fetch_optional(ex)
        .await?;
    row.map(|row| map_agent_run(&row)).transpose()
}

/// State [`ingest_into_run`] re-reads: Python holds the run instance
/// in memory (`done_signal.py:141`), so it issues only the `UPDATE`;
/// the stateless port re-reads the two columns the decision needs.
pub const INGEST_SELECT_SQL: &str = "SELECT status, ended_at FROM agent_run WHERE id = $1";

/// The single ingest `UPDATE` (`done_signal.py:162+180`). `SET` order
/// follows Django's `_meta` field order per the fixture
/// (`status, done_payload, error, ended_at`); `$1..$4` are the new
/// values, `$5` the run id.
pub const INGEST_UPDATE_SQL: &str =
    "UPDATE agent_run SET status = $1, done_payload = $2, error = $3, ended_at = $4 WHERE id = $5";

/// The pure half of [`ingest_into_run`]: the parsed signal (if any)
/// plus the four columns the `UPDATE` writes.
#[derive(Debug, Clone, PartialEq)]
pub struct IngestPlan {
    pub signal: Option<DoneSignal>,
    pub status: AgentRunStatus,
    pub done_payload: Option<Value>,
    pub error: String,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Decide the ingest outcome without touching the database
/// (`done_signal.py:153-181`).
///
/// * Parse error: `error` records `done-signal parse error: {exc}`,
///   `done_payload` is cleared, `ended_at` is stamped, and the status
///   becomes `FAILED` unless the run is already terminal (kept).
/// * `completed`/`blocked`/`noop`: payload stored, `error` cleared,
///   `ended_at` stamped; `noop` lands the run in `COMPLETED`.
/// * `paused`: non-terminal — `ended_at` keeps its previous value
///   (`NULL` in practice) so observability does not treat the row as
///   finished.
pub fn plan_ingest(
    current_status: AgentRunStatus,
    current_ended_at: Option<chrono::DateTime<chrono::Utc>>,
    text: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> IngestPlan {
    let signal = match parse(text) {
        Err(err) => {
            return IngestPlan {
                signal: None,
                status: if is_terminal_status(current_status) {
                    current_status
                } else {
                    AgentRunStatus::Failed
                },
                done_payload: None,
                error: format!("done-signal parse error: {err}"),
                ended_at: Some(now),
            }
        }
        Ok(signal) => signal,
    };
    // The `elif` chain (`:165-179`): `parse` only returns the four
    // `VALID_STATUSES`, so the fall-through mirrors Python's implicit
    // no-`else` (status/ended_at untouched) and is unreachable.
    let (status, ended_at) = match signal.status.as_str() {
        "completed" => (AgentRunStatus::Completed, Some(now)),
        "blocked" => (AgentRunStatus::Blocked, Some(now)),
        "noop" => (AgentRunStatus::Completed, Some(now)),
        "paused" => (AgentRunStatus::PausedAwaitingInput, current_ended_at),
        _ => (current_status, current_ended_at),
    };
    IngestPlan {
        status,
        done_payload: Some(signal.payload.clone()),
        error: String::new(),
        ended_at,
        signal: Some(signal),
    }
}

/// Parse `text` and persist the normalized payload onto the run
/// (`done_signal.py:141-181`). Returns the parsed signal, or `None`
/// when the text had no usable fence (the run is then failed or
/// annotated per [`plan_ingest`]).
///
/// Takes `&mut PgConnection` (an acquired connection, or `&mut *tx`
/// inside a transaction): the re-read plus the write are two
/// statements on one session.
pub async fn ingest_into_run(
    conn: &mut sqlx::PgConnection,
    run_id: uuid::Uuid,
    text: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<DoneSignal>, sqlx::Error> {
    let row: Option<PgRow> = sqlx::query(INGEST_SELECT_SQL)
        .bind(run_id)
        .fetch_optional(&mut *conn)
        .await?;
    // Python's `save()` on a deleted row would fall back to an
    // `INSERT`; that path is unreachable from live callers, so a
    // missing row surfaces as `RowNotFound` instead of recreating it.
    let Some(row) = row else {
        return Err(sqlx::Error::RowNotFound);
    };
    let status: String = row.try_get("status")?;
    let current =
        AgentRunStatus::from_value(&status).ok_or_else(|| decode_error("status", &status))?;
    let ended_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("ended_at")?;
    let plan = plan_ingest(current, ended_at, text, now);
    sqlx::query(INGEST_UPDATE_SQL)
        .bind(plan.status.value())
        .bind(&plan.done_payload)
        .bind(&plan.error)
        .bind(plan.ended_at)
        .bind(run_id)
        .execute(&mut *conn)
        .await?;
    Ok(plan.signal)
}

#[cfg(test)]
mod tests {
    use super::*;

    static ACTIVE_RUN_SQL_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/active_run.sql");
    static LATEST_PRIOR_RUN_SQL_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/latest_prior_run.sql");
    static INGEST_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx02_reads/ingest.before_after.json");

    fn fixture_sql(raw: &str) -> String {
        let parsed: Value = serde_json::from_str(raw).expect("fixture parses");
        parsed["executed_sql"][0]["sql"]
            .as_str()
            .expect("executed_sql[0].sql")
            .to_string()
    }

    /// The `IN (...)` literal list from the fixture's `WHERE` clause.
    fn fixture_in_list(sql: &str) -> Vec<String> {
        let start = sql.find("IN (").expect("IN (") + 4;
        let end = sql[start..].find(')').expect(")") + start;
        sql[start..end]
            .split(", ")
            .map(|entry| entry.trim_matches('\'').to_string())
            .collect()
    }

    /// Row-selection tail (`WHERE` onward) reduced to predicate order +
    /// values: quotes, parens, whitespace and table prefixes stripped,
    /// the uuid literal / `$1` bind folded to `X`. The `IN` contents are
    /// pinned separately (order-sensitive) by
    /// `active_statuses_match_fixture_in_list_in_order`.
    fn squashed_tail(sql: &str, literal_style: LiteralStyle) -> String {
        let mut tail = sql[sql.find("WHERE").expect("WHERE")..].to_string();
        match literal_style {
            LiteralStyle::Fixture => {
                let start = tail.find('\'').expect("uuid literal");
                let end = tail.find("::uuid").expect("uuid cast") + "::uuid".len();
                tail.replace_range(start..end, "X");
            }
            LiteralStyle::Bind => {
                tail = tail.replace("$1", "X");
            }
        }
        tail.replace('"', "")
            .replace("agent_run.", "")
            .replace(['(', ')', ' '], "")
    }

    enum LiteralStyle {
        Fixture,
        Bind,
    }

    #[test]
    fn active_statuses_match_fixture_in_list_in_order() {
        let sql = fixture_sql(ACTIVE_RUN_SQL_FIXTURE);
        let expected = fixture_in_list(&sql);
        let actual: Vec<String> = ACTIVE_STATUSES
            .iter()
            .map(|status| status.value().to_string())
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 7);
    }

    #[test]
    fn active_run_sql_projects_dispatch_columns_with_fixture_tail() {
        let sql = active_run_sql();
        let columns: Vec<String> = READ_COLUMNS.iter().map(|name| name.to_string()).collect();
        assert_eq!(columns.len(), 22);
        assert!(sql.starts_with(&format!(
            "SELECT {} FROM agent_run WHERE ",
            columns.join(", ")
        )));
        assert!(sql.contains(&format!(
            "status IN ({})",
            ACTIVE_STATUSES
                .iter()
                .map(|status| format!("'{status}'"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
        assert!(sql.ends_with("ORDER BY created_at DESC LIMIT 1"));
        // Same row-selection tail as Django, modulo quoted idents and `$1`.
        let fixture = fixture_sql(ACTIVE_RUN_SQL_FIXTURE);
        assert_eq!(
            squashed_tail(&sql, LiteralStyle::Bind),
            squashed_tail(&fixture, LiteralStyle::Fixture)
        );
    }

    #[test]
    fn latest_prior_run_sql_matches_fixture_tail() {
        let sql = latest_prior_run_sql();
        assert!(sql.starts_with(&format!(
            "SELECT {} FROM agent_run WHERE ",
            READ_COLUMNS.join(", ")
        )));
        assert!(!sql.contains("status IN"));
        assert!(sql.ends_with("ORDER BY created_at DESC LIMIT 1"));
        let fixture = fixture_sql(LATEST_PRIOR_RUN_SQL_FIXTURE);
        assert_eq!(
            squashed_tail(&sql, LiteralStyle::Bind),
            squashed_tail(&fixture, LiteralStyle::Fixture)
        );
    }

    #[test]
    fn terminal_set_matches_runner_model() {
        // `runner/models.py:1137-1145` (terminal) and `:1146-1159`
        // (active); paused is in neither set.
        let terminal = [
            AgentRunStatus::Completed,
            AgentRunStatus::Failed,
            AgentRunStatus::Cancelled,
            AgentRunStatus::Blocked,
            AgentRunStatus::Refused,
        ];
        for status in terminal {
            assert!(is_terminal_status(status), "{status} is terminal");
        }
        for status in ACTIVE_STATUSES {
            assert!(!is_terminal_status(*status), "{status} is not terminal");
        }
        assert!(!is_terminal_status(AgentRunStatus::PausedAwaitingInput));
    }

    /// Paused runs keep their prior `ended_at` verbatim — the
    /// fixture only pins the `NULL` before-value, but Python writes
    /// back whatever the instance holds (`:175-180` never assigns
    /// `ended_at`).
    #[test]
    fn plan_ingest_paused_keeps_prior_ended_at() {
        let now = fixture_time(&serde_json::json!("2026-10-03T12:00:00+00:00")).expect("T0");
        let prior = fixture_time(&serde_json::json!("2026-10-02T08:00:00+00:00")).expect("prior");
        let plan = plan_ingest(
            AgentRunStatus::Running,
            Some(prior),
            "x\n```pi-dash-done\n{\"status\": \"paused\", \"autonomy\": {\"question_for_human\": \"q?\"}}\n```\n",
            now,
        );
        assert_eq!(plan.status, AgentRunStatus::PausedAwaitingInput);
        assert_eq!(plan.ended_at, Some(prior));
        assert_eq!(plan.error, "");
    }

    fn ingest_cases() -> Value {
        let parsed: Value = serde_json::from_str(INGEST_FIXTURE).expect("fixture parses");
        parsed["cases"].clone()
    }

    fn fixture_time(value: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
        value.as_str().map(|raw| {
            raw.parse::<chrono::DateTime<chrono::Utc>>()
                .expect("fixture timestamp parses")
        })
    }

    /// Every ingest case replays: same returned signal, same four
    /// written columns as the fixture's before/after rows.
    #[test]
    fn plan_ingest_replays_all_six_fixture_cases() {
        let now = fixture_time(&serde_json::json!("2026-10-03T12:00:00+00:00")).expect("T0");
        for case in ingest_cases().as_array().expect("cases") {
            let name = case["name"].as_str().expect("name");
            let before_status = AgentRunStatus::from_value(
                case["before"]["status"].as_str().expect("before.status"),
            )
            .expect("known status");
            let plan = plan_ingest(
                before_status,
                fixture_time(&case["before"]["ended_at"]),
                case["input_text"].as_str().expect("input_text"),
                now,
            );
            match &case["returned"] {
                Value::Null => assert_eq!(plan.signal, None, "{name}: no signal"),
                returned => {
                    let signal = plan.signal.as_ref().expect("{name}: signal");
                    assert_eq!(signal.status, returned["status"], "{name}: status");
                    assert_eq!(signal.payload, returned["payload"], "{name}: payload");
                }
            }
            assert_eq!(
                plan.status.value(),
                case["after"]["status"],
                "{name}: status"
            );
            let expected_payload = if case["after"]["done_payload"].is_null() {
                None
            } else {
                Some(case["after"]["done_payload"].clone())
            };
            assert_eq!(plan.done_payload, expected_payload, "{name}: done_payload");
            assert_eq!(
                plan.error,
                case["after"]["error"].as_str().expect("after.error"),
                "{name}: error"
            );
            assert_eq!(
                plan.ended_at,
                fixture_time(&case["after"]["ended_at"]),
                "{name}: ended_at"
            );
        }
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

    /// Scratch Postgres for the Done-when row replay. Unset (plain
    /// `cargo test`) skips these; CI sets no database either, so the
    /// suite stays green offline. Run with e.g.
    /// `export DATABASE_URL=postgresql://127.0.0.1:55432/pidash_554_scratch`
    /// for the real check.
    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    /// Temp `agent_run`: the 22 dispatch columns plus `ended_at` (the
    /// ingest re-read) and `created_at` (the ordering key).
    const LIVE_DDL: &str = "CREATE TEMPORARY TABLE agent_run (
        id UUID PRIMARY KEY, workspace_id UUID NOT NULL, created_by_id UUID NOT NULL,
        pod_id UUID NOT NULL, pinned_runner_id UUID, work_item_id UUID,
        scheduler_binding_id UUID, status TEXT NOT NULL, executor_kind TEXT NOT NULL,
        dispatch_attempts INTEGER NOT NULL, cancel_requested_at TIMESTAMPTZ,
        cancel_reason TEXT NOT NULL, error_code TEXT NOT NULL, tool_plan JSONB NOT NULL,
        prompt TEXT NOT NULL, trigger TEXT NOT NULL, lease_expires_at TIMESTAMPTZ,
        started_at TIMESTAMPTZ, llm_model TEXT NOT NULL, usage JSONB NOT NULL,
        done_payload JSONB, error TEXT NOT NULL, ended_at TIMESTAMPTZ,
        created_at TIMESTAMPTZ NOT NULL)";

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        sqlx::query(LIVE_DDL)
            .execute(&mut *tx)
            .await
            .expect("create temp agent_run");
        tx
    }

    fn live_uuid(n: u32) -> uuid::Uuid {
        uuid::Uuid::parse_str(&format!("11111111-2222-3333-4444-{n:012}")).expect("fixed uuid")
    }

    fn live_time(minute: u32) -> chrono::DateTime<chrono::Utc> {
        format!("2026-10-03T11:{minute:02}:00+00:00")
            .parse()
            .expect("fixed time")
    }

    /// Seed one run; returns its id.
    async fn seed_run(
        conn: &mut sqlx::PgConnection,
        tag: u32,
        work_item: uuid::Uuid,
        status: &str,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> uuid::Uuid {
        let id = live_uuid(tag);
        sqlx::query(
            "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, work_item_id,
             status, executor_kind, dispatch_attempts, cancel_reason, error_code, tool_plan,
             prompt, trigger, llm_model, usage, error, created_at)
             VALUES ($1, $2, $2, $2, $3, $4, 'local_runner', 0, '', '', '{}', '', 'direct',
             '', '{}', '', $5)",
        )
        .bind(id)
        .bind(live_uuid(9000 + tag))
        .bind(work_item)
        .bind(status)
        .bind(created_at)
        .execute(&mut *conn)
        .await
        .expect("seed run");
        id
    }

    /// `active_run.rows.json` replay: each of the 7 active statuses
    /// resolves on its own issue; the terminal-newest, paused-only and
    /// empty issues resolve per the fixture.
    #[tokio::test]
    async fn live_lookup_matrix_replays_fixture() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        for (index, status) in [
            "queued",
            "assigned",
            "waiting_for_worktree",
            "running",
            "cancel_requested",
            "awaiting_approval",
            "awaiting_reauth",
        ]
        .iter()
        .enumerate()
        {
            let issue = live_uuid(3100 + index as u32);
            let id = seed_run(
                &mut tx,
                4001 + index as u32,
                issue,
                status,
                live_time(50 + index as u32),
            )
            .await;
            let active = active_run_for(&mut *tx, issue)
                .await
                .expect("active lookup")
                .expect("one active run");
            assert_eq!(active.id, id);
            assert_eq!(active.status.value(), *status);
            let latest = latest_prior_run(&mut *tx, issue)
                .await
                .expect("latest lookup");
            assert_eq!(latest.map(|run| run.id), Some(id));
        }
        // Terminal-newest issue: active skips the newer terminal row.
        let term_issue = live_uuid(3110);
        seed_run(&mut tx, 4010, term_issue, "queued", live_time(10)).await;
        seed_run(&mut tx, 4011, term_issue, "failed", live_time(9)).await;
        let newest = seed_run(&mut tx, 4012, term_issue, "completed", live_time(30)).await;
        let active = active_run_for(&mut *tx, term_issue)
            .await
            .expect("active lookup");
        assert_eq!(active.map(|run| run.id), Some(live_uuid(4010)));
        let latest = latest_prior_run(&mut *tx, term_issue)
            .await
            .expect("latest lookup");
        assert_eq!(latest.map(|run| run.id), Some(newest));
        // Paused-only issue: no active run, latest is the paused row.
        let paused_issue = live_uuid(3120);
        let paused = seed_run(
            &mut tx,
            4013,
            paused_issue,
            "paused_awaiting_input",
            live_time(30),
        )
        .await;
        assert!(active_run_for(&mut *tx, paused_issue)
            .await
            .expect("active lookup")
            .is_none());
        let latest = latest_prior_run(&mut *tx, paused_issue)
            .await
            .expect("latest lookup");
        assert_eq!(latest.map(|run| run.id), Some(paused));
        // Empty issue: both lookups are `None`.
        let empty_issue = live_uuid(3130);
        assert!(active_run_for(&mut *tx, empty_issue)
            .await
            .expect("active lookup")
            .is_none());
        assert!(latest_prior_run(&mut *tx, empty_issue)
            .await
            .expect("latest lookup")
            .is_none());
    }

    /// Unknown stored enum values fail the mapping loudly instead of
    /// silently defaulting. (The unfiltered lookup reads them: the
    /// active lookup's `IN` list would simply not match.)
    #[tokio::test]
    async fn live_unknown_status_value_is_a_decode_error() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let issue = live_uuid(3200);
        seed_run(&mut tx, 4020, issue, "bogus", live_time(30)).await;
        let err = latest_prior_run(&mut *tx, issue)
            .await
            .expect_err("bogus status fails");
        assert!(matches!(err, sqlx::Error::Decode(_)), "got {err:?}");
        // Same for the other two mapped enums.
        let trigger_issue = live_uuid(3201);
        let trigger_run = seed_run(&mut tx, 4021, trigger_issue, "queued", live_time(31)).await;
        sqlx::query("UPDATE agent_run SET trigger = 'bogus' WHERE id = $1")
            .bind(trigger_run)
            .execute(&mut *tx)
            .await
            .expect("stage bogus trigger");
        let err = latest_prior_run(&mut *tx, trigger_issue)
            .await
            .expect_err("bogus trigger fails");
        assert!(matches!(err, sqlx::Error::Decode(_)), "got {err:?}");
        let kind_issue = live_uuid(3202);
        let kind_run = seed_run(&mut tx, 4022, kind_issue, "queued", live_time(32)).await;
        sqlx::query("UPDATE agent_run SET executor_kind = 'bogus' WHERE id = $1")
            .bind(kind_run)
            .execute(&mut *tx)
            .await
            .expect("stage bogus kind");
        let err = latest_prior_run(&mut *tx, kind_issue)
            .await
            .expect_err("bogus kind fails");
        assert!(matches!(err, sqlx::Error::Decode(_)), "got {err:?}");
    }

    /// `ingest.before_after.json` replay against live rows: completed
    /// (payload + cleared error + stamped end), paused (`ended_at`
    /// stays `NULL`), parse-error on a running row (`FAILED` + error
    /// text), parse-error on a terminal row (status kept).
    #[tokio::test]
    async fn live_ingest_roundtrip_replays_fixture() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let now = fixture_time(&serde_json::json!("2026-10-03T12:00:00+00:00")).expect("T0");
        let completed_text = "some agent chatter\n```pi-dash-done\n{\"status\": \"completed\", \"summary\": \"did the thing\"}\n```\n";
        let paused_text = "some agent chatter\n```pi-dash-done\n{\"status\": \"paused\", \"autonomy\": {\"question_for_human\": \"which approach?\"}}\n```\n";

        // Completed path.
        let issue = live_uuid(3300);
        let run = seed_run(&mut tx, 4030, issue, "running", live_time(30)).await;
        sqlx::query("UPDATE agent_run SET error = 'prior error' WHERE id = $1")
            .bind(run)
            .execute(&mut *tx)
            .await
            .expect("stage prior error");
        let signal = ingest_into_run(&mut tx, run, completed_text, now)
            .await
            .expect("ingest completed");
        assert_eq!(
            signal.map(|signal| signal.status),
            Some("completed".to_string())
        );
        let row: PgRow = sqlx::query(
            "SELECT status, done_payload, error, ended_at FROM agent_run WHERE id = $1",
        )
        .bind(run)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        let status: String = row.try_get("status").expect("status");
        let error: String = row.try_get("error").expect("error");
        let ended_at: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("ended_at").expect("ended_at");
        let payload: Value = row.try_get("done_payload").expect("payload");
        assert_eq!(status, "completed");
        assert_eq!(error, "");
        assert_eq!(ended_at, Some(now));
        assert_eq!(payload["status"], Value::String("completed".to_string()));
        assert_eq!(
            payload["summary"],
            Value::String("did the thing".to_string())
        );

        // Paused path: `ended_at` stays `NULL`.
        let paused_issue = live_uuid(3301);
        let paused_run = seed_run(&mut tx, 4031, paused_issue, "running", live_time(31)).await;
        let signal = ingest_into_run(&mut tx, paused_run, paused_text, now)
            .await
            .expect("ingest paused");
        assert_eq!(
            signal.map(|signal| signal.status),
            Some("paused".to_string())
        );
        let row: PgRow = sqlx::query("SELECT status, ended_at FROM agent_run WHERE id = $1")
            .bind(paused_run)
            .fetch_one(&mut *tx)
            .await
            .expect("read back");
        let status: String = row.try_get("status").expect("status");
        let ended_at: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("ended_at").expect("ended_at");
        assert_eq!(status, "paused_awaiting_input");
        assert_eq!(ended_at, None);

        // Parse-error on a non-terminal row: `FAILED` + error text.
        let failing = seed_run(&mut tx, 4032, live_uuid(3302), "running", live_time(32)).await;
        let signal = ingest_into_run(&mut tx, failing, "no fence here", now)
            .await
            .expect("ingest parse error");
        assert_eq!(signal, None);
        let row: PgRow = sqlx::query(
            "SELECT status, done_payload, error, ended_at FROM agent_run WHERE id = $1",
        )
        .bind(failing)
        .fetch_one(&mut *tx)
        .await
        .expect("read back");
        let status: String = row.try_get("status").expect("status");
        let error: String = row.try_get("error").expect("error");
        let ended_at: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("ended_at").expect("ended_at");
        let payload: Option<Value> = row.try_get("done_payload").expect("payload");
        assert_eq!(status, "failed");
        assert_eq!(
            error,
            "done-signal parse error: no pi-dash-done fenced block found"
        );
        assert_eq!(ended_at, Some(now));
        assert_eq!(payload, None);

        // Parse-error on a terminal row: status kept, error recorded.
        let kept = seed_run(&mut tx, 4033, live_uuid(3303), "completed", live_time(33)).await;
        let signal = ingest_into_run(&mut tx, kept, "no fence here", now)
            .await
            .expect("ingest parse error");
        assert_eq!(signal, None);
        let row: PgRow = sqlx::query("SELECT status, error, ended_at FROM agent_run WHERE id = $1")
            .bind(kept)
            .fetch_one(&mut *tx)
            .await
            .expect("read back");
        let status: String = row.try_get("status").expect("status");
        assert_eq!(status, "completed");
        let ended_at: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("ended_at").expect("ended_at");
        assert_eq!(ended_at, Some(now));
    }

    /// Ingesting into a missing run is `RowNotFound` (Python's
    /// `save()`-then-`INSERT` fallback is unreachable from live
    /// callers and deliberately not recreated).
    #[tokio::test]
    async fn live_ingest_missing_run_is_not_found() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let now = fixture_time(&serde_json::json!("2026-10-03T12:00:00+00:00")).expect("T0");
        let err = ingest_into_run(&mut tx, live_uuid(9999), "no fence here", now)
            .await
            .expect_err("missing run fails");
        assert!(matches!(err, sqlx::Error::RowNotFound), "got {err:?}");
    }
}
