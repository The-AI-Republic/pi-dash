#![forbid(unsafe_code)]

//! Project issue-list family handlers (pilot 2 of D-26).
//!
//! Ports the read slice of `app/views/issue/base.py` onto the foundation
//! crates:
//! - `GET .../issues/` (`IssueViewSet.list`)
//! - `GET .../issues/list/` (`IssueListEndpoint.get`)
//! - `GET .../issues-detail/` (`IssueDetailEndpoint.get`)
//! - `GET .../v2/issues/` (`IssuePaginatedViewSet.list`)
//! - `GET .../deleted-issues/` (`DeletedIssuesListViewSet.get`)
//!
//! Only these four GET paths are registered, so the edge serves exactly
//! this family from Rust while every sibling path (detail, create, …)
//! keeps proxying to Django — route registration is the cutover
//! granularity, no flag needed.
//!
//! Layering: query-param parsing, ordering and shapes live in
//! `pidash_services::app_issues`; the filter compilers in
//! `pidash_db::{filter, filterset, issue_filters}`; cursor math in
//! [`crate::paginator`]; datetime rendering in [`crate::serializer`].
//! This module owns the HTTP shell (routes, session auth, the
//! `@allow_permission` gate), the SQL text, and the row fetching.
//!
//! SQL reads go through `row_to_json`: Postgres renders every column
//! (uuids, timestamptz, dates, arrays) and the handlers re-render only
//! the datetime keys through the serializer kernel, so key order and
//! scalar bytes stay under handler control.
//!
//! Ported bugs (also listed in the PR):
//! - `recent_visited_task.delay` is a deferred publish the suite
//!   environment never consumes, so the handlers perform no write
//!   (an inline upsert broke the gate's teardown with rows Django never
//!   produces); faithful deferral belongs to the tasks layer.
//! - `order_by=priority` / `-priority` both sort ascending (the queryset
//!   `.order_by("priority_order", "-created_at")` is unconditional; only
//!   the echoed param differs) — see `pidash_services::app_issues`.
//! - ascending `state__group` uses the forward state order (the
//!   `[::-1]` branch is dead) — same module.
//! - grouped totals count a zero-count group as one
//!   (`1 if count == 0 else count` in `__get_total_dict`).
//! - the manager's triage exclusion drops NULL-state rows with it
//!   (`NOT (group = 'triage')` over a left join, three-valued logic).

pub mod render;

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono_tz::Tz;
use sea_query::backend::PostgresQueryBuilder;
use sea_query::{Condition, Expr, SelectStatement};
use serde_json::{Map, Value};
use sqlx::postgres::PgArguments;
use sqlx::{Postgres, Row};

use crate::state::AppState;

use pidash_services::app_issues::{
    deleted_ids_body, envelope, on_results_fields, order_sql, raw_group_mismatch, v2_fields,
    ListParams, OrderSpec, ParseOptions, DETAIL_FIELDS, LIST_VALUES_FIELDS, PRIORITY_VALUES,
    STATE_GROUP_VALUES,
};

use render::v2_page;

/// Register the five list-family GET routes. Nothing else: sibling paths
/// stay unmatched and proxy to Django.
///
/// Non-GET methods on owned paths proxy too. DRF authenticates before it
/// checks the method, and `POST issues/` is the create endpoint other
/// splits own — answering 405 in Rust would break both (`POST issues/`
/// must be Django's 401-anon / create, `POST issues/list/` Django's own
/// 405-after-auth). Proxying every non-GET method reproduces all of that
/// with no per-method logic; `HEAD` rides axum's `get` handling like
/// Django's `GET`-backed `HEAD`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/issues/",
            owned(get(list_issues)),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/issues/list/",
            owned(get(flat_list)),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/issues-detail/",
            owned(get(detail_list)),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/v2/issues/",
            owned(get(v2_list)),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/deleted-issues/",
            owned(get(deleted_list)),
        )
}

/// A list-family path: the GET handler owns reads, everything else falls
/// through to Django (its create/update/delete/405s live there). OPTIONS
/// proxies too: DRF answers metadata (401 anon / 200 authed) where axum
/// would 405.
fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// Exact bytes of the DRF `IsAuthenticated` denial.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// DRF `exception_handler` maps `Http404(*args)` to `NotFound(*args)`, so
/// the `_rewrite_project_kwarg` miss (`Project.resolve`, "Project not
/// found") renders with the resolve message — verified against live
/// Django, not the bare default.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `ValidationError` branch (bad UUID in `issues=`).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;

/// One query value, repeated or not. `serde_html_form` (axum's `Query`
/// backend) does not coerce a lone `?key=value` into a sequence, so the
/// extractor uses this untagged shape and callers read first/last —
/// mirroring Django's `QueryDict`, where repeats are legal and `.get`
/// returns the last value.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every list handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// All values for `key`, in order; `None` when absent.
pub fn query_values(query: &QueryMap, key: &str) -> Option<Vec<String>> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => vec![one.clone()],
        OneOrMany::Many(many) => many.clone(),
    })
}

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query_values(query, key).and_then(|values| values.into_iter().last())
}

/// Plain multi-map for the services layer (pure, axum-free).
pub fn multi_map(query: &QueryMap) -> HashMap<String, Vec<String>> {
    query
        .keys()
        .filter_map(|key| query_values(query, key).map(|values| (key.clone(), values)))
        .collect()
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` / view-inline body.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"detail":"Project not found"}` (project-kwarg rewrite miss).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (`ParseError`, filter validation).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 400, `{"message": ..., "code": ...}` (filter validation).
    BadFilter(String, String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadFilter(message, code) => (
                StatusCode::BAD_REQUEST,
                format!(
                    "{{\"message\":{},\"code\":{}}}",
                    json_string(message),
                    json_string(code)
                ),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// The authenticated, authorized request context: who acts, in which
/// tenant, with which list scoping.
#[derive(Debug, Clone, Copy)]
pub struct Gate {
    pub user_id: uuid::Uuid,
    pub timezone: Tz,
    pub workspace_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    /// `created_by = user` scoping for guests without view-all.
    pub guest_scoped: bool,
}

/// Session auth + `_rewrite_project_kwarg` + `@allow_permission([ADMIN,
/// MEMBER, GUEST])` + guest scoping, in Django's order: anonymous skips
/// the identifier rewrite (the slug-existence oracle stays closed) and is
/// rejected 401 before anything else; the rewrite 404s unresolvable
/// identifiers with `{"detail":"Project not found"}`; the role gate 403s before the
/// view body runs, so a missing project row for a valid UUID answers 403
/// (no membership) rather than 404.
pub async fn resolve_gate(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Gate, Denial> {
    let pool = state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let project_id = resolve_project_id(pool, slug, project_raw).await?;
    allow_project(pool, slug, &project_id, &user_id).await?;
    let tenant = tenant_context(pool, &project_id, &user_id).await?;
    Ok(Gate {
        user_id,
        timezone: tenant.timezone,
        workspace_id: tenant.workspace_id,
        project_id,
        guest_scoped: tenant.guest_scoped,
    })
}

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, or a non-UUID id means anonymous → 401. (Django PKs are UUIDs;
/// a session id that is not a UUID cannot be a user.)
fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    let raw = session.get("_auth_user_id")?.as_str()?.to_owned();
    raw.parse::<uuid::Uuid>().ok()
}

struct TenantContext {
    workspace_id: uuid::Uuid,
    timezone: Tz,
    guest_scoped: bool,
}

/// `Project.resolve(workspace_slug, value)`: UUIDs pass through (the row
/// check happens in the view body); other identifiers match
/// `UPPER(identifier)` in the workspace; misses raise `Http404`.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
}

/// `allow_permission([ADMIN, MEMBER, GUEST])` at `PROJECT` level: an
/// active project membership with role 20/15/5, or any active project
/// membership plus an active workspace ADMIN membership. Everything else
/// (including a valid UUID with no membership row) is the allow-style
/// 403. Soft-deleted memberships do not count (`SoftDeletionManager`).
async fn allow_project(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(), Denial> {
    let role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match role {
        Some((20,)) | Some((15,)) | Some((5,)) => Ok(()),
        _ => {
            let member: Option<(i32,)> = sqlx::query_as(
                r#"SELECT 1 FROM project_members pm
                   JOIN workspaces w ON w.id = pm.workspace_id
                   WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
                   AND pm.is_active AND pm.deleted_at IS NULL"#,
            )
            .bind(user_id)
            .bind(project_id)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let admin: Option<(i32,)> = sqlx::query_as(
                r#"SELECT 1 FROM workspace_members wm
                   JOIN workspaces w ON w.id = wm.workspace_id
                   WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = 20
                   AND wm.is_active AND wm.deleted_at IS NULL"#,
            )
            .bind(user_id)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            if member.is_some() && admin.is_some() {
                Ok(())
            } else {
                Err(Denial::Forbidden)
            }
        }
    }
}

/// View-body tenant facts: the project row must exist in the workspace
/// (`Project.objects.get` → 404 error body), plus the actor's timezone
/// and the guest `created_by` scoping
/// (`role == 5 and not project.guest_view_all_features`).
async fn tenant_context(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<TenantContext, Denial> {
    let row: Option<(uuid::Uuid, bool)> = sqlx::query_as(
        r#"SELECT p.workspace_id, p.guest_view_all_features FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (workspace_id, guest_view_all) = row.ok_or(Denial::NotFound)?;
    let member: Option<(String, i16)> = sqlx::query_as(
        r#"SELECT u.user_timezone, pm.role FROM users u
           LEFT JOIN project_members pm ON pm.member_id = u.id
             AND pm.project_id = $2 AND pm.is_active AND pm.deleted_at IS NULL
           WHERE u.id = $1"#,
    )
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (timezone_name, role) = member.ok_or(Denial::ServerError)?;
    let timezone: Tz = timezone_name.parse().map_err(|_| Denial::ServerError)?;
    let guest_scoped = role == 5 && !guest_view_all;
    Ok(TenantContext {
        workspace_id,
        timezone,
        guest_scoped,
    })
}

// ---- SQL assembly ---------------------------------------------------------

/// Ordered bind parameters. Placeholders are `$1…`; [`Binder::splice`]
/// merges a pre-rendered fragment (kernel conditions) by shifting its
/// `$k` references past the parameters already bound.
#[derive(Default)]
pub struct Binder {
    values: Vec<sea_query::Value>,
}

impl Binder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&mut self, value: sea_query::Value) -> String {
        self.values.push(value);
        format!("${}", self.values.len())
    }

    pub fn bind_uuid(&mut self, id: uuid::Uuid) -> String {
        self.bind(sea_query::Value::Uuid(Some(Box::new(id))))
    }

    pub fn bind_string(&mut self, text: String) -> String {
        self.bind(sea_query::Value::String(Some(Box::new(text))))
    }

    /// Splice a `(sql, values)` fragment in, renumbering `$k`.
    pub fn splice(&mut self, sql: &str, values: Vec<sea_query::Value>) -> String {
        let offset = self.values.len();
        self.values.extend(values);
        if offset == 0 {
            return sql.to_owned();
        }
        let mut out = String::with_capacity(sql.len());
        let mut chars = sql.chars().peekable();
        while let Some(char) = chars.next() {
            if char == '$' {
                let mut digits = String::new();
                while let Some(next) = chars.peek() {
                    if next.is_ascii_digit() {
                        digits.push(*next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if digits.is_empty() {
                    out.push('$');
                } else {
                    let shifted: usize = digits.parse().unwrap_or(0) + offset;
                    out.push('$');
                    out.push_str(&shifted.to_string());
                }
            } else {
                out.push(char);
            }
        }
        out
    }

    pub fn values(self) -> Vec<sea_query::Value> {
        self.values
    }
}

/// Render a kernel [`Condition`] to a `WHERE`-ready fragment plus binds.
/// The `SELECT 1 WHERE` shell keeps user placeholders numbered from `$1`
/// (`Expr::cust` emits no bind of its own).
pub fn render_condition(cond: &Condition) -> (String, Vec<sea_query::Value>) {
    let mut stmt = SelectStatement::new();
    stmt.expr(Expr::cust("1")).cond_where(cond.clone());
    let (sql, values) = stmt.build(PostgresQueryBuilder);
    let fragment = sql
        .strip_prefix("SELECT 1 WHERE ")
        .expect("condition shell")
        .to_owned();
    (fragment, values.0)
}

fn bind_sea_value<'q>(
    query: sqlx::query::Query<'q, Postgres, PgArguments>,
    value: &sea_query::Value,
) -> Result<sqlx::query::Query<'q, Postgres, PgArguments>, Denial> {
    use sea_query::Value as SeaValue;
    match value {
        SeaValue::Bool(value) => Ok(query.bind(*value)),
        SeaValue::TinyInt(value) => Ok(query.bind(*value)),
        SeaValue::SmallInt(value) => Ok(query.bind(*value)),
        SeaValue::Int(value) => Ok(query.bind(*value)),
        SeaValue::BigInt(value) => Ok(query.bind(*value)),
        SeaValue::TinyUnsigned(value) => Ok(query.bind(value.map(i16::from))),
        SeaValue::SmallUnsigned(value) => Ok(query.bind(value.map(i32::from))),
        SeaValue::Unsigned(value) => Ok(query.bind(value.map(|v| v as i64))),
        SeaValue::BigUnsigned(value) => Ok(query.bind(value.map(|v| v as i64))),
        SeaValue::Float(value) => Ok(query.bind(*value)),
        SeaValue::Double(value) => Ok(query.bind(*value)),
        SeaValue::String(value) => Ok(query.bind(value.clone().map(|text| *text))),
        SeaValue::Char(value) => Ok(query.bind(value.map(|char| char.to_string()))),
        SeaValue::Bytes(value) => Ok(query.bind(value.clone().map(|bytes| (*bytes).clone()))),
        SeaValue::Uuid(value) => Ok(query.bind(value.clone().map(|id| *id))),
    }
}

fn bind_all<'a>(
    sql: &'a str,
    values: Vec<sea_query::Value>,
) -> Result<sqlx::query::Query<'a, Postgres, PgArguments>, Denial> {
    let mut query = sqlx::query(sql);
    for value in &values {
        query = bind_sea_value(query, value)?;
    }
    Ok(query)
}

/// Fetch every row of `inner_sql` as a JSON map each, via `row_to_json`.
/// Numbers, strings, bools, nulls and arrays splice verbatim downstream;
/// only the datetime keys are re-rendered by the caller.
pub async fn fetch_json_rows(
    pool: &sqlx::PgPool,
    inner_sql: &str,
    values: Vec<sea_query::Value>,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner_sql}) AS __r");
    let query = bind_all(&sql, values)?;
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

pub async fn fetch_count(
    pool: &sqlx::PgPool,
    sql: &str,
    values: Vec<sea_query::Value>,
) -> Result<i64, Denial> {
    let query = bind_all(sql, values)?;
    let row = query
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.try_get::<i64, _>(0).map_err(|_| Denial::ServerError)
}

// ---- filter stack -----------------------------------------------------------

/// Map a kernel [`pidash_db::filter::FilterError`] to the exact
/// `{"message", "code"}` 400 the Python backend renders. Messages that
/// embed runtime values the kernel does not carry (operator names, field
/// names) fall back to the kernel's own text with the Python code.
pub fn filter_denial(error: pidash_db::filter::FilterError) -> Denial {
    use pidash_db::filter::FilterError as E;
    let (message, code) = match &error {
        E::InvalidJson => (
            "Invalid JSON for 'filter'. Expected a valid JSON object.".to_owned(),
            "invalid_json",
        ),
        E::InvalidNode => (
            "Each filter node must be a JSON object".to_owned(),
            "invalid_filter_node",
        ),
        E::EmptyNode => (
            "Filter objects must not be empty".to_owned(),
            "empty_filter_object",
        ),
        E::MaxDepthExceeded(max) => (
            format!(
                "Filter nesting is too deep (max {max}); found depth {}",
                max + 1
            ),
            "max_depth_exceeded",
        ),
        E::MultipleOperators => (
            "A filter object cannot contain multiple logical operators at the same level"
                .to_owned(),
            "multiple_logical_operators",
        ),
        E::MixedOperatorAndFields => (error.to_string(), "mixed_operator_and_fields"),
        E::InvalidOperatorChildren => (error.to_string(), "invalid_operator_children"),
        E::InvalidNotChild => (
            "'not' must be a single JSON object".to_owned(),
            "invalid_not_child",
        ),
        E::OperatorInLeaf => (
            "Logical operators cannot appear in a leaf filter object".to_owned(),
            "operator_in_leaf",
        ),
        E::EmptyListValue => (error.to_string(), "empty_list_value"),
        E::InvalidValue => (error.to_string(), "invalid_value_type"),
        E::FilteringNotEnabled => (
            "Filtering is not enabled for this endpoint (missing filterset_class)".to_owned(),
            "filtering_not_enabled",
        ),
        E::InvalidField(field) => (
            format!("Filtering on field '{field}' is not allowed"),
            "invalid_filter_field",
        ),
        E::InvalidLookupValue(field) => (
            format!("Invalid value for lookup on field '{field}'"),
            "invalid_filterset",
        ),
        E::EmptyRangeBounds(_) => return Denial::ServerError,
    };
    Denial::BadFilter(message, code.to_owned())
}

/// Compile one `filters`-param tree against `IssueFilterSet`: structure
/// and shapes via the kernel tree, every leaf through
/// [`pidash_db::filterset`] (the `_build_leaf_q` equivalent — the leaf
/// dict goes to the FilterSet, not to bare columns).
pub fn compile_filter_tree(
    tree: &pidash_db::filter::FilterTree,
) -> Result<Condition, pidash_db::filter::FilterError> {
    use pidash_db::filter::{FilterTree, Lookup};
    fn leaf_name(field: &str, lookup: Lookup) -> String {
        let suffix = match lookup {
            Lookup::Exact => return field.to_owned(),
            Lookup::In => "in",
            Lookup::Gt => "gt",
            Lookup::Gte => "gte",
            Lookup::Lt => "lt",
            Lookup::Lte => "lte",
            Lookup::Contains => "contains",
            Lookup::IContains => "icontains",
            Lookup::StartsWith => "startswith",
            Lookup::IStartsWith => "istartswith",
            Lookup::EndsWith => "endswith",
            Lookup::IEndsWith => "iendswith",
            Lookup::IExact => "iexact",
            Lookup::IsNull => "isnull",
            Lookup::Range => "range",
        };
        format!("{field}__{suffix}")
    }
    match tree {
        FilterTree::And(children) => {
            let mut cond = Condition::all();
            for child in children {
                cond = cond.add(compile_filter_tree(child)?);
            }
            Ok(cond)
        }
        FilterTree::Or(children) => {
            let mut cond = Condition::any();
            for child in children {
                cond = cond.add(compile_filter_tree(child)?);
            }
            Ok(cond)
        }
        FilterTree::Not(child) => Ok(compile_filter_tree(child)?.not()),
        FilterTree::Leaf(leaves) => {
            let provided: Vec<(String, Value)> = leaves
                .iter()
                .map(|leaf| (leaf_name(&leaf.field, leaf.lookup), leaf.value.clone()))
                .collect();
            pidash_db::filterset::build_combined(&provided)
        }
    }
}

/// The `filters` query param compiled to a [`Condition`], or `None` when
/// absent/empty (the backend returns the queryset unchanged then).
/// Python parses `request.query_params.get("filters")` — the last value
/// wins for repeats.
pub fn complex_filter(query: &QueryMap) -> Result<Option<Condition>, Denial> {
    let raw = query_last(query, "filters").unwrap_or_default();
    if raw.is_empty() {
        return Ok(None);
    }
    let tree = pidash_db::filter::FilterTree::parse_param(&raw).map_err(filter_denial)?;
    compile_filter_tree(&tree).map(Some).map_err(filter_denial)
}

// ---- legacy predicates --------------------------------------------------------
// Compile the `issue_filters(query_params, "GET")` predicates to SQL.
// Predicate names are Django ORM paths; the alias table mirrors the
// kernel conditions (`issue` for the base table, `state` for states,
// the relation aliases for the m2m joins).

use pidash_db::issue_filters::FilterValue;

/// A legacy predicate compiled to SQL text (placeholders bound inline).
/// Shared `pub(crate)` with the views handlers (D-29), whose view-issues
/// list applies the same `issue_filters(params, "GET")` stack over the
/// same `issue` alias — referenced, never re-ported. Visibility only; no
/// behavior change.
pub(crate) fn legacy_sql(
    binder: &mut Binder,
    name: &str,
    value: &FilterValue,
) -> Result<String, Denial> {
    // `__isnull` flags.
    if let Some(path) = name.strip_suffix("__isnull") {
        let column = isnull_column(path).ok_or(Denial::ServerError)?;
        let flag = match value {
            FilterValue::Flag(flag) => *flag,
            _ => return Err(Denial::ServerError),
        };
        return Ok(if flag {
            format!("{column} IS NULL")
        } else {
            format!("{column} IS NOT NULL")
        });
    }
    // `issue_cycle__deleted_at__isnull`-style guards arrive here too
    // (same suffix, relation column).
    match value {
        FilterValue::Uuids(ids) => {
            let column = uuid_in_column(name).ok_or(Denial::ServerError)?;
            if ids.is_empty() {
                return Ok("FALSE".to_owned());
            }
            let mut holders = Vec::with_capacity(ids.len());
            for id in ids {
                holders.push(binder.bind_uuid(*id));
            }
            Ok(format!("{column} IN ({})", holders.join(",")))
        }
        FilterValue::Strings(items) => {
            let column = strings_in_column(name).ok_or(Denial::ServerError)?;
            if items.is_empty() {
                return Ok("FALSE".to_owned());
            }
            if name == "issue_intake__status__in" {
                let mut numbers = Vec::with_capacity(items.len());
                for item in items {
                    numbers.push(
                        item.parse::<i32>()
                            .map_err(|_| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?,
                    );
                }
                let mut holders = Vec::with_capacity(numbers.len());
                for number in numbers {
                    holders.push(binder.bind(sea_query::Value::Int(Some(number))));
                }
                return Ok(format!("{column} IN ({})", holders.join(",")));
            }
            if name == "estimate_point__in" {
                // UUID PKs compared to strings: Django coerces via
                // `UUIDField.get_prep_value`; garbage is a ValidationError.
                let mut holders = Vec::with_capacity(items.len());
                for item in items {
                    let id: uuid::Uuid = item
                        .parse()
                        .map_err(|_| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?;
                    holders.push(binder.bind_uuid(id));
                }
                return Ok(format!("{column} IN ({})", holders.join(",")));
            }
            let mut holders = Vec::with_capacity(items.len());
            for item in items {
                holders.push(binder.bind_string(item.clone()));
            }
            Ok(format!("{column} IN ({})", holders.join(",")))
        }
        FilterValue::Text(text) => legacy_text_sql(binder, name, text),
        FilterValue::Flag(_) => Err(Denial::ServerError),
        FilterValue::Day(day) => {
            let (column, operator) = day_comparison(name).ok_or(Denial::ServerError)?;
            let holder = binder.bind_string(day.to_string());
            Ok(format!("{column} {operator} {holder}::date"))
        }
        FilterValue::Null => {
            let column = isnull_column(name).ok_or(Denial::ServerError)?;
            Ok(format!("{column} IS NULL"))
        }
    }
}

const INVALID_DETAIL_BODY_MSG: &str = "Please provide valid detail";

/// Columns for `__isnull` predicates (and bare `Null` values).
fn isnull_column(path: &str) -> Option<&'static str> {
    Some(match path {
        "parent" => "issue.parent_id",
        "labels" => "label_issue.label_id",
        "assignees" => "issue_assignee.assignee_id",
        "created_by" => "issue.created_by_id",
        "issue_cycle__cycle_id" => "issue_cycle.cycle_id",
        "issue_module__module_id" => "issue_module.module_id",
        "label_issue__deleted_at" => "label_issue.deleted_at",
        "issue_assignee__deleted_at" => "issue_assignee.deleted_at",
        "issue_cycle__deleted_at" => "issue_cycle.deleted_at",
        "issue_module__deleted_at" => "issue_module.deleted_at",
        "issue_subscribers__deleted_at" => "issue_subscribers.deleted_at",
        "target_date" => "issue.target_date",
        "start_date" => "issue.start_date",
        _ => return None,
    })
}

/// Columns for UUID `__in` predicates. `logged_by` has no model field:
/// Django raises `FieldError` (generic 500).
fn uuid_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__in" => "issue.state_id",
        "parent__in" => "issue.parent_id",
        "labels__in" => "label_issue.label_id",
        "assignees__in" => "issue_assignee.assignee_id",
        "issue_mention__mention__id__in" => "issue_mention.mention_id",
        "created_by__in" => "issue.created_by_id",
        "project__in" => "issue.project_id",
        "issue_cycle__cycle_id__in" => "issue_cycle.cycle_id",
        "issue_module__module_id__in" => "issue_module.module_id",
        "issue_subscribers__subscriber_id__in" => "issue_subscribers.subscriber_id",
        _ => return None,
    })
}

/// Columns for string `__in` predicates.
fn strings_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__group__in" => "state.\"group\"",
        "estimate_point__in" => "issue.estimate_point_id",
        "priority__in" => "issue.priority",
        "issue_intake__status__in" => "issue_intake.status",
        _ => return None,
    })
}

/// `(column, operator)` for `Day` predicates (`__gte` / `__lte`).
fn day_comparison(name: &str) -> Option<(&'static str, &'static str)> {
    let (term, operator) = name.rsplit_once("__")?;
    let column = match term {
        "created_at__date" => "issue.created_at::date",
        "completed_at__date" => "issue.completed_at::date",
        "start_date" => "issue.start_date",
        "target_date" => "issue.target_date",
        _ => return None,
    };
    let operator = match operator {
        "gte" => ">=",
        "lte" => "<=",
        _ => return None,
    };
    Some((column, operator))
}

/// Text predicates: `name__icontains`, explicit date bounds
/// (`__gte`/`__lte` on a `__date` term or plain date term), and the
/// single-value `__contains` form.
fn legacy_text_sql(binder: &mut Binder, name: &str, text: &str) -> Result<String, Denial> {
    if name == "name__icontains" {
        // Django `icontains`: LIKE with `\`, `%`, `_` escaped.
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let holder = binder.bind_string(format!("%{escaped}%"));
        return Ok(format!("issue.name ILIKE {holder}"));
    }
    let Some((term, operator)) = name.rsplit_once("__") else {
        return Err(Denial::ServerError);
    };
    {
        let operator = match operator {
            "gte" => ">=",
            "lte" => "<=",
            "contains" => "=",
            _ => return Err(Denial::ServerError),
        };
        let column = match term {
            "created_at__date" => "issue.created_at::date",
            "completed_at__date" => "issue.completed_at::date",
            "start_date" => "issue.start_date",
            "target_date" => "issue.target_date",
            _ => return Err(Denial::ServerError),
        };
        // Explicit bounds must parse as dates: Django's `get_prep_value`
        // raises `ValidationError` (400 invalid detail) on garbage.
        if text.parse::<chrono::NaiveDate>().is_err() && parse_datetime_param(text).is_none() {
            return Err(Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()));
        }
        let holder = binder.bind_string(text.to_owned());
        if operator == "=" {
            // Single-value form on a date term: Django's `contains`
            // lookup, i.e. LIKE with metacharacters escaped.
            let escaped = text
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let holder = binder.bind_string(format!("%{escaped}%"));
            return Ok(format!("{column}::text LIKE {holder}"));
        }
        let operator = if operator == ">=" { ">=" } else { "<=" };
        Ok(format!("{column} {operator} {holder}::date"))
    }
}

/// Parse an `updated_at__gt`-style datetime param the way Django's
/// `DateTimeField.get_prep_value` does (naive values attach UTC).
/// Garbage is a `ValidationError`, not SQL text.
/// `updated_at__gt` parsing: RFC-3339 (with offsets) plus Django's
/// `DATETIME_INPUT_FORMATS` naive shapes, interpreted as UTC exactly like
/// the naive datetimes Django compares in `__gt` lookups. Anything else is
/// a `ValidationError`.
fn parse_datetime_param(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(aware) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(aware.with_timezone(&chrono::Utc));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M",
        "%m/%d/%y %H:%M:%S",
        "%m/%d/%y %H:%M",
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%m/%d/%y",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return Some(naive.and_utc());
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, format) {
            return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    None
}

// ---- main query ---------------------------------------------------------------
// FROM/JOIN/WHERE shared by the list paths. Alias contract (set by the
// kernels): `issue` (issues), `state` (states), `issue_assignee`,
// `issue_cycle`, `issue_module`, `issue_mention`, `label_issue`,
// `issue_subscribers`, `issue_intake`. A relation join is INNER when a
// compiled condition references it or the m2m group filter needs it
// (Django's `filter()` join), else LEFT (Django's `__isnull` join).
// Joined tables carry no soft-delete guard: `filter()` never applies a
// related manager to a join. (The annotation subqueries are different —
// those query `.objects` managers directly, so they keep their guards.)

/// Relation joins the list query can use: SQL table plus kernel alias.
const RELATION_JOINS: &[(&str, &str, &str)] = &[
    ("issue_assignees", "issue_assignee", "assignee_id"),
    ("cycle_issues", "issue_cycle", "cycle_id"),
    ("module_issues", "issue_module", "module_id"),
    ("issue_mentions", "issue_mention", "mention_id"),
    ("issue_labels", "label_issue", "label_id"),
    ("issue_subscribers", "issue_subscribers", "subscriber_id"),
];

/// True when every `"alias".` reference in the filter SQL is an
/// `IS NULL` / `IS NOT NULL` test (the `__isnull` predicates).
fn alias_nullable_only(where_sql: &str, alias: &str) -> bool {
    let marker = format!("\"{alias}\".");
    let mut rest = where_sql;
    while let Some(start) = rest.find(&marker) {
        let after = &rest[start + marker.len()..];
        let ident_len = after
            .chars()
            .take_while(|char| char.is_alphanumeric() || *char == '_' || *char == '"')
            .map(|char| char.len_utf8())
            .sum::<usize>();
        let tail = after[ident_len..].trim_start();
        if !(tail.starts_with("IS NULL") || tail.starts_with("IS NOT NULL")) {
            return false;
        }
        rest = &after[ident_len..];
    }
    true
}

/// M2M group filters (`GROUP_FILTER_MAPPER`): grouped by this raw field
/// forces an INNER join on that relation with the deleted guard.
fn group_join_alias(group_by: Option<&str>, sub_group_by: Option<&str>, alias: &str) -> bool {
    let raw = match alias {
        "issue_assignee" => "assignees__id",
        "label_issue" => "labels__id",
        "issue_module" => "issue_module__module_id",
        _ => return false,
    };
    group_by == Some(raw) || sub_group_by == Some(raw)
}

/// Scalar subquery annotations shared by every list path
/// (`apply_annotations` + the grouper array annotations).
/// `unguarded_arrays` reproduces `IssueDetailEndpoint.apply_annotations` +
/// `IssueListDetailSerializer`'s prefetch reads: the reverse managers run
/// on the plain `objects` manager, so the array subqueries carry *no*
/// soft-delete guard and the module one neither joins `modules` nor checks
/// Count annotations render `NULL` when empty: Django's grouped
/// `Subquery(...values().annotate(count=Count()).values("count"))` has no
/// `Coalesce`, so zero related rows yield `NULL` → JSON `null`, never `0`.
/// `NULLIF(COUNT(*), 0)` reproduces that on every list path.
/// Array guards mirror the managers Django queries through:
/// `IssueLabel`/`IssueAssignee`/`ModuleIssue.objects` are soft-deletion
/// managers (deleted rows excluded everywhere); the v2 assignee filter
/// adds the active-member join; the grouper and the v2 view add the
/// archived-module guard, while the detail endpoint's
/// `Prefetch(...objects.all())` carries the manager's deleted filter only
/// (`module_archived_guard = false` there). The extra `m.deleted_at`
/// guard on modules is a disclosed divergence (shared helper, all paths).
fn annotation_selects(
    arrays: bool,
    skip_array: Option<&str>,
    assignee_active_member: bool,
    module_archived_guard: bool,
) -> String {
    let mut selects = String::from(
        r#"issue.id, issue.name, issue.state_id, issue.sort_order, issue.completed_at,
        issue.estimate_point_id AS estimate_point, issue.priority, issue.start_date,
        issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id,
        (SELECT ci.cycle_id FROM cycle_issues ci
          WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1) AS cycle_id,
        (SELECT NULLIF(COUNT(*), 0) FROM issue_links il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS link_count,
        (SELECT NULLIF(COUNT(*), 0) FROM file_assets fa
          WHERE fa.issue_id = issue.id AND fa.entity_type = 'ISSUE_ATTACHMENT'
            AND fa.deleted_at IS NULL) AS attachment_count,
        (SELECT NULLIF(COUNT(*), 0) FROM issues c
           LEFT JOIN states cs ON cs.id = c.state_id AND cs.deleted_at IS NULL
           JOIN projects cp ON cp.id = c.project_id
          WHERE c.parent_id = issue.id AND c.deleted_at IS NULL
            AND (cs."group" IS NULL OR NOT (cs."group" = 'triage'))
            AND c.archived_at IS NULL AND cp.archived_at IS NULL AND c.is_draft = FALSE
        ) AS sub_issues_count,
        issue.created_at, issue.updated_at, issue.created_by_id AS created_by,
        issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at,
        issue.deleted_at, state."group" AS "state__group""#,
    );
    if arrays {
        if skip_array != Some("label_ids") {
            selects.push_str(
                r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT il.label_id), '{}'::uuid[])
           FROM issue_labels il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS label_ids"#,
            );
        }
        if skip_array != Some("assignee_ids") {
            if assignee_active_member {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[])
           FROM issue_assignees ia
          WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL
            AND EXISTS (SELECT 1 FROM project_members pm
                         WHERE pm.member_id = ia.assignee_id
                           AND pm.is_active AND pm.deleted_at IS NULL)) AS assignee_ids"#,
                );
            } else {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[])
           FROM issue_assignees ia
          WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL) AS assignee_ids"#,
                );
            }
        }
        if skip_array != Some("module_ids") {
            if module_archived_guard {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT mi.module_id), '{}'::uuid[])
           FROM module_issues mi JOIN modules m ON m.id = mi.module_id
          WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL
            AND m.archived_at IS NULL AND m.deleted_at IS NULL) AS module_ids"#,
                );
            } else {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT mi.module_id), '{}'::uuid[])
           FROM module_issues mi
          WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL) AS module_ids"#,
                );
            }
        }
    }
    selects
}

/// `WHERE` preamble: tenant scope plus the `issue_objects` manager
/// (`SoftDeletionManager` + triage/archived/draft exclusions).
/// `detail` selects the `IssueDetailEndpoint` permission model: the
/// `Exists` membership subquery (passed separately via `extra`) replaces
/// the guest `created_by` scoping, so the preamble skips it. The flat
/// `issues/list/` endpoint never scopes guests either
/// (`IssueListEndpoint.get` has no role-5 check), so it passes
/// `guest_scope = false` too.
/// The triage exclusion keeps NULL-state rows: `state` is a nullable FK
/// and Django's `exclude(state__group=TRIAGE)` retains them via
/// `split_exclude`'s `IS NULL` disjunct; a bare `NOT (group = 'triage')`
/// over the left join would drop them (three-valued logic).
fn base_where(
    binder: &mut Binder,
    gate: &Gate,
    slug: &str,
    detail: bool,
    guest_scope: bool,
) -> String {
    let slug_holder = binder.bind_string(slug.to_owned());
    let project_holder = binder.bind_uuid(gate.project_id);
    let mut where_sql = format!(
        r#"workspaces.slug = {slug_holder} AND issue.project_id = {project_holder}
        AND issue.deleted_at IS NULL
        AND (state."group" IS NULL OR NOT (state."group" = 'triage'))
        AND issue.archived_at IS NULL AND project.archived_at IS NULL
        AND issue.is_draft = FALSE"#
    );
    if gate.guest_scoped && !detail && guest_scope {
        let user_holder = binder.bind_uuid(gate.user_id);
        where_sql.push_str(&format!(" AND issue.created_by_id = {user_holder}"));
    }
    where_sql
}

/// Full FROM…WHERE for the filtered issue set. Returns the SQL through
/// `LIMIT/OFFSET`-free (callers add windows, counts, ordering) and
/// reports which relation aliases render in the filter SQL (for the
/// INNER/LEFT join decision).
pub struct FilteredSet {
    pub from_where: String,
    pub values: Vec<sea_query::Value>,
    pub referenced: Vec<&'static str>,
}

/// Which filter layers a path applies. The main list, flat endpoint and
/// detail endpoint apply the rich (`ComplexFilterBackend` +
/// `IssueFilterSet`) and legacy (`issue_filters`) stacks; the v2 cursor
/// page reads only `cursor`, `description` and `updated_at__gt` in Python
/// and must skip both stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterLayers {
    /// Rich filters (`ComplexFilterBackend` + `IssueFilterSet`).
    pub rich: bool,
    /// Legacy `issue_filters` predicates.
    pub legacy: bool,
    /// Guest `created_by` scoping (false on flat/detail paths, which never
    /// check it in Python).
    pub guest_scope: bool,
}

impl FilterLayers {
    /// Main list, flat `.values()` branch and detail endpoint: everything.
    pub const FULL: Self = Self {
        rich: true,
        legacy: true,
        guest_scope: true,
    };
    /// v2 cursor page: tenant + guest + `updated_at__gt` only.
    pub const V2: Self = Self {
        rich: false,
        legacy: false,
        guest_scope: true,
    };
    /// Flat endpoint and detail endpoint: no guest scoping in Python.
    pub const NO_GUEST: Self = Self {
        rich: true,
        legacy: true,
        guest_scope: false,
    };
    /// Flat `fields=`/`expand=` branch: the view serializes the
    /// rich-filtered `queryset` — the legacy `issue_filters` never run on
    /// it (a ported bug).
    pub const SERIALIZER: Self = Self {
        rich: true,
        legacy: false,
        guest_scope: false,
    };
}

#[allow(clippy::too_many_arguments)]
pub fn filtered_set(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    extra: Option<(String, Vec<sea_query::Value>)>,
    detail: bool,
    layers: FilterLayers,
) -> Result<FilteredSet, Denial> {
    let mut binder = Binder::new();
    let preamble = base_where(&mut binder, gate, slug, detail, layers.guest_scope);
    let mut fragments: Vec<String> = vec![preamble];
    // Rich filters (`filters` JSON via ComplexFilterBackend + IssueFilterSet).
    let mut complex_sql = String::new();
    if layers.rich {
        if let Some(cond) = complex_filter(query)? {
            let (fragment, values) = render_condition(&cond);
            complex_sql = binder.splice(&fragment, values);
        }
    }
    // Legacy filters. Relative dates resolve against the UTC date, exactly
    // like `timezone.now().date()` in `issue_filters.py` (`now()` is always
    // UTC; the request tz activation only affects `localtime()`).
    let mut legacy_sql_text = String::new();
    if layers.legacy {
        let flat: HashMap<String, String> = query
            .keys()
            .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
            .collect();
        let today = chrono::Utc::now().date_naive();
        let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
            .map_err(|_| Denial::ServerError)?;
        for (name, value) in legacy.predicates() {
            let fragment = legacy_sql(&mut binder, name, value)?;
            if legacy_sql_text.is_empty() {
                legacy_sql_text = fragment;
            } else {
                legacy_sql_text = format!("{legacy_sql_text} AND {fragment}");
            }
        }
    }
    if !complex_sql.is_empty() {
        fragments.push(complex_sql);
    }
    if !legacy_sql_text.is_empty() {
        fragments.push(legacy_sql_text);
    }
    if let Some((fragment, binds)) = extra {
        fragments.push(binder.splice(&fragment, binds));
    }
    let where_sql = fragments.join(" AND ");
    // INNER when referenced by a positive condition or m2m-grouped
    // (Django's `filter()` join); LEFT when every reference is an
    // `IS [NOT] NULL` test (Django's `__isnull` join) or unreferenced.
    // Joined tables carry no soft-delete guard: `filter()` never applies a
    // related manager to a join, so `filter(labels__in=...)` matches
    // soft-deleted link rows and `labels__isnull=False` keeps issues whose
    // only links are deleted.
    let mut joins = String::new();
    let mut referenced = Vec::new();
    if layers.rich || layers.legacy {
        for (table, alias, key_column) in RELATION_JOINS {
            let marker = format!("\"{alias}\".");
            let mentioned = where_sql.contains(&marker);
            let nullable_only = mentioned && alias_nullable_only(&where_sql, alias);
            let used = mentioned || group_join_alias(group_by, sub_group_by, alias);
            let inner = used && !nullable_only || group_join_alias(group_by, sub_group_by, alias);
            if used {
                referenced.push(*alias);
            }
            let kind = if inner { "INNER JOIN" } else { "LEFT JOIN" };
            joins.push_str(&format!(
                " {kind} {table} AS {alias} ON {alias}.issue_id = issue.id"
            ));
            let _ = key_column;
        }
        let intake_used = where_sql.contains("\"issue_intake\".");
        joins.push_str(&format!(
            " {} intake_issues AS issue_intake ON issue_intake.issue_id = issue.id",
            if intake_used {
                "INNER JOIN"
            } else {
                "LEFT JOIN"
            }
        ));
        if intake_used {
            referenced.push("issue_intake");
        }
    }
    let from_where = format!(
        r#"FROM issues AS issue
        JOIN projects AS project ON project.id = issue.project_id
        JOIN workspaces ON workspaces.id = issue.workspace_id
        LEFT JOIN states AS state ON state.id = issue.state_id AND state.deleted_at IS NULL
        {joins}
        WHERE {where_sql}"#
    );
    Ok(FilteredSet {
        from_where,
        values: binder.values(),
        referenced,
    })
}

// ---- ordering ---------------------------------------------------------------
// The paginator re-orders by the *rewritten* `order_by` param with
// `NULLS LAST` plus a `-created_at` tiebreak (`OffsetPaginator.get_result`
// and the grouped variants). This resolves the rewritten key to SQL.

/// `(key expression, descending)` for the rewritten `order_by` param.
/// The default branch of `order_issue_queryset` orders by the raw param,
/// so any valid ORM path works in Python and only a truly invalid path
/// raises `FieldError` (generic 500). FK names order by their id column
/// (`estimate_point` → `estimate_point_id`, `project` → `project_id`,
/// `parent` → `parent_id`, `state` → `state_id`); `state__name` resolves
/// through the state join. Deeper relation traversals beyond the
/// `min_values` trio stay a 500 (out of pilot scope; the suite never sends
/// them).
pub fn order_key(out_param: &str, orig_param: &str) -> Result<(String, bool), Denial> {
    let descending = out_param.starts_with('-');
    let key = out_param.trim_start_matches('-');
    let expr = match key {
        "priority_order" => pidash_services::app_issues::ordering::priority_case_sql(),
        "state_order" => {
            pidash_services::app_issues::ordering::state_case_sql("state.\"group\"")
        }
        "min_values" => min_order_subquery(orig_param.trim_start_matches('-'))?,
        "cycle_id" => "(SELECT ci.cycle_id FROM cycle_issues ci WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1)".to_owned(),
        "created_at" | "updated_at" | "sort_order" | "sequence_id" | "name" | "start_date"
        | "target_date" | "completed_at" | "id" | "is_draft" | "archived_at"
        | "deleted_at" => {
            format!("issue.\"{key}\"")
        }
        "estimate_point" => "issue.estimate_point_id".to_owned(),
        "project" => "issue.project_id".to_owned(),
        "parent" => "issue.parent_id".to_owned(),
        "state" => "issue.state_id".to_owned(),
        "state__name" => "state.name".to_owned(),
        "created_by" => "issue.created_by_id".to_owned(),
        "updated_by" => "issue.updated_by_id".to_owned(),
        _ => return Err(Denial::ServerError),
    };
    Ok((expr, descending))
}

/// The `Min(...)` annotation for the assignee/label/module order branch,
/// as a scalar subquery (equivalent over the joined set; NULL when empty,
/// which sorts with `NULLS LAST` like Django).
fn min_order_subquery(relation: &str) -> Result<String, Denial> {
    match relation {
        "labels__name" => Ok("(SELECT MIN(l.name) FROM issue_labels il JOIN labels l ON l.id = il.label_id AND l.deleted_at IS NULL WHERE il.issue_id = issue.id AND il.deleted_at IS NULL)".to_owned()),
        "assignees__first_name" => Ok("(SELECT MIN(u.first_name) FROM issue_assignees ia JOIN users u ON u.id = ia.assignee_id WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL)".to_owned()),
        "issue_module__module__name" => Ok("(SELECT MIN(m.name) FROM module_issues mi JOIN modules m ON m.id = mi.module_id AND m.deleted_at IS NULL WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL)".to_owned()),
        _ => Err(Denial::ServerError),
    }
}

// ---- row rendering ------------------------------------------------------------
// Shape a `row_to_json` record into the response object: keys in the
// view's `.values()` order, datetimes re-rendered through the serializer
// kernel, `sort_order` through Python-float formatting (Postgres JSON
// drops the `.0` DRF renders).

/// Render one row's selected keys in order as a compact JSON object.
/// `shift` selects the datetime rule: the flat `issues/list/` `.values()`
/// branch (via `user_timezone_converter`), the v2 page and the
/// `IssueSerializer` branch (via DRF `enforce_timezone` against the
/// activated actor tz) render `created_at`/`updated_at` in the actor's
/// zone; the main `issues/` list and `issues-detail/` render `.values()`
/// dicts / raw serializer dicts through the plain JSON encoder, i.e. as
/// stored (UTC).
pub fn shape_row(
    row: &Map<String, Value>,
    fields: &[String],
    timezone: &Tz,
    shift: bool,
) -> String {
    let mut out = String::from("{");
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        out.push_str(&shape_value(
            field,
            row.get(field).unwrap_or(&Value::Null),
            timezone,
            shift,
        ));
    }
    out.push('}');
    out
}

fn shape_value(field: &str, value: &Value, timezone: &Tz, shift: bool) -> String {
    match field {
        "created_at" | "updated_at" if shift => shift_datetime(value, timezone),
        "created_at" | "updated_at" | "completed_at" | "deleted_at" => utc_datetime(value),
        "sort_order" => match value {
            Value::Number(number) => {
                let float = number.as_f64().unwrap_or(f64::NAN);
                crate::paginator::py_float_str(float)
            }
            _ => serde_json::to_string(value).unwrap_or("null".to_owned()),
        },
        _ => serde_json::to_string(value).unwrap_or("null".to_owned()),
    }
}

/// `user_timezone_converter` + DRF rendering for one value.
fn shift_datetime(value: &Value, timezone: &Tz) -> String {
    let text = match value {
        Value::String(text) => text,
        _ => return serde_json::to_string(value).unwrap_or("null".to_owned()),
    };
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(aware) => crate::serializer::render_datetime_in(&aware, timezone),
        Err(_) => serde_json::to_string(value).unwrap_or("null".to_owned()),
    }
    .pipe(quote_json)
}

/// DRF UTC rendering (`Z` suffix) for one value.
fn utc_datetime(value: &Value) -> String {
    let text = match value {
        Value::String(text) => text,
        _ => return serde_json::to_string(value).unwrap_or("null".to_owned()),
    };
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(aware) => crate::serializer::render_datetime(&aware.with_timezone(&chrono::Utc)),
        Err(_) => serde_json::to_string(value).unwrap_or("null".to_owned()),
    }
    .pipe(quote_json)
}

trait Pipe: Sized {
    fn pipe<F, T>(self, f: F) -> T
    where
        F: FnOnce(Self) -> T,
    {
        f(self)
    }
}

impl Pipe for String {}

fn quote_json(rendered: String) -> String {
    serde_json::to_string(&rendered).unwrap_or("null".to_owned())
}

// ---- group values ---------------------------------------------------------------
// Port of `issue_group_values`: the known group buckets per field.

pub async fn group_values(
    pool: &sqlx::PgPool,
    field: &str,
    slug: &str,
    project_id: &uuid::Uuid,
    filtered: &FilteredSet,
) -> Result<Vec<String>, Denial> {
    // Binds for a fresh statement: slug + project first (same order).
    let scalar_text = |rows: Vec<(Option<String>,)>| {
        rows.into_iter()
            .filter_map(|row| row.0)
            .collect::<Vec<String>>()
    };
    match field {
        "state_id" => {
            let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT s.id FROM states s JOIN workspaces w ON w.id = s.workspace_id
                   WHERE s.is_triage = FALSE AND w.slug = $1 AND s.project_id = $2
                   AND s.deleted_at IS NULL"#,
            )
            .bind(slug)
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            Ok(rows.into_iter().map(|row| row.0.to_string()).collect())
        }
        "labels__id" => {
            let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT l.id FROM labels l JOIN workspaces w ON w.id = l.workspace_id
                   WHERE w.slug = $1 AND l.project_id = $2 AND l.deleted_at IS NULL"#,
            )
            .bind(slug)
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let mut out: Vec<String> = rows.into_iter().map(|row| row.0.to_string()).collect();
            out.push("None".to_owned());
            Ok(out)
        }
        "assignees__id" => {
            let rows: Vec<(Option<uuid::Uuid>,)> = sqlx::query_as(
                r#"SELECT pm.member_id FROM project_members pm
                   JOIN workspaces w ON w.id = pm.workspace_id
                   WHERE w.slug = $1 AND pm.project_id = $2 AND pm.is_active
                   AND pm.deleted_at IS NULL"#,
            )
            .bind(slug)
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            Ok(scalar_text(
                rows.into_iter()
                    .map(|row| (row.0.map(|id| id.to_string()),))
                    .collect(),
            ))
        }
        "issue_module__module_id" => {
            let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT m.id FROM modules m JOIN workspaces w ON w.id = m.workspace_id
                   WHERE w.slug = $1 AND m.project_id = $2 AND m.deleted_at IS NULL"#,
            )
            .bind(slug)
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let mut out: Vec<String> = rows.into_iter().map(|row| row.0.to_string()).collect();
            out.push("None".to_owned());
            Ok(out)
        }
        "cycle_id" => {
            let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT c.id FROM cycles c JOIN workspaces w ON w.id = c.workspace_id
                   WHERE w.slug = $1 AND c.project_id = $2 AND c.deleted_at IS NULL"#,
            )
            .bind(slug)
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let mut out: Vec<String> = rows.into_iter().map(|row| row.0.to_string()).collect();
            out.push("None".to_owned());
            Ok(out)
        }
        "project_id" => {
            let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
                   WHERE w.slug = $1 AND p.deleted_at IS NULL"#,
            )
            .bind(slug)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            Ok(rows.into_iter().map(|row| row.0.to_string()).collect())
        }
        "priority" => Ok(PRIORITY_VALUES
            .iter()
            .map(|value| (*value).to_owned())
            .collect()),
        "state__group" => Ok(STATE_GROUP_VALUES
            .iter()
            .map(|value| (*value).to_owned())
            .collect()),
        "target_date" | "start_date" | "created_by" => {
            let column = match field {
                "target_date" => "issue.target_date::text",
                "start_date" => "issue.start_date::text",
                _ => "issue.created_by_id::text",
            };
            let sql = format!(
                "SELECT DISTINCT {column} AS bucket {} WHERE {column} IS NOT NULL",
                filtered.from_where
            );
            let query = bind_all(&sql, filtered.values.clone())?;
            let rows = query
                .fetch_all(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
            let mut out = Vec::new();
            for row in rows {
                let bucket: String = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
                out.push(bucket);
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

// ---- handlers ---------------------------------------------------------------

type HandlerResult = Result<Response, Denial>;

fn json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// Shared preamble: gate + parsed params + filtered set + order key.
/// `extra_where` carries path-specific predicates (`updated_at__gt`,
/// `pk__in`); callers splice their binds through `extra_binds`.
pub struct ListContext {
    pub gate: Gate,
    pub params: ListParams,
    pub pool: sqlx::PgPool,
    pub slug: String,
}

/// Which list-family path a [`list_context`] call serves. The main
/// `issues/` list validates `per_page` and the group mismatch; the detail
/// endpoint validates `per_page` (through its paginator) but never the
/// mismatch; the flat, v2 and deleted paths read neither param in Python,
/// so they parse leniently (a strict parse would 400 where Django 200s).
/// The flat path additionally checks `issues`-required first, before any
/// `per_page` handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    Main,
    Detail,
    Flat,
    Lenient,
}

fn denial_from_param(error: pidash_services::app_issues::ParamError) -> Denial {
    if error.key == "detail" {
        Denial::BadDetail(error.message)
    } else {
        Denial::BadError(error.message)
    }
}

pub async fn list_context(
    state: AppState,
    slug: String,
    project_id: String,
    query: &QueryMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    mode: ListMode,
) -> Result<ListContext, Denial> {
    let gate = resolve_gate(&state, &slug, &project_id, extension).await?;
    let (require_issues, strict_per_page, check_mismatch) = match mode {
        ListMode::Main => (false, true, true),
        ListMode::Detail => (false, true, false),
        ListMode::Flat => (true, false, false),
        ListMode::Lenient => (false, false, false),
    };
    // Main-list order: the group mismatch is checked in-view before
    // `paginate` parses `per_page`/cursor.
    if check_mismatch {
        let multi = multi_map(query);
        let group_by = multi
            .get("group_by")
            .and_then(|values| values.last().map(String::as_str));
        let sub_group_by = multi
            .get("sub_group_by")
            .and_then(|values| values.last().map(String::as_str));
        if let Some(mismatch) = raw_group_mismatch(group_by, sub_group_by) {
            return Err(denial_from_param(mismatch));
        }
    }
    let options = ParseOptions {
        require_issues,
        strict_per_page,
    };
    let params = ListParams::parse_with(&multi_map(query), options).map_err(denial_from_param)?;
    if check_mismatch {
        if let Some(mismatch) = params.group_mismatch() {
            return Err(Denial::BadError(mismatch.message));
        }
    }
    let pool = state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)?;
    Ok(ListContext {
        gate,
        params,
        pool,
        slug,
    })
}

/// `GET .../issues/`: grouped / sub-grouped / flat paginated list.
pub async fn list_issues(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = list_context(state, slug, project_id, &query, extension, ListMode::Main).await?;
    record_recent_visit(&context).await;
    let order_spec = order_sql(&context.params.order_by, "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let updated_gt = updated_at_gt(&query)?;
    let extra = match updated_gt {
        (Some(fragment), binds) => Some((fragment, binds)),
        (None, _) => None,
    };
    let filtered = filtered_set(
        &context.gate,
        &context.slug,
        &query,
        context.params.group_by.as_deref(),
        context.params.sub_group_by.as_deref(),
        extra,
        false,
        FilterLayers::FULL,
    )?;
    let (key_expr, descending) = order_key(&order_spec.out_param, &context.params.order_by)?;
    let direction = if descending { "DESC" } else { "ASC" };
    // `per_page` already validated by the strict preamble; the cursor
    // parses here with the paginator kernel's exact errors.
    let per_page = context.params.per_page;
    let cursor = crate::paginator::Cursor::from_string(&context.params.cursor_raw)
        .map_err(|error| Denial::BadDetail(error.detail()))?;
    let group_by = context.params.group_by.clone();
    let sub_group_by = context.params.sub_group_by.clone();
    if let Some(group) = group_by.clone() {
        return grouped_response(
            &context,
            &filtered,
            &group,
            sub_group_by.clone(),
            &key_expr,
            direction,
            per_page,
            cursor,
        )
        .await;
    }
    let selects = annotation_selects(true, None, false, true);
    let fields = on_results_fields(None, None);
    flat_paginated_response(
        &context, &filtered, &key_expr, direction, per_page, cursor, selects, fields, false,
    )
    .await
}

/// The flat (non-grouped) branch of `IssueViewSet.list`, through
/// `issue_on_results` (hence `state__group`, no `deleted_at`) — shared
/// with `issues-detail/`, which passes the `IssueListDetailSerializer`
/// selects (unguarded arrays) and field list (no `state__group`).
#[allow(clippy::too_many_arguments)]
async fn flat_paginated_response(
    context: &ListContext,
    filtered: &FilteredSet,
    key_expr: &str,
    direction: &str,
    per_page: i64,
    cursor: crate::paginator::Cursor,
    selects: String,
    fields: Vec<String>,
    shift: bool,
) -> HandlerResult {
    use crate::paginator::{
        apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor,
    };
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    let inner = format!(
        "SELECT DISTINCT {selects}, ({key_expr}) AS __order_key {} ORDER BY __order_key {direction} NULLS LAST, issue.created_at DESC LIMIT {} OFFSET {}",
        filtered.from_where,
        window.stop - window.offset,
        window.offset
    );
    let rows = fetch_json_rows(&context.pool, &inner, filtered.values.clone()).await?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Map<String, Value>> = apply_offset_window(&rows, limit)
        .map_err(page_denial)?
        .into_iter()
        .collect();
    let total_count = {
        let sql = format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where);
        fetch_count(&context.pool, &sql, filtered.values.clone()).await?
    };
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let shaped: Vec<String> = page
        .iter()
        .map(|row| shape_row(row, &fields, &context.gate.timezone, shift))
        .collect();
    Ok(json_response(envelope(
        None,
        None,
        total_count,
        &next.to_string(),
        &prev.to_string(),
        next.has_results_or_false(),
        prev.has_results_or_false(),
        shaped.len(),
        max_hits(total_count, limit).map_err(page_denial)?,
        total_count,
        &format!("[{}]", shaped.join(",")),
    )))
}

/// Map a paginator-kernel error to its HTTP fate: `BadPaginationError`
/// subclasses become `ParseError` 400s; lazy-queryset `ValueError`s and
/// arithmetic errors propagate to the generic 500.
pub fn page_denial(error: crate::paginator::PageError) -> Denial {
    use crate::paginator::PageError as E;
    match error {
        E::InvalidCursor
        | E::InvalidPerPage
        | E::PerPageTooLarge(_)
        | E::OffsetTooLarge
        | E::NegativeOffset => Denial::BadDetail(error.detail()),
        E::NegativeSlice | E::ZeroLimit | E::NonFiniteCursor | E::MissingOrderKey => {
            Denial::ServerError
        }
    }
}

/// Grouped / sub-grouped branch with the window-function paginators.
/// Grouping itself runs through the F-07 kernel groupers
/// (`process_grouped_results` / `process_sub_grouped_results`); `count`
/// is the pre-grouping window length, exactly like `CursorResult.__len__`.
#[allow(clippy::too_many_arguments)]
async fn grouped_response(
    context: &ListContext,
    filtered: &FilteredSet,
    group_by: &str,
    sub_group_by: Option<String>,
    key_expr: &str,
    direction: &str,
    per_page: i64,
    cursor: crate::paginator::Cursor,
) -> HandlerResult {
    use crate::paginator::{
        grouped_max_hits, grouped_window, next_cursor, prev_cursor, process_grouped_results,
        process_sub_grouped_results, sub_field_dict, sub_total_dicts, total_dict,
    };
    let limit = per_page.min(1000);
    let window = grouped_window(limit, cursor.offset, cursor.value, None).map_err(page_denial)?;
    let group_expr = group_expression(group_by)?;
    let selects = annotation_selects(true, skip_array_for(group_by), false, true);
    let member_select = group_member_select(group_by);
    let sub_expr = match &sub_group_by {
        Some(sub) => Some(group_expression(sub)?),
        None => None,
    };
    let partition = match &sub_expr {
        Some(sub) => format!("{group_expr}, {sub}"),
        None => group_expr.clone(),
    };
    let inner = format!(
        "SELECT * FROM (SELECT DISTINCT {selects}{member_select}, ({key_expr}) AS __order_key,
         ROW_NUMBER() OVER (PARTITION BY {partition} ORDER BY ({key_expr}) {direction} NULLS LAST, issue.created_at DESC) AS __rn
         {from_where}) __w
         WHERE __w.__rn > {offset} AND __w.__rn <= {stop}
         ORDER BY __w.__order_key {direction} NULLS LAST, __w.created_at DESC",
        from_where = filtered.from_where,
        offset = window.offset,
        stop = window.stop,
    );
    let rows = fetch_json_rows(&context.pool, &inner, filtered.values.clone()).await?;
    let has_more = rows.iter().any(|row| {
        row.get("__rn")
            .and_then(|value| value.as_i64())
            .map(|rn| rn >= window.stop)
            .unwrap_or(false)
    });
    let page: Vec<Map<String, Value>> = rows
        .into_iter()
        .filter(|row| {
            row.get("__rn")
                .and_then(|value| value.as_i64())
                .map(|rn| rn < window.stop)
                .unwrap_or(false)
        })
        .collect();
    // `if results:` tests the window, and `count` is its length.
    let window_empty = page.is_empty();
    let window_len = page.len();
    // Totals over the filtered set with the intake/archived/draft count filter.
    let count_filter =
        "((issue_intake.status = ANY('{1,-1,2}')) OR issue_intake.id IS NULL) AND issue.archived_at IS NULL AND issue.is_draft = FALSE";
    let group_totals = group_total_pairs(context, filtered, &group_expr, count_filter).await?;
    if !window_empty && group_totals.is_empty() {
        // `...order_by("-count")[0]` on an empty group list: IndexError.
        return Err(Denial::ServerError);
    }
    let totals = total_dict(&group_totals);
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let total_count = {
        let sql = format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where);
        fetch_count(&context.pool, &sql, filtered.values.clone()).await?
    };
    let fields = on_results_fields(Some(group_by), sub_group_by.as_deref());
    let shaped: Vec<Map<String, Value>> = page
        .iter()
        .map(|row| shape_map(row, &fields, &context.gate.timezone, false))
        .collect();
    let group_fields = group_values(
        &context.pool,
        group_by,
        &context.slug,
        &context.gate.project_id,
        filtered,
    )
    .await?;
    let results_value = match &sub_expr {
        Some(sub) => {
            let sub_pairs =
                sub_total_pairs(context, filtered, &group_expr, sub, count_filter).await?;
            let (group_totals_map, sub_totals_map) = sub_total_dicts(&group_totals, &sub_pairs);
            let _ = group_totals_map;
            let seeded = sub_field_dict(&group_fields, &totals, &sub_totals_map)
                .map_err(|_| Denial::ServerError)?;
            let Value::Object(cells) = seeded else {
                return Err(Denial::ServerError);
            };
            process_sub_grouped_results(
                &shaped,
                group_by,
                &sub_group_by.clone().unwrap_or_default(),
                cells,
            )
            .map_err(|_| Denial::ServerError)?
        }
        None => process_grouped_results(&shaped, group_by, &group_fields, &totals)
            .map_err(|_| Denial::ServerError)?,
    };
    let results_json = serde_json::to_string(&results_value).map_err(|_| Denial::ServerError)?;
    Ok(json_response(envelope(
        Some(group_by),
        sub_group_by.as_deref(),
        total_count,
        &next.to_string(),
        &prev.to_string(),
        next.has_results_or_false(),
        prev.has_results_or_false(),
        window_len,
        grouped_max_hits(window_empty, top_group_count(&group_totals), limit)
            .map_err(page_denial)?,
        total_count,
        &results_json,
    )))
}

/// Largest raw group count, for grouped `max_hits`.
fn top_group_count(pairs: &[(String, i64)]) -> i64 {
    pairs.iter().map(|(_, count)| *count).max().unwrap_or(0)
}

/// Shape a row into an ordered map (for grouping). Grouped main-list rows
/// render datetimes as stored (no `user_timezone_converter` on that path).
fn shape_map(
    row: &Map<String, Value>,
    fields: &[String],
    timezone: &Tz,
    shift: bool,
) -> Map<String, Value> {
    let mut out = Map::new();
    for field in fields {
        let value = row.get(field).unwrap_or(&Value::Null);
        out.insert(
            field.clone(),
            shape_json_value(field, value, timezone, shift),
        );
    }
    // Group raw keys ride along for the grouper (not in the field list
    // for m2m groups they are appended by on_results_fields already).
    for (key, value) in row {
        if (key == "labels__id" || key == "assignees__id" || key == "issue_module__module_id")
            && !out.contains_key(key)
        {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

fn shape_json_value(field: &str, value: &Value, timezone: &Tz, shift: bool) -> Value {
    match field {
        "created_at" | "updated_at" if shift => match value {
            Value::String(text) => match chrono::DateTime::parse_from_rfc3339(text) {
                Ok(aware) => Value::String(crate::serializer::render_datetime_in(&aware, timezone)),
                Err(_) => value.clone(),
            },
            _ => value.clone(),
        },
        "completed_at" | "deleted_at" => match value {
            Value::String(text) => match chrono::DateTime::parse_from_rfc3339(text) {
                Ok(aware) => Value::String(crate::serializer::render_datetime(
                    &aware.with_timezone(&chrono::Utc),
                )),
                Err(_) => value.clone(),
            },
            _ => value.clone(),
        },
        "sort_order" => match value {
            Value::Number(number) => {
                let float = number.as_f64().unwrap_or(f64::NAN);
                Value::String(crate::paginator::py_float_str(float))
            }
            _ => value.clone(),
        },
        _ => value.clone(),
    }
}

/// The SQL expression a group field partitions by.
fn group_expression(field: &str) -> Result<String, Denial> {
    Ok(match field {
        "labels__id" => "label_issue.label_id".to_owned(),
        "assignees__id" => "issue_assignee.assignee_id".to_owned(),
        "issue_module__module_id" => "issue_module.module_id".to_owned(),
        "state_id" => "issue.state_id".to_owned(),
        "priority" => "issue.priority".to_owned(),
        "cycle_id" => "(SELECT ci.cycle_id FROM cycle_issues ci WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1)".to_owned(),
        "project_id" => "issue.project_id".to_owned(),
        "state__group" => "state.\"group\"".to_owned(),
        "target_date" => "issue.target_date".to_owned(),
        "start_date" => "issue.start_date".to_owned(),
        "created_by" => "issue.created_by_id".to_owned(),
        _ => return Err(Denial::ServerError),
    })
}

/// The array annotation the grouper skips for an m2m group field.
fn skip_array_for(group_by: &str) -> Option<&'static str> {
    match group_by {
        "labels__id" => Some("label_ids"),
        "assignees__id" => Some("assignee_ids"),
        "issue_module__module_id" => Some("module_ids"),
        _ => None,
    }
}

/// Extra select carrying the raw m2m group key per joined row.
fn group_member_select(group_by: &str) -> &'static str {
    match group_by {
        "labels__id" => ", label_issue.label_id AS \"labels__id\"",
        "assignees__id" => ", issue_assignee.assignee_id AS \"assignees__id\"",
        "issue_module__module_id" => ", issue_module.module_id AS \"issue_module__module_id\"",
        _ => "",
    }
}

/// `(group, filtered count)` pairs for the totals dict.
async fn group_total_pairs(
    context: &ListContext,
    filtered: &FilteredSet,
    group_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COUNT(DISTINCT issue.id) FILTER (WHERE {strip_and}) AS n {} GROUP BY 1",
        filtered.from_where,
        strip_and = count_filter.strip_prefix("AND ").unwrap_or(count_filter),
    );
    let query = bind_all(&sql, filtered.values.clone())?;
    let rows = query
        .fetch_all(&context.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::new();
    for row in rows {
        let bucket: String = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
        let count: i64 = row.try_get("n").map_err(|_| Denial::ServerError)?;
        out.push((bucket, count));
    }
    Ok(out)
}

/// `(group, sub, count)` pairs for the nested sub-totals.
async fn sub_total_pairs(
    context: &ListContext,
    filtered: &FilteredSet,
    group_expr: &str,
    sub_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COALESCE(({sub_expr})::text, 'None') AS sub, COUNT(DISTINCT issue.id) FILTER (WHERE {strip_and}) AS n {} GROUP BY 1, 2",
        filtered.from_where,
        strip_and = count_filter.strip_prefix("AND ").unwrap_or(count_filter),
    );
    let query = bind_all(&sql, filtered.values.clone())?;
    let rows = query
        .fetch_all(&context.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::new();
    for row in rows {
        let bucket: String = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
        let sub: String = row.try_get("sub").map_err(|_| Denial::ServerError)?;
        let count: i64 = row.try_get("n").map_err(|_| Denial::ServerError)?;
        out.push((bucket, sub, count));
    }
    Ok(out)
}

/// Whether the pagination window itself is empty (`if results:` in
/// `get_result` guards `max_hits`).
/// `GET .../issues/list/`: flat fetch of an explicit id set. With
/// `fields`/`expand` present the view returns `IssueSerializer` rows over
/// the rich-filtered `queryset` instead of the `.values()` list (see
/// [`serializer_list`]); otherwise the `.values()` branch below runs.
/// Neither branch scopes guests — the view has no role-5 check.
pub async fn flat_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = list_context(state, slug, project_id, &query, extension, ListMode::Flat).await?;
    record_recent_visit(&context).await;
    let ids = context.params.issue_ids.clone().unwrap_or_default();
    let mut validated = Vec::with_capacity(ids.len());
    for id in &ids {
        validated.push(
            id.parse::<uuid::Uuid>()
                .map_err(|_| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?,
        );
    }
    if !context.params.is_flat_shape() {
        return serializer_list(&context, &query, validated).await;
    }
    let order_spec = order_sql(&context.params.order_by, "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let mut extra_binder = Binder::new();
    let mut holders = Vec::with_capacity(validated.len());
    for id in validated {
        holders.push(extra_binder.bind_uuid(id));
    }
    let extra = format!("issue.id IN ({})", holders.join(","));
    let filtered = filtered_set(
        &context.gate,
        &context.slug,
        &query,
        context.params.group_by.as_deref(),
        context.params.sub_group_by.as_deref(),
        Some((extra, extra_binder.values())),
        false,
        FilterLayers::NO_GUEST,
    )?;
    // A grouped m2m field drops its array annotation, and then `.values()`
    // cannot resolve it: Django's `FieldError` (generic 500).
    if let Some(group) = context.params.group_by.as_deref() {
        if skip_array_for(group).is_some() {
            return Err(Denial::ServerError);
        }
    }
    // Flat ordering: the `order_issue_queryset` fragment directly — no
    // paginator re-ordering, no `NULLS LAST` (Postgres defaults apply).
    let order_clause = flat_order_sql(&order_spec, &context.params.order_by)?;
    let selects = annotation_selects(true, None, false, true);
    let inner = format!(
        "SELECT {selects} {} ORDER BY {order_clause}",
        filtered.from_where
    );
    let rows = fetch_json_rows(&context.pool, &inner, filtered.values.clone()).await?;
    // The flat `.values()` shape: 26 keys with `deleted_at`, no `state__group`.
    let fields: Vec<String> = LIST_VALUES_FIELDS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let shaped: Vec<String> = rows
        .iter()
        .map(|row| shape_row(row, &fields, &context.gate.timezone, true))
        .collect();
    Ok(json_response(format!("[{}]", shaped.join(","))))
}

/// `IssueSerializer` keys present on the flat `fields=`/`expand=` branch,
/// in `Meta.fields` order. Annotation-backed keys (`cycle_id`,
/// `module_ids`, `label_ids`, `assignee_ids`, `sub_issues_count`,
/// `attachment_count`, `link_count`) are absent: the view serializes the
/// un-annotated `queryset`, so DRF's missing-attribute rule (`SkipField`
/// on non-required fields) omits them.
const SERIALIZER_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "assigned_pod_id",
    "agent_executor",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "archived_at",
    "is_synced",
];

/// The flat `fields=`/`expand=` branch: `IssueSerializer(queryset, ...)`
/// rows, where `queryset` is the rich-filtered base — the legacy
/// `issue_filters` never run on it (ported bug), there are no annotations
/// and no explicit ordering (the model's `-created_at` default applies).
/// `fields=` itself is discarded (`DynamicBaseSerializer` overwrites it
/// with `expand`), and `expand` only ever *adds* nested keys, so the base
/// shape below is what every `fields`-only request renders. Nested
/// `expand` objects (the expansion mapper + `issue_attachments`) are a
/// known residual gap: the contract suite never sends `expand` on this
/// path, and the nested lite-serializer fleet is out of pilot scope.
async fn serializer_list(
    context: &ListContext,
    query: &QueryMap,
    validated: Vec<uuid::Uuid>,
) -> HandlerResult {
    let mut extra_binder = Binder::new();
    let mut holders = Vec::with_capacity(validated.len());
    for id in validated {
        holders.push(extra_binder.bind_uuid(id));
    }
    let extra = format!("issue.id IN ({})", holders.join(","));
    let filtered = filtered_set(
        &context.gate,
        &context.slug,
        query,
        None,
        None,
        Some((extra, extra_binder.values())),
        false,
        FilterLayers::SERIALIZER,
    )?;
    // `is_synced`: empty `external_source` short-circuits false before any
    // query; otherwise either sync table holds a live row (both use
    // soft-deletion managers, so deleted rows do not count).
    let selects = r#"issue.id, issue.name, issue.state_id, issue.sort_order,
        issue.completed_at, issue.estimate_point_id AS estimate_point, issue.priority,
        issue.complexity_score, issue.start_date, issue.target_date, issue.sequence_id,
        issue.project_id, issue.parent_id, issue.assigned_pod_id, issue.agent_executor,
        issue.created_at, issue.updated_at, issue.created_by_id AS created_by,
        issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at,
        (CASE WHEN issue.external_source IS NULL OR issue.external_source = '' THEN FALSE
         ELSE (EXISTS (SELECT 1 FROM git_issue_syncs g
                       WHERE g.issue_id = issue.id AND g.deleted_at IS NULL)
            OR EXISTS (SELECT 1 FROM github_issue_syncs gh
                       WHERE gh.issue_id = issue.id AND gh.deleted_at IS NULL))
         END) AS is_synced"#;
    let inner = format!(
        "SELECT {selects} {} ORDER BY issue.created_at DESC",
        filtered.from_where
    );
    let rows = fetch_json_rows(&context.pool, &inner, filtered.values.clone()).await?;
    let fields: Vec<String> = SERIALIZER_FIELDS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let shaped: Vec<String> = rows
        .iter()
        .map(|row| serializer_shape_row(row, &fields, &context.gate.timezone))
        .collect();
    Ok(json_response(format!("[{}]", shaped.join(","))))
}

/// One `IssueSerializer` row: model fields in `Meta.fields` order with
/// every DRF `DateTimeField` shifted into the actor's zone
/// (`enforce_timezone` against the activated tz, `Z`-normalized like the
/// flat `.values()` converter path).
fn serializer_shape_row(row: &Map<String, Value>, fields: &[String], timezone: &Tz) -> String {
    let mut out = String::from("{");
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        let value = row.get(field).unwrap_or(&Value::Null);
        let rendered = match field.as_str() {
            "created_at" | "updated_at" | "completed_at" | "archived_at" => {
                shift_datetime(value, timezone)
            }
            "sort_order" => match value {
                Value::Number(number) => {
                    let float = number.as_f64().unwrap_or(f64::NAN);
                    crate::paginator::py_float_str(float)
                }
                _ => serde_json::to_string(value).unwrap_or("null".to_owned()),
            },
            _ => serde_json::to_string(value).unwrap_or("null".to_owned()),
        };
        out.push_str(&rendered);
    }
    out.push('}');
    out
}

/// `ORDER BY` for the unpaginated flat endpoint: the rewritten branches
/// carry complete SQL; the default branch resolves the raw param without
/// the paginator's `NULLS LAST` or forced tiebreak.
fn flat_order_sql(order_spec: &OrderSpec, orig_param: &str) -> Result<String, Denial> {
    let out = order_spec.out_param.as_str();
    if out == "priority_order"
        || out == "-priority_order"
        || out == "state_order"
        || out == "-state_order"
    {
        return Ok(order_spec.order_by_sql.clone());
    }
    if out == "min_values" || out == "-min_values" {
        let descending = out.starts_with('-');
        let sub = min_order_subquery(orig_param.trim_start_matches('-'))?;
        return Ok(format!(
            "{sub} {}, created_at DESC",
            if descending { "DESC" } else { "ASC" }
        ));
    }
    let (expr, descending) = order_key(out, orig_param)?;
    let mut sql = format!("{expr} {}", if descending { "DESC" } else { "ASC" });
    if !orig_param.contains("created_at") {
        sql.push_str(", created_at DESC");
    }
    Ok(sql)
}

/// The `IssueDetailEndpoint.get` permission subquery: the issue survives
/// when the actor holds an active project membership with a role above
/// guest, or an active guest membership on a project with
/// `guest_view_all_features`, or an active guest membership without it on
/// issues they created. The outer query already pins the issue's workspace
/// and project, so correlating the primary key (plus the project, belt and
/// braces) makes the subquery's own workspace/project filters redundant —
/// omitted, not weakened. Role and soft-delete semantics mirror
/// [`allow_project`].
fn permission_exists_fragment(binder: &mut Binder, gate: &Gate) -> String {
    let project_holder = binder.bind_uuid(gate.project_id);
    let user_holder = binder.bind_uuid(gate.user_id);
    let branch = |role_pred: &str, view_all: &str, own_only: bool| {
        let own = if own_only {
            format!(" AND _perm.created_by_id = {user_holder}")
        } else {
            String::new()
        };
        format!(
            "EXISTS (SELECT 1 FROM project_members AS _pm \
             JOIN projects AS _pp ON _pp.id = _pm.project_id \
             WHERE _pm.project_id = _perm.project_id \
             AND _pm.member_id = {user_holder} AND _pm.is_active \
             AND _pm.deleted_at IS NULL AND {role_pred} \
             AND _pp.guest_view_all_features = {view_all}{own})"
        )
    };
    format!(
        "EXISTS (SELECT 1 FROM issues AS _perm \
         WHERE _perm.id = issue.id AND _perm.project_id = {project_holder} \
         AND ({} OR {} OR {}))",
        branch("_pm.role > 5", "TRUE", false),
        branch("_pm.role = 5", "TRUE", false),
        branch("_pm.role = 5", "FALSE", true),
    )
}

/// `GET .../issues-detail/`: `IssueDetailEndpoint.get` — the permission
/// `Exists` subquery over the same filtered set, the prefetch-equivalent
/// arrays (manager-deleted-guarded, no archived-module guard),
/// `order_issue_queryset`, and the plain `OffsetPaginator` shaping rows
/// with `IssueListDetailSerializer`.
///
/// Differences from [`list_issues`], all literal from base.py:
/// - no `updated_at__gt` and no `group_by`/`sub_group_by` (those params
///   are parsed for `per_page`/`cursor`/`order_by` only; they never
///   mismatch here),
/// - no `recent_visited_task` side effect (the view never fires it),
/// - rows render datetimes as stored (the hand-written `to_representation`
///   returns raw values; no `user_timezone_converter` runs here),
/// - `fields=` is ignored by the serializer and `expand=` only appends
///   relation arrays, so with no `expand` the shape is [`DETAIL_FIELDS`].
///   `expand=issue_relation|issue_related` is a known gap (recorded in the
///   workpad; the contract suite never sends it).
pub async fn detail_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context =
        list_context(state, slug, project_id, &query, extension, ListMode::Detail).await?;
    let params = context.params.clone();
    let gate = context.gate;
    let order_spec = order_sql(&params.order_by, "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let mut perm_binder = Binder::new();
    let exists = permission_exists_fragment(&mut perm_binder, &gate);
    let filtered = filtered_set(
        &gate,
        &context.slug,
        &query,
        None,
        None,
        Some((exists, perm_binder.values())),
        true,
        FilterLayers::NO_GUEST,
    )?;
    let (key_expr, descending) = order_key(&order_spec.out_param, &params.order_by)?;
    let direction = if descending { "DESC" } else { "ASC" };
    // `per_page` already validated by the strict preamble; the cursor
    // parses here with the paginator kernel's exact errors.
    let per_page = params.per_page;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw)
        .map_err(|error| Denial::BadDetail(error.detail()))?;
    let selects = annotation_selects(true, None, false, false);
    let fields: Vec<String> = DETAIL_FIELDS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    flat_paginated_response(
        &context, &filtered, &key_expr, direction, per_page, cursor, selects, fields, false,
    )
    .await
}

/// `GET .../v2/issues/`: cursor page over `updated_at`.
pub async fn v2_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    // Lenient params: v2 never reads `per_page`/`group_by` in Python.
    // Only tenant + guest scoping and `updated_at__gt` shape the rows —
    // the rich and legacy filter stacks never run here.
    let context = list_context(
        state,
        slug,
        project_id,
        &query,
        extension,
        ListMode::Lenient,
    )
    .await?;
    let cursor_raw = query_last(&query, "cursor");
    let cursor_raw = cursor_raw.as_deref();
    let updated_gt = updated_at_gt(&query)?;
    let extra = match updated_gt {
        (Some(fragment), binds) => Some((fragment, binds)),
        (None, _) => None,
    };
    let filtered = filtered_set(
        &context.gate,
        &context.slug,
        &query,
        None,
        None,
        extra,
        false,
        FilterLayers::V2,
    )?;
    let total_results = {
        let sql = format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where);
        fetch_count(&context.pool, &sql, filtered.values.clone()).await?
    };
    let page = v2_page(cursor_raw, total_results).map_err(|_| Denial::ServerError)?;
    let mut selects = annotation_selects(true, None, true, true);
    // `description_html` is a plain model column, not an annotation: v2
    // always selects it (like `.values()` does) so `?description=true`
    // renders it instead of null.
    selects.push_str(", issue.description_html");
    let inner = format!(
        "SELECT {selects} {} ORDER BY issue.updated_at LIMIT {} OFFSET {}",
        filtered.from_where,
        page.end - page.start,
        page.start
    );
    let rows = fetch_json_rows(&context.pool, &inner, filtered.values.clone()).await?;
    let fields: Vec<String> = v2_fields(context.params.description)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let shaped: Vec<String> = rows
        .iter()
        .map(|row| shape_row(row, &fields, &context.gate.timezone, true))
        .collect();
    let next_cursor = page
        .next_cursor
        .map(|cursor| format!("\"{cursor}\""))
        .unwrap_or_else(|| "null".to_owned());
    let body = format!(
        "{{\"prev_cursor\":\"{}\",\"cursor\":\"{}\",\"next_cursor\":{},\"prev_page_results\":{},\"next_page_results\":{},\"page_count\":{},\"total_results\":{},\"total_pages\":{},\"results\":[{}]}}",
        page.prev_cursor,
        page.cursor,
        next_cursor,
        page.prev_page_results,
        page.next_page_results,
        shaped.len(),
        total_results,
        page.total_pages,
        shaped.join(","),
    );
    Ok(json_response(body))
}

/// `GET .../deleted-issues/`: bare id array of archived-or-deleted issues.
pub async fn deleted_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    // Lenient params: deleted-issues never reads `per_page` in Python.
    let context = list_context(
        state,
        slug,
        project_id,
        &query,
        extension,
        ListMode::Lenient,
    )
    .await?;
    let mut binder = Binder::new();
    let slug_holder = binder.bind_string(context.slug.clone());
    let project_holder = binder.bind_uuid(context.gate.project_id);
    let mut where_sql = format!(
        "FROM issues AS issue JOIN projects AS project ON project.id = issue.project_id
         JOIN workspaces ON workspaces.id = issue.workspace_id
         WHERE workspaces.slug = {slug_holder} AND issue.project_id = {project_holder}
         AND (issue.archived_at IS NOT NULL OR issue.deleted_at IS NOT NULL)"
    );
    // `all_objects`: no soft-delete scope, no manager exclusions, and —
    // note — no guest `created_by` scoping either (the view never applies
    // it). The allow gate already ran; deleted-issues has no rich/legacy
    // filters, only `updated_at__gt`.
    let updated_gt = updated_at_gt(&query)?;
    if let Some(extra) = updated_gt.0 {
        let (fragment, binds) = (extra, updated_gt.1);
        where_sql.push_str(&format!(" AND {}", binder.splice(&fragment, binds)));
    }
    let sql = format!("SELECT issue.id::text AS id {where_sql}");
    let query_bound = bind_all(&sql, binder.values())?;
    let rows = query_bound
        .fetch_all(&context.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut ids = Vec::new();
    for row in rows {
        let id: String = row.try_get("id").map_err(|_| Denial::ServerError)?;
        ids.push(id);
    }
    Ok(json_response(deleted_ids_body(&ids)))
}

/// `updated_at__gt` shared by list, v2 and deleted-issues. Garbage is a
/// `ValidationError` (400 invalid detail), not SQL text.
fn updated_at_gt(query: &QueryMap) -> Result<(Option<String>, Vec<sea_query::Value>), Denial> {
    let raw = query_last(query, "updated_at__gt");
    let Some(raw) = raw else {
        return Ok((None, Vec::new()));
    };
    let parsed = parse_datetime_param(&raw)
        .ok_or_else(|| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?;
    let mut binder = Binder::new();
    let holder = binder.bind_string(parsed.to_rfc3339());
    Ok((
        Some(format!("issue.updated_at > {holder}::timestamptz")),
        binder.values(),
    ))
}

/// `recent_visited_task.delay` is a deferred publish, not inline work: in
/// every environment the contract suite runs (memory broker, no worker),
/// Django writes *no* `user_recent_visits` row, and the suite's teardown
/// proves it (it deletes projects without clearing visits). An earlier
/// revision reproduced the task's row inline; live verification showed
/// that breaks the gate's teardown with a `ForeignKeyViolation` Django
/// never produces, so the call point is kept (same fire site as the
/// `.delay()`) but performs no write. Faithful deferral — publishing the
/// Celery-protocol message for a worker to consume — belongs to the tasks
/// layer (it needs jobs-publish wiring the request path must not grow);
/// until then the response bytes are identical and the observable DB state
/// matches Django exactly. See `bgtasks/recent_visited_task.py`.
async fn record_recent_visit(_context: &ListContext) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    fn app() -> Router {
        // Unreachable upstream: proxied siblings fail closed with 502.
        crate::routes::with_routes(
            AppState::with_edge(
                "0.1.0",
                crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
            ),
            routes(),
        )
    }

    async fn status(app: Router, path: &str) -> StatusCode {
        app.oneshot(
            axum::http::Request::get(path)
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve")
        .status()
    }

    /// Non-GET methods on owned paths proxy (502 fail-closed with no
    /// upstream) instead of answering 405: `POST issues/` is Django's
    /// create, and DRF authenticates before it checks the method.
    #[tokio::test]
    async fn non_get_methods_proxy_instead_of_405() {
        for (method, path) in [
            (
                "POST",
                "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/issues/",
            ),
            (
                "POST",
                "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/issues/list/",
            ),
            (
                "DELETE",
                "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/issues-detail/",
            ),
        ] {
            let response = app()
                .oneshot(
                    axum::http::Request::builder()
                        .method(method)
                        .uri(path)
                        .body(axum::body::Body::empty())
                        .expect("request"),
                )
                .await
                .expect("serve");
            assert_eq!(
                response.status(),
                StatusCode::BAD_GATEWAY,
                "{method} {path}"
            );
        }
    }

    /// The five list paths are Rust-owned (they reach the handlers: 500
    /// here only because the test state carries no pools), while the
    /// issue-detail sibling keeps proxying (502 fail-closed with no
    /// upstream).
    #[tokio::test]
    async fn list_paths_are_routed_and_detail_proxies() {
        for path in [
            "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/issues/",
            "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/issues/list/?issues=1",
            "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/issues-detail/",
            "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/v2/issues/",
            "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/deleted-issues/",
        ] {
            assert_eq!(
                status(app(), path).await,
                StatusCode::INTERNAL_SERVER_ERROR,
                "{path}"
            );
        }
        assert_eq!(
            status(
                app(),
                "/api/workspaces/w/projects/00000000-0000-0000-0000-000000000000/issues/00000000-0000-0000-0000-000000000001/"
            )
            .await,
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn count_annotations_render_null_when_empty() {
        // No `Coalesce` in Python: zero related rows yield NULL, not 0.
        let selects = annotation_selects(true, None, false, true);
        assert!(selects.contains("NULLIF(COUNT(*), 0)"));
        assert!(!selects.contains("(SELECT COUNT(*)"));
    }

    #[test]
    fn detail_arrays_keep_deleted_guard_without_module_join() {
        // The detail prefetches (`...objects.all()`) carry the managers'
        // deleted filter but no archived-module guard.
        let guarded = annotation_selects(true, None, false, true);
        assert!(guarded.contains("il.deleted_at IS NULL"));
        assert!(guarded.contains("JOIN modules m ON m.id = mi.module_id"));
        assert!(guarded.contains("m.archived_at IS NULL"));
        let detail = annotation_selects(true, None, false, false);
        assert!(detail.contains("il.deleted_at IS NULL"));
        assert!(detail.contains("ia.deleted_at IS NULL"));
        assert!(detail.contains("mi.deleted_at IS NULL"));
        assert!(!detail.contains("JOIN modules"));
        assert!(!detail.contains("m.archived_at IS NULL"));
    }

    #[test]
    fn permission_exists_has_three_membership_branches() {
        let gate = Gate {
            user_id: uuid::Uuid::nil(),
            timezone: "UTC".parse().expect("tz"),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            guest_scoped: true,
        };
        let mut binder = Binder::new();
        let fragment = permission_exists_fragment(&mut binder, &gate);
        assert!(fragment.starts_with("EXISTS (SELECT 1 FROM issues AS _perm"));
        assert!(fragment.contains("_perm.id = issue.id"));
        assert!(fragment.contains("_pm.role > 5"));
        assert!(fragment.contains("_pm.role = 5"));
        assert!(fragment.contains("_pp.guest_view_all_features = TRUE"));
        assert!(fragment.contains("_pp.guest_view_all_features = FALSE"));
        assert!(fragment.contains("_perm.created_by_id = $2"));
        assert_eq!(binder.values().len(), 2);
    }

    #[test]
    fn detail_filtered_set_skips_guest_scoping_for_exists() {
        let gate = Gate {
            user_id: uuid::Uuid::nil(),
            timezone: "UTC".parse().expect("tz"),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            guest_scoped: true,
        };
        let query: QueryMap = Default::default();
        let scoped = filtered_set(
            &gate,
            "w",
            &query,
            None,
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        assert!(scoped.from_where.contains("issue.created_by_id"));
        // The triage exclusion keeps NULL-state rows (nullable FK +
        // `split_exclude` semantics).
        assert!(scoped
            .from_where
            .contains("(state.\"group\" IS NULL OR NOT (state.\"group\" = 'triage'))"));
        let mut binder = Binder::new();
        let exists = permission_exists_fragment(&mut binder, &gate);
        let detail = filtered_set(
            &gate,
            "w",
            &query,
            None,
            None,
            Some((exists, binder.values())),
            true,
            FilterLayers::NO_GUEST,
        )
        .expect("set");
        assert!(!detail.from_where.contains("issue.created_by_id"));
        assert!(detail
            .from_where
            .contains("EXISTS (SELECT 1 FROM issues AS _perm"));
    }

    #[test]
    fn v2_layers_skip_filter_stacks_but_keep_guest_scope() {
        let gate = Gate {
            user_id: uuid::Uuid::nil(),
            timezone: "UTC".parse().expect("tz"),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            guest_scoped: true,
        };
        let mut query: QueryMap = Default::default();
        query.insert("state".to_owned(), OneOrMany::One("x".to_owned()));
        let v2 = filtered_set(
            &gate,
            "w",
            &query,
            None,
            None,
            None,
            false,
            FilterLayers::V2,
        )
        .expect("set");
        // Guest scoping stays; no rich/legacy predicates, no relation joins.
        assert!(v2.from_where.contains("issue.created_by_id"));
        assert!(!v2.from_where.contains("issue_labels"));
        assert!(!v2.from_where.contains("issue_assignees"));
        assert!(!v2.from_where.contains("intake_issues"));
        assert!(v2.referenced.is_empty());
        let flat = filtered_set(
            &gate,
            "w",
            &Default::default(),
            None,
            None,
            None,
            false,
            FilterLayers::NO_GUEST,
        )
        .expect("set");
        assert!(!flat.from_where.contains("issue.created_by_id"));
    }

    #[test]
    fn order_key_resolves_fk_and_state_names() {
        let (expr, descending) = order_key("estimate_point", "estimate_point").expect("key");
        assert_eq!(expr, "issue.estimate_point_id");
        assert!(!descending);
        let (expr, _) = order_key("-project", "-project").expect("key");
        assert_eq!(expr, "issue.project_id");
        let (expr, _) = order_key("parent", "parent").expect("key");
        assert_eq!(expr, "issue.parent_id");
        let (expr, _) = order_key("-deleted_at", "-deleted_at").expect("key");
        assert_eq!(expr, "issue.\"deleted_at\"");
        let (expr, _) = order_key("state__name", "state__name").expect("key");
        assert_eq!(expr, "state.name");
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            Denial::Unauthorized.status_and_body().1,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            Denial::Forbidden.status_and_body().1,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(
            Denial::NotFound.status_and_body().1,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            Denial::ProjectNotFound.status_and_body().1,
            r#"{"detail":"Project not found"}"#
        );
    }

    #[test]
    fn binder_splices_with_shifted_placeholders() {
        let mut binder = Binder::new();
        let first = binder.bind_string("a".to_owned());
        assert_eq!(first, "$1");
        let merged = binder.splice(
            "x = $1 AND y = $2",
            vec![sea_query::Value::Bool(Some(true))],
        );
        assert_eq!(merged, "x = $2 AND y = $3");
    }

    #[test]
    fn legacy_isnull_and_in_predicates() {
        let mut binder = Binder::new();
        let sql = legacy_sql(&mut binder, "labels__isnull", &FilterValue::Flag(true)).expect("sql");
        assert_eq!(sql, "label_issue.label_id IS NULL");
        let id = uuid::Uuid::nil();
        let sql = legacy_sql(&mut binder, "state__in", &FilterValue::Uuids(vec![id])).expect("sql");
        assert!(sql.starts_with("issue.state_id IN ($"));
    }

    #[test]
    fn logged_by_is_a_server_error_like_djangos_field_error() {
        let mut binder = Binder::new();
        let denial = legacy_sql(
            &mut binder,
            "logged_by__in",
            &FilterValue::Uuids(vec![uuid::Uuid::nil()]),
        )
        .unwrap_err();
        assert!(matches!(denial, Denial::ServerError));
    }

    #[test]
    fn order_key_resolves_rewritten_params() {
        let (expr, descending) = order_key("-created_at", "-created_at").expect("key");
        assert_eq!(expr, "issue.\"created_at\"");
        assert!(descending);
        let (expr, descending) = order_key("priority_order", "-priority").expect("key");
        assert!(expr.contains("CASE"));
        assert!(!descending);
        assert!(order_key("nope", "nope").is_err());
    }

    #[test]
    fn filter_error_maps_to_python_bodies() {
        use pidash_db::filter::FilterError as E;
        let denial = filter_denial(E::InvalidField("zzz".to_owned()));
        match denial {
            Denial::BadFilter(message, code) => {
                assert_eq!(message, "Filtering on field 'zzz' is not allowed");
                assert_eq!(code, "invalid_filter_field");
            }
            _ => panic!("wrong denial"),
        }
        assert!(matches!(
            filter_denial(E::EmptyRangeBounds("x".to_owned())),
            Denial::ServerError
        ));
    }
}
