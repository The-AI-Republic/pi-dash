//! Due-set scanners: `scan_due_tickers` + `scan_due_bindings` (D-10).
//!
//! Ports the scan halves of `pi_dash/bgtasks/agent_ticker.py`
//! (`scan_due_tickers`, lines 42-74) and `pi_dash/bgtasks/scheduler.py`
//! (`_is_enabled` + `scan_due_bindings`, lines 65-136), plus the two owned
//! beat entries from `pi_dash/celery.py` (lines 108-123).
//!
//! Each scanner selects its due id set in one query and fans out one fire
//! job per id through the F-09 queue ([`crate::queue`]); the worker loop
//! forwards fire jobs to the Python plane in Celery protocol v2 until the
//! fire tasks land (PIDASHCONV-208/209 flip ownership by registering
//! handlers, which is exactly the "unported groups keep serving via
//! ownership routing" contract). The due-set SQL is built with sea-query
//! and mirrors the Django ORM predicates arm for arm:
//!
//! - FX-TICKER-03: `scanners/scan_due_tickers.sql` + `.rows.json`
//!   (pending-entry OR infinite-pool OR under-cap admission,
//!   `ORDER BY next_run_at`) and `scanners/scan_due_bindings.sql` +
//!   `.rows.json` (kill switch, scheduler/project soft-delete guards,
//!   NULL-`next_run_at` admission).
//! - FX-TICKER-06: `beat.json` (the two owned entries; `scan-due-loop-targets`
//!   stays excluded for D-03, every other entry for its own domain).
//!
//! **Beat must run as a singleton** — the Python module keeps this note
//! because multiple Beat schedulers would double the scan rate. On the Rust
//! side the F-09 scheduler loop ([`crate::scheduler`]) already holds the
//! singleton via `pg_try_advisory_lock`; the scan itself stays race-safe
//! regardless because the atomic claim lives in the fire tasks.

use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use sea_query::{Condition, Expr, Order, PostgresQueryBuilder, Query};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::celery::CeleryTaskMessage;
use crate::queue::{enqueue, JobRow, NewJob};
use crate::schedule::{beat_schedule, BeatEntry};
use crate::worker::{HandlerError, Registry, Verdict};
use crate::Error;

/// Celery task name of the agent-ticker scanner
/// (`agent_ticker.py:42`, `@shared_task(name=...)`).
pub const SCAN_DUE_TICKERS_TASK: &str = "pi_dash.bgtasks.agent_ticker.scan_due_tickers";
/// Celery task name fanned out per due ticker (`agent_ticker.py:73`,
/// `fire_tick.delay(str(ticker_id))`).
pub const FIRE_TICK_TASK: &str = "pi_dash.bgtasks.agent_ticker.fire_tick";
/// Celery task name of the scheduler scanner
/// (`scheduler.py:110`, `@shared_task(name=...)`).
pub const SCAN_DUE_BINDINGS_TASK: &str = "pi_dash.bgtasks.scheduler.scan_due_bindings";
/// Celery task name fanned out per due binding (`scheduler.py:131`,
/// `fire_scheduler_binding.delay(str(binding_id))`).
pub const FIRE_SCHEDULER_BINDING_TASK: &str = "pi_dash.bgtasks.scheduler.fire_scheduler_binding";

/// Every Celery task name this domain owns. The two scan names get local
/// handlers in [`register_scanners`]; the two fire names stay Python-owned
/// (see [`crate::worker::route_for`]) until PIDASHCONV-208/209 register
/// theirs — either side speaks the same wire payloads, so no fan-out is
/// ever dropped or double-run across the handoff.
pub const TASK_NAMES: [&str; 4] = [
    SCAN_DUE_TICKERS_TASK,
    FIRE_TICK_TASK,
    SCAN_DUE_BINDINGS_TASK,
    FIRE_SCHEDULER_BINDING_TASK,
];

/// Beat entry name for the ticker scanner (`celery.py:108-112`).
pub const SCAN_DUE_TICKERS_BEAT: &str = "scan-due-agent-tickers";
/// Beat entry name for the scheduler scanner (`celery.py:121-124`).
pub const SCAN_DUE_BINDINGS_BEAT: &str = "scan-due-scheduler-bindings";

/// Environment variable for the instance-level kill switch
/// (`scheduler.py:65-67`, `settings/common.py:446`).
pub const SCHEDULER_ENABLED_ENV: &str = "SCHEDULER_ENABLED";

/// Instance-level kill switch (`scheduler.py:65-67`).
///
/// Mirrors `getattr(settings, "SCHEDULER_ENABLED", True)` exactly:
/// `settings/common.py:446` builds the setting as
/// `get_config("SCHEDULER_ENABLED", "true").lower() == "true"`, so a missing
/// variable means enabled and only the literal `"true"` (any case) enables.
pub fn scheduler_enabled() -> bool {
    std::env::var(SCHEDULER_ENABLED_ENV)
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(true)
}

/// The two beat entries this domain owns, selected from the F-09 schedule
/// ([`crate::schedule::beat_schedule`], already transcribed from
/// `celery.py`): `scan-due-agent-tickers` then `scan-due-scheduler-bindings`,
/// both `crontab(minute=*)`. `scan-due-loop-targets` (D-03), the other 19
/// literal entries (their own domains) and the settings-backed entries
/// (F-09 mechanism) are excluded by construction — selection, not a fork.
pub fn beat_entries() -> Vec<BeatEntry> {
    beat_schedule()
        .into_iter()
        .filter(|entry| entry.name == SCAN_DUE_TICKERS_BEAT || entry.name == SCAN_DUE_BINDINGS_BEAT)
        .collect()
}

/// Render `now` the way the fixture goldens record it: whole seconds, explicit
/// `+00:00` offset. The literal is generated from our own clock, never from
/// caller input, so inlining it into the sea-query statement is safe.
fn now_literal(now: &DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Due-ticker id set (`agent_ticker.py:57-69`).
///
/// `enabled AND next_run_at <= now AND (pending_entry OR project pool = -1
/// OR used < pool + granted + waited)`, soft-delete scoped, ordered by
/// `next_run_at`. The cap arm is the SQL mirror of
/// `IssueAgentTicker.effective_max_ticks` (pool + granted + waited, `-1`
/// infinite — [`pidash_db::tasks_ticker::models::INFINITE_MAX_TICKS`]); the
/// two must agree or a waited issue is admitted here and refused in
/// `fire_tick`. A row owing a pending entry is admitted regardless of cap.
pub fn due_tickers_sql(now: &DateTime<Utc>) -> String {
    let now = now_literal(now);
    let mut query = Query::select();
    query
        .column(("issue_agent_ticker", "id"))
        .from("issue_agent_ticker")
        .inner_join(
            "issues",
            Expr::col(("issue_agent_ticker", "issue_id")).equals(("issues", "id")),
        )
        .inner_join(
            "projects",
            Expr::col(("issues", "project_id")).equals(("projects", "id")),
        )
        .and_where(Expr::col(("issue_agent_ticker", "deleted_at")).is_null())
        .and_where(Expr::col(("issue_agent_ticker", "enabled")).eq(true))
        .and_where(Expr::col(("issue_agent_ticker", "next_run_at")).lte(now))
        .cond_where(
            Condition::any()
                .add(Expr::col(("issue_agent_ticker", "pending_entry")).eq(true))
                .add(
                    Expr::col(("projects", "agent_default_max_ticks"))
                        .eq(pidash_db::tasks_ticker::models::INFINITE_MAX_TICKS),
                )
                .add(
                    Expr::col(("issue_agent_ticker", "used")).lt(Expr::col((
                        "projects",
                        "agent_default_max_ticks",
                    ))
                    .add(Expr::col(("issue_agent_ticker", "granted")))
                    .add(Expr::col(("issue_agent_ticker", "waited")))),
                ),
        )
        .order_by(("issue_agent_ticker", "next_run_at"), Order::Asc);
    query.to_string(PostgresQueryBuilder)
}

/// Due-binding id set (`scheduler.py:115-128`).
///
/// `enabled AND scheduler is_enabled AND scheduler/project deleted_at NULL
/// AND (next_run_at <= now OR next_run_at IS NULL)`, ordered by
/// `next_run_at` (Postgres `ASC` puts NULLs last: never-fired bindings fire
/// after past-due ones, exactly as the fixture fan-out order shows). NULL
/// `next_run_at` means "never fired; due immediately" — Postgres NULL
/// semantics exclude such rows from `<=`, so the explicit OR is required.
/// The scheduler/project `deleted_at` guards cover the async-cascade window
/// where a binding is still un-tombstoned but its parent is not
/// (`db/mixins.py soft_delete_related_objects.delay`).
pub fn due_bindings_sql(now: &DateTime<Utc>) -> String {
    let now = now_literal(now);
    let mut query = Query::select();
    query
        .column(("scheduler_bindings", "id"))
        .from("scheduler_bindings")
        .left_join(
            "projects",
            Expr::col(("scheduler_bindings", "project_id")).equals(("projects", "id")),
        )
        .inner_join(
            "schedulers",
            Expr::col(("scheduler_bindings", "scheduler_id")).equals(("schedulers", "id")),
        )
        .and_where(Expr::col(("scheduler_bindings", "deleted_at")).is_null())
        .and_where(Expr::col(("scheduler_bindings", "enabled")).eq(true))
        .and_where(Expr::col(("projects", "deleted_at")).is_null())
        .and_where(Expr::col(("schedulers", "deleted_at")).is_null())
        .and_where(Expr::col(("schedulers", "is_enabled")).eq(true))
        .cond_where(
            Condition::any()
                .add(Expr::col(("scheduler_bindings", "next_run_at")).lte(now))
                .add(Expr::col(("scheduler_bindings", "next_run_at")).is_null()),
        )
        .order_by(("scheduler_bindings", "next_run_at"), Order::Asc);
    query.to_string(PostgresQueryBuilder)
}

/// One fire job per due id: `fire_*.delay(str(id))` is `args=[str(id)]`,
/// `kwargs={}` on the wire, so the queue row carries exactly that. The
/// worker loop's forward path ([`crate::worker::dispatch`]) converts the row
/// back to the identical Celery v2 body — wire-identical whichever plane
/// serves the fire task.
fn fire_job(task: &str, id: &Uuid) -> NewJob {
    NewJob::new(task, json!([id.to_string()]), json!({}))
}

/// The Celery v2 message a fan-out row becomes on the wire: same task name,
/// `args=[str(id)]`, empty kwargs — byte-shape identical to the Python
/// `.delay(str(id))` call. The row-to-message mapping below repeats
/// [`crate::worker::dispatch`]'s forward path arm for arm (that function is
/// the runtime source of truth; this one exists so tests and the fire-task
/// ports in PIDASHCONV-208/209 can assert the wire contract without a broker).
pub fn fire_message(task: &str, id: &Uuid) -> CeleryTaskMessage {
    let job = fire_job(task, id);
    let args = match job.args {
        serde_json::Value::Array(items) => items,
        other => vec![other],
    };
    let kwargs = match job.kwargs {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    CeleryTaskMessage::new(task, args, kwargs)
}

/// Run one scanner end to end: select the due ids, enqueue one fire job per
/// id, return the fan-out count (mirrors the Python `return len(due_ids)`,
/// including the log line only when something fired).
async fn run_scan(pool: &PgPool, sql: &str, fire_task: &str) -> Result<usize, Error> {
    let ids: Vec<Uuid> = sqlx::query_scalar(sql).fetch_all(pool).await?;
    for id in &ids {
        enqueue(pool, &fire_job(fire_task, id)).await?;
    }
    if !ids.is_empty() {
        tracing::info!(
            count = ids.len(),
            task = fire_task,
            "scan: dispatched fire tasks"
        );
    }
    Ok(ids.len())
}

/// Fan out `fire_tick` tasks for every due ticker row
/// (`agent_ticker.py:42-74`). Returns the number of fan-outs.
pub async fn scan_due_tickers(pool: &PgPool, now: &DateTime<Utc>) -> Result<usize, Error> {
    run_scan(pool, &due_tickers_sql(now), FIRE_TICK_TASK).await
}

/// Fan out `fire_scheduler_binding` tasks for every due binding
/// (`scheduler.py:110-136`). Returns `0` without touching the database when
/// the [`scheduler_enabled`] kill switch is off.
pub async fn scan_due_bindings(pool: &PgPool, now: &DateTime<Utc>) -> Result<usize, Error> {
    if !scheduler_enabled() {
        return Ok(0);
    }
    run_scan(pool, &due_bindings_sql(now), FIRE_SCHEDULER_BINDING_TASK).await
}

/// Register the two local scan handlers. The pool is captured by the
/// closures because [`crate::worker::Handler`] receives only the claimed row;
/// each fire runs its due-set query with the firing instant as `now`, then
/// enqueues its fan-outs. A database failure reports the error text so the
/// worker loop retries with budget instead of acking a missed minute.
pub fn register_scanners(registry: &mut Registry, pool: PgPool) {
    let tickers = pool.clone();
    registry.register(
        SCAN_DUE_TICKERS_TASK,
        Arc::new(move |_job: JobRow| {
            let pool = tickers.clone();
            let fut: std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Verdict, HandlerError>> + Send>,
            > = Box::pin(async move {
                let now = Utc::now();
                scan_due_tickers(&pool, &now)
                    .await
                    .map(|_| Verdict::Ack)
                    .map_err(|error| error.to_string())
            });
            fut
        }),
    );
    registry.register(
        SCAN_DUE_BINDINGS_TASK,
        Arc::new(move |_job: JobRow| {
            let pool = pool.clone();
            let fut: std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Verdict, HandlerError>> + Send>,
            > = Box::pin(async move {
                let now = Utc::now();
                scan_due_bindings(&pool, &now)
                    .await
                    .map(|_| Verdict::Ack)
                    .map_err(|error| error.to_string())
            });
            fut
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{is_due, Cadence};
    use crate::scheduler::default_schedule;
    use crate::worker::{route_for, Route};
    use chrono::TimeZone;

    /// The frozen instant the fixtures record (`scanners/*.rows.json "now"`).
    fn frozen() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap()
    }

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tasks_ticker")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let text = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    /// Normalize two SQL spellings to one comparable form: lowercase,
    /// quotes stripped, whitespace collapsed, `OUTER` dropped (sea-query
    /// renders `LEFT JOIN`, Django `LEFT OUTER JOIN` — the same join).
    fn norm(sql: &str) -> String {
        sql.to_ascii_lowercase()
            .replace('"', "")
            .replace("outer join", "join")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn parse(sql: &str) -> sqlparser::ast::Statement {
        let mut stmts =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
                .unwrap_or_else(|e| panic!("SQL parses: {e}\n{sql}"));
        assert_eq!(stmts.len(), 1);
        stmts.pop().unwrap()
    }

    // FX-TICKER-03, tickers half (`bgtasks/agent_ticker.py:57-69`): the
    // generated SQL carries every predicate arm of the golden, joins the
    // same tables the same way, and orders by `next_run_at`.
    #[test]
    fn due_tickers_sql_mirrors_fx_ticker_03() {
        let sql = due_tickers_sql(&frozen());
        assert!(matches!(parse(&sql), sqlparser::ast::Statement::Query(_)));
        let mine = norm(&sql);
        // Joins: ticker -> issue -> project, both inner (Django follows
        // non-nullable FKs with INNER JOIN).
        for fragment in [
            "select issue_agent_ticker.id from issue_agent_ticker",
            "inner join issues on issue_agent_ticker.issue_id = issues.id",
            "inner join projects on issues.project_id = projects.id",
            // Soft-delete scoping: the ticker manager filters its own
            // tombstones. No cascade guard on issues/projects here — the
            // golden has none either (pinned below).
            "issue_agent_ticker.deleted_at is null",
            "issue_agent_ticker.enabled = true",
            "issue_agent_ticker.next_run_at <= '2026-09-28t12:00:00+00:00'",
            // The three admission arms: pending entry, infinite pool,
            // under-cap. Exactly two ORs.
            "issue_agent_ticker.pending_entry = true",
            "projects.agent_default_max_ticks = -1",
            "issue_agent_ticker.used <",
            "projects.agent_default_max_ticks + issue_agent_ticker.granted",
            "issue_agent_ticker.waited",
            "order by issue_agent_ticker.next_run_at asc",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        assert_eq!(
            mine.matches(" or ").count(),
            2,
            "exactly the 3-arm OR:\n{sql}"
        );
        // Faithful negatives: the golden scopes no other table here.
        for absent in ["issues.deleted_at", "projects.deleted_at"] {
            assert!(
                !mine.contains(absent),
                "unexpected guard {absent} in:\n{sql}"
            );
        }
        // The golden records the same shape: same arms, same order, five
        // fan-outs in `next_run_at` ascending order.
        let golden = fixture("scanners/scan_due_tickers.sql");
        let golden_norm = norm(golden["executed_sql"][0]["sql"].as_str().unwrap());
        for fragment in [
            "pending_entry",
            "agent_default_max_ticks = -1",
            "order by issue_agent_ticker.next_run_at asc",
        ] {
            assert!(golden_norm.contains(fragment), "golden lacks {fragment}");
        }
        assert_eq!(golden["fanout_count"], 5);
        assert_eq!(
            golden["fanout_order"],
            json!([
                "T1-due-under-cap",
                "T6-granted",
                "T7-waited",
                "T5-spent-pool-pending",
                "T8-infinite"
            ])
        );
    }

    // FX-TICKER-03, bindings half (`bgtasks/scheduler.py:115-128`): every
    // guard and the NULL-admission OR are present; project joins LEFT
    // (nullable FK), scheduler INNER.
    #[test]
    fn due_bindings_sql_mirrors_fx_ticker_03() {
        let sql = due_bindings_sql(&frozen());
        assert!(matches!(parse(&sql), sqlparser::ast::Statement::Query(_)));
        let mine = norm(&sql);
        for fragment in [
            "select scheduler_bindings.id from scheduler_bindings",
            "left join projects on scheduler_bindings.project_id = projects.id",
            "inner join schedulers on scheduler_bindings.scheduler_id = schedulers.id",
            // Async-cascade guards: binding, project and scheduler
            // tombstones all excluded.
            "scheduler_bindings.deleted_at is null",
            "scheduler_bindings.enabled = true",
            "projects.deleted_at is null",
            "schedulers.deleted_at is null",
            "schedulers.is_enabled = true",
            // NULL next_run_at = never fired = due immediately; Postgres
            // excludes NULLs from `<=`, hence the explicit OR.
            "scheduler_bindings.next_run_at <= '2026-09-28t12:00:00+00:00'",
            "scheduler_bindings.next_run_at is null",
            "order by scheduler_bindings.next_run_at asc",
        ] {
            assert!(mine.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        assert_eq!(
            mine.matches(" or ").count(),
            1,
            "exactly the 2-arm OR:\n{sql}"
        );
        let golden = fixture("scanners/scan_due_bindings.sql");
        assert_eq!(golden["fanout_count"], 2);
        // Postgres ASC is NULLS LAST: past-due B6 fans out before NULL B1.
        assert_eq!(
            golden["fanout_order"],
            json!(["B6-past-due", "B1-null-due"])
        );
    }

    // The frozen instant renders whole-second with an explicit offset and
    // matches the fixture clock both rows files record.
    #[test]
    fn now_literal_matches_fixture_clock() {
        assert_eq!(now_literal(&frozen()), "2026-09-28T12:00:00+00:00");
        assert_eq!(
            now_literal(&frozen()).parse::<DateTime<Utc>>().unwrap(),
            frozen()
        );
        for name in [
            "scanners/scan_due_tickers.rows.json",
            "scanners/scan_due_bindings.rows.json",
        ] {
            assert_eq!(fixture(name)["now"], "2026-09-28T12:00:00+00:00");
        }
    }

    // FX-TICKER-03 admission matrix, pinned as data with the arm each
    // admitted row exercises: T1 under-cap, T5 pending-on-spent-pool, T6
    // granted, T7 waited, T8 infinite; T2 future, T3 disabled, T4 at-cap
    // excluded. Bindings: B6 past-due + B1 NULL admitted; B2 future, B3
    // disabled, B4 scheduler-off, B5 soft-deleted-scheduler cascade window,
    // B7 soft-deleted-project excluded.
    #[test]
    fn fixture_row_matrix_pins_admission_sets() {
        let tickers = fixture("scanners/scan_due_tickers.rows.json");
        let rows = tickers["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        let fanned: Vec<&str> = rows
            .iter()
            .filter(|r| r["fanned"].as_bool().unwrap())
            .map(|r| r["label"].as_str().unwrap())
            .collect();
        assert_eq!(
            fanned,
            [
                "T1-due-under-cap",
                "T5-spent-pool-pending",
                "T6-granted",
                "T7-waited",
                "T8-infinite"
            ]
        );
        let bindings = fixture("scanners/scan_due_bindings.rows.json");
        let rows = bindings["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        let fanned: Vec<&str> = rows
            .iter()
            .filter(|r| r["fanned"].as_bool().unwrap())
            .map(|r| r["label"].as_str().unwrap())
            .collect();
        assert_eq!(fanned, ["B1-null-due", "B6-past-due"]);
    }

    // `SCHEDULER_ENABLED` (`settings/common.py:446`): missing means
    // enabled; only case-insensitive `"true"` enables, everything else —
    // including `"1"` and `"yes"` — disables, exactly like the Python
    // `.lower() == "true"`.
    #[test]
    fn scheduler_enabled_matches_python_semantics() {
        let saved = std::env::var(SCHEDULER_ENABLED_ENV).ok();
        let probe = |value: Option<&str>| {
            match value {
                Some(v) => std::env::set_var(SCHEDULER_ENABLED_ENV, v),
                None => std::env::remove_var(SCHEDULER_ENABLED_ENV),
            }
            scheduler_enabled()
        };
        assert!(probe(None));
        assert!(probe(Some("true")));
        assert!(probe(Some("TRUE")));
        assert!(!probe(Some("false")));
        assert!(!probe(Some("0")));
        assert!(!probe(Some("1")));
        assert!(!probe(Some("yes")));
        assert!(!probe(Some("")));
        match saved {
            Some(v) => std::env::set_var(SCHEDULER_ENABLED_ENV, v),
            None => std::env::remove_var(SCHEDULER_ENABLED_ENV),
        }
    }

    // The kill switch short-circuits before any database touch: with the
    // switch off, the scan returns 0 fan-outs on a pool that could never
    // connect (`connect_lazy` opens no connection until first use).
    #[tokio::test]
    async fn disabled_switch_fans_out_zero_without_db() {
        let saved = std::env::var(SCHEDULER_ENABLED_ENV).ok();
        std::env::set_var(SCHEDULER_ENABLED_ENV, "false");
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server");
        let count = scan_due_bindings(&pool, &frozen())
            .await
            .expect("kill switch");
        assert_eq!(count, 0);
        match saved {
            Some(v) => std::env::set_var(SCHEDULER_ENABLED_ENV, v),
            None => std::env::remove_var(SCHEDULER_ENABLED_ENV),
        }
    }

    // FX-TICKER-06 (`celery.py:108-112,121-124` + `beat.json`): exactly the
    // two owned entries resolve from the F-09 schedule with the recorded
    // task names, both firing every minute; the loop entry, the other 19
    // literal entries and the settings-backed entries are excluded.
    #[test]
    fn beat_entries_match_fx_ticker_06() {
        let beat = fixture("beat.json");
        let owned = beat["owned_entries"].as_object().unwrap();
        assert_eq!(owned.len(), 2);
        let entries = beat_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, SCAN_DUE_TICKERS_BEAT);
        assert_eq!(entries[1].name, SCAN_DUE_BINDINGS_BEAT);
        for entry in &entries {
            let golden = &owned[entry.name];
            assert_eq!(entry.task, golden["task"].as_str().unwrap());
            // `crontab(minute=*)`: every minute of every hour matches.
            let Cadence::Crontab(crontab) = &entry.cadence else {
                panic!("{} is not a crontab entry", entry.name);
            };
            for (month, day, hour, minute) in [
                (1, 2, 0, 0),
                (6, 15, 12, 30),
                (12, 31, 23, 59),
                (3, 10, 4, 7),
            ] {
                let at = Utc
                    .with_ymd_and_hms(2026, month, day, hour, minute, 0)
                    .unwrap();
                assert!(crontab.matches(&at), "{} misses {at}", entry.name);
            }
            // Never-run entries are due: the F-09 loop will fire them.
            let at = Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap();
            assert!(is_due(entry, &at, None));
            // Golden pins the full 0-59 minute set.
            let minutes = golden["crontab_minute"].as_array().unwrap();
            assert_eq!(minutes.len(), 60);
        }
        // Exclusions, by name and by task.
        let names: Vec<&str> = entries.iter().map(|e| e.name).collect();
        assert!(!names.contains(&"scan-due-loop-targets"));
        let tasks: Vec<&str> = entries.iter().map(|e| e.task).collect();
        assert!(!tasks.contains(&"pi_dash.bgtasks.loop.scan_due_targets"));
        // The loop entry exists in the full schedule (D-03's), just not ours.
        assert!(
            default_schedule()
                .iter()
                .any(|e| e.name == "scan-due-loop-targets"),
            "loop entry still registered with the F-09 loop"
        );
        // Both owned entries ride the loop's default schedule.
        for name in [SCAN_DUE_TICKERS_BEAT, SCAN_DUE_BINDINGS_BEAT] {
            assert!(
                default_schedule().iter().any(|e| e.name == name),
                "{name} registered with the F-09 scheduler loop"
            );
        }
        // The golden lists every other literal entry as out of scope.
        assert_eq!(beat["literal_entry_names"].as_array().unwrap().len(), 22);
    }

    // Fan-out payloads are Celery v2 wire-identical to the Python
    // `.delay(str(id))` calls: exact task names, `args=[str(id)]`, empty
    // kwargs — the shape the worker forward path publishes.
    #[test]
    fn fanout_messages_are_celery_wire_identical() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        for task in [FIRE_TICK_TASK, FIRE_SCHEDULER_BINDING_TASK] {
            let message = fire_message(task, &id);
            assert_eq!(message.task, task);
            assert_eq!(message.args, vec![json!(id.to_string())]);
            assert!(message.kwargs.is_empty());
            assert_eq!(message.retries, 0);
            assert_eq!(
                message.body(),
                json!([
                    [id.to_string()],
                    {},
                    {"callbacks": null, "errbacks": null, "chain": null, "chord": null}
                ])
            );
            let headers = message.headers();
            assert_eq!(headers["task"], task);
            assert_eq!(headers["id"], message.id);
            assert_eq!(headers["retries"], 0);
        }
        assert_eq!(FIRE_TICK_TASK, "pi_dash.bgtasks.agent_ticker.fire_tick");
        assert_eq!(
            FIRE_SCHEDULER_BINDING_TASK,
            "pi_dash.bgtasks.scheduler.fire_scheduler_binding"
        );
    }

    // Registry layer: the two scan names are locally owned once
    // registered; the fire names route Python-owned until PIDASHCONV-208/209
    // claim them — unported groups keep serving through ownership routing.
    #[tokio::test]
    async fn registry_owns_scanners_and_routes_fires_to_python() {
        assert_eq!(
            TASK_NAMES,
            [
                SCAN_DUE_TICKERS_TASK,
                FIRE_TICK_TASK,
                SCAN_DUE_BINDINGS_TASK,
                FIRE_SCHEDULER_BINDING_TASK
            ]
        );
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("lazy pool needs no server");
        let mut registry = Registry::new();
        for name in TASK_NAMES {
            assert_eq!(route_for(&registry, name), Route::PythonOwned);
        }
        register_scanners(&mut registry, pool);
        assert!(registry.owns(SCAN_DUE_TICKERS_TASK));
        assert!(registry.owns(SCAN_DUE_BINDINGS_TASK));
        assert_eq!(route_for(&registry, SCAN_DUE_TICKERS_TASK), Route::Local);
        assert_eq!(route_for(&registry, SCAN_DUE_BINDINGS_TASK), Route::Local);
        assert!(!registry.owns(FIRE_TICK_TASK));
        assert!(!registry.owns(FIRE_SCHEDULER_BINDING_TASK));
        assert_eq!(route_for(&registry, FIRE_TICK_TASK), Route::PythonOwned);
        assert_eq!(
            route_for(&registry, FIRE_SCHEDULER_BINDING_TASK),
            Route::PythonOwned
        );
    }
}
