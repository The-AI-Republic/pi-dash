//! Workspace analytics + analytic-view viewset (D-35, stage 5, PIDASHCONV-389).
//!
//! Ports `AnalyticsEndpoint.get` (`apps/api/pi_dash/app/views/analytic/base.py:38-176`)
//! and `AnalyticViewViewset` list/create/retrieve/partial_update/destroy
//! (`base.py:177-189`) with identical URL paths, status codes and JSON bytes:
//!
//! - `GET workspaces/<slug>/analytics/` (`app/urls/analytic.py:25-29`, route 1)
//! - `GET`/`POST workspaces/<slug>/analytic-view/` (`analytic.py:30-34`, route 2)
//! - `GET`/`PATCH`/`DELETE workspaces/<slug>/analytic-view/<uuid:pk>/`
//!   (`analytic.py:35-39`, route 3)
//!
//! Only these three routes are registered, so the edge serves exactly this
//! family from Rust while every sibling path keeps proxying to Django —
//! route registration is the cutover granularity, no flag needed.
//!
//! Handler notes (all verified against the Python source at drift baseline
//! `01a93e17`):
//! - Gate order: session authN first (anon 401), then the gate, then the
//!   body. The GET uses `@allow_permission([ADMIN, MEMBER], level="WORKSPACE")`
//!   ([`Gate::Workspace`], denial [`FORBIDDEN_BODY`]); the viewset uses
//!   `permission_classes = [WorkSpaceAdminPermission]` ([`Gate::ViewsetAdmin`],
//!   denial [`VIEWSET_FORBIDDEN_BODY`]). See [`crate::app_analytics::gates`].
//! - GET param defaults are `request.GET.get(key, False)`: missing, empty
//!   and repeated-last-wins all flow through [`validate_base_axes`] exactly
//!   like `base.py:41-57`.
//! - `filters = issue_filters(request.GET, "GET")` compiles through the
//!   F-04 `pidash_db::issue_filters` kernels; the SQL text below only
//!   re-points the predicate columns at this scope's quoted table refs
//!   (the pilot `app_issues` compiler uses short aliases that do not exist
//!   here). Relation joins are Django `filter()` joins: INNER when a value
//!   predicate references the relation, else LEFT (the `__isnull` join).
//!   Joined tables carry no soft-delete guard — `filter()` never applies a
//!   related manager across a join (same split as the pilot).
//! - The base scope is the `IssueManager` spelling pinned by
//!   [`pidash_services::app_analytics::queries::ISSUE_OBJECTS_SCOPE`];
//!   the label-details branch reads through plain `objects`
//!   (`base.py:83`, ported bug — no manager exclusions there).
//! - `build_graph_plot` regroups by `str(dimension)` over rows already
//!   ordered by dimension and applies `sort_data` (priority axes sort
//!   low/medium/high/urgent/none with missing keys dropped); NULL
//!   dimensions group under `"None"` (Python `str(None)`).
//! - `AnalyticViewSerializer.create` reads `query_dict` through
//!   `issue_filters(..., "POST")` (`{}` when falsy); `update` reads
//!   `query_data` (absent in practice — unknown input keys are ignored by
//!   DRF) and then unconditionally overwrites with the PATCH mapping, so
//!   the effective query is always the PATCH mapping (ported bug).
//! - `perform_create` looks the workspace up by slug (miss → the
//!   `ObjectDoesNotExist` 404); `get_queryset` filters
//!   `workspace__slug` with `-created_at` ordering; `destroy` soft-deletes
//!   (`SoftDeleteModel.delete` sets `deleted_at`), so reads after delete
//!   404 through the default manager.
//! - `destroy` also publishes `soft_delete_related_objects` (a Celery beat
//!   of the shared soft-delete machinery). Like the pilot's
//!   `recent_visited_task` call site, the publish has no observable effect
//!   in the gate environment, so the handler performs the row write only.
//!
//! Fixture ids: FX-A-H-01 (route pairs), FX-A-Q-01 (base analytics SQL),
//! FX-A-Q-02 (viewset queryset), FX-A-G-01 (gates for these routes).
//!
//! Ported bugs (translation, don't redesign; also listed in the PR):
//! - B1: label-details reads through plain `Issue.objects` — the
//!   triage/archived/draft exclusions do not apply there (`base.py:83`).
//! - B2: `AnalyticViewSerializer.update` always overwrites `query` with the
//!   PATCH mapping, even when `query_data` is empty or missing (`base.py:30`
//!   runs unconditionally, killing the POST line above it).
//!
//! Sibling plumbing mirrors `app_cycles::handlers_analytics` (and through
//! it the D-34 `app_notifications` shape): [`owned`], session [`actor`],
//! exact denial bodies, manual envelope assembly for DRF key order/bytes.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::NaiveDate;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{
    decide_gate, deny_body, gate_for, tenant_context, Gate, GateOutcome, ANON_BODY, FORBIDDEN_BODY,
    NOT_FOUND_BODY, VIEWSET_FORBIDDEN_BODY,
};
use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_services::app_analytics::queries;
use pidash_types::WorkspaceId;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Route path templates for this family, in `app/urls/analytic.py` form
/// (the matching rows also live in [`crate::app_analytics::gates::GATES`]).
pub const ANALYTICS_PATH: &str = "workspaces/<slug>/analytics/";
/// Route 2: the viewset list/create path.
pub const ANALYTIC_VIEW_PATH: &str = "workspaces/<slug>/analytic-view/";
/// Route 3: the viewset detail path (spelled like the [`gate_for`] table:
/// `<uuid>`, not `<uuid:pk>`).
pub const ANALYTIC_VIEW_DETAIL_PATH: &str = "workspaces/<slug>/analytic-view/<uuid>/";

/// Register the three owned routes. Nothing else: sibling paths stay
/// unmatched and proxy to Django, and every non-owned method on the owned
/// paths falls through to Django too (its 405-after-auth and metadata
/// responses live there). `HEAD` rides axum's `get` handling like Django's
/// `GET`-backed `HEAD`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/analytics/",
            owned(
                axum::routing::get(analytics_get),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/analytic-view/",
            owned(
                axum::routing::get(view_list).post(view_create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/analytic-view/{pk}/",
            owned(
                axum::routing::get(view_retrieve)
                    .patch(view_partial_update)
                    .delete(view_destroy),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
}

/// An owned path: the owned methods serve from Rust, everything else proxies
/// to Django (DRF metadata, 401-anon-before-405).
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// DRF `IsAuthenticated` / `NotAuthenticated` 401.
pub const UNAUTHENTICATED_BODY: &str = ANON_BODY;
/// `@allow_permission` 403 on the analytics GET.
pub const PERMISSION_DENIED_BODY: &str = FORBIDDEN_BODY;
/// Viewset class-gate 403 (DRF default — `WorkSpaceAdminPermission` sets no
/// `message`).
pub const VIEWSET_DENIED_BODY: &str = VIEWSET_FORBIDDEN_BODY;
/// `handle_exception`'s `ObjectDoesNotExist` branch (bare `.get()` miss:
/// the `perform_create` workspace lookup).
pub const OBJECT_NOT_FOUND_BODY: &str = NOT_FOUND_BODY;
/// DRF `get_object()` miss on the viewset detail routes:
/// `Http404("No AnalyticView matches the given query.")`.
pub const VIEW_NOT_FOUND_BODY: &str = r#"{"detail":"No AnalyticView matches the given query."}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `ValidationError` branch (bad UUIDs, bad dates).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// DRF `JSONParser` failure prefix (`rest_framework/parsers.py` raises
/// `ParseError('JSON parse error - %s')`); the reason follows the dash.
/// Blank input carries the CPython position (`line 1 column N+1 (char N)`,
/// same mapping as the license console's parser).
pub const JSON_PARSE_PREFIX: &str = "JSON parse error - ";

/// Handler denials with byte-exact bodies.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403 with the gate's own body (decorator vs viewset class).
    Forbidden(&'static str),
    /// 400, axes validation (`AnalyticsEndpoint.get`).
    BadAxes,
    /// 400, segment validation.
    BadSegment,
    /// 404, `ObjectDoesNotExist` branch.
    ObjectNotFound,
    /// 404, viewset `get_object()` miss.
    ViewNotFound,
    /// 400, `ValidationError` branch.
    BadValidation,
    /// 400, malformed JSON body (carries the full `detail` text, reason
    /// included, like DRF's `ParseError`).
    BadJson(String),
    /// 400, serializer field errors (`{"name": [...]}`).
    BadFields(Value),
    /// 500, anything Python lets escape (`FieldError`, `TypeError`, DB
    /// errors, unparseable stored values).
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden(body) => (StatusCode::FORBIDDEN, (*body).to_owned()),
            Denial::BadAxes => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&queries::axes_error_body()).expect("axes body"),
            ),
            Denial::BadSegment => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&queries::segment_error_body()).expect("segment body"),
            ),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ViewNotFound => (StatusCode::NOT_FOUND, VIEW_NOT_FOUND_BODY.to_owned()),
            Denial::BadValidation => (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY.to_owned()),
            Denial::BadJson(detail) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&serde_json::json!({"detail": detail}))
                    .expect("json parse body"),
            ),
            Denial::BadFields(errors) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(errors).expect("field errors"),
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
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}

fn json_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    let pool = pool_of(state)?;
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// Active workspace role for `(user, slug)`, or `None` (no row). Mirrors
/// the `allow_permission` workspace lookup (`is_active=True`, soft-deleted
/// rows excluded, `app/permissions/base.py:44-51`).
async fn workspace_role(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    slug: &str,
) -> Result<Option<i32>, Denial> {
    let row: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.and_then(|row| row.0).map(i32::from))
}

/// Membership facts for one route's gate: the caller passes the route's
/// [`Gate`] so the allowed-role set matches the Python source
/// (`[ADMIN, MEMBER]` on the analytics GET via the decorator;
/// `[ADMIN, MEMBER]` on the viewset via `WorkSpaceAdminPermission`).
fn workspace_facts(slug: &str, role: Option<i32>, gate: &Gate) -> AllowFacts {
    let allowed_roles: &[i32] = match gate {
        Gate::Workspace { roles } => roles,
        Gate::ViewsetAdmin => &[ROLE_ADMIN, ROLE_MEMBER],
        Gate::Project { roles } => roles,
    };
    AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.is_some_and(|role| allowed_roles.contains(&role)),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(ROLE_ADMIN),
    }
}

/// Enforce one gate row: allow runs, deny answers the gate's own 403 body.
/// Anonymous never reaches here ([`actor`] 401s first).
#[allow(clippy::result_large_err)]
fn enforce(outcome: GateOutcome, gate: &Gate) -> Result<(), Response> {
    match outcome {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Deny => Err(Denial::Forbidden(deny_body(gate)).into_response()),
        GateOutcome::Unauthenticated => Err(Denial::Unauthorized.into_response()),
    }
}

/// Resolve the gate row for `method` + `path`, run it for `(slug, user)`,
/// and return the actor on allow. `path` is the `gate_for` template
/// (`workspaces/<slug>/analytics/` form).
#[allow(clippy::result_large_err)]
async fn gated_actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
    method: &str,
    path: &str,
    slug: &str,
) -> Result<crate::license::Actor, Response> {
    let actor = actor(state, extension).await.map_err(|denial| {
        // `actor` only fails Unauthorized (no session) or ServerError;
        // both render directly.
        denial.into_response()
    })?;
    let gate = gate_for(method, path).ok_or(Denial::ServerError.into_response())?;
    let pool = pool_of(state).map_err(|denial| denial.into_response())?;
    let role = workspace_role(pool, &actor.id, slug)
        .await
        .map_err(|denial| denial.into_response())?;
    let scope = tenant_context(slug);
    let outcome = decide_gate(&gate.gate, &scope, &workspace_facts(slug, role, &gate.gate));
    enforce(outcome, &gate.gate)?;
    Ok(actor)
}

// ---------------------------------------------------------------------------
// Query params + issue_filters(GET) SQL
// ---------------------------------------------------------------------------

/// One query value, repeated or not. `serde_html_form` (axum's `Query`
/// backend) does not coerce a lone `?key=value` into a sequence, so the
/// extractor uses this untagged shape and callers read the last value —
/// mirroring Django's `QueryDict`, where repeats are legal and `.get`
/// returns the last value.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every analytics handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    match query.get(key) {
        Some(OneOrMany::One(one)) => Some(one.clone()),
        Some(OneOrMany::Many(many)) => many.last().cloned(),
        None => None,
    }
}

/// Flat last-wins map for the F-04 `pidash_db::issue_filters` kernels
/// (their `params.get(key)` is Django's `.get`).
pub fn flat_params(query: &QueryMap) -> HashMap<String, String> {
    query
        .keys()
        .filter_map(|key| query_last(query, key).map(|value| (key.clone(), value)))
        .collect()
}

/// A bound value in filter-SQL position.
#[derive(Debug, Clone)]
pub enum BindVal {
    Text(String),
    Uid(Uuid),
    Int(i32),
}

/// Sequential bind collector. Placeholders start at `$2` (`$1` is the
/// workspace slug in every Q-01 builder); each compiled statement binds
/// slug first, then these values in order.
#[derive(Debug, Default)]
pub struct Binds {
    values: Vec<BindVal>,
}

impl Binds {
    pub fn placeholder(&mut self, value: BindVal) -> String {
        self.values.push(value);
        // `$1` is the workspace slug; filter binds follow it.
        format!("${}", self.values.len() + 1)
    }

    pub fn values(&self) -> &[BindVal] {
        &self.values
    }
}

/// Bind `$1` (workspace slug) plus the filter binds, for typed
/// `sqlx::query_as` fetches.
pub fn bind_all_as<'q, O>(
    query: sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>,
    slug: &'q str,
    binds: &'q [BindVal],
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments> {
    let mut query = query.bind(slug);
    for value in binds {
        query = match value {
            BindVal::Text(text) => query.bind(text),
            BindVal::Uid(id) => query.bind(id),
            BindVal::Int(number) => query.bind(number),
        };
    }
    query
}

/// A relation join the scope may need: handler-local alias plus SQL table.
/// Predicate columns reference the alias; the pilot `app_issues` compiler
/// uses the same aliases (this scope has no short aliases of its own).
const RELATION_JOINS: &[(&str, &str)] = &[
    ("label_issue", "issue_labels"),
    ("issue_assignee", "issue_assignees"),
    ("issue_cycle", "cycle_issues"),
    ("issue_module", "module_issues"),
    ("issue_mention", "issue_mentions"),
    ("issue_subscribers", "issue_subscribers"),
    ("issue_intake", "intake_issues"),
];

/// True when every `"alias".` reference in the filter SQL is an
/// `IS NULL` / `IS NOT NULL` test (the `__isnull` predicates): Django
/// joins those relations `LEFT OUTER`, value predicates `INNER`.
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

/// Join legs for the relation aliases a filter fragment references.
/// `INNER` when a value predicate touches the alias, else `LEFT OUTER`
/// (Django's `filter()` vs `__isnull` join). Through-table FK attnames are
/// `issue_id` on every leg (Django `<model>_id` for the `issue` FK).
pub fn relation_joins(fragment: &str) -> String {
    let mut out = String::new();
    for (alias, table) in RELATION_JOINS {
        if !fragment.contains(&format!("\"{alias}\".")) {
            continue;
        }
        let join = if alias_nullable_only(fragment, alias) {
            "LEFT OUTER JOIN"
        } else {
            "INNER JOIN"
        };
        out.push_str(&format!(
            " {join} \"{table}\" \"{alias}\" ON (\"issues\".\"id\" = \"{alias}\".\"issue_id\")"
        ));
    }
    out
}

/// Columns for `__isnull` predicates (and bare `Null` values), in this
/// scope's quoted refs.
fn isnull_column(path: &str) -> Option<&'static str> {
    Some(match path {
        "parent" => "\"issues\".\"parent_id\"",
        "labels" => "\"label_issue\".\"label_id\"",
        "assignees" => "\"issue_assignee\".\"assignee_id\"",
        "created_by" => "\"issues\".\"created_by_id\"",
        "issue_cycle__cycle_id" => "\"issue_cycle\".\"cycle_id\"",
        "issue_module__module_id" => "\"issue_module\".\"module_id\"",
        "label_issue__deleted_at" => "\"label_issue\".\"deleted_at\"",
        "issue_assignee__deleted_at" => "\"issue_assignee\".\"deleted_at\"",
        "issue_cycle__deleted_at" => "\"issue_cycle\".\"deleted_at\"",
        "issue_module__deleted_at" => "\"issue_module\".\"deleted_at\"",
        "issue_subscribers__deleted_at" => "\"issue_subscribers\".\"deleted_at\"",
        "target_date" => "\"issues\".\"target_date\"",
        "start_date" => "\"issues\".\"start_date\"",
        _ => return None,
    })
}

/// Columns for UUID `__in` predicates. `logged_by` has no model field:
/// Django raises `FieldError` (generic 500).
fn uuid_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__in" => "\"issues\".\"state_id\"",
        "parent__in" => "\"issues\".\"parent_id\"",
        "labels__in" => "\"label_issue\".\"label_id\"",
        "assignees__in" => "\"issue_assignee\".\"assignee_id\"",
        "issue_mention__mention__id__in" => "\"issue_mention\".\"mention_id\"",
        "created_by__in" => "\"issues\".\"created_by_id\"",
        "project__in" => "\"issues\".\"project_id\"",
        "issue_cycle__cycle_id__in" => "\"issue_cycle\".\"cycle_id\"",
        "issue_module__module_id__in" => "\"issue_module\".\"module_id\"",
        "issue_subscribers__subscriber_id__in" => "\"issue_subscribers\".\"subscriber_id\"",
        _ => return None,
    })
}

/// Columns for string `__in` predicates.
fn strings_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__group__in" => "\"states\".\"group\"",
        "estimate_point__in" => "\"issues\".\"estimate_point_id\"",
        "priority__in" => "\"issues\".\"priority\"",
        "issue_intake__status__in" => "\"issue_intake\".\"status\"",
        _ => return None,
    })
}

/// `(column, operator)` for `Day` predicates (`__gte` / `__lte`).
fn day_comparison(name: &str) -> Option<(&'static str, &'static str)> {
    let (term, operator) = name.rsplit_once("__")?;
    let column = match term {
        "created_at__date" => "\"issues\".\"created_at\"::date",
        "completed_at__date" => "\"issues\".\"completed_at\"::date",
        "start_date" => "\"issues\".\"start_date\"",
        "target_date" => "\"issues\".\"target_date\"",
        _ => return None,
    };
    let operator = match operator {
        "gte" => ">=",
        "lte" => "<=",
        _ => return None,
    };
    Some((column, operator))
}

/// Compile one `issue_filters(query_params, "GET")` predicate to SQL text
/// (placeholders allocated from `binds`). Predicate names are Django ORM
/// paths; unknown names are a `FieldError` in Python, a 500 here (same
/// split as the pilot compiler).
#[allow(clippy::result_large_err)]
pub fn filter_sql(
    binds: &mut Binds,
    name: &str,
    value: &pidash_db::issue_filters::FilterValue,
) -> Result<String, Denial> {
    use pidash_db::issue_filters::FilterValue;
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
    match value {
        FilterValue::Uuids(ids) => {
            let column = uuid_in_column(name).ok_or(Denial::ServerError)?;
            if ids.is_empty() {
                return Ok("FALSE".to_owned());
            }
            let mut holders = Vec::with_capacity(ids.len());
            for id in ids {
                holders.push(binds.placeholder(BindVal::Uid(*id)));
            }
            Ok(format!("{column} IN ({})", holders.join(",")))
        }
        FilterValue::Strings(items) => {
            let column = strings_in_column(name).ok_or(Denial::ServerError)?;
            if items.is_empty() {
                return Ok("FALSE".to_owned());
            }
            if name == "issue_intake__status__in" {
                let mut holders = Vec::with_capacity(items.len());
                for item in items {
                    let number: i32 = item.parse().map_err(|_| Denial::BadValidation)?;
                    holders.push(binds.placeholder(BindVal::Int(number)));
                }
                return Ok(format!("{column} IN ({})", holders.join(",")));
            }
            if name == "estimate_point__in" {
                // UUID PKs compared to strings: Django coerces via
                // `UUIDField.get_prep_value`; garbage is a ValidationError.
                let mut holders = Vec::with_capacity(items.len());
                for item in items {
                    let id: Uuid = item.parse().map_err(|_| Denial::BadValidation)?;
                    holders.push(binds.placeholder(BindVal::Uid(id)));
                }
                return Ok(format!("{column} IN ({})", holders.join(",")));
            }
            let mut holders = Vec::with_capacity(items.len());
            for item in items {
                holders.push(binds.placeholder(BindVal::Text(item.clone())));
            }
            Ok(format!("{column} IN ({})", holders.join(",")))
        }
        FilterValue::Text(text) => filter_text_sql(binds, name, text),
        FilterValue::Flag(_) => Err(Denial::ServerError),
        FilterValue::Day(day) => {
            let (column, operator) = day_comparison(name).ok_or(Denial::ServerError)?;
            let holder = binds.placeholder(BindVal::Text(day.to_string()));
            Ok(format!("{column} {operator} {holder}::date"))
        }
        FilterValue::Null => {
            let column = isnull_column(name).ok_or(Denial::ServerError)?;
            Ok(format!("{column} IS NULL"))
        }
    }
}

/// Text predicates: `name__icontains`, explicit date bounds
/// (`__gte`/`__lte` on a `__date` term or plain date term), and the
/// single-value `__contains` form. Bare `start_date`/`target_date` are
/// Django exact lookups.
#[allow(clippy::result_large_err)]
fn filter_text_sql(binds: &mut Binds, name: &str, text: &str) -> Result<String, Denial> {
    if name == "name__icontains" {
        // Django `icontains`: LIKE with `\`, `%`, `_` escaped.
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let holder = binds.placeholder(BindVal::Text(format!("%{escaped}%")));
        return Ok(format!("\"issues\".\"name\" ILIKE {holder}"));
    }
    // Bare date terms are exact lookups (`filter(start_date="...")`).
    if name == "start_date" || name == "target_date" {
        let column = if name == "start_date" {
            "\"issues\".\"start_date\""
        } else {
            "\"issues\".\"target_date\""
        };
        if text.parse::<NaiveDate>().is_err() && parse_datetime_param(text).is_none() {
            return Err(Denial::BadValidation);
        }
        let holder = binds.placeholder(BindVal::Text(text.to_owned()));
        return Ok(format!("{column} = {holder}::date"));
    }
    let Some((term, operator)) = name.rsplit_once("__") else {
        return Err(Denial::ServerError);
    };
    let operator = match operator {
        "gte" => ">=",
        "lte" => "<=",
        "contains" => "=",
        _ => return Err(Denial::ServerError),
    };
    let column = match term {
        "created_at__date" => "\"issues\".\"created_at\"::date",
        "completed_at__date" => "\"issues\".\"completed_at\"::date",
        "start_date" => "\"issues\".\"start_date\"",
        "target_date" => "\"issues\".\"target_date\"",
        _ => return Err(Denial::ServerError),
    };
    // Explicit bounds must parse as dates: Django's `get_prep_value`
    // raises `ValidationError` (400 invalid detail) on garbage.
    if text.parse::<NaiveDate>().is_err() && parse_datetime_param(text).is_none() {
        return Err(Denial::BadValidation);
    }
    if operator == "=" {
        // Single-value form on a date term: Django's `contains`
        // lookup, i.e. LIKE with metacharacters escaped.
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let holder = binds.placeholder(BindVal::Text(format!("%{escaped}%")));
        return Ok(format!("{column}::text LIKE {holder}"));
    }
    let holder = binds.placeholder(BindVal::Text(text.to_owned()));
    Ok(format!("{column} {operator} {holder}::date"))
}

/// Parse a datetime param the way Django's `DateTimeField.get_prep_value`
/// does (naive values attach UTC). Garbage is a `ValidationError`.
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

/// Compile the request's `issue_filters(params, "GET")` predicates plus the
/// scope into one `{filters}` fragment and its binds. `manager` selects the
/// `IssueManager` scope (every analytics read) vs the plain soft-delete
/// scope (the label-details bug branch, `base.py:83`).
#[allow(clippy::result_large_err)]
pub fn scope_and_filters(query: &QueryMap, manager: bool) -> Result<(String, Binds), Denial> {
    let flat = flat_params(query);
    let today = chrono::Utc::now().date_naive();
    let compiled = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut binds = Binds::default();
    let mut parts: Vec<String> = Vec::new();
    parts.push(if manager {
        format!("({})", queries::ISSUE_OBJECTS_SCOPE)
    } else {
        "(\"issues\".\"deleted_at\" IS NULL)".to_owned()
    });
    for (name, value) in compiled.predicates() {
        parts.push(format!("({})", filter_sql(&mut binds, name, value)?));
    }
    Ok((parts.join(" AND "), binds))
}

/// The legs the Q-01 builders do not emit: relation joins for whatever the
/// fragment references, plus the manager scope's states leg (Django's
/// exclude join — always `LEFT OUTER`; NULL-state rows still drop out of
/// the `!=` test, same as Django). Legs the statement already joined are
/// skipped (the state/cycle/module detail builders join their own tables).
pub fn scope_joins(statement: &str, fragment: &str, manager: bool) -> String {
    let mut joins = relation_joins(fragment);
    let wants_states = manager || fragment.contains("\"states\".");
    if wants_states && !statement.contains("JOIN \"states\"") {
        joins.push_str(
            " LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
        );
    }
    joins
}

/// Splice scope joins into a builder's statement before the scope's own
/// `WHERE ("workspaces".` clause, which every Q-01 builder carries exactly
/// once (inside the plot subquery where `"issues"` lives, or at top level
/// for the detail lookups).
pub fn scoped_sql(statement: &str, joins: &str) -> String {
    if joins.is_empty() {
        return statement.to_owned();
    }
    match statement.find(" WHERE (\"workspaces\".") {
        Some(index) => {
            let (head, tail) = statement.split_at(index);
            format!("{head}{joins}{tail}")
        }
        None => statement.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// GET analytics/ — AnalyticsEndpoint.get (base.py:38-176)
// ---------------------------------------------------------------------------

/// `GET /api/workspaces/{slug}/analytics/?x_axis=&y_axis=[&segment=][&filters]`.
async fn analytics_get(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
) -> Response {
    let actor = match gated_actor(&state, extension, "GET", ANALYTICS_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let _ = actor;
    match analytics_body(&state, &slug, &query).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(denial) => denial.into_response(),
    }
}

/// Build the analytics response body: validate, filter, count, plot, extras.
#[allow(clippy::result_large_err)]
async fn analytics_body(state: &AppState, slug: &str, query: &QueryMap) -> Result<String, Denial> {
    let pool = pool_of(state)?;
    let x_axis = query_last(query, "x_axis");
    let y_axis = query_last(query, "y_axis");
    let segment = query_last(query, "segment").filter(|value| !value.is_empty());
    queries::validate_base_axes(x_axis.as_deref(), y_axis.as_deref(), segment.as_deref()).map_err(
        |error| match error {
            queries::AxisError::Axes => Denial::BadAxes,
            queries::AxisError::Segment => Denial::BadSegment,
        },
    )?;
    let (x_axis, y_axis) = (x_axis.unwrap_or_default(), y_axis.unwrap_or_default());

    // Manager scope + GET predicates, once; the label branch recompiles
    // without the manager scope (B1).
    let (fragment, binds) = scope_and_filters(query, true)?;
    let (label_fragment, label_binds) = scope_and_filters(query, false)?;

    let total = fetch_count(pool, slug, &fragment, &binds).await?;

    let distribution = if y_axis == "issue_count" {
        let sql = queries::base_plot_count_sql(&x_axis, segment.as_deref(), &fragment)
            .ok_or(Denial::ServerError)?;
        let joins = scope_joins(&sql, &fragment, true);
        let rows = fetch_count_plot(
            pool,
            slug,
            &scoped_sql(&sql, &joins),
            &binds,
            segment.as_deref(),
        )
        .await?;
        super::render::distribution_from_count_rows(rows, &x_axis, segment.is_some())
    } else {
        let sql = queries::base_plot_estimate_sql(&x_axis, segment.as_deref(), &fragment)
            .ok_or(Denial::ServerError)?;
        let joins = scope_joins(&sql, &fragment, true);
        let rows = fetch_estimate_plot(
            pool,
            slug,
            &scoped_sql(&sql, &joins),
            &binds,
            segment.as_deref(),
        )
        .await?;
        super::render::distribution_from_estimate_rows(rows, &x_axis, segment.is_some())
    };

    // Extras: each arm runs only for its axis (`base.py:71-159`); anything
    // else stays `{}`.
    let state_details = if x_axis == "state_id" || segment.as_deref() == Some("state_id") {
        Some(fetch_state_details(pool, slug, &fragment, &binds).await?)
    } else {
        None
    };
    let assignee_details =
        if x_axis == "assignees__id" || segment.as_deref() == Some("assignees__id") {
            Some(fetch_assignee_details(pool, slug, &fragment, &binds).await?)
        } else {
            None
        };
    let label_details = if x_axis == "labels__id" || segment.as_deref() == Some("labels__id") {
        Some(fetch_label_details(pool, slug, &label_fragment, &label_binds).await?)
    } else {
        None
    };
    let cycle_details = if x_axis == "issue_cycle__cycle_id"
        || segment.as_deref() == Some("issue_cycle__cycle_id")
    {
        Some(fetch_cycle_details(pool, slug, &fragment, &binds).await?)
    } else {
        None
    };
    let module_details = if x_axis == "issue_module__module_id"
        || segment.as_deref() == Some("issue_module__module_id")
    {
        Some(fetch_module_details(pool, slug, &fragment, &binds).await?)
    } else {
        None
    };
    let extras = super::render::extras_envelope(
        state_details,
        assignee_details,
        label_details,
        cycle_details,
        module_details,
    );
    Ok(super::render::analytics_envelope(
        total,
        distribution,
        extras,
    ))
}

/// Q-01a base count (`base.py:63-66`).
#[allow(clippy::result_large_err)]
async fn fetch_count(
    pool: &sqlx::PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<i64, Denial> {
    let sql = queries::base_count_sql(fragment);
    let scoped = scoped_sql(&sql, &scope_joins(&sql, fragment, true));
    let row: (i64,) = bind_all_as(sqlx::query_as(&scoped), slug, binds.values())
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.0)
}

/// Wrap a plot statement so `dimension`/`segment` decode uniformly as text
/// (`str(dimension)` in the regroup): the builders emit native column
/// types (uuid, numeric, text) that differ per axis.
fn wrapped_plot_sql(inner: &str, value_column: &str, segment: Option<&str>) -> String {
    let segment_select = segment
        .filter(|segment| !segment.is_empty())
        .map(|_| ", \"segment\"::text AS \"segment\"")
        .unwrap_or("");
    format!(
        "SELECT \"dimension\"::text AS \"dimension\"{segment_select}, \"{value_column}\" \
         FROM ({inner}) AS wrapped ORDER BY 1{order_segment}",
        order_segment = if segment.filter(|segment| !segment.is_empty()).is_some() {
            ", 2"
        } else {
            ""
        },
    )
}

/// Q-01b issue_count plot rows: `(dimension, segment, count)`.
#[allow(clippy::result_large_err)]
async fn fetch_count_plot(
    pool: &sqlx::PgPool,
    slug: &str,
    sql: &str,
    binds: &Binds,
    segment: Option<&str>,
) -> Result<Vec<(Option<String>, Option<String>, i64)>, Denial> {
    let wrapped = wrapped_plot_sql(sql, "count", segment);
    if segment.filter(|segment| !segment.is_empty()).is_some() {
        let rows: Vec<(Option<String>, Option<String>, i64)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        Ok(rows)
    } else {
        let rows: Vec<(Option<String>, i64)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        Ok(rows
            .into_iter()
            .map(|(dimension, count)| (dimension, None, count))
            .collect())
    }
}

/// Q-01c estimate plot rows: `(dimension, segment, estimate)`.
#[allow(clippy::result_large_err)]
async fn fetch_estimate_plot(
    pool: &sqlx::PgPool,
    slug: &str,
    sql: &str,
    binds: &Binds,
    segment: Option<&str>,
) -> Result<Vec<(Option<String>, Option<String>, Option<f64>)>, Denial> {
    let wrapped = wrapped_plot_sql(sql, "estimate", segment);
    if segment.filter(|segment| !segment.is_empty()).is_some() {
        let rows: Vec<(Option<String>, Option<String>, Option<f64>)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        Ok(rows)
    } else {
        let rows: Vec<(Option<String>, Option<f64>)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        Ok(rows
            .into_iter()
            .map(|(dimension, estimate)| (dimension, None, estimate))
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Detail lookups — extras arms (base.py:71-159)
// ---------------------------------------------------------------------------

/// Q-01d state details (`base.py:72-78`): `DISTINCT ON (state_id)` rows as
/// `.values("state_id", "state__name", "state__color")`, in that key order.
#[allow(clippy::result_large_err)]
async fn fetch_state_details(
    pool: &sqlx::PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = queries::base_state_details_sql(fragment);
    let scoped = scoped_sql(&sql, &scope_joins(&sql, fragment, true));
    // `state_id` is nullable (`Issue.state`, `null=True`): a NULL group
    // renders `{"state_id": null, ...}`, never a 500.
    let rows: Vec<(Option<Uuid>, Option<String>, Option<String>)> =
        bind_all_as(sqlx::query_as(&scoped), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(id, name, color)| {
            let mut row = Map::new();
            row.insert(
                "state_id".to_owned(),
                id.map(|id| Value::String(id.to_string()))
                    .unwrap_or(Value::Null),
            );
            row.insert(
                "state__name".to_owned(),
                name.map(Value::String).unwrap_or(Value::Null),
            );
            row.insert(
                "state__color".to_owned(),
                color.map(Value::String).unwrap_or(Value::Null),
            );
            row
        })
        .collect())
}

/// Q-01e label details (`base.py:81-92`): `.values("labels__id",
/// "labels__color", "labels__name")`. Reads through the plain scope (B1):
/// callers pass the non-manager fragment and binds.
#[allow(clippy::result_large_err)]
async fn fetch_label_details(
    pool: &sqlx::PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = queries::base_label_details_sql(fragment);
    let scoped = scoped_sql(&sql, &scope_joins(&sql, fragment, false));
    let rows: Vec<(Uuid, Option<String>, Option<String>)> =
        bind_all_as(sqlx::query_as(&scoped), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(id, color, name)| {
            let mut row = Map::new();
            row.insert("labels__id".to_owned(), Value::String(id.to_string()));
            row.insert(
                "labels__color".to_owned(),
                color.map(Value::String).unwrap_or(Value::Null),
            );
            row.insert(
                "labels__name".to_owned(),
                name.map(Value::String).unwrap_or(Value::Null),
            );
            row
        })
        .collect())
}

/// One assignee-details row: `(id, avatar_url, display_name, first_name,
/// last_name)` — the id sorts first in SQL but renders last.
type AssigneeDetailRow = (
    Uuid,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Q-01f assignee details (`base.py:94-131`): `.values("assignees__avatar_url",
/// "assignees__display_name", "assignees__first_name", "assignees__last_name",
/// "assignees__id")` — the id sorts first in SQL but renders last.
#[allow(clippy::result_large_err)]
async fn fetch_assignee_details(
    pool: &sqlx::PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = queries::base_assignee_details_sql(fragment);
    let scoped = scoped_sql(&sql, &scope_joins(&sql, fragment, true));
    let rows: Vec<AssigneeDetailRow> = bind_all_as(sqlx::query_as(&scoped), slug, binds.values())
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(id, avatar_url, display_name, first_name, last_name)| {
            let mut row = Map::new();
            row.insert(
                "assignees__avatar_url".to_owned(),
                avatar_url.map(Value::String).unwrap_or(Value::Null),
            );
            row.insert(
                "assignees__display_name".to_owned(),
                display_name.map(Value::String).unwrap_or(Value::Null),
            );
            row.insert(
                "assignees__first_name".to_owned(),
                first_name.map(Value::String).unwrap_or(Value::Null),
            );
            row.insert(
                "assignees__last_name".to_owned(),
                last_name.map(Value::String).unwrap_or(Value::Null),
            );
            row.insert("assignees__id".to_owned(), Value::String(id.to_string()));
            row
        })
        .collect())
}

/// Q-01g cycle details (`base.py:133-145`): `.values("issue_cycle__cycle_id",
/// "issue_cycle__cycle__name")`.
#[allow(clippy::result_large_err)]
async fn fetch_cycle_details(
    pool: &sqlx::PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = queries::base_cycle_details_sql(fragment);
    let scoped = scoped_sql(&sql, &scope_joins(&sql, fragment, true));
    let rows: Vec<(Uuid, Option<String>)> =
        bind_all_as(sqlx::query_as(&scoped), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| {
            let mut row = Map::new();
            row.insert(
                "issue_cycle__cycle_id".to_owned(),
                Value::String(id.to_string()),
            );
            row.insert(
                "issue_cycle__cycle__name".to_owned(),
                name.map(Value::String).unwrap_or(Value::Null),
            );
            row
        })
        .collect())
}

/// Q-01h module details (`base.py:147-159`): `.values("issue_module__module_id",
/// "issue_module__module__name")`.
#[allow(clippy::result_large_err)]
async fn fetch_module_details(
    pool: &sqlx::PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = queries::base_module_details_sql(fragment);
    let scoped = scoped_sql(&sql, &scope_joins(&sql, fragment, true));
    let rows: Vec<(Uuid, Option<String>)> =
        bind_all_as(sqlx::query_as(&scoped), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| {
            let mut row = Map::new();
            row.insert(
                "issue_module__module_id".to_owned(),
                Value::String(id.to_string()),
            );
            row.insert(
                "issue_module__module__name".to_owned(),
                name.map(Value::String).unwrap_or(Value::Null),
            );
            row
        })
        .collect())
}

// ---------------------------------------------------------------------------
// AnalyticViewViewset — list/create/retrieve/partial_update/destroy
// (base.py:177-189, Q-02a)
// ---------------------------------------------------------------------------

/// One analytic-view row in `pidash_db::app_analytics::models` column
/// order: `id, created_at, updated_at, created_by_id, updated_by_id,
/// deleted_at, workspace_id, name, description, query, query_dict`.
#[derive(Debug, Clone)]
pub struct ViewRow {
    pub id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: String,
    pub query: Value,
    pub query_dict: Value,
}

const VIEW_COLUMNS: &str = "\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \
     \"updated_by_id\", \"deleted_at\", \"workspace_id\", \"name\", \"description\", \
     \"query\", \"query_dict\"";

/// One decoded analytic-view row: the [`VIEW_COLUMNS`] order.
type ViewRowTuple = (
    Uuid,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Utc>,
    Option<Uuid>,
    Option<Uuid>,
    Option<chrono::DateTime<chrono::Utc>>,
    Uuid,
    String,
    String,
    Value,
    Value,
);

/// Q-02a list (`base.py:186-187`): the default-manager queryset filtered by
/// `workspace__slug`, ordered `-created_at` (`Meta.ordering`). The
/// `deleted_at IS NULL` arm is the `SoftDeletionManager` every default
/// queryset carries (`db/mixins.py:56-58`); without it reads after
/// `destroy` would 200 instead of the pinned 404.
#[allow(clippy::result_large_err)]
async fn fetch_view_list(pool: &sqlx::PgPool, slug: &str) -> Result<Vec<ViewRow>, Denial> {
    let sql = format!(
        "SELECT {VIEW_COLUMNS} FROM \"analytic_views\" WHERE (\"analytic_views\".\"workspace_id\" IN \
         (SELECT \"workspaces\".\"id\" FROM \"workspaces\" WHERE \"workspaces\".\"slug\" = $1) \
         AND \"analytic_views\".\"deleted_at\" IS NULL) \
         ORDER BY \"analytic_views\".\"created_at\" DESC"
    );
    let rows: Vec<ViewRowTuple> = sqlx::query_as(&sql)
        .bind(slug)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                created_at,
                updated_at,
                created_by_id,
                updated_by_id,
                _,
                workspace_id,
                name,
                description,
                query,
                query_dict,
            )| {
                ViewRow {
                    id,
                    created_at,
                    updated_at,
                    created_by_id,
                    updated_by_id,
                    workspace_id,
                    name,
                    description,
                    query,
                    query_dict,
                }
            },
        )
        .collect())
}

/// Detail lookup: `get_object()` — pk scoped to the workspace through the
/// default manager. Miss (absent, other-workspace, or soft-deleted) is
/// DRF's `Http404`, the `{"detail": "No AnalyticView ..."}` body.
#[allow(clippy::result_large_err)]
async fn fetch_view_detail(pool: &sqlx::PgPool, slug: &str, pk: &Uuid) -> Result<ViewRow, Denial> {
    let sql = format!(
        "SELECT {VIEW_COLUMNS} FROM \"analytic_views\" WHERE (\"analytic_views\".\"id\" = $1 \
         AND \"analytic_views\".\"workspace_id\" IN \
         (SELECT \"workspaces\".\"id\" FROM \"workspaces\" WHERE \"workspaces\".\"slug\" = $2) \
         AND \"analytic_views\".\"deleted_at\" IS NULL) LIMIT 1"
    );
    let row: Option<ViewRowTuple> = sqlx::query_as(&sql)
        .bind(pk)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(
        |(
            id,
            created_at,
            updated_at,
            created_by_id,
            updated_by_id,
            _,
            workspace_id,
            name,
            description,
            query,
            query_dict,
        )| {
            ViewRow {
                id,
                created_at,
                updated_at,
                created_by_id,
                updated_by_id,
                workspace_id,
                name,
                description,
                query,
                query_dict,
            }
        },
    )
    .ok_or(Denial::ViewNotFound)
}

/// `AnalyticViewSerializer.to_representation`: `Meta.fields = "__all__"`
/// in DRF order (`ANALYTIC_VIEW_FIELDS`), datetimes through the DRF
/// renderer in the actor's zone (`TimezoneMixin`), FKs as UUID strings.
pub fn render_view_row(row: &ViewRow, tz: &chrono_tz::Tz) -> Map<String, Value> {
    use crate::serializer::render_datetime_in;
    let mut out = Map::new();
    out.insert("id".to_owned(), Value::String(row.id.to_string()));
    out.insert(
        "created_at".to_owned(),
        Value::String(render_datetime_in(&row.created_at, tz)),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(render_datetime_in(&row.updated_at, tz)),
    );
    out.insert("deleted_at".to_owned(), Value::Null);
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert(
        "description".to_owned(),
        Value::String(row.description.clone()),
    );
    out.insert("query".to_owned(), row.query.clone());
    out.insert("query_dict".to_owned(), row.query_dict.clone());
    out.insert(
        "created_by".to_owned(),
        row.created_by_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    out.insert(
        "updated_by".to_owned(),
        row.updated_by_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    out.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    out
}

// ---------------------------------------------------------------------------
// Stored-query mapping — issue_filters(..., "POST"/"PATCH") at JSON level
// ---------------------------------------------------------------------------

/// Keys whose POST branch reads a neighbouring key or a default instead of
/// the value itself (`type`, `sub_issue`, `start_target_date`): a present
/// non-string behaves like the comparison failing, i.e. exactly like the
/// key being absent — so only strings reach the kernel for these keys.
const DEFAULTED_POST_KEYS: &[&str] = &["type", "sub_issue", "start_target_date"];

/// Date keys whose POST branch runs the mini-language (`post_date_param`):
/// a present object iterates its keys (`for query in queries` over a
/// `dict`), matching Python exactly.
const DATE_POST_KEYS: &[&str] = &["created_at", "updated_at", "completed_at"];

/// Bridge one `query_dict` entry to a `pidash_db::issue_filters::PostVal`.
/// Returns `Ok(None)` when Python would not see the key (falsy values), and
/// `Err` when Python would crash (`len()` of a truthy number/bool →
/// `TypeError`, a 500). Truthy objects and mixed arrays have no `PostVal`
/// spelling: Python stores them raw, which the stored-query column never
/// executes on these routes — fail closed (500) rather than store bytes
/// the kernels cannot reproduce.
#[allow(clippy::result_large_err)]
fn bridge_post_value(
    key: &str,
    value: &Value,
) -> Result<Option<pidash_db::issue_filters::PostVal>, Denial> {
    use pidash_db::issue_filters::PostVal;
    match value {
        Value::Null => Ok(None),
        // `True == "backlog"` is False: the branch default applies, which
        // is what omitting the key does — so falsy and defaulted keys
        // share the absent arm.
        Value::Bool(flag) => {
            if !flag || DEFAULTED_POST_KEYS.contains(&key) {
                Ok(None)
            } else {
                Err(Denial::ServerError)
            }
        }
        Value::Number(number) => {
            let falsy = number.as_f64().is_some_and(|float| float == 0.0);
            if falsy || DEFAULTED_POST_KEYS.contains(&key) {
                Ok(None)
            } else {
                Err(Denial::ServerError)
            }
        }
        Value::String(text) => Ok(Some(PostVal::Text(text.clone()))),
        Value::Array(items) => {
            if items.is_empty() {
                Ok(None)
            } else if items.iter().all(|item| item.is_string()) {
                Ok(Some(PostVal::List(
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect(),
                )))
            } else {
                Err(Denial::ServerError)
            }
        }
        Value::Object(fields) => {
            if fields.is_empty() {
                Ok(None)
            } else if DATE_POST_KEYS.contains(&key) {
                // `for query in queries` over a dict yields its keys.
                Ok(Some(PostVal::List(fields.keys().cloned().collect())))
            } else if DEFAULTED_POST_KEYS.contains(&key) {
                Ok(None)
            } else {
                Err(Denial::ServerError)
            }
        }
    }
}

/// `AnalyticViewSerializer.create`: `query_dict` (default `{}`) through
/// `issue_filters(..., "POST")` when truthy, else `{}` (`analytic.py:16-22`).
///
/// Non-object truthies follow Python's dynamic dispatch, not the kernel:
/// a truthy string runs `key in query_params` substring checks (a filter
/// key inside it reaches `params.get` → `AttributeError`, a 500; otherwise
/// `{}`); a truthy list runs membership checks (an element equal to a
/// filter key crashes the same way, otherwise `{}`); truthy numbers/bools
/// crash on `in` (500). Only truthy objects take the kernel path.
#[allow(clippy::result_large_err)]
pub fn stored_create_query(query_dict: Option<&Value>, today: NaiveDate) -> Result<Value, Denial> {
    use pidash_services::app_analytics::shape::{empty_query, is_truthy_json};
    let query_dict = query_dict.unwrap_or(&Value::Null);
    if !is_truthy_json(query_dict) {
        return Ok(empty_query());
    }
    match query_dict {
        Value::String(text) => {
            let crash = pidash_db::issue_filters::ISSUE_FILTER_KEYS
                .iter()
                .any(|key| text.contains(*key));
            if crash {
                return Err(Denial::ServerError);
            }
            return Ok(empty_query());
        }
        Value::Array(items) => {
            let crash = items.iter().any(|item| match item {
                Value::String(text) => pidash_db::issue_filters::ISSUE_FILTER_KEYS
                    .iter()
                    .any(|key| *key == text),
                _ => false,
            });
            if crash {
                return Err(Denial::ServerError);
            }
            return Ok(empty_query());
        }
        Value::Object(_) => {}
        _ => return Err(Denial::ServerError),
    }
    let fields = query_dict.as_object().ok_or(Denial::ServerError)?;
    let mut params = HashMap::new();
    for (key, value) in fields {
        if let Some(post) = bridge_post_value(key, value)? {
            params.insert(key.clone(), post);
        }
    }
    let compiled = pidash_db::issue_filters::issue_filters_post(&params, "", today)
        .map_err(|_| Denial::ServerError)?;
    Ok(stored_query_value(&compiled))
}

/// Convert compiled POST predicates to the stored `query` JSON object:
/// uuid lists to string arrays, strings to arrays, text to string, flags
/// to bool, days to ISO dates (Django's JSON encoder renders dates as
/// `"YYYY-MM-DD"`), `Null` to null. Predicate order is the kernel's
/// `ISSUE_FILTER_KEYS` order.
fn stored_query_value(compiled: &pidash_db::issue_filters::IssueFilter) -> Value {
    use pidash_db::issue_filters::FilterValue;
    let mut out = Map::new();
    for (name, value) in compiled.predicates() {
        let json = match value {
            FilterValue::Uuids(ids) => {
                Value::Array(ids.iter().map(|id| Value::String(id.to_string())).collect())
            }
            FilterValue::Strings(items) => Value::Array(
                items
                    .iter()
                    .map(|item| Value::String(item.clone()))
                    .collect(),
            ),
            FilterValue::Text(text) => Value::String(text.clone()),
            FilterValue::Flag(flag) => Value::Bool(*flag),
            FilterValue::Day(day) => Value::String(day.to_string()),
            FilterValue::Null => Value::Null,
        };
        out.insert(name.clone(), json);
    }
    Value::Object(out)
}

// ---------------------------------------------------------------------------
// Viewset input validation (DRF field errors)
// ---------------------------------------------------------------------------

/// Validated create/patch input. `query_dict` is the raw JSON (stored
/// verbatim); `query`/`workspace` and unknown keys are declined — DRF
/// read-only/unknown fields never reach `validated_data`.
#[derive(Debug, Clone)]
pub struct ViewInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub query_dict: Option<Value>,
}

/// Validate one `name` value: DRF `CharField(max_length=255)` — type, then
/// blank, then length in code points (Python `[:255]`-style slicing counts
/// code points, never UTF-8 bytes).
fn validate_name(value: &Value) -> Result<String, Value> {
    let text = match value {
        Value::String(text) => text,
        Value::Null => {
            return Err(field_error("This field may not be null."));
        }
        _ => return Err(field_error("Not a valid string.")),
    };
    if text.is_empty() {
        return Err(field_error("This field may not be blank."));
    }
    if text.chars().count() > 255 {
        return Err(field_error(
            "Ensure this field has no more than 255 characters.",
        ));
    }
    Ok(text.clone())
}

/// Validate one `description` value (`TextField(blank=True)`): type only —
/// `""` is allowed, missing defaults to `""`.
fn validate_description(value: &Value) -> Result<String, Value> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Null => Err(field_error("This field may not be null.")),
        _ => Err(field_error("Not a valid string.")),
    }
}

fn field_error(message: &str) -> Value {
    Value::Array(vec![Value::String(message.to_owned())])
}

/// Validate a create body (full write): `name` required, `description`
/// defaults to `""`, `query_dict` defaults to `{}` (stored verbatim, any
/// JSON but null).
#[allow(clippy::result_large_err)]
pub fn validate_create(body: &Map<String, Value>) -> Result<ViewInput, Denial> {
    let mut errors = Map::new();
    let name = match body.get("name") {
        None => {
            errors.insert("name".to_owned(), field_error("This field is required."));
            None
        }
        Some(value) => match validate_name(value) {
            Ok(name) => Some(name),
            Err(detail) => {
                errors.insert("name".to_owned(), detail);
                None
            }
        },
    };
    let description = match body.get("description") {
        None => Some(String::new()),
        Some(value) => match validate_description(value) {
            Ok(description) => Some(description),
            Err(detail) => {
                errors.insert("description".to_owned(), detail);
                None
            }
        },
    };
    let query_dict = match body.get("query_dict") {
        None => Some(empty_query_dict()),
        Some(Value::Null) => {
            errors.insert(
                "query_dict".to_owned(),
                field_error("This field may not be null."),
            );
            None
        }
        Some(value) => Some(value.clone()),
    };
    if !errors.is_empty() {
        return Err(Denial::BadFields(Value::Object(errors)));
    }
    Ok(ViewInput {
        name,
        description,
        query_dict,
    })
}

/// Validate a patch body (partial write): only present keys validate.
#[allow(clippy::result_large_err)]
pub fn validate_patch(body: &Map<String, Value>) -> Result<ViewInput, Denial> {
    let mut errors = Map::new();
    let mut input = ViewInput {
        name: None,
        description: None,
        query_dict: None,
    };
    if let Some(value) = body.get("name") {
        match validate_name(value) {
            Ok(name) => input.name = Some(name),
            Err(detail) => {
                errors.insert("name".to_owned(), detail);
            }
        }
    }
    if let Some(value) = body.get("description") {
        match validate_description(value) {
            Ok(description) => input.description = Some(description),
            Err(detail) => {
                errors.insert("description".to_owned(), detail);
            }
        }
    }
    if let Some(value) = body.get("query_dict") {
        if value.is_null() {
            errors.insert(
                "query_dict".to_owned(),
                field_error("This field may not be null."),
            );
        } else {
            input.query_dict = Some(value.clone());
        }
    }
    if !errors.is_empty() {
        return Err(Denial::BadFields(Value::Object(errors)));
    }
    Ok(input)
}

fn empty_query_dict() -> Value {
    Value::Object(Map::new())
}

// ---------------------------------------------------------------------------
// Viewset handlers
// ---------------------------------------------------------------------------

fn parse_pk(raw: &str) -> Result<Uuid, Denial> {
    // `get_object()` filters `pk=<raw>`: garbage is a `ValidationError`.
    Uuid::parse_str(raw).map_err(|_| Denial::BadValidation)
}

/// Map a body-parse failure to DRF's `ParseError` shape. Blank input is
/// byte-exact (`Expecting value: line 1 column N+1 (char N)`); anything
/// else carries the parser reason after the same prefix.
fn json_parse_denial(raw: &[u8], error: &serde_json::Error) -> Denial {
    if let Ok(text) = std::str::from_utf8(raw) {
        if text.trim().is_empty() {
            let len = text.len();
            return Denial::BadJson(format!(
                "{JSON_PARSE_PREFIX}Expecting value: line 1 column {} (char {})",
                len + 1,
                len
            ));
        }
    }
    Denial::BadJson(format!("{JSON_PARSE_PREFIX}{error}"))
}

fn parse_body(raw: &[u8]) -> Result<Map<String, Value>, Denial> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|error| json_parse_denial(raw, &error))?;
    value.as_object().cloned().ok_or_else(|| {
        // DRF interpolates `type(data).__name__`.
        let kind = match &value {
            Value::Null => "NoneType",
            Value::Bool(_) => "bool",
            Value::Number(number) if number.is_i64() || number.is_u64() => "int",
            Value::Number(_) => "float",
            Value::String(_) => "str",
            Value::Array(_) => "list",
            Value::Object(_) => "dict",
        };
        Denial::BadFields(serde_json::json!({
            "non_field_errors": [format!("Invalid data. Expected a dictionary, but got {kind}.")]
        }))
    })
}

/// `GET analytic-view/`: session auth, the viewset gate, then the
/// workspace-scoped `-created_at` list as a bare JSON array (no pagination
/// class on this viewset).
async fn view_list(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
) -> Response {
    let actor = match gated_actor(&state, extension, "GET", ANALYTIC_VIEW_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    match fetch_view_list(pool, &slug).await {
        Ok(rows) => {
            let items: Vec<Value> = rows
                .iter()
                .map(|row| Value::Object(render_view_row(row, &actor.timezone)))
                .collect();
            json_response(
                StatusCode::OK,
                serde_json::to_string(&Value::Array(items)).expect("view list"),
            )
        }
        Err(denial) => denial.into_response(),
    }
}

/// `POST analytic-view/`: validate, `perform_create` (workspace lookup,
/// then `serializer.save(workspace_id=...)`), 201 + representation.
/// `BaseModel.save` stamps `created_by` (adding) and leaves `updated_by`
/// null.
async fn view_create(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let actor = match gated_actor(&state, extension, "POST", ANALYTIC_VIEW_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let fields = match parse_body(&body) {
        Ok(fields) => fields,
        Err(denial) => return denial.into_response(),
    };
    let input = match validate_create(&fields) {
        Ok(input) => input,
        Err(denial) => return denial.into_response(),
    };
    match create_view(pool, &actor, &slug, &input).await {
        Ok(row) => json_response(
            StatusCode::CREATED,
            serde_json::to_string(&Value::Object(render_view_row(&row, &actor.timezone)))
                .expect("view create"),
        ),
        Err(denial) => denial.into_response(),
    }
}

#[allow(clippy::result_large_err)]
async fn create_view(
    pool: &sqlx::PgPool,
    actor: &crate::license::Actor,
    slug: &str,
    input: &ViewInput,
) -> Result<ViewRow, Denial> {
    // `perform_create`: `Workspace.objects.get(slug=slug)` — miss is the
    // `ObjectDoesNotExist` 404. (Unreachable when the gate passes, since
    // the gate needs a membership row on the same workspace; kept for
    // parity.)
    let workspace: Option<(Uuid,)> =
        sqlx::query_as("SELECT \"id\" FROM \"workspaces\" WHERE \"slug\" = $1 LIMIT 1")
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace else {
        return Err(Denial::ObjectNotFound);
    };
    let today = chrono::Utc::now().date_naive();
    let query = stored_create_query(input.query_dict.as_ref(), today)?;
    let query_dict = input.query_dict.clone().unwrap_or_else(empty_query_dict);
    let now = chrono::Utc::now();
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO \"analytic_views\" (\"id\", \"created_at\", \"updated_at\", \
         \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"workspace_id\", \
         \"name\", \"description\", \"query\", \"query_dict\") \
         VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(actor.id)
    .bind(workspace_id)
    .bind(input.name.clone().unwrap_or_default())
    .bind(input.description.clone().unwrap_or_default())
    .bind(&query)
    .bind(&query_dict)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(ViewRow {
        id,
        created_at: now,
        updated_at: now,
        created_by_id: Some(actor.id),
        updated_by_id: None,
        workspace_id,
        name: input.name.clone().unwrap_or_default(),
        description: input.description.clone().unwrap_or_default(),
        query,
        query_dict,
    })
}

/// `GET analytic-view/<pk>/`: the scoped row or the `get_object()` 404.
async fn view_retrieve(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
) -> Response {
    let actor = match gated_actor(&state, extension, "GET", ANALYTIC_VIEW_DETAIL_PATH, &slug).await
    {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let id = match parse_pk(&pk) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    match fetch_view_detail(pool, &slug, &id).await {
        Ok(row) => json_response(
            StatusCode::OK,
            serde_json::to_string(&Value::Object(render_view_row(&row, &actor.timezone)))
                .expect("view detail"),
        ),
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH analytic-view/<pk>/`: partial validation, then `update`. The
/// stored `query` is always the PATCH mapping of the (ignored) `query_data`
/// key — i.e. always `{}` (B2); `BaseModel.save` stamps `updated_by`.
async fn view_partial_update(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    let actor =
        match gated_actor(&state, extension, "PATCH", ANALYTIC_VIEW_DETAIL_PATH, &slug).await {
            Ok(actor) => actor,
            Err(denied) => return denied,
        };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let id = match parse_pk(&pk) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let fields = match parse_body(&body) {
        Ok(fields) => fields,
        Err(denial) => return denial.into_response(),
    };
    let input = match validate_patch(&fields) {
        Ok(input) => input,
        Err(denial) => return denial.into_response(),
    };
    match patch_view(pool, &actor, &slug, &id, &input).await {
        Ok(row) => json_response(
            StatusCode::OK,
            serde_json::to_string(&Value::Object(render_view_row(&row, &actor.timezone)))
                .expect("view patch"),
        ),
        Err(denial) => denial.into_response(),
    }
}

#[allow(clippy::result_large_err)]
async fn patch_view(
    pool: &sqlx::PgPool,
    actor: &crate::license::Actor,
    slug: &str,
    id: &Uuid,
    input: &ViewInput,
) -> Result<ViewRow, Denial> {
    let mut row = fetch_view_detail(pool, slug, id).await?;
    if let Some(name) = &input.name {
        row.name = name.clone();
    }
    if let Some(description) = &input.description {
        row.description = description.clone();
    }
    if let Some(query_dict) = &input.query_dict {
        row.query_dict = query_dict.clone();
    }
    // B2: `validated_data["query"] = issue_filters(query_params, "PATCH")`
    // runs unconditionally over the ignored `query_data` key (always absent
    // → `{}`), overwriting whatever the POST line computed.
    row.query = Value::Object(Map::new());
    row.updated_by_id = Some(actor.id);
    row.updated_at = chrono::Utc::now();
    sqlx::query(
        "UPDATE \"analytic_views\" SET \"name\" = $1, \"description\" = $2, \
         \"query\" = $3, \"query_dict\" = $4, \"updated_by_id\" = $5, \
         \"updated_at\" = $6 WHERE \"id\" = $7",
    )
    .bind(&row.name)
    .bind(&row.description)
    .bind(&row.query)
    .bind(&row.query_dict)
    .bind(actor.id)
    .bind(row.updated_at)
    .bind(row.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row)
}

/// `DELETE analytic-view/<pk>/`: `get_object()` then the soft `delete()`
/// (sets `deleted_at`); 204 with an empty body.
async fn view_destroy(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
) -> Response {
    let actor = match gated_actor(
        &state,
        extension,
        "DELETE",
        ANALYTIC_VIEW_DETAIL_PATH,
        &slug,
    )
    .await
    {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let id = match parse_pk(&pk) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    match destroy_view(pool, &actor, &slug, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(denial) => denial.into_response(),
    }
}

#[allow(clippy::result_large_err)]
async fn destroy_view(
    pool: &sqlx::PgPool,
    actor: &crate::license::Actor,
    slug: &str,
    id: &Uuid,
) -> Result<(), Denial> {
    // The lookup first: a miss is the `get_object()` 404, not a silent 204.
    fetch_view_detail(pool, slug, id).await?;
    // `SoftDeleteModel.delete` stamps `deleted_at` then `save()`s, so the
    // `auto_now` `updated_at` moves too — and `BaseModel.save` on the
    // updating instance stamps `updated_by` (`db/models/base.py:40-42`).
    sqlx::query(
        "UPDATE \"analytic_views\" SET \"deleted_at\" = $1, \"updated_at\" = $1, \
         \"updated_by_id\" = $2 WHERE \"id\" = $3",
    )
    .bind(chrono::Utc::now())
    .bind(actor.id)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 30).expect("date")
    }

    fn query_map(pairs: &[(&str, &str)]) -> QueryMap {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OneOrMany::One((*value).to_owned())))
            .collect()
    }

    #[test]
    fn gate_rows_cover_routes_1_to_3() {
        // FX-A-G-01: the analytics GET is a WORKSPACE ADMIN_MEMBER gate;
        // every viewset method is the ViewsetAdmin class gate with the
        // DRF-default denial body.
        let get = gate_for("GET", ANALYTICS_PATH).expect("analytics gate");
        assert_eq!(
            deny_body(&get.gate),
            FORBIDDEN_BODY,
            "decorator denial, not the viewset one"
        );
        for (method, path) in [
            ("GET", ANALYTIC_VIEW_PATH),
            ("POST", ANALYTIC_VIEW_PATH),
            ("GET", ANALYTIC_VIEW_DETAIL_PATH),
            ("PATCH", ANALYTIC_VIEW_DETAIL_PATH),
            ("DELETE", ANALYTIC_VIEW_DETAIL_PATH),
        ] {
            let row = gate_for(method, path).expect("viewset gate");
            assert_eq!(
                deny_body(&row.gate),
                VIEWSET_FORBIDDEN_BODY,
                "{method} {path}"
            );
        }
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            Denial::Unauthorized.status_and_body().1,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            Denial::Forbidden(FORBIDDEN_BODY).status_and_body().1,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(
            Denial::Forbidden(VIEWSET_FORBIDDEN_BODY)
                .status_and_body()
                .1,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(
            Denial::BadAxes.status_and_body().1,
            r#"{"error":"x-axis and y-axis dimensions are required and the values should be valid"}"#
        );
        assert_eq!(
            Denial::BadSegment.status_and_body().1,
            r#"{"error":"Both segment and x axis cannot be same and segment should be valid"}"#
        );
        assert_eq!(
            Denial::ViewNotFound.status_and_body().1,
            r#"{"detail":"No AnalyticView matches the given query."}"#
        );
        assert_eq!(
            Denial::ObjectNotFound.status_and_body().1,
            r#"{"error":"The required object does not exist."}"#
        );
    }

    #[test]
    fn query_last_wins_and_flat_params() {
        let mut query: QueryMap = HashMap::new();
        query.insert(
            "x_axis".to_owned(),
            OneOrMany::Many(vec!["nope".to_owned(), "priority".to_owned()]),
        );
        assert_eq!(query_last(&query, "x_axis").as_deref(), Some("priority"));
        assert_eq!(query_last(&query, "missing"), None);
    }

    #[test]
    fn stored_create_query_contract_pin() {
        // FX-A-H-01: `{"priority": "high"}` stores `{"priority__in": "high"}`.
        let query_dict = json!({"priority": "high"});
        let query = stored_create_query(Some(&query_dict), today()).expect("query");
        assert_eq!(query, json!({"priority__in": "high"}));
    }

    #[test]
    fn stored_create_query_empty_and_missing() {
        assert_eq!(
            stored_create_query(Some(&json!({})), today()).expect("empty"),
            json!({})
        );
        assert_eq!(
            stored_create_query(None, today()).expect("missing"),
            json!({})
        );
        assert_eq!(
            stored_create_query(Some(&json!({"priority": ""})), today()).expect("blank"),
            json!({})
        );
        assert_eq!(
            stored_create_query(Some(&json!({"priority": null})), today()).expect("null"),
            json!({})
        );
    }

    #[test]
    fn stored_create_query_dynamic_dispatch() {
        // A filter key inside a truthy string reaches `.get` → 500.
        assert!(stored_create_query(Some(&json!("priority-high")), today()).is_err());
        // Plain strings with no filter key inside store nothing.
        assert_eq!(
            stored_create_query(Some(&json!("hello")), today()).expect("plain"),
            json!({})
        );
        // A list element equal to a filter key crashes the branch → 500.
        assert!(stored_create_query(Some(&json!(["priority"])), today()).is_err());
        // Lists with no filter key store nothing.
        assert_eq!(
            stored_create_query(Some(&json!(["a", "b"])), today()).expect("list"),
            json!({})
        );
        // Truthy numbers/bools crash on `in` → 500.
        assert!(stored_create_query(Some(&json!(5)), today()).is_err());
        assert!(stored_create_query(Some(&json!(true)), today()).is_err());
    }

    #[test]
    fn validate_create_contract_pin() {
        // The contract's create body validates with description defaulted.
        let body = json!({"name": "AV-CRUD", "description": "roundtrip", "query_dict": {"priority": "high"}});
        let input = validate_create(body.as_object().expect("object")).expect("valid");
        assert_eq!(input.name.as_deref(), Some("AV-CRUD"));
        assert_eq!(input.description.as_deref(), Some("roundtrip"));
        assert_eq!(
            input.query_dict.as_ref(),
            Some(&json!({"priority": "high"}))
        );
        // Missing description defaults to "".
        let body = json!({"name": "AV"});
        let input = validate_create(body.as_object().expect("object")).expect("valid");
        assert_eq!(input.description.as_deref(), Some(""));
        assert_eq!(input.query_dict.as_ref(), Some(&json!({})));
    }

    #[test]
    fn validate_create_field_errors() {
        let body = json!({});
        let denial = validate_create(body.as_object().expect("object")).expect_err("required");
        match denial {
            Denial::BadFields(errors) => {
                assert_eq!(errors, json!({"name": ["This field is required."]}));
            }
            _ => panic!("wrong denial"),
        }
        let body = json!({"name": ""});
        let denial = validate_create(body.as_object().expect("object")).expect_err("blank");
        match denial {
            Denial::BadFields(errors) => {
                assert_eq!(errors, json!({"name": ["This field may not be blank."]}));
            }
            _ => panic!("wrong denial"),
        }
        let long = "n".repeat(256);
        let body = json!({"name": long});
        let denial = validate_create(body.as_object().expect("object")).expect_err("long");
        match denial {
            Denial::BadFields(errors) => {
                assert_eq!(
                    errors,
                    json!({"name": ["Ensure this field has no more than 255 characters."]})
                );
            }
            _ => panic!("wrong denial"),
        }
        // Code points, not bytes: 255 emoji are fine, 256 are not.
        let boundary = "é".repeat(255);
        let body = json!({"name": boundary});
        assert!(validate_create(body.as_object().expect("object")).is_ok());
    }

    #[test]
    fn validate_patch_allows_empty_and_rejects_bad_name() {
        let body = json!({});
        let input = validate_patch(body.as_object().expect("object")).expect("empty patch");
        assert!(input.name.is_none());
        let body = json!({"name": "AV-CRUD2"});
        let input = validate_patch(body.as_object().expect("object")).expect("rename");
        assert_eq!(input.name.as_deref(), Some("AV-CRUD2"));
        // The ignored `query_data` key never validates: only `query_dict`
        // would, and it is absent here.
        assert!(input.query_dict.is_none());
    }

    #[test]
    fn render_view_row_key_order() {
        let row = ViewRow {
            id: Uuid::nil(),
            created_at: chrono::DateTime::from_naive_utc_and_offset(
                NaiveDate::from_ymd_opt(2026, 9, 30)
                    .expect("date")
                    .and_hms_opt(6, 0, 0)
                    .expect("time"),
                chrono::Utc,
            ),
            updated_at: chrono::DateTime::from_naive_utc_and_offset(
                NaiveDate::from_ymd_opt(2026, 9, 30)
                    .expect("date")
                    .and_hms_opt(6, 0, 0)
                    .expect("time"),
                chrono::Utc,
            ),
            created_by_id: None,
            updated_by_id: None,
            workspace_id: Uuid::nil(),
            name: "AV".to_owned(),
            description: String::new(),
            query: json!({}),
            query_dict: json!({}),
        };
        let rendered = render_view_row(&row, &chrono_tz::UTC);
        let keys: Vec<&str> = rendered.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "description",
                "query",
                "query_dict",
                "created_by",
                "updated_by",
                "workspace",
            ]
        );
    }

    #[test]
    fn scope_and_filters_empty_params_is_manager_scope() {
        let (fragment, binds) = scope_and_filters(&query_map(&[]), true).expect("scope");
        assert!(fragment.contains(queries::ISSUE_OBJECTS_SCOPE));
        assert!(binds.values().is_empty());
        let (fragment, _) = scope_and_filters(&query_map(&[]), false).expect("plain");
        assert!(fragment.contains("\"issues\".\"deleted_at\" IS NULL"));
        assert!(!fragment.contains("triage"));
    }

    #[test]
    fn filter_predicates_compile_with_slug_first_binds() {
        use pidash_db::issue_filters::FilterValue;
        let mut binds = Binds::default();
        let sql = filter_sql(
            &mut binds,
            "priority__in",
            &FilterValue::Strings(vec!["high".to_owned()]),
        )
        .expect("priority");
        // `$1` is the workspace slug; the first filter bind is `$2`.
        assert_eq!(sql, "\"issues\".\"priority\" IN ($2)");
        let sql = filter_sql(
            &mut binds,
            "state__in",
            &FilterValue::Uuids(vec![Uuid::nil()]),
        )
        .expect("state");
        assert_eq!(sql, "\"issues\".\"state_id\" IN ($3)");
    }

    #[test]
    fn scope_joins_add_states_leg_once() {
        // The manager scope needs the states leg; a statement that already
        // joined states (state details) must not get a second one.
        let joins = scope_joins(
            "SELECT ... FROM \"issues\"",
            "(\"states\".\"group\" != 'triage')",
            true,
        );
        assert!(joins.contains("LEFT OUTER JOIN \"states\""));
        let state_sql = queries::base_state_details_sql("(\"states\".\"group\" != 'triage')");
        let joins = scope_joins(&state_sql, "(\"states\".\"group\" != 'triage')", true);
        assert!(!joins.contains("\"states\""));
        // The plain (label) scope adds no states leg without a reference.
        let joins = scope_joins("SELECT ...", "(\"issues\".\"deleted_at\" IS NULL)", false);
        assert!(joins.is_empty());
    }

    #[test]
    fn scoped_sql_splices_before_scope_where() {
        let sql = queries::base_count_sql("(\"issues\".\"priority\" IN ($2))");
        let scoped = scoped_sql(
            &sql,
            " INNER JOIN \"issue_labels\" \"label_issue\" ON (\"issues\".\"id\" = \"label_issue\".\"issue_id\")",
        );
        let join_at = scoped.find("INNER JOIN \"issue_labels\"").expect("join");
        let where_at = scoped.find(" WHERE (\"workspaces\".").expect("where");
        assert!(join_at < where_at);
    }

    #[test]
    fn plot_wrapper_casts_dimensions_to_text() {
        let wrapped = wrapped_plot_sql("SELECT 1", "count", None);
        assert!(wrapped.contains("\"dimension\"::text"));
        assert!(!wrapped.contains("segment"));
        let wrapped = wrapped_plot_sql("SELECT 1", "estimate", Some("state__group"));
        assert!(wrapped.contains("\"segment\"::text AS \"segment\""));
        assert!(wrapped.ends_with("ORDER BY 1, 2"));
    }

    #[test]
    fn bad_pk_is_a_validation_400() {
        let denial = parse_pk("nope").expect_err("bad uuid");
        assert!(matches!(denial, Denial::BadValidation));
    }

    #[test]
    fn malformed_json_carries_drf_parse_error_shape() {
        // Empty input is byte-exact with DRF (`parsers.py` raises
        // `ParseError('JSON parse error - %s')` around the CPython reason).
        let denial = parse_body(b"").expect_err("empty");
        match denial {
            Denial::BadJson(detail) => {
                let (_, body) = Denial::BadJson(detail).status_and_body();
                assert_eq!(
                    body,
                    r#"{"detail":"JSON parse error - Expecting value: line 1 column 1 (char 0)"}"#
                );
            }
            _ => panic!("wrong denial"),
        }
        // Blank input shifts the reported position.
        let denial = parse_body(b"  ").expect_err("blank");
        match denial {
            Denial::BadJson(detail) => {
                assert!(detail
                    .starts_with("JSON parse error - Expecting value: line 1 column 3 (char 2)"))
            }
            _ => panic!("wrong denial"),
        }
        // Other malformed input keeps the prefix with the parser reason.
        let denial = parse_body(b"{oops").expect_err("garbage");
        match denial {
            Denial::BadJson(detail) => {
                assert!(detail.starts_with("JSON parse error - "));
                let (_, body) = Denial::BadJson(detail).status_and_body();
                assert!(body.starts_with(r#"{"detail":"JSON parse error - "#));
            }
            _ => panic!("wrong denial"),
        }
    }

    #[test]
    fn filtered_count_sql_composes_end_to_end() {
        // `?priority=high`: kernel → predicate → binds → joins → splice.
        let (fragment, binds) =
            scope_and_filters(&query_map(&[("priority", "high")]), true).expect("scope");
        assert!(fragment.contains("\"issues\".\"priority\" IN ($2)"));
        assert_eq!(binds.values().len(), 1);
        let sql = queries::base_count_sql(&fragment);
        let joins = scope_joins(&sql, &fragment, true);
        // No relation predicate: only the manager states leg.
        assert!(joins.contains("LEFT OUTER JOIN \"states\""));
        assert!(!joins.contains("issue_labels"));
        let scoped = scoped_sql(&sql, &joins);
        assert!(scoped.contains("$2"));
        let join_at = scoped.find("LEFT OUTER JOIN \"states\"").expect("join");
        let where_at = scoped.find(" WHERE (\"workspaces\".").expect("where");
        assert!(join_at < where_at);
    }

    #[test]
    fn m2m_filter_uses_inner_join() {
        // `?labels=<uuid>`: the value predicate joins INNER (Django's
        // `filter()` join), and the bind is the parsed UUID at `$2`.
        let id = Uuid::nil();
        let (fragment, binds) =
            scope_and_filters(&query_map(&[("labels", &id.to_string())]), true).expect("scope");
        assert!(fragment.contains("\"label_issue\".\"label_id\" IN ($2)"));
        let sql = queries::base_count_sql(&fragment);
        let joins = scope_joins(&sql, &fragment, true);
        assert!(joins.contains("INNER JOIN \"issue_labels\" \"label_issue\""));
        assert_eq!(binds.values().len(), 1);
    }
}
