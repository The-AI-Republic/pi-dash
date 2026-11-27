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
//!   default) are EXCLUDED from both runs. [`candidate_issues_statement`]
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
use sea_orm::sea_query::{Alias, Expr, JoinType, Query, SelectStatement};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, QueryFilter,
    QueryOrder, QuerySelect, Select,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{
    cycle_issues, cycles, intake_issues, issues, module_issues, modules, projects, states,
};
use pidash_db::orm_tx::into_sqlx;
use pidash_db::Pools;
use pidash_services::domain::issues as domain_issues;

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
fn archive_projects_query() -> Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::ArchiveIn,
            projects::Column::CreatedById,
        ])
        .filter(projects::Column::ArchiveIn.gt(0))
        .filter(projects::Column::DeletedAt.is_null())
}

/// Close phase projects (`:93`): `.filter(close_in__gt=0)` with
/// `.select_related(\"default_state\")`. The FK value lives on the project
/// row itself, so no join is needed to port the prefetch.
fn close_projects_query() -> Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::CloseIn,
            projects::Column::CreatedById,
            projects::Column::DefaultStateId,
        ])
        .filter(projects::Column::CloseIn.gt(0))
        .filter(projects::Column::DeletedAt.is_null())
}

/// Cancelled fallback (`:121`):
/// `State.objects.filter(group=\"cancelled\").first()` — the soft-deleting
/// `StateManager` (the triage exclusion is vacuous for `cancelled`) with
/// `Meta.ordering = (\"sequence\",)`, so `.first()` renders
/// `ORDER BY sequence ASC LIMIT 1`. May return no row (QUIRK-5).
fn cancelled_state_query() -> Select<states::Entity> {
    states::Entity::find()
        .select_only()
        .column(states::Column::Id)
        .filter(states::Column::Group.eq("cancelled"))
        .filter(states::Column::DeletedAt.is_null())
        .order_by_asc(states::Column::Sequence)
        .limit(1)
}

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
/// * `end_date < bound` takes the full timestamp (`DateTimeField`) while
///   `target_date < bound` takes a DATE (QUIRK-3: Django truncates `now()`
///   to a date for the `DateField` comparison).
///
/// Built with sea-query (`Query::select` + `join_as` with aliases) because
/// the reverse links (`cycle_issues`, `module_issues`, `intake_issues`)
/// have no `has_many` on `issues` to join through; conditions use the
/// builder, never string concatenation. `NOT (x IS NOT NULL)` renders as
/// `x IS NULL` and `NOT (group = 'triage')` as `group <> 'triage'` —
/// truth-table identical for these columns.
fn candidate_issues_statement(
    project_id: Uuid,
    state_groups: &[&str],
    cutoff: DateTime<Utc>,
    cycle_bound: DateTime<Utc>,
    module_bound: NaiveDate,
) -> SelectStatement {
    let i = Alias::new("i");
    let s = Alias::new("s");
    let p = Alias::new("p");
    let ci = Alias::new("ci");
    let c = Alias::new("c");
    let mi = Alias::new("mi");
    let m = Alias::new("m");
    let ii = Alias::new("ii");
    let cycle_guard = Condition::any()
        .add(
            Condition::all()
                .add(Expr::col((c.clone(), cycles::Column::EndDate)).lt(cycle_bound))
                .add(Expr::col((ci.clone(), cycle_issues::Column::Id)).is_not_null()),
        )
        .add(Expr::col((ci.clone(), cycle_issues::Column::Id)).is_null());
    let module_guard = Condition::any()
        .add(
            Condition::all()
                .add(Expr::col((m.clone(), modules::Column::TargetDate)).lt(module_bound))
                .add(Expr::col((mi.clone(), module_issues::Column::Id)).is_not_null()),
        )
        .add(Expr::col((mi.clone(), module_issues::Column::Id)).is_null());
    let intake_guard = Condition::any()
        .add(Expr::col((ii.clone(), intake_issues::Column::Status)).eq(1))
        .add(Expr::col((ii.clone(), intake_issues::Column::Status)).eq(-1))
        .add(Expr::col((ii.clone(), intake_issues::Column::Status)).eq(2))
        .add(Expr::col((ii.clone(), intake_issues::Column::Id)).is_null());
    let mut stmt = Query::select();
    stmt.column((i.clone(), issues::Column::Id))
        .from_as(issues::Entity, i.clone())
        .join_as(
            JoinType::InnerJoin,
            states::Entity,
            s.clone(),
            Expr::col((i.clone(), issues::Column::StateId)).equals((s.clone(), states::Column::Id)),
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            p.clone(),
            Expr::col((i.clone(), issues::Column::ProjectId))
                .equals((p.clone(), projects::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            cycle_issues::Entity,
            ci.clone(),
            Expr::col((i.clone(), issues::Column::Id))
                .equals((ci.clone(), cycle_issues::Column::IssueId)),
        )
        .join_as(
            JoinType::LeftJoin,
            cycles::Entity,
            c.clone(),
            Expr::col((ci.clone(), cycle_issues::Column::CycleId))
                .equals((c.clone(), cycles::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            module_issues::Entity,
            mi.clone(),
            Expr::col((i.clone(), issues::Column::Id))
                .equals((mi.clone(), module_issues::Column::IssueId)),
        )
        .join_as(
            JoinType::LeftJoin,
            modules::Entity,
            m.clone(),
            Expr::col((mi.clone(), module_issues::Column::ModuleId))
                .equals((m.clone(), modules::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            intake_issues::Entity,
            ii.clone(),
            Expr::col((i.clone(), issues::Column::Id))
                .equals((ii.clone(), intake_issues::Column::IssueId)),
        )
        .and_where(Expr::col((i.clone(), issues::Column::DeletedAt)).is_null())
        .and_where(Expr::col((s.clone(), states::Column::Group)).ne("triage"))
        .and_where(Expr::col((i.clone(), issues::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((p.clone(), projects::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((i.clone(), issues::Column::IsDraft)).eq(false))
        .and_where(Expr::col((i.clone(), issues::Column::ArchivedAt)).is_null())
        .and_where(Expr::col((i.clone(), issues::Column::ProjectId)).eq(project_id))
        .and_where(Expr::col((s.clone(), states::Column::Group)).is_in(state_groups.to_vec()))
        .and_where(Expr::col((i.clone(), issues::Column::UpdatedAt)).lte(cutoff))
        .and_where(cycle_guard.into())
        .and_where(module_guard.into())
        .and_where(intake_guard.into());
    stmt
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
#[derive(Debug, Clone, FromQueryResult)]
struct ArchiveProject {
    id: Uuid,
    archive_in: i32,
    created_by_id: Option<Uuid>,
}

/// One close-phase project row (`:93-97`).
#[derive(Debug, Clone, FromQueryResult)]
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let stmt =
        candidate_issues_statement(project_id, state_groups, cutoff, cycle_bound, module_bound);
    let rows = db
        .query_all(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(|error| {
            AutomationError::Log(format!("{phase}: candidate lookup: {}", into_sqlx(error)))
        })?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        ids.push(row.try_get("", "id").map_err(|error| {
            AutomationError::Log(format!("{phase}: candidate lookup: {}", into_sqlx(error)))
        })?);
    }
    Ok(ids)
}

/// Which single-column `bulk_update` a chunked write runs: the archive
/// stamp (`["archived_at"]`, `:69`) or the close state (`["state"]`,
/// `:132`, nullable — QUIRK-5).
enum BulkIssueWrite {
    Archive(NaiveDate),
    Close(Option<Uuid>),
}

/// Bulk write in `batch_size=100` chunks (`:69,132`), routed through the
/// `domain::issues` bare bulk fns.
async fn bulk_update_in_chunks(
    pool: &PgPool,
    phase: &str,
    write: &BulkIssueWrite,
    ids: &[Uuid],
) -> Result<(), AutomationError> {
    for chunk in ids.chunks(BULK_BATCH_SIZE) {
        let outcome = match write {
            BulkIssueWrite::Archive(archived_at) => {
                domain_issues::bulk_set_archived_at(
                    &pidash_db::pool::sea_orm_connection(pool),
                    chunk,
                    *archived_at,
                )
                .await
            }
            BulkIssueWrite::Close(state_id) => {
                domain_issues::bulk_set_state(
                    &pidash_db::pool::sea_orm_connection(pool),
                    chunk,
                    *state_id,
                )
                .await
            }
        };
        outcome.map_err(|error| AutomationError::Log(format!("{phase}: bulk update: {error}")))?;
    }
    Ok(())
}

/// `archive_old_issues` (`:29-88`): per project with `archive_in > 0`,
/// select the stale closed issues, stamp `archived_at` to today's date,
/// and enqueue one `issue_activity.delay` per issue. Any exception aborts
/// this phase into log-and-return (`:86-88`); the close phase still runs.
async fn run_archive(pool: &PgPool) -> Result<(), AutomationError> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let projects: Vec<ArchiveProject> = archive_projects_query()
        .into_model::<ArchiveProject>()
        .all(&db)
        .await
        .map_err(|error| {
            AutomationError::Log(format!("archive: project lookup: {}", into_sqlx(error)))
        })?;
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
        bulk_update_in_chunks(pool, "archive", &BulkIssueWrite::Archive(archive_at), &ids).await?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let id: Option<Uuid> = cancelled_state_query()
        .into_tuple::<Uuid>()
        .one(&db)
        .await
        .map_err(|error| {
            AutomationError::Log(format!("close: cancelled lookup: {}", into_sqlx(error)))
        })?;
    Ok(id)
}

/// `close_old_issues` (`:90-150`): per project with `close_in > 0`, select
/// the stale open issues, move them to the close state, and enqueue one
/// `issue_activity.delay` per issue. Any exception aborts this phase into
/// log-and-return (`:148-150`).
async fn run_close(pool: &PgPool) -> Result<(), AutomationError> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let projects: Vec<CloseProject> = close_projects_query()
        .into_model::<CloseProject>()
        .all(&db)
        .await
        .map_err(|error| {
            AutomationError::Log(format!("close: project lookup: {}", into_sqlx(error)))
        })?;
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
        bulk_update_in_chunks(pool, "close", &BulkIssueWrite::Close(close_state), &ids).await?;
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
    fn candidate_statement_pins_archive_shape() {
        let project = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("valid uuid");
        let sql = candidate_issues_statement(
            project,
            &CLOSED_STATE_GROUPS,
            utc(2026, 9, 21, 12, 0, 0),
            utc(2026, 9, 28, 12, 0, 0),
            NaiveDate::from_ymd_opt(2026, 9, 28).expect("valid test date"),
        )
        .to_string(sea_orm::sea_query::PostgresQueryBuilder);
        // Joins.
        assert!(sql.contains("\"issues\" AS \"i\""), "{sql}");
        assert!(sql.contains("\"states\" AS \"s\""), "{sql}");
        assert!(sql.contains("\"projects\" AS \"p\""), "{sql}");
        assert!(sql.contains("\"cycle_issues\" AS \"ci\""), "{sql}");
        assert!(sql.contains("\"cycles\" AS \"c\""), "{sql}");
        assert!(sql.contains("\"module_issues\" AS \"mi\""), "{sql}");
        assert!(sql.contains("\"modules\" AS \"m\""), "{sql}");
        assert!(sql.contains("\"intake_issues\" AS \"ii\""), "{sql}");
        // Explicit filters.
        assert!(sql.contains("\"project_id\""), "{sql}");
        assert!(sql.contains("\"archived_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"updated_at\" <="), "{sql}");
        // Manager exclusions riding along.
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"group\""), "{sql}");
        assert!(sql.contains("\"is_draft\""), "{sql}");
        // Cycle / module / intake guards with independent bounds.
        assert!(sql.contains("\"end_date\" <"), "{sql}");
        assert!(sql.contains("\"target_date\" <"), "{sql}");
        // QUIRK-2: only 1 / -1 / 2 plus the null-intake branch.
        assert!(sql.contains("\"status\""), "{sql}");
    }

    #[test]
    fn candidate_statement_pins_close_shape() {
        let project = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("valid uuid");
        let sql = candidate_issues_statement(
            project,
            &OPEN_STATE_GROUPS,
            utc(2026, 9, 21, 12, 0, 0),
            utc(2026, 9, 28, 12, 0, 0),
            NaiveDate::from_ymd_opt(2026, 9, 28).expect("valid test date"),
        )
        .to_string(sea_orm::sea_query::PostgresQueryBuilder);
        assert!(sql.contains("backlog"), "{sql}");
        assert!(sql.contains("unstarted"), "{sql}");
        // Same guards, only the group list changes.
        assert!(sql.contains("\"updated_at\" <="), "{sql}");
        assert!(sql.contains("\"status\""), "{sql}");
    }

    #[test]
    fn project_and_bulk_statement_pins_shapes() {
        use sea_orm::{DbBackend, QueryTrait};
        // Strictly-greater windows over live projects.
        let archive_sql = archive_projects_query()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(archive_sql.contains("\"archive_in\" >"), "{archive_sql}");
        assert!(
            archive_sql.contains("\"deleted_at\" IS NULL"),
            "{archive_sql}"
        );
        let close_sql = close_projects_query()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(close_sql.contains("\"close_in\" >"), "{close_sql}");
        assert!(close_sql.contains("\"default_state_id\""), "{close_sql}");
        assert!(close_sql.contains("\"deleted_at\" IS NULL"), "{close_sql}");
        // Bulk writes touch exactly one column each (the statement pins
        // moved to `domain::issues::ticker_and_automation_bare_sql_shapes`
        // with the SQL; the batch size stays here with the chunking).
        assert_eq!(BULK_BATCH_SIZE, 100);
        // Cancelled fallback: soft-filtered, sequence-ordered, first row.
        let cancelled_sql = cancelled_state_query()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(cancelled_sql.contains("cancelled"), "{cancelled_sql}");
        assert!(
            cancelled_sql.contains("\"deleted_at\" IS NULL"),
            "{cancelled_sql}"
        );
        assert!(
            cancelled_sql.contains("\"sequence\" ASC"),
            "{cancelled_sql}"
        );
        assert!(cancelled_sql.contains("LIMIT 1"), "{cancelled_sql}");
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
            crate::worker::Route::Unknown
        );
    }
}
