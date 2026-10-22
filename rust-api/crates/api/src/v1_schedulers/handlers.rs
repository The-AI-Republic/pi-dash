//! Read-only v1 scheduler handlers (stage 8 drift, PIDASHCONV-1061).
//!
//! Ports `apps/api/pi_dash/api/views/scheduler.py` (04920ea3) plus the
//! render half of `api/serializers/scheduler.py`:
//!
//! * `GET workspaces/<slug>/schedulers/` — workspace definitions with the
//!   bindings the caller may see (workspace members only; installs on
//!   projects the caller cannot access, or on archived projects, are
//!   omitted)
//! * `GET workspaces/<slug>/projects/<project_id>/schedulers/` —
//!   definitions installed on this project, each with its binding here
//! * `GET .../schedulers/<uuid:scheduler_id>/` — one full definition with
//!   its binding here (404 unless installed on this project)
//! * `GET .../schedulers/<uuid:scheduler_id>/runs/` — recent agent runs
//!   fired by this install, newest first, each with the issues it wrote to
//!
//! Query shapes mirror the Django querysets one for one (same JOIN/WHERE
//! semantics, quoted per statement): scheduler soft-delete scope,
//! binding soft-delete scope, the workspace-list archived-project filter,
//! plain joins for the membership spans, `ORDER BY name` on the lists and
//! `ORDER BY created_at DESC` on bindings and runs. Renders follow the
//! serializer `Meta.fields` orders with DRF value rules
//! ([`crate::serializer`], [`crate::paginator`]).
//!
//! Ported from `04920ea31d0178b61aa87926e54795634c7cb4ce`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, Uri};
use axum::response::Response;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::Value;

use crate::state::AppState;

use super::{
    activate_timezone, db_error, ensure_feature_enabled, envelope, fields_param,
    into_result_response, owned, page_denial, preamble, proxy_through, query_last,
    require_project_entity, require_workspace_entity, rewrite_project_id, Denial, QueryMap,
};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the four scheduler paths (`api/urls/scheduler.py`). GET-only:
/// reads serve Rust, every other method falls through to Django through
/// [`owned`].
pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        // POST on the two list routes is the create / install write
        // (PDASHOSS01-318, [`super::writes`]).
        .route(
            "/api/v1/workspaces/{slug}/schedulers/",
            super::owned_get_post(
                get(ws_scheduler_list),
                post(super::writes::definition_create),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/",
            super::owned_get_post(
                get(project_scheduler_list),
                post(super::writes::binding_install),
            ),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/",
            owned(get(project_scheduler_detail)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/runs/",
            owned(get(project_scheduler_runs)),
        )
}

// ---------------------------------------------------------------------------
// Rows (`sqlx::FromRow` maps by column name)
// ---------------------------------------------------------------------------

/// One `schedulers` row: the columns `SchedulerAPISerializer` renders.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SchedulerRow {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub source: String,
    pub is_enabled: bool,
    pub color: String,
}

/// One binding row with its `select_related("scheduler", "last_run", "pod")`
/// legs: the columns `SchedulerBindingAPISerializer` renders.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BindingRow {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub scheduler_id: uuid::Uuid,
    pub dtstart: DateTime<Utc>,
    pub tzid: String,
    pub rrule: String,
    pub rdates: Value,
    pub exdates: Value,
    pub extra_context: String,
    pub enabled: bool,
    pub outcome_mode: String,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_id: Option<uuid::Uuid>,
    pub last_error: String,
    pub pod_id: Option<uuid::Uuid>,
    pub scheduler_slug: String,
    pub scheduler_name: String,
    pub pod_name: Option<String>,
    pub last_run_status: Option<String>,
    pub last_run_started_at: Option<DateTime<Utc>>,
    pub last_run_ended_at: Option<DateTime<Utc>>,
}

/// One agent-run row with its install's scheduler/project ids: the columns
/// `SchedulerRunAPISerializer` renders.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RunRow {
    pub id: uuid::Uuid,
    pub scheduler_binding_id: Option<uuid::Uuid>,
    pub status: String,
    pub trigger: String,
    pub phase_kind: String,
    pub error_code: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub scheduler_id: Option<uuid::Uuid>,
    pub project_id: Option<uuid::Uuid>,
}

/// One `_issues_by_run` attribution row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IssueTouchRow {
    pub speaker_agent_run_id: Option<uuid::Uuid>,
    pub issue_id: uuid::Uuid,
    pub name: String,
    pub sequence_id: i32,
    pub identifier: String,
}

// ---------------------------------------------------------------------------
// SQL (same JOIN/WHERE semantics as the Django querysets)
// ---------------------------------------------------------------------------

/// `Scheduler.objects.filter(workspace__slug=...).order_by("name")`: the
/// base manager scopes `schedulers.deleted_at IS NULL`.
const WORKSPACE_SCHEDULERS_SQL: &str = concat!(
    "SELECT ",
    r#""s"."created_at", "s"."updated_at", "s"."id", "s"."workspace_id", "s"."slug", "s"."name", "s"."description", "s"."prompt", "s"."source", "s"."is_enabled", "s"."color""#,
    r#" FROM "schedulers" "s" JOIN "workspaces" "w" ON "w"."id" = "s"."workspace_id" WHERE "s"."deleted_at" IS NULL AND "w"."slug" = $1 ORDER BY "s"."name" ASC"#,
);

/// `visible_bindings` for the workspace list: `_binding_queryset()` (soft-
/// delete scope, `select_related` legs, `-created_at`) filtered to the
/// page's schedulers, the caller's active memberships, and non-archived
/// projects. The membership span is a plain join (no `deleted_at` on
/// `project_members`), exactly as Django builds it.
const WORKSPACE_BINDINGS_SQL: &str = concat!(
    "SELECT ",
    r#""b"."created_at", "b"."updated_at", "b"."id", "b"."workspace_id", "b"."project_id", "b"."scheduler_id", "b"."dtstart", "b"."tzid", "b"."rrule", "b"."rdates", "b"."exdates", "b"."extra_context", "b"."enabled", "b"."outcome_mode", "b"."next_run_at", "b"."last_run_id", "b"."last_error", "b"."pod_id", "s"."slug" AS "scheduler_slug", "s"."name" AS "scheduler_name", "p"."name" AS "pod_name", "lr"."status" AS "last_run_status", "lr"."started_at" AS "last_run_started_at", "lr"."ended_at" AS "last_run_ended_at""#,
    r#" FROM "scheduler_bindings" "b" JOIN "schedulers" "s" ON "s"."id" = "b"."scheduler_id" LEFT JOIN "pod" "p" ON "p"."id" = "b"."pod_id" LEFT JOIN "agent_run" "lr" ON "lr"."id" = "b"."last_run_id" JOIN "projects" "pr" ON "pr"."id" = "b"."project_id" JOIN "project_members" "pm" ON "pm"."project_id" = "pr"."id" AND "pm"."member_id" = $2 AND "pm"."is_active" WHERE "b"."deleted_at" IS NULL AND "b"."scheduler_id" = ANY($1) AND "pr"."archived_at" IS NULL ORDER BY "b"."created_at" DESC"#,
);

/// `Scheduler.objects.filter(workspace__slug=..., bindings__project_id=...,
/// bindings__deleted_at__isnull=True).distinct().order_by("name")`: the
/// `EXISTS` leg is the `bindings__` span plus `distinct()` (one row per
/// scheduler either way).
const PROJECT_SCHEDULERS_SQL: &str = concat!(
    "SELECT ",
    r#""s"."created_at", "s"."updated_at", "s"."id", "s"."workspace_id", "s"."slug", "s"."name", "s"."description", "s"."prompt", "s"."source", "s"."is_enabled", "s"."color""#,
    r#" FROM "schedulers" "s" JOIN "workspaces" "w" ON "w"."id" = "s"."workspace_id" WHERE "s"."deleted_at" IS NULL AND "w"."slug" = $1 AND EXISTS (SELECT 1 FROM "scheduler_bindings" "b" WHERE "b"."scheduler_id" = "s"."id" AND "b"."project_id" = $2 AND "b"."deleted_at" IS NULL) ORDER BY "s"."name" ASC"#,
);

/// `_binding_queryset().filter(project_id=...)`: the project binding leg
/// for the project list/detail. No archived-project filter here (ported
/// quirk — only the workspace list filters it).
const PROJECT_BINDINGS_SQL: &str = concat!(
    "SELECT ",
    r#""b"."created_at", "b"."updated_at", "b"."id", "b"."workspace_id", "b"."project_id", "b"."scheduler_id", "b"."dtstart", "b"."tzid", "b"."rrule", "b"."rdates", "b"."exdates", "b"."extra_context", "b"."enabled", "b"."outcome_mode", "b"."next_run_at", "b"."last_run_id", "b"."last_error", "b"."pod_id", "s"."slug" AS "scheduler_slug", "s"."name" AS "scheduler_name", "p"."name" AS "pod_name", "lr"."status" AS "last_run_status", "lr"."started_at" AS "last_run_started_at", "lr"."ended_at" AS "last_run_ended_at""#,
    r#" FROM "scheduler_bindings" "b" JOIN "schedulers" "s" ON "s"."id" = "b"."scheduler_id" LEFT JOIN "pod" "p" ON "p"."id" = "b"."pod_id" LEFT JOIN "agent_run" "lr" ON "lr"."id" = "b"."last_run_id" WHERE "b"."deleted_at" IS NULL AND "b"."scheduler_id" = ANY($1) AND "b"."project_id" = $2 ORDER BY "b"."created_at" DESC"#,
);

/// The detail lookup: the project-list predicate plus the pk
/// (`get_object_or_404`).
const SCHEDULER_DETAIL_SQL: &str = concat!(
    "SELECT ",
    r#""s"."created_at", "s"."updated_at", "s"."id", "s"."workspace_id", "s"."slug", "s"."name", "s"."description", "s"."prompt", "s"."source", "s"."is_enabled", "s"."color""#,
    r#" FROM "schedulers" "s" JOIN "workspaces" "w" ON "w"."id" = "s"."workspace_id" WHERE "s"."deleted_at" IS NULL AND "w"."slug" = $1 AND "s"."id" = $2 AND EXISTS (SELECT 1 FROM "scheduler_bindings" "b" WHERE "b"."scheduler_id" = "s"."id" AND "b"."project_id" = $3 AND "b"."deleted_at" IS NULL)"#,
);

/// The runs install guard: `get_object_or_404(SchedulerBinding active,
/// scheduler, project, workspace slug)` — 404, not an empty page, when the
/// scheduler is not installed on this project.
const RUNS_GUARD_SQL: &str = r#"SELECT EXISTS(SELECT 1 FROM "scheduler_bindings" "b" JOIN "workspaces" "w" ON "w"."id" = "b"."workspace_id" WHERE "b"."deleted_at" IS NULL AND "b"."scheduler_id" = $1 AND "b"."project_id" = $2 AND "w"."slug" = $3)"#;

/// `AgentRun.objects.filter(workspace__slug=...,
/// scheduler_binding__scheduler_id=..., scheduler_binding__project_id=...)
/// .select_related("scheduler_binding").order_by("-created_at")`. The
/// binding span is an INNER JOIN (a NULL `scheduler_binding_id` row is
/// excluded in Django too); `agent_run` has no soft-delete column.
const RUNS_LIST_SQL: &str = r#"SELECT "r"."id", "r"."scheduler_binding_id", "r"."status", "r"."trigger", "r"."phase_kind", "r"."error_code", "r"."created_at", "r"."started_at", "r"."ended_at", "b"."scheduler_id", "b"."project_id" FROM "agent_run" "r" JOIN "scheduler_bindings" "b" ON "b"."id" = "r"."scheduler_binding_id" JOIN "workspaces" "w" ON "w"."id" = "r"."workspace_id" WHERE "w"."slug" = $1 AND "b"."scheduler_id" = $2 AND "b"."project_id" = $3 ORDER BY "r"."created_at" DESC"#;

/// `_issues_by_run`: the comments the runs posted
/// (`speaker_agent_run_id`), scoped to projects the caller is an active
/// member of. Base-manager soft-delete scope on `issue_comments` only; the
/// issue/project/membership spans are plain joins.
///
/// `IssueComment.Meta.ordering = ("-created_at",)` still applies to the
/// `.distinct()` queryset, and Django's `SQLCompiler.get_extra_select`
/// appends the ordering column to the `SELECT DISTINCT` list when it is
/// not already selected. The SQL therefore selects `created_at` and
/// `ORDER BY created_at DESC`, so each comment is its own row and a run
/// that commented N times on one issue lists it N times, newest comment
/// first (PIDASHCONV-1204). The extra `created_at` column is unmapped:
/// `IssueTouchRow` ignores it and the rendered entry stays
/// `{"id", "identifier", "name"}`.
const ISSUES_BY_RUN_SQL: &str = r#"SELECT DISTINCT "c"."speaker_agent_run_id", "c"."issue_id", "i"."name", "i"."sequence_id", "pr"."identifier", "c"."created_at" FROM "issue_comments" "c" JOIN "issues" "i" ON "i"."id" = "c"."issue_id" JOIN "projects" "pr" ON "pr"."id" = "i"."project_id" JOIN "project_members" "pm" ON "pm"."project_id" = "pr"."id" AND "pm"."member_id" = $2 AND "pm"."is_active" WHERE "c"."speaker_agent_run_id" = ANY($1) AND "c"."deleted_at" IS NULL ORDER BY "c"."created_at" DESC"#;

// ---------------------------------------------------------------------------
// Renders (`Meta.fields` order, DRF value rules)
// ---------------------------------------------------------------------------

fn uuid_string(id: &uuid::Uuid) -> Value {
    Value::String(id.to_string())
}

fn opt_uuid(id: &Option<uuid::Uuid>) -> Value {
    id.as_ref().map(uuid_string).unwrap_or(Value::Null)
}

fn opt_string(value: &Option<String>) -> Value {
    value
        .as_ref()
        .map(|v| Value::String(v.clone()))
        .unwrap_or(Value::Null)
}

fn render_datetime(dt: &DateTime<Utc>, tz: &Tz) -> Value {
    Value::String(crate::serializer::render_datetime_in(dt, tz))
}

fn opt_datetime(dt: &Option<DateTime<Utc>>, tz: &Tz) -> Value {
    dt.as_ref()
        .map(|d| render_datetime(d, tz))
        .unwrap_or(Value::Null)
}

/// `SchedulerBindingAPISerializer` in `Meta.fields` order. The nested
/// serializer is constructed without `fields`/`expand`, so no selection
/// applies inside.
pub fn render_binding(row: &BindingRow, tz: &Tz) -> Value {
    let mut map = serde_json::Map::with_capacity(24);
    map.insert("id".to_owned(), uuid_string(&row.id));
    map.insert("scheduler".to_owned(), uuid_string(&row.scheduler_id));
    map.insert(
        "scheduler_slug".to_owned(),
        Value::String(row.scheduler_slug.clone()),
    );
    map.insert(
        "scheduler_name".to_owned(),
        Value::String(row.scheduler_name.clone()),
    );
    map.insert("project".to_owned(), uuid_string(&row.project_id));
    map.insert("workspace".to_owned(), uuid_string(&row.workspace_id));
    map.insert("dtstart".to_owned(), render_datetime(&row.dtstart, tz));
    map.insert("tzid".to_owned(), Value::String(row.tzid.clone()));
    map.insert("rrule".to_owned(), Value::String(row.rrule.clone()));
    map.insert("rdates".to_owned(), row.rdates.clone());
    map.insert("exdates".to_owned(), row.exdates.clone());
    map.insert(
        "extra_context".to_owned(),
        Value::String(row.extra_context.clone()),
    );
    map.insert("enabled".to_owned(), Value::Bool(row.enabled));
    map.insert(
        "outcome_mode".to_owned(),
        Value::String(row.outcome_mode.clone()),
    );
    map.insert("pod".to_owned(), opt_uuid(&row.pod_id));
    // `pod_name = CharField(source="pod.name", default=None)`: null when
    // no pod (the LEFT JOIN leg).
    map.insert("pod_name".to_owned(), opt_string(&row.pod_name));
    map.insert("next_run_at".to_owned(), opt_datetime(&row.next_run_at, tz));
    map.insert("last_run".to_owned(), opt_uuid(&row.last_run_id));
    // `get_last_run_*`: the `last_run` leg when `last_run_id` is set,
    // else null. `last_run_status` reads `.status` off the cached row.
    map.insert(
        "last_run_status".to_owned(),
        opt_string(&row.last_run_status),
    );
    map.insert(
        "last_run_started_at".to_owned(),
        opt_datetime(&row.last_run_started_at, tz),
    );
    map.insert(
        "last_run_ended_at".to_owned(),
        opt_datetime(&row.last_run_ended_at, tz),
    );
    map.insert(
        "last_error".to_owned(),
        Value::String(row.last_error.clone()),
    );
    map.insert(
        "created_at".to_owned(),
        render_datetime(&row.created_at, tz),
    );
    map.insert(
        "updated_at".to_owned(),
        render_datetime(&row.updated_at, tz),
    );
    Value::Object(map)
}

/// `SchedulerAPISerializer` in `Meta.fields` order, before `fields`/`expand`
/// selection. `bindings` renders the `visible_bindings` the caller fetched.
pub fn render_scheduler(
    row: &SchedulerRow,
    bindings: &[BindingRow],
    tz: &Tz,
) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::with_capacity(13);
    map.insert("id".to_owned(), uuid_string(&row.id));
    map.insert("workspace".to_owned(), uuid_string(&row.workspace_id));
    map.insert("slug".to_owned(), Value::String(row.slug.clone()));
    map.insert("name".to_owned(), Value::String(row.name.clone()));
    map.insert(
        "description".to_owned(),
        Value::String(row.description.clone()),
    );
    map.insert("prompt".to_owned(), Value::String(row.prompt.clone()));
    map.insert("color".to_owned(), Value::String(row.color.clone()));
    map.insert("source".to_owned(), Value::String(row.source.clone()));
    map.insert(
        "is_builtin".to_owned(),
        Value::Bool(super::writes::is_builtin_slug(&row.slug)),
    );
    map.insert("is_enabled".to_owned(), Value::Bool(row.is_enabled));
    map.insert(
        "bindings".to_owned(),
        Value::Array(bindings.iter().map(|b| render_binding(b, tz)).collect()),
    );
    map.insert(
        "created_at".to_owned(),
        render_datetime(&row.created_at, tz),
    );
    map.insert(
        "updated_at".to_owned(),
        render_datetime(&row.updated_at, tz),
    );
    map
}

/// `BaseSerializer` selection (`api/serializers/base.py:28-117`): `fields`
/// keeps the named subset in declaration order (unknown names silently
/// ignored); then each `expand` name present in the (filtered) fields is
/// replaced — `workspace` by its lite serialization, anything else by the
/// `<expand>_id` getattr fallback, which is `None` on a scheduler (so
/// `expand=bindings` nulls the nested list — ported as written).
pub fn apply_fields_expand(
    mut map: serde_json::Map<String, Value>,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
    workspace_lite: &Value,
) -> serde_json::Map<String, Value> {
    if let Some(keep) = fields {
        map.retain(|key, _| keep.iter().any(|k| k == key));
    }
    if let Some(keys) = expand {
        for key in keys {
            if !map.contains_key(key) {
                continue;
            }
            if key == "workspace" {
                map.insert(key.clone(), workspace_lite.clone());
            } else {
                map.insert(key.clone(), Value::Null);
            }
        }
    }
    map
}

/// `WorkspaceLiteSerializer` (`api/serializers/workspace.py:10-21`):
/// `Meta.fields = ["name", "slug", "id"]`, no expansion inside.
pub async fn expand_workspace_lite(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
) -> Result<Value, Denial> {
    let row: Option<(String, String)> =
        sqlx::query_as(r#"SELECT "name", "slug" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| db_error(error, "expand-workspace"))?;
    let (name, slug) = row.ok_or(Denial::ServerError)?;
    let mut map = serde_json::Map::with_capacity(3);
    map.insert("name".to_owned(), Value::String(name));
    map.insert("slug".to_owned(), Value::String(slug));
    map.insert("id".to_owned(), uuid_string(workspace_id));
    Ok(Value::Object(map))
}

/// One `_issues_by_run` entry: `{"id", "identifier", "name"}` in insertion
/// order, with `identifier = f"{project_identifier}-{sequence_id}"`.
pub fn render_issue_touch(row: &IssueTouchRow) -> Value {
    let mut map = serde_json::Map::with_capacity(3);
    map.insert("id".to_owned(), uuid_string(&row.issue_id));
    map.insert(
        "identifier".to_owned(),
        Value::String(format!("{}-{}", row.identifier, row.sequence_id)),
    );
    map.insert("name".to_owned(), Value::String(row.name.clone()));
    Value::Object(map)
}

/// `SchedulerRunAPISerializer` in `Meta.fields` order. Constructed without
/// `fields`/`expand`, so no selection applies.
pub fn render_run(row: &RunRow, issues: &[Value], tz: &Tz) -> Value {
    let mut map = serde_json::Map::with_capacity(12);
    map.insert("id".to_owned(), uuid_string(&row.id));
    // `UUIDField(source="scheduler_binding.scheduler_id", default=None)`:
    // the install's ids via the joined binding.
    map.insert("scheduler".to_owned(), opt_uuid(&row.scheduler_id));
    map.insert(
        "scheduler_binding".to_owned(),
        opt_uuid(&row.scheduler_binding_id),
    );
    map.insert("project".to_owned(), opt_uuid(&row.project_id));
    map.insert("status".to_owned(), Value::String(row.status.clone()));
    map.insert("trigger".to_owned(), Value::String(row.trigger.clone()));
    map.insert(
        "phase_kind".to_owned(),
        Value::String(row.phase_kind.clone()),
    );
    map.insert(
        "error_code".to_owned(),
        Value::String(row.error_code.clone()),
    );
    map.insert(
        "created_at".to_owned(),
        render_datetime(&row.created_at, tz),
    );
    map.insert("started_at".to_owned(), opt_datetime(&row.started_at, tz));
    map.insert("ended_at".to_owned(), opt_datetime(&row.ended_at, tz));
    map.insert("issues".to_owned(), Value::Array(issues.to_vec()));
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// Fetches
// ---------------------------------------------------------------------------

async fn fetch_schedulers(
    pool: &sqlx::PgPool,
    sql: &str,
    slug: &str,
    project_id: Option<&uuid::Uuid>,
) -> Result<Vec<SchedulerRow>, Denial> {
    let mut query = sqlx::query_as::<_, SchedulerRow>(sql).bind(slug);
    if let Some(pid) = project_id {
        query = query.bind(pid);
    }
    query
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "schedulers-list"))
}

async fn fetch_bindings(
    pool: &sqlx::PgPool,
    sql: &str,
    scheduler_ids: &[uuid::Uuid],
    scope_id: &uuid::Uuid,
) -> Result<Vec<BindingRow>, Denial> {
    sqlx::query_as::<_, BindingRow>(sql)
        .bind(scheduler_ids)
        .bind(scope_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "bindings-list"))
}

fn group_bindings(rows: Vec<BindingRow>) -> HashMap<uuid::Uuid, Vec<BindingRow>> {
    let mut grouped: HashMap<uuid::Uuid, Vec<BindingRow>> = HashMap::new();
    for row in rows {
        grouped.entry(row.scheduler_id).or_default().push(row);
    }
    grouped
}

async fn fetch_runs(
    pool: &sqlx::PgPool,
    slug: &str,
    scheduler_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<Vec<RunRow>, Denial> {
    sqlx::query_as::<_, RunRow>(RUNS_LIST_SQL)
        .bind(slug)
        .bind(scheduler_id)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "runs-list"))
}

async fn fetch_issues_by_run(
    pool: &sqlx::PgPool,
    run_ids: &[uuid::Uuid],
    actor_id: &uuid::Uuid,
) -> Result<HashMap<uuid::Uuid, Vec<Value>>, Denial> {
    if run_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<IssueTouchRow> = sqlx::query_as::<_, IssueTouchRow>(ISSUES_BY_RUN_SQL)
        .bind(run_ids)
        .bind(actor_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "issues-by-run"))?;
    let mut grouped: HashMap<uuid::Uuid, Vec<Value>> = HashMap::new();
    for row in rows {
        if let Some(run_id) = row.speaker_agent_run_id {
            grouped
                .entry(run_id)
                .or_default()
                .push(render_issue_touch(&row));
        }
    }
    Ok(grouped)
}

// ---------------------------------------------------------------------------
// Pagination (the `v1_projects` list shape over `OffsetPaginator` math)
// ---------------------------------------------------------------------------

/// Slice evaluated rows into the requested window: `queryset[offset:stop]`
/// over the full list, the back-slice rule for mismatched reverse cursors,
/// and `has_more` for the next cursor.
fn window_page<'a, T>(
    rows: &'a [T],
    per_page: i64,
    cursor: &crate::paginator::Cursor,
) -> Result<(&'a [T], bool), Denial> {
    let window = crate::paginator::offset_window(
        per_page,
        cursor.offset,
        cursor.value,
        cursor.is_prev,
        None,
    )
    .map_err(page_denial)?;
    let start = (window.offset as usize).min(rows.len());
    let stop = (window.stop as usize).min(rows.len());
    let window_rows = &rows[start..stop];
    // Backwards walk with a mismatched cursor value reads nothing
    // (`results[-(limit+1):]` on the lazy queryset raises into the 500).
    if !cursor.value.equals_limit(per_page) && cursor.is_prev {
        return Err(Denial::ServerError);
    }
    let has_more = window_rows.len() as i64 > per_page;
    // `results[:limit]` over the evaluated window (negative limits already
    // errored in `offset_window`).
    let trim = usize::try_from(per_page)
        .unwrap_or(usize::MAX)
        .min(window_rows.len());
    Ok((&window_rows[..trim], has_more))
}

fn parse_pagination(
    query: &QueryMap,
    default_per_page: i64,
    max_per_page: i64,
) -> Result<(i64, crate::paginator::Cursor), Denial> {
    let per_page = crate::paginator::parse_per_page(
        query_last(query, "per_page").as_deref(),
        default_per_page,
        max_per_page,
    )
    .map_err(page_denial)?;
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = crate::paginator::Cursor::from_string(&cursor_raw).map_err(page_denial)?;
    Ok((per_page, cursor))
}

// ---------------------------------------------------------------------------
// `GET workspaces/<slug>/schedulers/`
// ---------------------------------------------------------------------------

/// `WorkspaceSchedulerListAPIEndpoint.get`: workspace definitions with the
/// caller's visible bindings, paginated (default/max `per_page` 1000).
async fn ws_scheduler_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
) -> Response {
    into_result_response(ws_scheduler_list_inner(&state, &headers, &slug, &query).await)
}

async fn ws_scheduler_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    require_workspace_entity(&pre.pool, pre.workspace_id.as_ref(), slug, &pre.actor.id).await?;
    ensure_feature_enabled(state.settings())?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s only for survivors).
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    let (per_page, cursor) = parse_pagination(query, 1000, 1000)?;
    let rows = fetch_schedulers(&pre.pool, WORKSPACE_SCHEDULERS_SQL, slug, None).await?;
    let total_count = rows.len() as i64;
    let (page_rows, has_more) = window_page(&rows, per_page, &cursor)?;
    let scheduler_ids: Vec<uuid::Uuid> = page_rows.iter().map(|r| r.id).collect();
    let grouped = group_bindings(
        fetch_bindings(
            &pre.pool,
            WORKSPACE_BINDINGS_SQL,
            &scheduler_ids,
            &pre.actor.id,
        )
        .await?,
    );
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let workspace_lite = expand_workspace_lite(&pre.pool, &workspace_id).await?;
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let bindings = grouped.get(&row.id).map(Vec::as_slice).unwrap_or(&[]);
        let shaped = render_scheduler(row, bindings, &timezone);
        let selected = apply_fields_expand(
            shaped,
            fields.as_deref(),
            expand.as_deref(),
            &workspace_lite,
        );
        rendered.push(Value::Object(selected));
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

// ---------------------------------------------------------------------------
// `GET workspaces/<slug>/projects/<project_id>/schedulers/`
// ---------------------------------------------------------------------------

/// `ProjectSchedulerListAPIEndpoint.get`: definitions installed on this
/// project, each with its binding here, paginated (default/max 1000).
async fn project_scheduler_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
) -> Response {
    into_result_response(
        project_scheduler_list_inner(&state, &headers, &slug, &project_raw, &query).await,
    )
}

async fn project_scheduler_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_project_entity(
        &pre.pool,
        pre.workspace_id.as_ref(),
        slug,
        &pre.actor.id,
        &project_id,
        super::perms::V1Route::ProjectList,
    )
    .await?;
    ensure_feature_enabled(state.settings())?;
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    let (per_page, cursor) = parse_pagination(query, 1000, 1000)?;
    let rows = fetch_schedulers(&pre.pool, PROJECT_SCHEDULERS_SQL, slug, Some(&project_id)).await?;
    let total_count = rows.len() as i64;
    let (page_rows, has_more) = window_page(&rows, per_page, &cursor)?;
    let scheduler_ids: Vec<uuid::Uuid> = page_rows.iter().map(|r| r.id).collect();
    let grouped = group_bindings(
        fetch_bindings(&pre.pool, PROJECT_BINDINGS_SQL, &scheduler_ids, &project_id).await?,
    );
    let fields = fields_param(query, "fields");
    let expand = fields_param(query, "expand");
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    let workspace_lite = expand_workspace_lite(&pre.pool, &workspace_id).await?;
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let bindings = grouped.get(&row.id).map(Vec::as_slice).unwrap_or(&[]);
        let shaped = render_scheduler(row, bindings, &timezone);
        let selected = apply_fields_expand(
            shaped,
            fields.as_deref(),
            expand.as_deref(),
            &workspace_lite,
        );
        rendered.push(Value::Object(selected));
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

// ---------------------------------------------------------------------------
// `GET .../schedulers/<uuid:scheduler_id>/`
// ---------------------------------------------------------------------------

/// `ProjectSchedulerDetailAPIEndpoint.get`: one full definition with its
/// binding here. 404 unless installed on this project. `?fields=` /
/// `?expand=` are ignored (the serializer is constructed without them).
async fn project_scheduler_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, scheduler_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Django routes this segment through the `<uuid:>` converter
    // (`api/urls/scheduler.py:26`), whose regex is lowercase-hyphenated
    // canonical form only and is matched case-sensitively, so every other
    // spelling resolver-404s before auth (and before the view). `Uuid::parse`
    // also accepts uppercase, simple, braced and URN spellings, so gate on the
    // raw segment first and proxy the rest — parseable-but-non-canonical and
    // garbage alike — for Django's own resolver 404 (PIDASHCONV-1095 /
    // -1132 / -1170 / -1180 / -1193 precedent).
    let scheduler_id = match scheduler_raw.parse::<uuid::Uuid>() {
        Ok(scheduler_id) if is_canonical_uuid(&scheduler_raw) => scheduler_id,
        _ => return proxy_through(state, method, uri, headers, body).await,
    };
    into_result_response(
        project_scheduler_detail_inner(&state, &headers, &slug, &project_raw, &scheduler_id).await,
    )
}

async fn project_scheduler_detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    scheduler_id: &uuid::Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_project_entity(
        &pre.pool,
        pre.workspace_id.as_ref(),
        slug,
        &pre.actor.id,
        &project_id,
        super::perms::V1Route::ProjectDetail,
    )
    .await?;
    ensure_feature_enabled(state.settings())?;
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    let row: Option<SchedulerRow> = sqlx::query_as::<_, SchedulerRow>(SCHEDULER_DETAIL_SQL)
        .bind(slug)
        .bind(scheduler_id)
        .bind(project_id)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "scheduler-detail"))?;
    let Some(row) = row else {
        return Err(Denial::SchedulerNotFound);
    };
    let bindings = fetch_bindings(&pre.pool, PROJECT_BINDINGS_SQL, &[row.id], &project_id).await?;
    let shaped = render_scheduler(&row, &bindings, &timezone);
    Ok(super::ok_json(Value::Object(shaped)))
}

// ---------------------------------------------------------------------------
// `GET .../schedulers/<uuid:scheduler_id>/runs/`
// ---------------------------------------------------------------------------

/// `ProjectSchedulerRunsAPIEndpoint.get`: recent runs fired by this install,
/// newest first, paginated (`per_page` default 30, max 100), each with the
/// issues it wrote to.
async fn project_scheduler_runs(
    State(state): State<AppState>,
    Path((slug, project_raw, scheduler_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Same canonical-spelling gate as the detail handler: only a
    // lowercase-hyphenated segment matches Django's `<uuid:>` route;
    // everything else proxies for the resolver 404 (PIDASHCONV-1205).
    let scheduler_id = match scheduler_raw.parse::<uuid::Uuid>() {
        Ok(scheduler_id) if is_canonical_uuid(&scheduler_raw) => scheduler_id,
        _ => return proxy_through(state, method, uri, headers, body).await,
    };
    into_result_response(
        project_scheduler_runs_inner(&state, &headers, &slug, &project_raw, &scheduler_id, &query)
            .await,
    )
}

async fn project_scheduler_runs_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    scheduler_id: &uuid::Uuid,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_raw).await?;
    require_project_entity(
        &pre.pool,
        pre.workspace_id.as_ref(),
        slug,
        &pre.actor.id,
        &project_id,
        super::perms::V1Route::ProjectRuns,
    )
    .await?;
    ensure_feature_enabled(state.settings())?;
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    // 404 (not an empty page) for a scheduler that isn't installed on
    // this project, matching the detail endpoint.
    let installed: bool = sqlx::query_scalar(RUNS_GUARD_SQL)
        .bind(scheduler_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(&pre.pool)
        .await
        .map_err(|error| db_error(error, "runs-guard"))?
        .unwrap_or(false);
    if !installed {
        return Err(Denial::BindingNotFound);
    }
    let (per_page, cursor) = parse_pagination(query, 30, 100)?;
    let rows = fetch_runs(&pre.pool, slug, scheduler_id, &project_id).await?;
    let total_count = rows.len() as i64;
    let (page_rows, has_more) = window_page(&rows, per_page, &cursor)?;
    let run_ids: Vec<uuid::Uuid> = page_rows.iter().map(|r| r.id).collect();
    let issues_by_run = fetch_issues_by_run(&pre.pool, &run_ids, &pre.actor.id).await?;
    let mut rendered: Vec<Value> = Vec::with_capacity(page_rows.len());
    for row in page_rows {
        let issues = issues_by_run.get(&row.id).map(Vec::as_slice).unwrap_or(&[]);
        rendered.push(render_run(row, issues, &timezone));
    }
    let next = crate::paginator::next_cursor(per_page, cursor.offset, has_more);
    let prev = crate::paginator::prev_cursor(per_page, cursor.offset);
    envelope(total_count, per_page, &next, &prev, Value::Array(rendered))
}

/// Whether a path segment is spelled exactly the way Django's
/// `UUIDConverter` routes it: lowercase hex, hyphenated, 36 characters
/// (`django/urls/converters.py:25`; the `v1_cli_auth` PIDASHCONV-1095 and
/// `license` PIDASHCONV-1132 precedents).
pub(crate) fn is_canonical_uuid(raw: &str) -> bool {
    if raw.len() != 36 {
        return false;
    }
    raw.bytes().enumerate().all(|(index, byte)| {
        if index == 8 || index == 13 || index == 18 || index == 23 {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn row() -> SchedulerRow {
        SchedulerRow {
            created_at: Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap(),
            id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            slug: "nightly".to_owned(),
            name: "Nightly".to_owned(),
            description: "d".to_owned(),
            prompt: "Scan.".to_owned(),
            source: "builtin".to_owned(),
            is_enabled: true,
            color: "#3b82f6".to_owned(),
        }
    }

    fn binding() -> BindingRow {
        BindingRow {
            created_at: Utc.with_ymd_and_hms(2026, 10, 3, 12, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap(),
            id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            scheduler_id: uuid::Uuid::nil(),
            dtstart: Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap(),
            tzid: "UTC".to_owned(),
            rrule: "FREQ=HOURLY".to_owned(),
            rdates: Value::Array(vec![]),
            exdates: Value::Array(vec![]),
            extra_context: String::new(),
            enabled: true,
            outcome_mode: "create_issue".to_owned(),
            next_run_at: None,
            last_run_id: None,
            last_error: String::new(),
            pod_id: None,
            scheduler_slug: "nightly".to_owned(),
            scheduler_name: "Nightly".to_owned(),
            pod_name: None,
            last_run_status: None,
            last_run_started_at: None,
            last_run_ended_at: None,
        }
    }

    fn workspace_lite() -> Value {
        serde_json::json!({"name": "Acme", "slug": "acme", "id": uuid::Uuid::nil().to_string()})
    }

    #[test]
    fn scheduler_key_order_matches_meta_fields() {
        let map = render_scheduler(&row(), &[binding()], &chrono_tz::UTC);
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "workspace",
                "slug",
                "name",
                "description",
                "prompt",
                "color",
                "source",
                "is_enabled",
                "bindings",
                "created_at",
                "updated_at",
            ]
        );
    }

    #[test]
    fn binding_key_order_matches_meta_fields() {
        let Value::Object(map) = render_binding(&binding(), &chrono_tz::UTC) else {
            panic!("binding renders an object");
        };
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "scheduler",
                "scheduler_slug",
                "scheduler_name",
                "project",
                "workspace",
                "dtstart",
                "tzid",
                "rrule",
                "rdates",
                "exdates",
                "extra_context",
                "enabled",
                "outcome_mode",
                "pod",
                "pod_name",
                "next_run_at",
                "last_run",
                "last_run_status",
                "last_run_started_at",
                "last_run_ended_at",
                "last_error",
                "created_at",
                "updated_at",
            ]
        );
    }

    #[test]
    fn fields_keep_declaration_order() {
        let map = render_scheduler(&row(), &[], &chrono_tz::UTC);
        let keep = ["prompt".to_owned(), "slug".to_owned(), "nope".to_owned()];
        let selected = apply_fields_expand(map, Some(&keep), None, &workspace_lite());
        let keys: Vec<&str> = selected.keys().map(String::as_str).collect();
        // Declaration order (`slug` before `prompt`), unknown names dropped.
        assert_eq!(keys, vec!["slug", "prompt"]);
    }

    #[test]
    fn expand_workspace_replaces_id_with_lite() {
        let map = render_scheduler(&row(), &[], &chrono_tz::UTC);
        let expand = ["workspace".to_owned()];
        let selected = apply_fields_expand(map, None, Some(&expand), &workspace_lite());
        assert_eq!(selected["workspace"], workspace_lite());
    }

    #[test]
    fn expand_unknown_name_nulls_the_field() {
        let map = render_scheduler(&row(), &[binding()], &chrono_tz::UTC);
        // `expand=bindings` hits the `<name>_id` getattr fallback (`None`).
        let expand = ["bindings".to_owned()];
        let selected = apply_fields_expand(map, None, Some(&expand), &workspace_lite());
        assert_eq!(selected["bindings"], Value::Null);
    }

    #[test]
    fn expand_skips_fields_trimmed_by_fields() {
        let map = render_scheduler(&row(), &[], &chrono_tz::UTC);
        let keep = ["slug".to_owned()];
        let expand = ["workspace".to_owned()];
        let selected = apply_fields_expand(map, Some(&keep), Some(&expand), &workspace_lite());
        assert!(!selected.contains_key("workspace"));
        assert_eq!(keys_of(&selected), vec!["slug"]);
    }

    fn keys_of(map: &serde_json::Map<String, Value>) -> Vec<&str> {
        map.keys().map(String::as_str).collect()
    }

    #[test]
    fn run_key_order_matches_meta_fields() {
        let run = RunRow {
            id: uuid::Uuid::nil(),
            scheduler_binding_id: Some(uuid::Uuid::nil()),
            status: "failed".to_owned(),
            trigger: "scheduler".to_owned(),
            phase_kind: String::new(),
            error_code: "watchdog_timeout".to_owned(),
            created_at: Utc.with_ymd_and_hms(2026, 10, 5, 12, 0, 0).unwrap(),
            started_at: None,
            ended_at: None,
            scheduler_id: Some(uuid::Uuid::nil()),
            project_id: Some(uuid::Uuid::nil()),
        };
        let Value::Object(map) = render_run(&run, &[], &chrono_tz::UTC) else {
            panic!("run renders an object");
        };
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "scheduler",
                "scheduler_binding",
                "project",
                "status",
                "trigger",
                "phase_kind",
                "error_code",
                "created_at",
                "started_at",
                "ended_at",
                "issues",
            ]
        );
    }

    #[test]
    fn issue_touch_identifier_joins_project_and_sequence() {
        let touch = IssueTouchRow {
            speaker_agent_run_id: Some(uuid::Uuid::nil()),
            issue_id: uuid::Uuid::nil(),
            name: "Filed by scheduler".to_owned(),
            sequence_id: 7,
            identifier: "SCH".to_owned(),
        };
        assert_eq!(
            render_issue_touch(&touch),
            serde_json::json!({
                "id": uuid::Uuid::nil().to_string(),
                "identifier": "SCH-7",
                "name": "Filed by scheduler",
            })
        );
    }

    #[test]
    fn datetimes_render_drf_iso8601() {
        let map = render_scheduler(&row(), &[], &chrono_tz::UTC);
        assert_eq!(
            map["created_at"],
            Value::String("2026-10-01T12:00:00Z".to_owned())
        );
        let Value::Object(bmap) = render_binding(&binding(), &chrono_tz::UTC) else {
            panic!("binding renders an object");
        };
        assert_eq!(
            bmap["dtstart"],
            Value::String("2026-10-01T00:00:00Z".to_owned())
        );
    }

    #[test]
    fn canonical_uuid_accepts_lowercase_hyphenated_only() {
        assert!(is_canonical_uuid("12345678-1234-1234-1234-1234567890ab"));
        assert!(is_canonical_uuid("00000000-0000-0000-0000-000000000000"));
        // Every spelling `Uuid::parse` accepts but Django's `<uuid:>`
        // converter does not route.
        for raw in [
            "12345678-1234-1234-1234-1234567890AB",
            "123456781234123412341234567890ab",
            "{12345678-1234-1234-1234-1234567890ab}",
            "urn:uuid:12345678-1234-1234-1234-1234567890ab",
            "12345678-1234-1234-1234-1234567890a",
            "12345678-1234-1234-1234-1234567890abc",
            "123456781234-1234-1234-1234567890ab",
        ] {
            assert!(!is_canonical_uuid(raw), "{raw}");
        }
    }
}
