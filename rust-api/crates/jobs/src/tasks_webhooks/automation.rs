//! Archive-and-close automation task (D-08, jobs layer).
//!
//! Port of the `@shared_task` entry point of
//! `apps/api/pi_dash/bgtasks/issue_automation_task.py:23-150`
//! (`archive_and_close_old_issues`, with `archive_old_issues: 29-88` and
//! `close_old_issues: 90-150`). The beat row
//! `check-every-day-to-archive-and-close` (`pi_dash/celery.py:42-45`,
//! `crontab(hour=1, minute=0)`) is transcribed by D-10/F-09; this module
//! registers the identical Celery task name so the scheduled message
//! routes [`Route::Local`][crate::worker::Route] instead of forwarding to
//! Python. Fixture: `rust-api/fixtures/tasks_webhooks/fx-auto-01-archive-close.json`
//! (FX-AUTO-01).
//!
//! This module owns the Celery wire surface (the task name, the no-arg
//! binding) and the driver, which runs the same SQL the Django ORM
//! renders, statement by statement (no wrapping transaction: the ORM
//! calls autocommit individually). The per-issue `issue_activity.delay`
//! fan-out is enqueued in Celery kwargs wire format via
//! [`crate::queue::enqueue`], in the exact Python call order; the Rust
//! worker claims it through the handler PIDASHCONV-198 registered.
//!
//! The `#[cfg(test)]` suite below asserts the FX-AUTO-01 semantics:
//! cutoff math, payload goldens, kwargs order, bind rules, SQL shapes,
//! and registration routing.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * QUIRK-1 (`issue_automation_task.py:43,104`): a "month" is
//!   `timedelta(days=n*30)` — a 1-month setting means 30 days, 12 months
//!   means 360 days, never calendar months. [`cutoff_for`] keeps the
//!   `days(n*30)`.
//! * QUIRK-2 (intake rule): only intake statuses `1` (accepted), `-1`
//!   (rejected) and `2` (duplicate) — plus issues with no intake row —
//!   are archived/closed. Statuses `0` (snoozed) and `-2` (pending, the
//!   default) are EXCLUDED from both runs. [`find_candidate_issues_sql`]
//!   keeps the three-status list.
//! * QUIRK-3 (`module__target_date__lt=timezone.now()`): `target_date`
//!   is a `DateField`, so Django truncates `now()` to a date before
//!   comparing — a module whose target date is *today* is not "passed";
//!   it must be strictly before today. The driver binds
//!   [`module_day_for`] (`now().date()`), while the cycle `end_date`
//!   (`DateTimeField`) keeps the full timestamp.
//! * QUIRK-4 (`str(project.created_by_id)`): `created_by` is nullable,
//!   and `str(None)` is `"None"` — the activity payload carries the
//!   literal string `"None"` when the project has no creator.
//!   [`actor_string`] keeps it.
//! * QUIRK-5 (close fallback `:120-123`): when `default_state` is
//!   `None`, the close state is the first `group='cancelled'` row —
//!   which may itself not exist, in which case issues are bulk-set to
//!   `state = NULL` and the payload carries `"closed_to": "None"`.
//!   [`resolve_close_state`] ports the `None` through.
//! * QUIRK-6: `timezone.now()` is read fresh at every site — the
//!   updated-at cutoff, the cycle bound and the module bound are three
//!   independent timestamps per project, `archive_at` is read once per
//!   project, and `epoch` is read per issue. The driver calls
//!   [`Utc::now`] at each of those sites instead of sharing one.
//! * The `Issue.issue_objects` manager exclusions ride along with the
//!   explicit filters: soft-deleted rows skipped, `is_draft` rows
//!   skipped, triage-group states skipped (already implied by the group
//!   lists), and projects with non-null `archived_at` skipped (an extra
//!   `projects.archived_at IS NULL` join condition). Related-table
//!   managers do NOT filter the joins (verified against the ORM
//!   rendering: the cycle/module/intake links join unfiltered).
//!
//! Deliberate transport note (documented, not a bug): the issue ids and
//! project ids the Python code passes to `.delay()` as `UUID` objects
//! arrive on the Celery wire as hyphenated strings (kombu JSON-encodes
//! UUIDs), so the enqueued kwargs carry strings.
//!
//! Ack parity: plain `@shared_task` means ack-on-success with no
//! overrides, and both phase bodies end in `return` under a broad
//! `except Exception: log_exception(e)`. Every ported control path
//! therefore settles to [`Verdict::Ack`][crate::worker::Verdict]; only
//! an unbindable payload fails, which the worker settles into
//! requeue-with-budget exactly like the F-09 mechanism does for every
//! handler.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::Pools;

use crate::queue::{enqueue, NewJob};
use crate::tasks_webhooks::activity_dispatch::{django_dumps, ISSUE_ACTIVITY_TASK};
use crate::worker::{Handler, Registry, Verdict};

/// `archive_and_close_old_issues` (`issue_automation_task.py:23`).
pub const ARCHIVE_AND_CLOSE_TASK: &str =
    "pi_dash.bgtasks.issue_automation_task.archive_and_close_old_issues";

/// `CLOSED_STATE_GROUPS` (`utils/constants.py:88`): the last two lifecycle
/// groups, the archive run's `state__group__in` list.
pub const CLOSED_STATE_GROUPS: [&str; 2] = ["completed", "cancelled"];

/// `OPEN_STATE_GROUPS` (`utils/constants.py:86`): everything but the last
/// two lifecycle groups, the close run's `state__group__in` list.
pub const OPEN_STATE_GROUPS: [&str; 5] = ["backlog", "unstarted", "started", "review", "test"];

/// `bulk_update(..., batch_size=100)`: at most 100 rows per UPDATE
/// statement (`:69,132`).
pub const BULK_BATCH_SIZE: usize = 100;

/// Archive phase projects (`:32`): `Project.objects.filter(archive_in__gt=0)`
/// over the soft-deleting default manager. `NULL > 0` is never true, so a
/// NULL `archive_in` excludes the row at the SQL level.
pub const ARCHIVE_PROJECTS_SQL: &str = "SELECT id, archive_in, created_by_id FROM projects \
     WHERE archive_in > 0 AND deleted_at IS NULL";

/// Close phase projects (`:93`): `.filter(close_in__gt=0)` with
/// `.select_related(\"default_state\")`. The FK value lives on the project
/// row itself, so no join is needed to port the prefetch.
pub const CLOSE_PROJECTS_SQL: &str = "SELECT id, close_in, created_by_id, default_state_id \
     FROM projects WHERE close_in > 0 AND deleted_at IS NULL";

/// Cancelled fallback (`:121`):
/// `State.objects.filter(group=\"cancelled\").first()` — the soft-deleting
/// `StateManager` (the triage exclusion is vacuous for `cancelled`) with
/// `Meta.ordering = (\"sequence\",)`, so `.first()` renders
/// `ORDER BY sequence ASC LIMIT 1`. May return no row (QUIRK-5).
pub const FIND_CANCELLED_STATE_SQL: &str = "SELECT id FROM states WHERE \"group\" = 'cancelled' \
     AND deleted_at IS NULL ORDER BY sequence ASC LIMIT 1";

/// Archive bulk write (`:69`): `bulk_update(issues_to_update, [\"archived_at\"])`
/// touches ONLY `archived_at` — no signals, no `auto_now` bump.
pub const ARCHIVE_ISSUES_SQL: &str = "UPDATE issues SET archived_at = $1 WHERE id = ANY($2)";

/// Close bulk write (`:132`): `bulk_update(issues_to_update, [\"state\"])`
/// touches ONLY the state FK column.
pub const CLOSE_ISSUES_SQL: &str = "UPDATE issues SET state_id = $1 WHERE id = ANY($2)";

/// The candidate-issue SELECT both phases share (`:39-55,100-116`),
/// parameterized by the phase's state-group list. Renders what the ORM
/// renders for `Issue.issue_objects.filter(Q(...), Q(...), Q(...))` (see
/// module docs; join/negation shape verified against the ORM compiler):
///
/// * `INNER JOIN states` for `state__group__in` plus the manager's
///   `NOT (group = 'triage')` (vacuous beside the IN list, kept as-is);
/// * `INNER JOIN projects` for the manager's
///   `exclude(project__archived_at__isnull=False)`;
/// * the manager's soft-delete (`deleted_at IS NULL`), explicit
///   `archived_at IS NULL`, and `NOT (is_draft)`;
/// * `LEFT OUTER JOIN` + `link.id IS NULL` / `IS NOT NULL` for the
///   cycle/module/intake guards (reverse-FK `isnull`, link tables
///   unfiltered);
/// * `end_date < $3` takes the full timestamp (`DateTimeField`) while
///   `target_date < $4` takes a DATE (QUIRK-3: Django truncates `now()`
///   to a date for the `DateField` comparison).
pub fn find_candidate_issues_sql(state_groups: &[&str]) -> String {
    let mut groups = String::new();
    for (index, group) in state_groups.iter().enumerate() {
        if index > 0 {
            groups.push_str(", ");
        }
        groups.push('\'');
        groups.push_str(group);
        groups.push('\'');
    }
    format!(
        "SELECT i.id FROM issues i \
         INNER JOIN states s ON (i.state_id = s.id) \
         INNER JOIN projects p ON (i.project_id = p.id) \
         LEFT OUTER JOIN cycle_issues ci ON (i.id = ci.issue_id) \
         LEFT OUTER JOIN cycles c ON (ci.cycle_id = c.id) \
         LEFT OUTER JOIN module_issues mi ON (i.id = mi.issue_id) \
         LEFT OUTER JOIN modules m ON (mi.module_id = m.id) \
         LEFT OUTER JOIN intake_issues ii ON (i.id = ii.issue_id) \
         WHERE i.deleted_at IS NULL \
         AND NOT (s.\"group\" = 'triage') \
         AND NOT (i.archived_at IS NOT NULL) \
         AND NOT (p.archived_at IS NOT NULL) \
         AND NOT (i.is_draft) \
         AND i.archived_at IS NULL \
         AND i.project_id = $1 \
         AND s.\"group\" IN ({groups}) \
         AND i.updated_at <= $2 \
         AND (ci.id IS NULL OR (c.end_date < $3 AND ci.id IS NOT NULL)) \
         AND (mi.id IS NULL OR (m.target_date < $4 AND mi.id IS NOT NULL)) \
         AND (ii.status = 1 OR ii.status = -1 OR ii.status = 2 OR ii.id IS NULL)"
    )
}

/// QUIRK-1 (`:43,104`): months are `timedelta(days=n*30)` — never calendar
/// months. Pure so the approximation is pinned without a DB.
pub fn cutoff_for(now: DateTime<Utc>, months_setting: i32) -> DateTime<Utc> {
    now - chrono::Duration::days(months_setting as i64 * 30)
}

/// QUIRK-3: the module bound is `now()` truncated to a date
/// (`DateField` comparison). Pure so the truncation is pinned.
pub fn module_day_for(now: DateTime<Utc>) -> NaiveDate {
    now.date_naive()
}

/// QUIRK-4 (`str(project.created_by_id)`): a NULL creator renders as the
/// literal string `"None"`, not null. Pure so the quirk is pinned.
pub fn actor_string(created_by_id: Option<Uuid>) -> String {
    created_by_id.map_or_else(|| "None".to_owned(), |id| id.to_string())
}

/// Archive payload (`:73`):
/// `json.dumps({\"archived_at\": str(archive_at), \"automation\": True})`
/// with `archive_at = timezone.now().date()` (a DATE, `:62`).
pub fn archive_requested_data(archive_at: NaiveDate) -> String {
    let mut fields = Map::with_capacity(2);
    fields.insert(
        "archived_at".to_owned(),
        Value::String(archive_at.format("%Y-%m-%d").to_string()),
    );
    fields.insert("automation".to_owned(), Value::Bool(true));
    django_dumps(&Value::Object(fields))
}

/// Archive snapshot (`:78`): `json.dumps({\"archived_at\": None})` — always
/// null (the rows were just filtered `archived_at IS NULL`).
pub fn archive_current_instance() -> String {
    let mut fields = Map::with_capacity(1);
    fields.insert("archived_at".to_owned(), Value::Null);
    django_dumps(&Value::Object(fields))
}

/// Close payload (`:136`): `json.dumps({\"closed_to\": str(issue.state_id)})`
/// with the FK read AFTER `issue.state = close_state` (`:127`), i.e. the
/// resolved close state — or `"None"` when the cancelled fallback itself
/// found no row (QUIRK-5).
pub fn close_requested_data(closed_to: Option<Uuid>) -> String {
    let mut fields = Map::with_capacity(1);
    fields.insert(
        "closed_to".to_owned(),
        Value::String(closed_to.map_or_else(|| "None".to_owned(), |id| id.to_string())),
    );
    django_dumps(&Value::Object(fields))
}

/// One `issue_activity.delay(...)` fan-out (`:70-83,133-146`): all-keyword
/// call in signature order —
/// `type, requested_data, actor_id, issue_id, project_id,
/// current_instance, subscriber, epoch, notification` — with
/// `type=\"issue.activity.updated\"`, `subscriber=False`,
/// `notification=True`, and `epoch=int(timezone.now().timestamp())`
/// passed in by the caller (read per issue, QUIRK-6). UUIDs travel as
/// hyphenated strings (kombu JSON-encodes UUID objects).
pub fn build_issue_activity_job(
    project_id: Uuid,
    created_by_id: Option<Uuid>,
    issue_id: Uuid,
    requested_data: String,
    current_instance: Option<String>,
    epoch_secs: i64,
) -> NewJob {
    let mut kwargs = Map::with_capacity(9);
    kwargs.insert(
        "type".to_owned(),
        Value::String("issue.activity.updated".to_owned()),
    );
    kwargs.insert("requested_data".to_owned(), Value::String(requested_data));
    kwargs.insert(
        "actor_id".to_owned(),
        Value::String(actor_string(created_by_id)),
    );
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        current_instance.map(Value::String).unwrap_or(Value::Null),
    );
    kwargs.insert("subscriber".to_owned(), Value::Bool(false));
    kwargs.insert("epoch".to_owned(), Value::from(epoch_secs));
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    NewJob::new(
        ISSUE_ACTIVITY_TASK,
        Value::Array(Vec::new()),
        Value::Object(kwargs),
    )
}

/// How the driver fails. Both Python phases end in
/// `except Exception: log_exception(e); return`, so every failure is a
/// log-and-continue into the next phase and the handler always settles
/// `Ack`.
#[derive(Debug, Clone, PartialEq)]
pub enum AutomationError {
    Log(String),
}

impl std::fmt::Display for AutomationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutomationError::Log(message) => write!(f, "archive_and_close_old_issues: {message}"),
        }
    }
}

impl std::error::Error for AutomationError {}

/// One archive-phase project row (`:32-36`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct ArchiveProject {
    id: Uuid,
    archive_in: i32,
    created_by_id: Option<Uuid>,
}

/// One close-phase project row (`:93-97`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct CloseProject {
    id: Uuid,
    close_in: i32,
    created_by_id: Option<Uuid>,
    default_state_id: Option<Uuid>,
}

/// Candidate issue ids for one project (`:39-55,100-116`). The three time
/// bounds are read independently (QUIRK-6): the `updated_at` cutoff from
/// `cutoff_for(now, window)`, the cycle bound as a full timestamp, the
/// module bound truncated to a date (QUIRK-3).
async fn candidate_issue_ids(
    pool: &PgPool,
    phase: &str,
    project_id: Uuid,
    state_groups: &[&str],
    cutoff: DateTime<Utc>,
    cycle_bound: DateTime<Utc>,
    module_bound: NaiveDate,
) -> Result<Vec<Uuid>, AutomationError> {
    sqlx::query_scalar::<_, Uuid>(&find_candidate_issues_sql(state_groups))
        .bind(project_id)
        .bind(cutoff)
        .bind(cycle_bound)
        .bind(module_bound)
        .fetch_all(pool)
        .await
        .map_err(|error| AutomationError::Log(format!("{phase}: candidate lookup: {error}")))
}

/// Bulk write in `batch_size=100` chunks (`:69,132`).
async fn bulk_update_in_chunks<T>(
    pool: &PgPool,
    phase: &str,
    sql: &str,
    value: T,
    ids: &[Uuid],
) -> Result<(), AutomationError>
where
    T: for<'e> sqlx::Encode<'e, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Clone + Send,
{
    for chunk in ids.chunks(BULK_BATCH_SIZE) {
        sqlx::query(sql)
            .bind(value.clone())
            .bind(chunk.to_vec())
            .execute(pool)
            .await
            .map_err(|error| AutomationError::Log(format!("{phase}: bulk update: {error}")))?;
    }
    Ok(())
}

/// `archive_old_issues` (`:29-88`): per project with `archive_in > 0`,
/// select the stale closed issues, stamp `archived_at` to today's date,
/// and enqueue one `issue_activity.delay` per issue. Any exception aborts
/// this phase into log-and-return (`:86-88`); the close phase still runs.
async fn run_archive(pool: &PgPool) -> Result<(), AutomationError> {
    let projects: Vec<ArchiveProject> = sqlx::query_as(ARCHIVE_PROJECTS_SQL)
        .fetch_all(pool)
        .await
        .map_err(|error| AutomationError::Log(format!("archive: project lookup: {error}")))?;
    for project in &projects {
        // QUIRK-6: each bound reads `now()` independently.
        let cutoff = cutoff_for(Utc::now(), project.archive_in);
        let cycle_bound = Utc::now();
        let module_bound = module_day_for(Utc::now());
        let ids = candidate_issue_ids(
            pool,
            "archive",
            project.id,
            &CLOSED_STATE_GROUPS,
            cutoff,
            cycle_bound,
            module_bound,
        )
        .await?;
        // `if issues:` (`:59`) — an empty queryset skips the project.
        if ids.is_empty() {
            continue;
        }
        // `archive_at = timezone.now().date()` (`:62`): read once per
        // project, a DATE not a datetime.
        let archive_at = Utc::now().date_naive();
        bulk_update_in_chunks(pool, "archive", ARCHIVE_ISSUES_SQL, archive_at, &ids).await?;
        let requested_data = archive_requested_data(archive_at);
        let current_instance = archive_current_instance();
        for issue_id in &ids {
            // QUIRK-6: `epoch` reads `now()` per issue (`:81`).
            let job = build_issue_activity_job(
                project.id,
                project.created_by_id,
                *issue_id,
                requested_data.clone(),
                Some(current_instance.clone()),
                Utc::now().timestamp(),
            );
            enqueue(pool, &job).await.map_err(|error| {
                AutomationError::Log(format!("archive: activity enqueue: {error}"))
            })?;
        }
    }
    Ok(())
}

/// Resolve the close state for one project (`:118-123`): the prefetched
/// `default_state` FK as-is (never re-validated), else the first
/// `group='cancelled'` row — which may not exist (QUIRK-5: `None` ports
/// through to a NULL bulk write and a `"None"` payload).
async fn resolve_close_state(
    pool: &PgPool,
    project: &CloseProject,
) -> Result<Option<Uuid>, AutomationError> {
    if let Some(state_id) = project.default_state_id {
        return Ok(Some(state_id));
    }
    let row: Option<(Uuid,)> = sqlx::query_as(FIND_CANCELLED_STATE_SQL)
        .fetch_optional(pool)
        .await
        .map_err(|error| AutomationError::Log(format!("close: cancelled lookup: {error}")))?;
    Ok(row.map(|(id,)| id))
}

/// `close_old_issues` (`:90-150`): per project with `close_in > 0`, select
/// the stale open issues, move them to the close state, and enqueue one
/// `issue_activity.delay` per issue. Any exception aborts this phase into
/// log-and-return (`:148-150`).
async fn run_close(pool: &PgPool) -> Result<(), AutomationError> {
    let projects: Vec<CloseProject> = sqlx::query_as(CLOSE_PROJECTS_SQL)
        .fetch_all(pool)
        .await
        .map_err(|error| AutomationError::Log(format!("close: project lookup: {error}")))?;
    for project in &projects {
        // QUIRK-6: each bound reads `now()` independently.
        let cutoff = cutoff_for(Utc::now(), project.close_in);
        let cycle_bound = Utc::now();
        let module_bound = module_day_for(Utc::now());
        let ids = candidate_issue_ids(
            pool,
            "close",
            project.id,
            &OPEN_STATE_GROUPS,
            cutoff,
            cycle_bound,
            module_bound,
        )
        .await?;
        // `if issues:` (`:118`) — an empty queryset skips the project,
        // so the cancelled fallback query runs only when issues exist.
        if ids.is_empty() {
            continue;
        }
        let close_state = resolve_close_state(pool, project).await?;
        bulk_update_in_chunks(pool, "close", CLOSE_ISSUES_SQL, close_state, &ids).await?;
        // `str(issue.state_id)` AFTER the assignment (`:136`): every
        // issue in this project reports the same resolved state.
        let requested_data = close_requested_data(close_state);
        for issue_id in &ids {
            // QUIRK-6: `epoch` reads `now()` per issue (`:143`).
            let job = build_issue_activity_job(
                project.id,
                project.created_by_id,
                *issue_id,
                requested_data.clone(),
                None,
                Utc::now().timestamp(),
            );
            enqueue(pool, &job).await.map_err(|error| {
                AutomationError::Log(format!("close: activity enqueue: {error}"))
            })?;
        }
    }
    Ok(())
}

/// `archive_and_close_old_issues` (`:23-26`): both phases sequentially in
/// one worker run, each containing its own failures (the two
/// `try/except`s), so this entry itself never fails.
pub async fn run_archive_and_close(pool: &PgPool) {
    if let Err(error) = run_archive(pool).await {
        tracing::error!(task = ARCHIVE_AND_CLOSE_TASK, error = %error, "task failed");
    }
    if let Err(error) = run_close(pool).await {
        tracing::error!(task = ARCHIVE_AND_CLOSE_TASK, error = %error, "task failed");
    }
}

/// Bind the entry payload. `archive_and_close_old_issues()` takes no
/// parameters (`:24`): beat and the contract suite publish empty args
/// plus empty kwargs, and anything else is a `TypeError`-equivalent
/// rejection (the handler surfaces it as a failure, like the F-09
/// mechanism does for every unbindable payload).
fn bind_automation(args: &Value, kwargs: &Value) -> Result<(), String> {
    let empty_args = args.as_array().is_some_and(Vec::is_empty);
    let empty_kwargs = kwargs.as_object().is_some_and(Map::is_empty);
    if empty_args && empty_kwargs {
        Ok(())
    } else {
        Err(format!("{ARCHIVE_AND_CLOSE_TASK} takes no arguments"))
    }
}

/// Register the D-08 automation handler (ownership flips to Rust the
/// moment this name registers; unregistered names still forward to
/// Python).
pub fn register_automation_handlers(registry: &mut Registry, pools: Pools) {
    let handler_pools = pools.clone();
    let handler: Handler = Arc::new(move |job| {
        let pools = handler_pools.clone();
        Box::pin(async move {
            if !job.args.is_array() || !job.kwargs.is_object() {
                return Err(format!(
                    "{ARCHIVE_AND_CLOSE_TASK}: unexpected job payload shape"
                ));
            }
            bind_automation(&job.args, &job.kwargs)
                .map_err(|error| format!("{ARCHIVE_AND_CLOSE_TASK}: invalid payload: {error}"))?;
            run_archive_and_close(pools.primary()).await;
            Ok(Verdict::Ack)
        })
    });
    registry.register(ARCHIVE_AND_CLOSE_TASK, handler);
}

/// True for the D-08 automation task name.
pub fn is_automation_task(task: &str) -> bool {
    task == ARCHIVE_AND_CLOSE_TASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde_json::json;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(y, mo, d)
            .expect("valid test date")
            .and_hms_opt(h, mi, s)
            .expect("valid test time")
            .and_utc()
    }

    #[test]
    fn task_name_matches_celery_entry() {
        // celery.py:42-45 + FX-AUTO-01 `beat.task`.
        assert_eq!(
            ARCHIVE_AND_CLOSE_TASK,
            "pi_dash.bgtasks.issue_automation_task.archive_and_close_old_issues"
        );
        assert!(is_automation_task(ARCHIVE_AND_CLOSE_TASK));
        assert!(!is_automation_task(ISSUE_ACTIVITY_TASK));
        assert!(!is_automation_task(
            "pi_dash.bgtasks.issue_automation_task.archive_old_issues"
        ));
    }

    #[test]
    fn state_groups_match_constants() {
        // utils/constants.py:76-88 via FX-AUTO-01 `groups`.
        assert_eq!(CLOSED_STATE_GROUPS, ["completed", "cancelled"]);
        assert_eq!(
            OPEN_STATE_GROUPS,
            ["backlog", "unstarted", "started", "review", "test"]
        );
    }

    #[test]
    fn cutoff_keeps_month_approximation() {
        // QUIRK-1: days(n*30), never calendar months.
        let now = utc(2026, 9, 28, 1, 0, 0);
        assert_eq!(cutoff_for(now, 1), utc(2026, 8, 29, 1, 0, 0));
        assert_eq!(cutoff_for(now, 2), utc(2026, 7, 30, 1, 0, 0));
        assert_eq!(cutoff_for(now, 12), utc(2025, 10, 3, 1, 0, 0));
        assert_eq!(cutoff_for(now, 0), now);
    }

    #[test]
    fn module_bound_truncates_to_date() {
        // QUIRK-3: the DateField comparison sees the date, so a module
        // targeted today is not "passed".
        assert_eq!(
            module_day_for(utc(2026, 9, 28, 14, 59, 18)),
            NaiveDate::from_ymd_opt(2026, 9, 28).expect("valid test date")
        );
    }

    #[test]
    fn actor_string_keeps_none_quirk() {
        // QUIRK-4: str(None) == "None".
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("valid uuid");
        assert_eq!(
            actor_string(Some(id)),
            "11111111-1111-4111-8111-111111111111"
        );
        assert_eq!(actor_string(None), "None");
    }

    #[test]
    fn archive_payload_goldens() {
        // FX-AUTO-01 `archive.delay_per_issue`: insertion order +
        // default-separator dumps rendering.
        let day = NaiveDate::from_ymd_opt(2026, 9, 28).expect("valid test date");
        assert_eq!(
            archive_requested_data(day),
            r#"{"archived_at": "2026-09-28", "automation": true}"#
        );
        assert_eq!(archive_current_instance(), r#"{"archived_at": null}"#);
    }

    #[test]
    fn close_payload_goldens() {
        // FX-AUTO-01 `close.delay_per_issue`: str() of the FK AFTER
        // assignment; QUIRK-5: missing cancelled state -> "None".
        let id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("valid uuid");
        assert_eq!(
            close_requested_data(Some(id)),
            r#"{"closed_to": "22222222-2222-4222-8222-222222222222"}"#
        );
        assert_eq!(close_requested_data(None), r#"{"closed_to": "None"}"#);
    }

    #[test]
    fn activity_job_kwargs_match_delay_call_order() {
        // The `.delay()` keyword order (:70-83): type, requested_data,
        // actor_id, issue_id, project_id, current_instance, subscriber,
        // epoch, notification — with subscriber False / notification True.
        let project = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("valid uuid");
        let actor = Uuid::parse_str("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").expect("valid uuid");
        let issue = Uuid::parse_str("cccccccc-cccc-4ccc-8ccc-cccccccccccc").expect("valid uuid");
        let job = build_issue_activity_job(
            project,
            Some(actor),
            issue,
            r#"{"archived_at": "2026-09-28", "automation": true}"#.to_owned(),
            Some(r#"{"archived_at": null}"#.to_owned()),
            1_759_056_000,
        );
        assert_eq!(job.task, ISSUE_ACTIVITY_TASK);
        assert_eq!(job.args, json!([]));
        let kwargs = job.kwargs.as_object().expect("kwargs object");
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "subscriber",
                "epoch",
                "notification",
            ]
        );
        assert_eq!(kwargs["type"], json!("issue.activity.updated"));
        assert_eq!(
            kwargs["requested_data"],
            json!(r#"{"archived_at": "2026-09-28", "automation": true}"#)
        );
        assert_eq!(
            kwargs["actor_id"],
            json!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
        );
        assert_eq!(
            kwargs["issue_id"],
            json!("cccccccc-cccc-4ccc-8ccc-cccccccccccc")
        );
        assert_eq!(
            kwargs["project_id"],
            json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
        );
        assert_eq!(
            kwargs["current_instance"],
            json!(r#"{"archived_at": null}"#)
        );
        assert_eq!(kwargs["subscriber"], json!(false));
        assert_eq!(kwargs["epoch"], json!(1_759_056_000));
        assert_eq!(kwargs["notification"], json!(true));
    }

    #[test]
    fn close_job_carries_null_snapshot() {
        // `:139`: `current_instance=None` on the close path.
        let project = Uuid::nil();
        let issue = Uuid::nil();
        let job = build_issue_activity_job(
            project,
            None,
            issue,
            r#"{"closed_to": "None"}"#.to_owned(),
            None,
            0,
        );
        let kwargs = job.kwargs.as_object().expect("kwargs object");
        assert_eq!(kwargs["current_instance"], Value::Null);
        assert_eq!(kwargs["actor_id"], json!("None"));
    }

    #[test]
    fn bind_accepts_only_empty_call() {
        // `:24` takes no parameters; beat publishes ([], {}).
        assert!(bind_automation(&json!([]), &json!({})).is_ok());
        assert!(bind_automation(&json!(["x"]), &json!({})).is_err());
        assert!(bind_automation(&json!([]), &json!({"type": "x"})).is_err());
        assert!(bind_automation(&json!("not-array"), &json!({})).is_err());
        assert!(bind_automation(&json!([]), &json!([])).is_err());
    }

    #[test]
    fn candidate_sql_pins_archive_shape() {
        let sql = find_candidate_issues_sql(&CLOSED_STATE_GROUPS);
        // Explicit filters.
        assert!(sql.contains("i.project_id = $1"));
        assert!(sql.contains("i.archived_at IS NULL"));
        assert!(sql.contains("i.updated_at <= $2"));
        assert!(sql.contains("s.\"group\" IN ('completed', 'cancelled')"));
        // Manager exclusions riding along.
        assert!(sql.contains("i.deleted_at IS NULL"));
        assert!(sql.contains("NOT (s.\"group\" = 'triage')"));
        assert!(sql.contains("NOT (i.archived_at IS NOT NULL)"));
        assert!(sql.contains("NOT (p.archived_at IS NOT NULL)"));
        assert!(sql.contains("NOT (i.is_draft)"));
        // Cycle / module / intake guards with independent bounds.
        assert!(sql.contains("(ci.id IS NULL OR (c.end_date < $3 AND ci.id IS NOT NULL))"));
        assert!(sql.contains("(mi.id IS NULL OR (m.target_date < $4 AND mi.id IS NOT NULL))"));
        // QUIRK-2: only 1 / -1 / 2 plus the null-intake branch.
        assert!(sql.contains("(ii.status = 1 OR ii.status = -1 OR ii.status = 2 OR ii.id IS NULL)"));
    }

    #[test]
    fn candidate_sql_pins_close_shape() {
        let sql = find_candidate_issues_sql(&OPEN_STATE_GROUPS);
        assert!(
            sql.contains("s.\"group\" IN ('backlog', 'unstarted', 'started', 'review', 'test')")
        );
        // Same guards, only the group list changes.
        assert!(sql.contains("i.updated_at <= $2"));
        assert!(sql.contains("(ii.status = 1 OR ii.status = -1 OR ii.status = 2 OR ii.id IS NULL)"));
    }

    #[test]
    fn project_and_bulk_sql_pins_shapes() {
        // Strictly-greater windows over live projects.
        assert!(ARCHIVE_PROJECTS_SQL.contains("archive_in > 0"));
        assert!(ARCHIVE_PROJECTS_SQL.contains("deleted_at IS NULL"));
        assert!(CLOSE_PROJECTS_SQL.contains("close_in > 0"));
        assert!(CLOSE_PROJECTS_SQL.contains("default_state_id"));
        assert!(CLOSE_PROJECTS_SQL.contains("deleted_at IS NULL"));
        // Bulk writes touch exactly one column each.
        assert_eq!(
            ARCHIVE_ISSUES_SQL,
            "UPDATE issues SET archived_at = $1 WHERE id = ANY($2)"
        );
        assert_eq!(
            CLOSE_ISSUES_SQL,
            "UPDATE issues SET state_id = $1 WHERE id = ANY($2)"
        );
        assert_eq!(BULK_BATCH_SIZE, 100);
        // Cancelled fallback: soft-filtered, sequence-ordered, first row.
        assert!(FIND_CANCELLED_STATE_SQL.contains("\"group\" = 'cancelled'"));
        assert!(FIND_CANCELLED_STATE_SQL.contains("deleted_at IS NULL"));
        assert!(FIND_CANCELLED_STATE_SQL.contains("ORDER BY sequence ASC LIMIT 1"));
    }

    #[test]
    fn registry_routes_registered_task_locally() {
        let mut registry = Registry::new();
        let handler: Handler = Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
        registry.register(ARCHIVE_AND_CLOSE_TASK, handler);
        assert_eq!(
            crate::worker::route_for(&registry, ARCHIVE_AND_CLOSE_TASK),
            crate::worker::Route::Local
        );
        assert_eq!(
            crate::worker::route_for(&registry, "pi_dash.bgtasks.logger_task.process_logs"),
            crate::worker::Route::PythonOwned
        );
    }
}
