//! Workspace favorites + draft-issue handlers (D-24, stage 5, PIDASHCONV-622).
//!
//! Ports two handler units (`apps/api/pi_dash/app/views/workspace/`):
//!
//! - `WorkspaceFavoriteEndpoint` get/post/patch/delete + `WorkspaceFavoriteGroupEndpoint`
//!   get (`favorite.py:20-97`, W32-W34): parent-null + membership/page branch, entity
//!   dedupe (200 existing) else create (200), `IntegrityError` → 400
//!   `{"error": "Favorite already exists"}`, hard-delete 204. ADMIN+MEMBER gates.
//! - `WorkspaceDraftIssueViewSet` (`draft.py:46-312`, W35-W37): gzip list (own rows +
//!   legacy `issue_filters`, paginated), create (context `project_id`, 21-key re-read,
//!   201), patch (ADMIN+MEMBER + creator, own-or-404, 204), retrieve (ADMIN + creator,
//!   detail shape), destroy (ADMIN + creator, 204), draft-to-issue (project-required 400,
//!   `IssueCreateSerializer` save + 3 `issue_activity` sites + CycleIssue/ModuleIssue/
//!   FileAsset writes + draft delete, 201).
//!
//! Fixture ids: F-W24-15 (these routes) + consumed F-W24-05/12/13/14 (stayed green).
//!
//! Shapes come from the merged ports: favorite rows from
//! `services::app_workspace::ser_account_token` (SER-E, PIDASHCONV-604), SQL builders
//! from `services::app_workspace::queries_extras` R6/R7 (QRY-D, PIDASHCONV-611), gates
//! from [`super::gates`] (PIDASHCONV-613), activity emits from
//! `services::app_workspace::tasks` (PIDASHCONV-614), and the issue-create validate
//! kernel + write specs from `services::app_issues::serializers_create` (D-26,
//! PIDASHCONV-638). The `app/serializers/draft.py` triplet
//! (`DraftIssueCreate`/`Draft`/`DetailSerializer`) has no D-26 owner (verified: all 23
//! D-26 children swept, none names it), so its field validation and save halves are
//! translated here — this module is its only consumer.
//!
//! Layering: SeaORM (sea-query) builders project the same predicates the
//! services `:named`-placeholder fragments name; single-table reads use the
//! entity API, joins/subqueries the `list_engine::query_built` runner.
//! Endpoint-specific SQL lives here only where it names this endpoint's
//! projection; merged predicates are rebuilt verbatim, never re-ported. DRF field validation (`is_valid()` before `validate()`)
//! is translated here per serializer (verified field-by-field against the pinned
//! Django 4.2.30 + DRF 3.15.2); the issue-create object leg reuses the merged
//! [`issue_create_validate`](pidash_services::app_issues::serializers_create::issue_create_validate)
//! kernel with prefetched probes (the unmerged D-26 handlers-A branch
//! `origin/pi-dash/pidashconv-651` served as a cross-check, never a source).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * Favorites GET branch reads `(project-null AND NOT page) OR (project-member)`
//!   because `&` binds tighter than `|` — project-linked page favorites PASS; the group
//!   GET has NO page exclusion at all (`favorite.py:27-32`, `:89-94`).
//! * Favorites POST dedupe filters workspace + user + entity keys only — NO project and
//!   NO parent filter (`favorite.py:44-49`); a truthy-but-invalid `entity_identifier`
//!   400s `{"error": "Please provide valid detail"}` from the dedupe query itself,
//!   before serializer validation runs.
//! * Favorites POST answers 200 (not 201) on create; `project_id` rides the raw input
//!   (unvalidated) into `save()`, where `WorkspaceBaseModel.save` re-points `workspace`
//!   at the project's workspace; a missing project row 404s (not the FK 400).
//! * Draft list `issue_filters` run over the DraftIssue queryset, so `labels`,
//!   `assignees`, `cycle`, `module`, `mentions`, `subscriber`, `intake_status`,
//!   `inbox_status` and `logged_by` params raise `FieldError` → the generic 500; the
//!   `updated_at` param filters `created_at` (`issue_filters.py:236`).
//! * The draft cycle subquery is `[:1]` with NO `ORDER BY` (nondeterministic pick);
//!   the assignee `ArrayAgg` guard accepts ANY active project membership of the
//!   assignee (no project scoping) (`draft.py:55-60`, `:76`).
//! * Draft create reads `cycle_id`/`module_ids` from the RAW input (unvalidated):
//!   present-but-malformed values 400 (`ValidationError`, null items NULL-insert
//!   → `IntegrityError`); the
//!   `project` input key is validated and then silently overwritten by the context
//!   `project_id` (`DraftIssue(project=…, project_id=…)` keeps the latter).
//! * Draft patch/retrieve creator gates look the draft pk up in the `Issue` table
//!   (`model=Issue`), so the creator bypass ~never fires and the role check decides
//!   (`draft.py:159`, `:186`; ported as-is per [`gates::CreatorModel::Issue`]).
//! * Draft-to-issue on a missing draft 500s (`AttributeError` on `None`, no 404);
//!   the cycle activity's `project_id` is the string `"None"` (the route carries no
//!   `project_id` kwarg); the module activity fires once per `module_ids` entry.
//! * `description_binary` input is silently dropped everywhere (read-only `ModelField`);
//!   unknown input keys are silently ignored (DRF).
//!
//! Explicit `sort_order`/`sequence_id` on draft-to-issue are overwritten
//! (sibling max + 10000 / project max + 1); patch refreshes `completed_at`
//! from the state group even for name-only writes.
//!
//! # Deliberate non-executions (byte-identical JSON either way)
//!
//! * The `select_related`/`prefetch_related` sets (`draft.py:52-53`) are not executed:
//!   every rendered key is a local column or one of the four ported annotations.
//! * `cache_response` performs no cache I/O (same rationale as `handlers_lists`: the
//!   contract configs run Django with `DEBUG=True`, where the decorator never stores).
//!
//! # Auth edge (pilot precedent)
//!
//! Like every merged handler family, session auth checks only that the session carries
//! a UUID user id; a session for a deleted/inactive user 403s at the membership gate
//! instead of Django's 401. Unpinned by fixtures and untested by the committed suites.
//!
//! # Orchestration fire (draft-to-issue)
//!
//! The draft-to-issue save rides `domain::issues::create` (ORM-03):
//! the `Issue.save()` hooks resolve the pod/state defaults,
//! `completed_at`, stripped text, sequence and sort order, and the
//! post-commit chain runs the orchestration fire, the m2m legs and the
//! created activity in handler order. The `github_signals`
//! completion-comment leg fires only on updates (`created → return`),
//! never on this path.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_db::app_pages::strip::sync_description_stripped;
use pidash_db::entity::{
    assistant_user_llm_config, cycle_issues, cycles, draft_issue_assignees, draft_issue_cycles,
    draft_issue_labels, draft_issue_modules, draft_issues, estimate_points, file_assets,
    issue_assignees, issue_labels, issue_types, issue_views, issues, labels, module_issues,
    modules, pages, pod, project_members, project_pages, projects, states, user_favorites, users,
    workspaces,
};
use pidash_db::orm_tx::{is_integrity_violation as sea_is_integrity, TxConn};
use pidash_jobs::queue::{self, NewJob};
use pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK;
use pidash_jobs::TaskMessage;
use pidash_services::app_issues::serializers_create as create_q;
use pidash_services::app_issues::serializers_create::{
    issue_create_to_representation, issue_create_validate, CreateValidateError, ExecutorPolicy,
    IssueCreateRow, LockedIssueAttrs, PodRef, ResolvedProject, ValidateAttrs, ValidateContext,
    ValidateProbes, ValidatedAttrs,
};
use pidash_services::app_issues::shape::envelope;
use pidash_services::app_workspace::ser_account_token as fav;
use pidash_services::app_workspace::tasks as wtask;
use pidash_services::auth_session::shapes::{base_host, HostSettings};
use pidash_services::dispatch::admission::{
    self as admission_q, LlmProfile, ManagedRunnerProbe, HEARTBEAT_GRACE_SECS,
};
use pidash_services::dispatch::policy::{
    cloud_agent_is_configured, managed_runner_is_enabled, UserFlags,
};
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::issues::IssueFieldSet as IssueSaveRow;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{self, WriteActor, WriteCtx, WriteScope};
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use pidash_types::v1_assets::sticky as sticky_kernel;
use pidash_types::WorkspaceId;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::extension::postgres::{PgExpr, PgFunc};
use sea_orm::sea_query::{
    Alias, BinOper, Condition, Expr, ExprTrait, Func, IntoIden, JoinType, Order, Query,
    SelectStatement, SimpleExpr, SubQueryStatement,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr,
    EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect, RelationTrait, RuntimeErr,
    Set,
};

use super::gates;
use crate::app_issues::Denial;
use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::middleware::gzip::{accepts_gzip, compress_if_shorter, MIN_COMPRESS_BYTES};
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_request_data_spans, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};
use pidash_compat::datetime::{
    parse_digits, parse_iso_date_part, parse_iso_datetime, split_tz_suffix,
};
use pidash_compat::response::{json_response, no_content as empty_response, SERVER_ERROR_BODY};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// W32 (`app/urls/workspace.py:213-217`).
pub const FAVORITES_PATH: &str = "/api/workspaces/{slug}/user-favorites/";
/// W33 (`app/urls/workspace.py:218-222`).
pub const FAVORITE_DETAIL_PATH: &str = "/api/workspaces/{slug}/user-favorites/{favorite_id}/";
/// W34 (`app/urls/workspace.py:223-227`).
pub const FAVORITE_GROUP_PATH: &str = "/api/workspaces/{slug}/user-favorites/{favorite_id}/group/";
/// W35 (`app/urls/workspace.py:228-232`).
pub const DRAFTS_PATH: &str = "/api/workspaces/{slug}/draft-issues/";
/// W36 (`app/urls/workspace.py:233-237`).
pub const DRAFT_DETAIL_PATH: &str = "/api/workspaces/{slug}/draft-issues/{pk}/";
/// W37 (`app/urls/workspace.py:238-242`).
pub const DRAFT_TO_ISSUE_PATH: &str = "/api/workspaces/{slug}/draft-to-issue/{draft_id}/";

/// Owned methods run here; every other method falls through to Django (which answers
/// 405 itself, or the post-gate `TypeError` 500 on the favorite cross-dispatches).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            FAVORITES_PATH,
            axum::routing::get(favorite_list)
                .post(favorite_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            FAVORITE_DETAIL_PATH,
            axum::routing::patch(favorite_patch)
                .delete(favorite_delete)
                .get(crate::edge::proxy)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            FAVORITE_GROUP_PATH,
            axum::routing::get(favorite_group)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            DRAFTS_PATH,
            axum::routing::get(draft_list)
                .post(draft_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            DRAFT_DETAIL_PATH,
            axum::routing::get(draft_retrieve)
                .patch(draft_patch)
                .delete(draft_destroy)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            DRAFT_TO_ISSUE_PATH,
            axum::routing::post(draft_to_issue)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Bodies (byte-exact)
// ---------------------------------------------------------------------------

/// `handle_exception`'s `ObjectDoesNotExist` branch (`app/views/base.py:132-136`).
const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// Resolver 404 for a `<uuid:…>` segment that misses the converter (global `handler404`
/// → `custom_404_view`): `JsonResponse` bytes, i.e. `json.dumps` spacing.
const PAGE_NOT_FOUND_BODY: &str = r#"{"error": "Page not found."}"#;
/// `handle_exception`'s `IntegrityError` branch (`app/views/base.py:120-124`).
const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s `ValidationError` branch (`app/views/base.py:126-130`).
const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s `KeyError` branch (`app/views/base.py:137-141`).
const KEY_ERROR_BODY: &str = r#"{"error":"The required key does not exist."}"#;
/// `DraftIssueCreateSerializer.validate` project-membership messages
/// (`app/serializers/draft.py:119,129,138`), byte-exact.
const DRAFT_STATE_INVALID: &str = "State is not valid please pass a valid state_id";
const DRAFT_PARENT_INVALID: &str = "Parent is not valid issue_id please pass a valid issue_id";
const DRAFT_ESTIMATE_INVALID: &str =
    "Estimate point is not valid please pass a valid estimate_point_id";
/// Draft patch own-or-404 (`draft.py:165-166`).
const DRAFT_PATCH_404_BODY: &str = r#"{"error":"Issue not found"}"#;
/// Draft-to-issue project guard (`draft.py:209-213`).
const PROJECT_REQUIRED_BODY: &str = r#"{"error":"Project is required to create an issue."}"#;
/// Favorite POST `IntegrityError` arm (`favorite.py:66-67`).
const FAVORITE_EXISTS_BODY: &str = r#"{"error":"Favorite already exists"}"#;

// ---------------------------------------------------------------------------
// Shared request plumbing (handlers_lists / handlers_members precedent)
// ---------------------------------------------------------------------------

type HandlerResult = Result<Response, Denial>;

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn json_bad_request(body: &str) -> Response {
    json_response(StatusCode::BAD_REQUEST, body.to_owned())
}

/// `request.user` from the Django session (`app_issues` actor rule):
/// missing session, missing key, or a non-UUID id is anonymous → 401.
fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    session
        .get("_auth_user_id")
        .and_then(|value| value.as_str())
        .and_then(|raw| raw.parse::<Uuid>().ok())
        .ok_or(Denial::Unauthorized)
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// A SeaORM connection over the request pool (no I/O, `list_engine`
/// precedent).
fn orm(pool: &sqlx::PgPool) -> DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

fn a(name: &'static str) -> Alias {
    Alias::new(name)
}

fn col<C: IntoIden + 'static>(alias: &'static str, column: C) -> Expr {
    Expr::col((a(alias), column))
}

fn sub(stmt: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(SubQueryStatement::SelectStatement(stmt)))
}

async fn query_built<T: FromQueryResult>(
    db: &DatabaseConnection,
    stmt: &SelectStatement,
) -> Result<Vec<T>, Denial> {
    let rows = db
        .query_all(DbBackend::Postgres.build(stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter()
        .map(|row| T::from_query_result(row, "").map_err(|_| Denial::ServerError))
        .collect()
}

/// `(SELECT id FROM workspaces WHERE slug = $slug)` — the shared
/// `workspace__slug=:slug` subquery (`queries_membership::
/// workspace_id_by_slug_sql`). A NULL (unknown slug) matches no row,
/// exactly as before. No `deleted_at` guard: cross-FK traversal does
/// not apply the related manager.
fn workspace_id_sub(slug: &str) -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.column(workspaces::Column::Id)
        .from(workspaces::Entity)
        .and_where(workspaces::Column::Slug.eq(slug.to_owned()));
    sub(stmt)
}

/// The actor's `user_timezone` (`users.user_timezone`), parsed for DRF
/// serializer rendering (`TimezoneMixin` activates the actor zone, so every
/// datetime renders shifted into it). Runs after the membership gate, so
/// memberless callers 403 before this is reached.
async fn actor_timezone(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<Tz, Denial> {
    let db = orm(pool);
    let name = crate::app_issues::access::user_timezone(&db, *user_id)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// Workspace role facts for one `(user, slug)` over the same rows the
/// permission classes read (`WorkspaceMember.objects`, soft-deletion scoped,
/// `workspace__slug` + `member` + `is_active`).
async fn fetch_workspace_role(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &Uuid,
) -> Result<Option<i16>, Denial> {
    crate::app_issues::access::workspace_role(&orm(pool), slug, *user_id)
        .await
        .map_err(|_| Denial::ServerError)
}

/// Membership facts for one decorator gate: the caller passes the route's
/// [`gates::Gate`] so the allowed-role set matches the Python source.
fn workspace_facts(
    slug: &str,
    role: Option<i32>,
    allowed_roles: &[i32],
    is_creator: bool,
) -> AllowFacts {
    AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.is_some_and(|role| allowed_roles.contains(&role)),
        is_creator,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(ROLE_ADMIN),
    }
}

/// Run one `@allow_permission` decorator gate: anonymous already 401'd; `Allow`
/// yields `None` (the body runs), anything else yields the denial response.
/// Decorator denials render [`gates::FORBIDDEN_BODY`].
async fn check_allow_gate(
    pool: &sqlx::PgPool,
    gate: &gates::Gate,
    slug: &str,
    user_id: &Uuid,
    is_creator: bool,
) -> Result<Option<Response>, Denial> {
    let deny = |body: &str| json_response(StatusCode::FORBIDDEN, body.to_owned());
    let role = fetch_workspace_role(pool, slug, user_id)
        .await?
        .map(i32::from);
    let roles: &[i32] = match gate {
        gates::Gate::Workspace { roles } | gates::Gate::WorkspaceCreator { roles, .. } => roles,
        _ => &[],
    };
    let scope = gates::tenant_context(slug);
    let outcome = gates::decide_gate(
        gate,
        &scope,
        &workspace_facts(slug, role, roles, is_creator),
    );
    match outcome {
        gates::GateOutcome::Allow => Ok(None),
        gates::GateOutcome::Unauthenticated => Ok(Some(json_response(
            StatusCode::UNAUTHORIZED,
            gates::ANON_BODY.to_owned(),
        ))),
        // Only `Deny` is reachable here (decorator gates never yield
        // `DenyClass`; the slug always resolves so `MissingSlug` cannot
        // fire) — the arms stay for exhaustiveness.
        gates::GateOutcome::Deny | gates::GateOutcome::DenyClass => Ok(Some(deny(
            gates::outcome_body(outcome).unwrap_or(gates::FORBIDDEN_BODY),
        ))),
        gates::GateOutcome::MissingSlug => Ok(Some(json_response(
            StatusCode::BAD_REQUEST,
            gates::outcome_body(outcome)
                .unwrap_or(gates::FORBIDDEN_BODY)
                .to_owned(),
        ))),
    }
}

/// The creator probe behind the draft creator gates
/// (`app/permissions/base.py:36`): `model.objects.filter(id=pk,
/// created_by=user).exists()` over `table` (`issues` — the dead branch — or
/// `draft_issues`), default soft-delete scope.
async fn creator_exists(
    pool: &sqlx::PgPool,
    table: &str,
    pk: &Uuid,
    user_id: &Uuid,
) -> Result<bool, Denial> {
    let db = orm(pool);
    let hit: Option<Uuid> = match table {
        "issues" => issues::Entity::find()
            .select_only()
            .column(issues::Column::Id)
            .filter(issues::Column::Id.eq(*pk))
            .filter(issues::Column::CreatedById.eq(*user_id))
            .filter(issues::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?,
        "draft_issues" => draft_issues::Entity::find()
            .select_only()
            .column(draft_issues::Column::Id)
            .filter(draft_issues::Column::Id.eq(*pk))
            .filter(draft_issues::Column::CreatedById.eq(*user_id))
            .filter(draft_issues::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?,
        _ => unreachable!("creator table {table}"),
    };
    Ok(hit.is_some())
}

#[allow(clippy::result_large_err)]
fn parse_pk(raw: &str) -> Result<Uuid, Response> {
    // Django's `<uuid:…>` converter (`[0-9a-f]{8}-…`, lowercase-only,
    // case-sensitive match): anything else misses the route and 404s through
    // `custom_404_view` before auth ever runs.
    const HYPHENS: [usize; 4] = [8, 13, 18, 23];
    let bytes = raw.as_bytes();
    let valid = bytes.len() == 36
        && HYPHENS.iter().all(|&i| bytes[i] == b'-')
        && bytes.iter().enumerate().all(|(i, &b)| {
            HYPHENS.contains(&i) || (b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        });
    if !valid {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            PAGE_NOT_FOUND_BODY.to_owned(),
        ));
    }
    Uuid::parse_str(raw)
        .map_err(|_| json_response(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY.to_owned()))
}

fn utc_now_micros() -> DateTime<Utc> {
    Utc::now()
}

// ---------------------------------------------------------------------------
// Query params (last value wins, like `QueryDict.get`)
// ---------------------------------------------------------------------------

/// Parse the raw query string into last-wins pairs (`QueryDict.get`
/// semantics: `?a=1&a=2` reads `"2"`; a bare `?flag` reads `""`).
fn query_pairs(raw: &str) -> Vec<(String, String)> {
    serde_urlencoded::from_str::<Vec<(String, String)>>(raw).unwrap_or_default()
}

/// Last value for `key`, if the key is present at all.
fn query_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

// ---------------------------------------------------------------------------
// Request bodies (DRF `request.data`)
// ---------------------------------------------------------------------------

/// This module's HTML-input shape: the two `ListField`s arrive as arrays;
/// no scalar skips (the field pass applies the blank rules with required context).
const BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &["assignee_ids", "label_ids"],
    skip_blank_fields: &[],
};

/// Parsed `request.data`: the JSON value (or form text map as an object) plus
/// uploads per key, and whether HTML-input `get_value` rules apply.
struct RequestData {
    value: Value,
    files: BTreeMap<String, Vec<shared_body::FilePart>>,
    is_html: bool,
}

fn unsupported_media_type(message: String) -> Response {
    let body = format!("{{\"Detail\":{}}}", json_string(&message));
    Response::builder()
        .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("415 response")
}

/// Every write path parses through the shared DRF body pipeline:
/// empty-by-Content-Length is `{}`, unsupported/missing content type is the
/// 415, JSON text runs the CPython-error parser, forms run the shared
/// HTML-input machine.
#[allow(clippy::result_large_err)]
fn negotiate_input(headers: &HeaderMap, body: &[u8]) -> Result<RequestData, Response> {
    let map_error = |error: shared_body::BodyError| match error {
        shared_body::BodyError::UnsupportedMediaType(message) => unsupported_media_type(message),
        shared_body::BodyError::ParseDetail(message) => Denial::BadDetail(message).into_response(),
        shared_body::BodyError::ServerError => Denial::ServerError.into_response(),
    };
    match shared_body::negotiate_body(headers, body, &BODY_SPEC).map_err(map_error)? {
        shared_body::NegotiatedBody::Empty => Ok(RequestData {
            value: Value::Object(Map::new()),
            files: BTreeMap::new(),
            is_html: false,
        }),
        shared_body::NegotiatedBody::JsonText { text, surr } => {
            parse_request_data_spans(text.as_bytes(), &surr)
                .map(|parsed| RequestData {
                    value: to_serde_publish(&parsed),
                    files: BTreeMap::new(),
                    is_html: false,
                })
                .map_err(|fail| match fail {
                    JsonFail::Message(detail) => {
                        Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}")).into_response()
                    }
                    JsonFail::Recursion => Denial::ServerError.into_response(),
                })
        }
        shared_body::NegotiatedBody::Form { map, files, .. } => Ok(RequestData {
            value: Value::Object(map),
            files,
            is_html: true,
        }),
    }
}

/// One input value for field `get_value`: JSON/form scalars resolve to
/// `Json`, missing keys to `Missing`, and keys carrying any upload to
/// `Files` (DRF merges files into `request.data`, so a files-only key is
/// present and reads as the file object).
enum InputRef<'a> {
    Missing,
    Json(&'a Value),
    Files,
}

impl RequestData {
    fn input(&self, key: &str) -> InputRef<'_> {
        if let Some(files) = self.files.get(key) {
            if !files.is_empty() {
                return InputRef::Files;
            }
        }
        match self.value.as_object().and_then(|map| map.get(key)) {
            Some(value) => InputRef::Json(value),
            None => InputRef::Missing,
        }
    }

    /// `request.data.get(key)`: the JSON value, or `None` when missing (a
    /// files-only key reads as present-but-opaque — only the two `ListField`s
    /// consume uploads, via the shared spec's arrays).
    fn get(&self, key: &str) -> Option<&Value> {
        match self.input(key) {
            InputRef::Json(value) => Some(value),
            InputRef::Missing => None,
            InputRef::Files => self.value.as_object().and_then(|map| map.get(key)),
        }
    }
}

/// Python truthiness over parsed JSON: `false`/`null`/`0`/`""`/`[]`/`{}`
/// are falsy, everything else truthy.
fn json_is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// `json.dumps` with CPython defaults (Celery `requested_data` payloads)
// ---------------------------------------------------------------------------

/// `json.dumps(value)`: `separators=(', ', ': ')`, `ensure_ascii=True`,
/// insertion-ordered keys (the `handlers_engage` transcription, converged).
fn python_dumps(value: &Value) -> String {
    let mut out = String::new();
    python_dump_into(&mut out, value);
    out
}

/// A JSON integer spelling (`-` + digits): Python parses it as `int`
/// (rendered by value; only `-0` differs from its spelling).
fn is_plain_int_spelling(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// One `json.dumps` number: integers render their VALUE (`-0` → `0` —
/// the only non-canonical JSON int spelling, since leading zeros are
/// invalid); bignums beyond `u64`/`i64` echo verbatim (no normalization
/// exists for them). Anything else parsed as a float renders the CPython
/// `repr` spelling (`1e2` → `100.0`), with the `allow_nan` spellings for
/// non-finite values.
fn python_dump_number(out: &mut String, number: &serde_json::Number) {
    let text = number.to_string();
    if is_plain_int_spelling(&text) {
        if let Some(int) = number.as_i64() {
            out.push_str(&int.to_string());
            return;
        }
        if let Some(uint) = number.as_u64() {
            out.push_str(&uint.to_string());
            return;
        }
        out.push_str(&text);
        return;
    }
    match text.parse::<f64>() {
        Ok(float) if float.is_finite() => out.push_str(&py_float_str(float)),
        Ok(float) if float.is_nan() => out.push_str("NaN"),
        Ok(_) if text.starts_with('-') => out.push_str("-Infinity"),
        Ok(_) => out.push_str("Infinity"),
        Err(_) => out.push_str(&text),
    }
}

fn python_dump_into(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => python_dump_number(out, number),
        Value::String(text) => python_dump_str(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_str(out, key);
                out.push_str(": ");
                python_dump_into(out, item);
            }
            out.push('}');
        }
    }
}

/// CPython `py_encode_basestring_ascii`.
fn python_dump_str(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 || (ch as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) > 0x7E => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let v = code - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (v >> 10),
                        0xDC00 + (v & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{:04x}", code));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------------------
// Datetime rendering
// ---------------------------------------------------------------------------

/// DRF `DateTimeField.to_representation`: the instant in the actor zone,
/// `isoformat()`, microsecond precision, a UTC `+00:00` rewritten to `Z`.
fn render_drf_datetime(moment: &DateTime<Utc>, timezone: &Tz) -> String {
    render_datetime_in(moment, timezone)
}

/// `DjangoJSONEncoder` datetime rendering (the `.values()` re-read path, which
/// skips `user_timezone_converter`, so UTC): `isoformat()` with the microsecond
/// tail TRUNCATED to milliseconds (`r[:23] + r[26:]`), `+00:00` rewritten to `Z`;
/// a zero microsecond renders with no fraction at all.
fn render_encoder_datetime(moment: &DateTime<Utc>) -> String {
    let micros = moment.timestamp_subsec_micros();
    if micros == 0 {
        return moment.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    }
    let millis = micros / 1000;
    format!("{}.{:03}Z", moment.format("%Y-%m-%dT%H:%M:%S"), millis)
}

/// Apply `gzip_page` to a finished response body (`draft.py:97` sits outside
/// `@allow_permission`, so it wraps denials too): bodies under 200 bytes pass
/// untouched (not even `Vary`); longer ones gain `Vary: Accept-Encoding` and,
/// when the client accepts gzip, the compressed bytes iff strictly shorter.
fn gzip_body(request_headers: &HeaderMap, status: StatusCode, body: String) -> Response {
    let bytes = body.as_bytes();
    if bytes.len() < MIN_COMPRESS_BYTES {
        return json_response(status, body);
    }
    let accepts = request_headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(accepts_gzip);
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::VARY, gates::VARY_ACCEPT_ENCODING);
    if accepts {
        if let Some(compressed) = compress_if_shorter(bytes) {
            builder = builder
                .header(header::CONTENT_ENCODING, "gzip")
                .header(header::CONTENT_LENGTH, compressed.len().to_string());
            return builder
                .body(axum::body::Body::from(compressed))
                .expect("gzip response");
        }
    }
    builder
        .body(axum::body::Body::from(body))
        .expect("gzip response")
}

// ---------------------------------------------------------------------------
// DRF field validation (`is_valid()` before `validate()`)
// ---------------------------------------------------------------------------

/// DRF 3.15.2 message texts (`fields.py`, verified live against the pinned venv).
const MSG_REQUIRED: &str = "This field is required.";
const MSG_NULL: &str = "This field may not be null.";
const MSG_BLANK: &str = "This field may not be blank.";
const MSG_INVALID_STR: &str = "Not a valid string.";
const MSG_NULL_CHARACTERS: &str = "Null characters are not allowed.";
const MSG_SURROGATE_CHARACTERS: &str = "Surrogate characters are not allowed.";
const MSG_INVALID_INT: &str = "A valid integer is required.";
const MSG_INVALID_FLOAT: &str = "A valid number is required.";
const MSG_INVALID_BOOL: &str = "Must be a valid boolean.";
const MSG_STRING_TOO_LARGE: &str = "String value too large.";
const MSG_DATE_INVALID: &str =
    "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
const MSG_DATETIME_INVALID: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
const MSG_DATETIME_OVERFLOW: &str = "Datetime value out of range.";
const MSG_JSON_INVALID: &str = "Value must be valid JSON.";
const MSG_UUID_INVALID: &str = "Must be a valid UUID.";
/// DRF `IntegerField`/`FloatField.MAX_STRING_LENGTH` (chars, not bytes).
const MAX_STRING_LENGTH: usize = 1000;

/// `CharField.to_internal_value` (`fields.py`): bools and composites fail;
/// str/int/float coerce via `str()`, then whitespace-trimmed; `max_length`
/// counts code points; NUL and surrogate checks run as validators after it.
fn parse_char(value: &Value, max_length: Option<usize>) -> Result<String, Vec<String>> {
    let text = match value {
        Value::Bool(_) => return Err(vec![MSG_INVALID_STR.to_owned()]),
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            return Err(vec![MSG_INVALID_STR.to_owned()])
        }
    };
    let trimmed = text.trim().to_owned();
    if let Some(max) = max_length {
        if trimmed.chars().count() > max {
            return Err(vec![format!(
                "Ensure this field has no more than {max} characters."
            )]);
        }
    }
    if trimmed.contains('\0') {
        return Err(vec![MSG_NULL_CHARACTERS.to_owned()]);
    }
    if trimmed
        .chars()
        .any(|ch| (0xD800..0xE000).contains(&(ch as u32)))
    {
        return Err(vec![MSG_SURROGATE_CHARACTERS.to_owned()]);
    }
    Ok(trimmed)
}

/// `ChoiceField.to_internal_value`: `""` passes only with `allow_blank`;
/// lookup is over `str(data)`; failures echo the raw input.
fn parse_choice(value: &Value, choices: &[&str], allow_blank: bool) -> Result<String, Vec<String>> {
    if let Value::String(text) = value {
        if text.is_empty() && allow_blank {
            return Ok(String::new());
        }
    }
    // `str(data)` for the lookup and the echo: JSON scalars stringify;
    // composites use the CPython `repr`.
    let key = choice_stringify(value);
    if choices.contains(&key.as_str()) {
        return Ok(key);
    }
    Err(vec![format!("\"{key}\" is not a valid choice.")])
}

/// `str(data)` for choice lookup/echo: strings bare, numbers via the JSON
/// spelling, bools/null via CPython `repr`, composites via CPython `repr`.
fn choice_stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// `int(re_decimal.sub('', str(data)))` with `re_decimal = /\.0*\s*$/`
/// (`fields.py`): `"5.0"`/`5.0` → 5 (via the `str()` round-trip —
/// exponent-spelled floats like `1e16` stringify to `"1e+16"` and fail,
/// exactly like CPython); `"5.5"`/bools/composites → invalid. Out-of-`i64`
/// magnitudes are NOT invalid: Python ints are unbounded, so they flow to
/// the field's `min_value`/`max_value` validators.
fn parse_integer(value: &Value) -> Result<IntegerValue, Vec<String>> {
    match value {
        Value::Bool(_) => Err(vec![MSG_INVALID_INT.to_owned()]),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(IntegerValue::Int(int));
            }
            if let Some(uint) = number.as_u64() {
                if let Ok(int) = i64::try_from(uint) {
                    return Ok(IntegerValue::Int(int));
                }
                return Ok(IntegerValue::TooBig);
            }
            // Int-shaped literals past `u64`/`i64` are out of range (never
            // `invalid` — CPython ints are unbounded, so DRF reports
            // min/max); only float shapes fall through to `repr`.
            if let Some(literal) = int_literal(number) {
                return Ok(if literal.starts_with('-') {
                    IntegerValue::TooSmall
                } else {
                    IntegerValue::TooBig
                });
            }
            match number.as_f64() {
                Some(float) => {
                    let text = strip_decimal_zeros(&py_float_str(float));
                    match parse_python_int(&text) {
                        Some(int) => Ok(IntegerValue::Int(int)),
                        None if is_big_int_spelling(&text) => Ok(IntegerValue::TooBig),
                        None if is_small_int_spelling(&text) => Ok(IntegerValue::TooSmall),
                        None => Err(vec![MSG_INVALID_INT.to_owned()]),
                    }
                }
                None => Err(vec![MSG_INVALID_INT.to_owned()]),
            }
        }
        Value::String(text) => {
            if text.chars().count() > MAX_STRING_LENGTH {
                return Err(vec![MSG_STRING_TOO_LARGE.to_owned()]);
            }
            let stripped = strip_decimal_zeros(text);
            match parse_python_int(&stripped) {
                Some(int) => Ok(IntegerValue::Int(int)),
                None if is_big_int_spelling(&stripped) => Ok(IntegerValue::TooBig),
                None if is_small_int_spelling(&stripped) => Ok(IntegerValue::TooSmall),
                None => Err(vec![MSG_INVALID_INT.to_owned()]),
            }
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(vec![MSG_INVALID_INT.to_owned()]),
    }
}

/// An integer input: in-range, or out-of-`i64` (the driver's range check
/// turns these into the field's `min_value`/`max_value` messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntegerValue {
    Int(i64),
    TooBig,
    TooSmall,
}

/// CPython `int(str)`: surrounding whitespace, one sign, digits with
/// underscores between digits (`int()` allows `1_0`).
fn parse_python_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if digits.is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        prev_underscore = false;
        cleaned.push(ch);
    }
    if prev_underscore {
        return None;
    }
    let magnitude: i64 = cleaned.parse().ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

/// Whether `text` (post `strip_decimal_zeros`) spells an integer past `i64`
/// in the positive direction (digits only, modulo one sign + underscores).
fn is_big_int_spelling(text: &str) -> bool {
    int_spelling_sign(text) == Some(false)
}

/// Whether `text` spells an integer past `i64` in the negative direction.
fn is_small_int_spelling(text: &str) -> bool {
    int_spelling_sign(text) == Some(true)
}

/// `Some(negative)` when `text` is a well-formed over-`i64` integer
/// spelling, `None` otherwise.
fn int_spelling_sign(text: &str) -> Option<bool> {
    let trimmed = text.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if digits.is_empty() {
        return None;
    }
    let mut prev_underscore = true;
    let mut count = 0usize;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        prev_underscore = false;
        count += 1;
    }
    if prev_underscore || count == 0 {
        return None;
    }
    let stripped: String = digits.chars().filter(|ch| *ch != '_').collect();
    let significant = stripped.trim_start_matches('0');
    // `i64::MAX` is 19 digits; 19 significant digits may still fit (parse
    // already failed, so a 19-digit spelling here is out of range).
    if significant.len() < 19 {
        return None;
    }
    if significant.len() == 19 {
        let fits = if negative {
            significant <= "9223372036854775808"
        } else {
            significant <= "9223372036854775807"
        };
        if fits {
            return None;
        }
    }
    Some(negative)
}

/// `re_decimal.sub('', text)` for `re_decimal = /\.0*\s*$/`: strip one
/// all-zero fraction plus trailing whitespace.
fn strip_decimal_zeros(text: &str) -> String {
    let trimmed = text.trim_end();
    let Some(dot) = trimmed.rfind('.') else {
        return text.to_owned();
    };
    let (head, frac) = trimmed.split_at(dot);
    let frac_digits = &frac[1..];
    if !frac_digits.is_empty() && frac_digits.bytes().all(|b| b == b'0') {
        head.to_owned()
    } else {
        text.to_owned()
    }
}

/// The exact decimal text of an int-shaped JSON number (`arbitrary_precision`
/// keeps the source spelling), or `None` for float shapes.
fn int_literal(number: &serde_json::Number) -> Option<String> {
    let text = number.to_string();
    if text.contains(['.', 'e', 'E']) {
        return None;
    }
    let digits = text.strip_prefix('-').unwrap_or(&text);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        Some(text)
    } else {
        None
    }
}

/// `str(number)` for char/choice coercion: ints exact, floats via CPython `repr`.
fn json_number_str(number: &serde_json::Number) -> String {
    if let Some(literal) = int_literal(number) {
        return literal;
    }
    match number.as_f64() {
        Some(float) => py_float_str(float),
        None => number.to_string(),
    }
}

/// CPython `repr(float)` (the `paginator` kernel, shared).
fn py_float_str(float: f64) -> String {
    crate::paginator::py_float_str(float)
}

/// `float(data)`: bools coerce (`True` → 1.0), strings parse with CPython
/// leniency (`inf`/`nan`, underscores, surrounding whitespace), ints widen.
fn parse_float(value: &Value) -> Result<f64, Vec<String>> {
    match value {
        Value::Bool(true) => Ok(1.0),
        Value::Bool(false) => Ok(0.0),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int as f64);
            }
            if let Some(uint) = number.as_u64() {
                return Ok(uint as f64);
            }
            match number.as_f64() {
                Some(float) => Ok(float),
                None => Err(vec![MSG_INVALID_FLOAT.to_owned()]),
            }
        }
        Value::String(text) => {
            if text.chars().count() > MAX_STRING_LENGTH {
                return Err(vec![MSG_STRING_TOO_LARGE.to_owned()]);
            }
            parse_python_float(text).ok_or_else(|| vec![MSG_INVALID_FLOAT.to_owned()])
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(vec![MSG_INVALID_FLOAT.to_owned()]),
    }
}

/// CPython `float(str)`: surrounding whitespace tolerated, `inf`/`infinity`/
/// `nan` (any case, optional sign), underscores between digits.
fn parse_python_float(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let (negative, core) = match lower.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, lower.strip_prefix('+').unwrap_or(&lower)),
    };
    if core == "inf" || core == "infinity" {
        return Some(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if core == "nan" {
        return Some(f64::NAN);
    }
    // Underscore placement: between digits (or digit-and-dot) only.
    if trimmed.contains('_') {
        let bytes = trimmed.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'_' {
                let prev = index.checked_sub(1).and_then(|i| bytes.get(i));
                let next = bytes.get(index + 1);
                let ok_prev = prev.is_some_and(|b| b.is_ascii_digit());
                let ok_next = next.is_some_and(|b| b.is_ascii_digit() || *b == b'.');
                if !(ok_prev && ok_next) {
                    return None;
                }
            }
        }
    }
    let cleaned: String = trimmed.chars().filter(|ch| *ch != '_').collect();
    cleaned.parse::<f64>().ok()
}

/// `BooleanField.to_internal_value` (`fields.py`): case-insensitive
/// true/false sets over strings, `1`/`0`/`0.0` numerics, real bools.
/// `1.0` is TRUE (it hashes equal to `1` in the set lookup).
fn parse_boolean(value: &Value) -> Result<bool, Vec<String>> {
    const TRUE_STRINGS: &[&str] = &["t", "y", "yes", "true", "on", "1"];
    const FALSE_STRINGS: &[&str] = &["f", "n", "no", "false", "off", "0"];
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::String(text) => {
            let lower = text.to_lowercase();
            if TRUE_STRINGS.contains(&lower.as_str()) {
                Ok(true)
            } else if FALSE_STRINGS.contains(&lower.as_str()) {
                Ok(false)
            } else {
                Err(vec![MSG_INVALID_BOOL.to_owned()])
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int == 1 {
                    return Ok(true);
                }
                if int == 0 {
                    return Ok(false);
                }
                return Err(vec![MSG_INVALID_BOOL.to_owned()]);
            }
            if let Some(uint) = number.as_u64() {
                if uint == 1 {
                    return Ok(true);
                }
                if uint == 0 {
                    return Ok(false);
                }
                return Err(vec![MSG_INVALID_BOOL.to_owned()]);
            }
            match number.as_f64() {
                // `1.0 in TRUE_VALUES` (hash-equal to `1`); `0.0` is an
                // explicit `FALSE_VALUES` member.
                Some(1.0) => Ok(true),
                Some(0.0) => Ok(false),
                _ => Err(vec![MSG_INVALID_BOOL.to_owned()]),
            }
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(vec![MSG_INVALID_BOOL.to_owned()]),
    }
}

/// `JSONField.to_internal_value` (non-binary, JSON input): any parsed value
/// round-trips `json.dumps` — over real JSON input nothing fails.
fn parse_json_value(value: &Value) -> Result<Value, Vec<String>> {
    // `json.dumps` fails only on non-serializables (NaN/Infinity are emitted
    // by default, sets have no JSON spelling): a parsed `Value` always dumps.
    let _ = serde_json::to_string(value).map_err(|_| vec![MSG_JSON_INVALID.to_owned()])?;
    Ok(value.clone())
}

/// CPython `repr` for composite JSON (`str(data)` in choice/UUID echoes):
/// single-quoted strings, `True`/`False`/`None`, `", "`/`": "` separators.
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => json_number_str(number),
        Value::String(text) => py_repr_string(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr_string(key), py_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr(str)`: single quotes unless the text holds one (then double
/// quotes, escaping backslashes and the quote in use), ASCII escapes for
/// controls, `\x`/`\u`/`\U` for the rest.
fn py_repr_string(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch if (ch as u32) < 0x20 || (ch as u32) == 0x7F => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            ch if (ch as u32) < 0x10000 => out.push(ch),
            ch => out.push_str(&format!("\\U{:08x}", ch as u32)),
        }
    }
    out.push(quote);
    out
}

/// `DateField.to_internal_value` (`fields.py` + Django `parse_date`): the
/// `fromisoformat` surface first, then the `date_re` fallback
/// (`YYYY-M-D`, 1-2-digit fields).
fn parse_drf_date(text: &str) -> Option<NaiveDate> {
    if let Some(date) = parse_iso_date_part(text) {
        return Some(date);
    }
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || year.len() != 4 {
        return None;
    }
    if !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    NaiveDate::from_ymd_opt(
        parse_digits(year)? as i32,
        parse_digits(month)?,
        parse_digits(day)?,
    )
}

/// Django `parse_datetime` + DRF `enforce_timezone` in one: the
/// `fromisoformat` surface (plus the `datetime_re` fallback), then naive
/// values interpreted in the actor zone (`make_aware`) and aware values
/// converted to the stored UTC instant. Errors map to the DRF `invalid` /
/// `overflow` / `make_aware` messages.
fn parse_drf_datetime(text: &str, timezone: &Tz) -> Result<DateTime<Utc>, Vec<String>> {
    let invalid = || vec![MSG_DATETIME_INVALID.to_owned()];
    // Trailing whitespace is tolerated only when no tz designator is
    // present (`'...05 '` ok, `'...+00:00 '` rejected, leading rejected).
    let mut candidates = vec![text];
    let trimmed_end = text.trim_end();
    if trimmed_end.len() != text.len() && split_tz_suffix(trimmed_end).1.is_none() {
        candidates.push(trimmed_end);
    }
    let mut parsed = None;
    for candidate in candidates {
        if let Some(value) = parse_iso_datetime(candidate) {
            parsed = Some(value);
            break;
        }
        if let Some(value) = parse_fallback_datetime(candidate) {
            parsed = Some(value);
            break;
        }
    }
    // DRF's `strptime(value, 'iso-8601')` fallthrough (`to_internal_value`
    // runs it when `parse_datetime` returns `None` — PIDASHCONV-773):
    // the literal matches case-insensitively and yields naive
    // 1900-01-01 through the zone-attach arm below. Checked on the RAW
    // text — Django's `strptime` sees the value before any strip, so a
    // padded literal still fails. Exact match (probed); ASCII-only (765
    // unicode-gap family).
    if parsed.is_none() && text.eq_ignore_ascii_case("iso-8601") {
        let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("1900-01-01 valid");
        parsed = Some((naive, None));
    }
    let (naive, offset_secs) = parsed.ok_or_else(invalid)?;
    match offset_secs {
        Some(offset) => naive
            .and_utc()
            .checked_sub_signed(chrono::TimeDelta::seconds(offset))
            .ok_or_else(|| vec![MSG_DATETIME_OVERFLOW.to_owned()]),
        None => match timezone.from_local_datetime(&naive) {
            chrono::MappedLocalTime::Single(aware) => Ok(aware.to_utc()),
            chrono::MappedLocalTime::Ambiguous(early, _) => Ok(early.to_utc()),
            chrono::MappedLocalTime::None => Err(vec![format!(
                "Invalid datetime for the timezone \"{timezone}\"."
            )]),
        },
    }
}

/// Django's `datetime_re` fallback: extended date with 1-2-digit fields,
/// `[T ]` separator, `H:M[:S[.ffffff]]` (fraction dot-or-comma, at most 12
/// digits, extras ignored past 6), optional `Z`/`±HH[[:]MM]` tz.
fn parse_fallback_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<i64>)> {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    let (head, offset) = split_tz_suffix(text);
    if offset == Some(i64::MAX) {
        return None;
    }
    // Offset seconds are `fromisoformat`-only; the fallback allows at most
    // `±HH[:]MM`.
    if offset.is_some() {
        let tail = text[head.len()..].trim();
        let tail = tail.strip_prefix(['+', '-']).unwrap_or(tail);
        if tail.strip_suffix('Z').is_none() && tail.contains(':') && tail.matches(':').count() > 1 {
            return None;
        }
    }
    let sep_at = head.find(['T', ' '])?;
    let (date_text, time_text) = (&head[..sep_at], &head[sep_at + 1..]);
    let mut date_parts = date_text.split('-');
    let (year, month, day) = (date_parts.next()?, date_parts.next()?, date_parts.next()?);
    if date_parts.next().is_some() || year.len() != 4 {
        return None;
    }
    if !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(
        parse_digits(year)? as i32,
        parse_digits(month)?,
        parse_digits(day)?,
    )?;
    let (clock, micros) = match time_text.find(['.', ',']) {
        Some(dot) => {
            let (clock, frac) = time_text.split_at(dot);
            let digits = &frac[1..];
            if digits.is_empty() || digits.len() > 12 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let kept = &digits[..digits.len().min(6)];
            let padded = format!("{kept:0<6}");
            (clock, padded[..6].parse::<u32>().ok()?)
        }
        None => (time_text, 0),
    };
    let mut time_parts = clock.split(':');
    let (hour, minute) = (time_parts.next()?, time_parts.next()?);
    let second = time_parts.next().unwrap_or("0");
    if time_parts.next().is_some() {
        return None;
    }
    if !(1..=2).contains(&hour.len()) || !(1..=2).contains(&minute.len()) {
        return None;
    }
    if !(1..=2).contains(&second.len()) {
        return None;
    }
    let time = NaiveTime::from_hms_micro_opt(
        parse_digits(hour)?,
        parse_digits(minute)?,
        parse_digits(second)?,
        micros,
    )?;
    Some((NaiveDateTime::new(date, time), offset))
}

/// DRF's own `UUIDField.to_internal_value` (`entity_identifier`): UUIDs pass,
/// ints (bools included) ride `UUID(int=)`, strings ride `UUID(hex=)`
/// (case-insensitive, braces/`urn:`/dashless accepted); everything else is
/// the bare `"Must be a valid UUID."` (no echo).
fn parse_uuid_field(value: &Value) -> Result<Uuid, Vec<String>> {
    let invalid = || vec![MSG_UUID_INVALID.to_owned()];
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(invalid());
                }
                return Ok(Uuid::from_u128(int as u128));
            }
            if let Some(uint) = number.as_u64() {
                return Ok(Uuid::from_u128(uint as u128));
            }
            // `arbitrary_precision` keeps over-`u64` spellings: valid iff an
            // int literal under 2^128.
            if let Some(literal) = int_literal(number) {
                let digits = literal.strip_prefix('-').unwrap_or(&literal);
                if literal.starts_with('-') {
                    return Err(invalid());
                }
                if let Ok(int) = digits.parse::<u128>() {
                    return Ok(Uuid::from_u128(int));
                }
                return Err(invalid());
            }
            Err(invalid())
        }
        Value::String(text) => Uuid::parse_str(text).map_err(|_| invalid()),
        // `isinstance(True, int)`: bools ride the int path (`True` → 1).
        Value::Bool(true) => Ok(Uuid::from_u128(1)),
        Value::Bool(false) => Ok(Uuid::from_u128(0)),
        Value::Null | Value::Array(_) | Value::Object(_) => Err(invalid()),
    }
}

/// `PrimaryKeyRelatedField.to_internal_value` for UUID pks: bools REJECT
/// (`incorrect_type`, echoing the JSON type name); everything else goes to
/// the queryset, where Django's `UUIDField.to_python` tries `UUID(int=)` for
/// ints and `UUID(hex=)` otherwise — `AttributeError`/`ValueError` become the
/// fancy-quote Django `invalid` message echoing `str(value)`, and a
/// parsed-but-missing id is the caller's `does_not_exist` probe.
fn parse_uuid_pk(value: &Value) -> Result<Uuid, Vec<String>> {
    if value.is_boolean() {
        return Err(vec![format!(
            "Incorrect type. Expected pk value, received {}.",
            json_type_name(value)
        )]);
    }
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(vec![django_uuid_invalid(&number.to_string())]);
                }
                return Ok(Uuid::from_u128(int as u128));
            }
            if let Some(uint) = number.as_u64() {
                return Ok(Uuid::from_u128(uint as u128));
            }
            if let Some(literal) = int_literal(number) {
                if literal.starts_with('-') {
                    return Err(vec![django_uuid_invalid(&number.to_string())]);
                }
                if let Ok(int) = literal.parse::<u128>() {
                    return Ok(Uuid::from_u128(int));
                }
                return Err(vec![django_uuid_invalid(&number.to_string())]);
            }
            Err(vec![django_uuid_invalid(&json_number_str(number))])
        }
        Value::String(text) => Uuid::parse_str(text).map_err(|_| vec![django_uuid_invalid(text)]),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err(vec![django_uuid_invalid(&pk_echo(value))])
        }
        Value::Bool(_) => unreachable!("bools reject above"),
    }
}

/// Django `UUIDField.invalid`: `"\u201c%(value)s\u201d is not a valid UUID."`.
fn django_uuid_invalid(echo: &str) -> String {
    format!("\u{201C}{echo}\u{201D} is not a valid UUID.")
}

/// `str(value)` for the pk echoes: strings bare, numbers via the JSON
/// spelling, composites via CPython `repr`.
fn pk_echo(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        _ => py_repr(value),
    }
}

/// The `does_not_exist` message echoes the ORIGINAL input (`"123"` for ints,
/// the raw spelling for strings, CPython `repr` for composites).
fn does_not_exist_echo(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        _ => py_repr(value),
    }
}

/// `type(data).__name__` for `incorrect_type` (bools only here) and
/// `not_a_list` (every JSON type: `str`/`int`/`float`/`bool`/`dict`…).
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if int_literal(number).is_some() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// One validated field value (`None` at the map level means the key was
/// absent; `FieldValue::Null` means an explicit JSON `null` that passed
/// `allow_null`).
#[derive(Debug, Clone)]
enum FieldValue {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
    Uuid(Uuid),
    Json(Value),
    Ids(Vec<Uuid>),
    Null,
}

/// The queryset one `PrimaryKeyRelatedField` reads (DRF `get_queryset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PkQueryset {
    /// `State.all_state_objects.all()` (unscoped).
    StatesAll,
    /// `State.objects.all()` (live, non-triage group).
    States,
    /// `Issue.objects.all()` (live).
    Issues,
    /// `Issue._default_manager` (`IssueManager`: live + non-triage state +
    /// non-archived issue/project + non-draft) — the AUTO `parent` field's
    /// queryset on both create serializers (probed live).
    IssuesVisible,
    /// `Pod.all_objects` (unscoped).
    PodsAll,
    /// `Label.objects.all()` (live).
    Labels,
    /// `User.objects.all()` (the `users` table has no soft-delete column).
    Users,
    /// `EstimatePoint.objects.all()` (live).
    Estimates,
    /// `IssueType.objects.all()` (live).
    Types,
    /// `Project.objects.all()` (live).
    Projects,
    /// `UserFavorite.objects.all()` (live).
    Favorites,
}

/// One writable serializer field: DRF field class + flags, probed live
/// (`/tmp/probe_ser.py`) against the pinned venv.
struct FieldSpec {
    name: &'static str,
    shape: FieldShape,
    required: bool,
    allow_null: bool,
}

/// The DRF field class behind one spec (validators folded in).
#[derive(Clone)]
enum FieldShape {
    /// `CharField(max_length, allow_blank)` (trim + NUL/surrogate validators).
    Char {
        max: Option<usize>,
        allow_blank: bool,
    },
    /// `ChoiceField(choices, allow_blank)`.
    Choice {
        choices: &'static [&'static str],
        allow_blank: bool,
    },
    /// `IntegerField(min_value, max_value)` (bounds optional).
    Int { min: Option<i64>, max: Option<i64> },
    /// `FloatField()`.
    Float,
    /// `BooleanField()`.
    Bool,
    /// `DateField()`.
    Date,
    /// `DateTimeField()` (actor-zone aware).
    DateTime,
    /// DRF's own `UUIDField()` (int/bool/hex leniency, bare message).
    UuidField,
    /// `PrimaryKeyRelatedField(queryset)` (Django fancy-UUID / `does_not_exist`).
    Pk(PkQueryset),
    /// `ListField(child=PrimaryKeyRelatedField(queryset))` (index-keyed errors).
    PkList(PkQueryset),
    /// `JSONField()` (any parsed JSON).
    Json,
}

const PRIORITY_CHOICES: &[&str] = &["urgent", "high", "medium", "low", "none"];
const EXECUTOR_CHOICES: &[&str] = &["cloud_agent", "local_runner", "managed_runner"];

/// `UserFavoriteSerializer` writable fields, declaration order
/// (`favorite.py:64-81`): `entity_type` is the only required field.
const FAV_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "entity_type",
        shape: FieldShape::Char {
            max: Some(100),
            allow_blank: false,
        },
        required: true,
        allow_null: false,
    },
    FieldSpec {
        name: "entity_identifier",
        shape: FieldShape::UuidField,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "name",
        shape: FieldShape::Char {
            max: Some(255),
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "is_folder",
        shape: FieldShape::Bool,
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "sequence",
        shape: FieldShape::Float,
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "parent",
        shape: FieldShape::Pk(PkQueryset::Favorites),
        required: false,
        allow_null: true,
    },
];

/// `DraftIssueCreateSerializer` writable fields (`draft.py:14-62`): nothing
/// is required; `state_id`/`parent_id` write the `state`/`parent` sources
/// (the auto fields win on conflict — they run LATER); `project` is an
/// auto-included writable pk (validated, then overwritten by the context id).
const DRAFT_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "state_id",
        shape: FieldShape::Pk(PkQueryset::States),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "parent_id",
        shape: FieldShape::Pk(PkQueryset::Issues),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "label_ids",
        shape: FieldShape::PkList(PkQueryset::Labels),
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "assignee_ids",
        shape: FieldShape::PkList(PkQueryset::Users),
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "deleted_at",
        shape: FieldShape::DateTime,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "name",
        shape: FieldShape::Char {
            max: Some(255),
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "description_json",
        shape: FieldShape::Json,
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "description_html",
        shape: FieldShape::Char {
            max: None,
            allow_blank: true,
        },
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "description_stripped",
        shape: FieldShape::Char {
            max: None,
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "priority",
        shape: FieldShape::Choice {
            choices: PRIORITY_CHOICES,
            allow_blank: false,
        },
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "start_date",
        shape: FieldShape::Date,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "target_date",
        shape: FieldShape::Date,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "sort_order",
        shape: FieldShape::Float,
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "completed_at",
        shape: FieldShape::DateTime,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "external_source",
        shape: FieldShape::Char {
            max: Some(255),
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "external_id",
        shape: FieldShape::Char {
            max: Some(255),
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "project",
        shape: FieldShape::Pk(PkQueryset::Projects),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "parent",
        shape: FieldShape::Pk(PkQueryset::IssuesVisible),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "state",
        shape: FieldShape::Pk(PkQueryset::States),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "estimate_point",
        shape: FieldShape::Pk(PkQueryset::Estimates),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "type",
        shape: FieldShape::Pk(PkQueryset::Types),
        required: false,
        allow_null: true,
    },
];

/// `IssueCreateSerializer` writable fields (`issue.py:100-135`), declaration
/// order: the declared `_id` fields run FIRST, the auto `state`/`parent`
/// run later and win on conflict (even with `null`); `point` carries the
/// model `Min/MaxValueValidator(0/12)`, `complexity_score` `0/10`, and
/// `git_work_branch` the branch-name regex.
const ISSUE_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "state_id",
        shape: FieldShape::Pk(PkQueryset::StatesAll),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "parent_id",
        shape: FieldShape::Pk(PkQueryset::Issues),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "assigned_pod_id",
        shape: FieldShape::Pk(PkQueryset::PodsAll),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "label_ids",
        shape: FieldShape::PkList(PkQueryset::Labels),
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "assignee_ids",
        shape: FieldShape::PkList(PkQueryset::Users),
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "deleted_at",
        shape: FieldShape::DateTime,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "point",
        shape: FieldShape::Int {
            min: Some(0),
            max: Some(12),
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "name",
        shape: FieldShape::Char {
            max: Some(255),
            allow_blank: false,
        },
        required: true,
        allow_null: false,
    },
    FieldSpec {
        name: "description_json",
        shape: FieldShape::Json,
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "description_html",
        shape: FieldShape::Char {
            max: None,
            allow_blank: true,
        },
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "description_stripped",
        shape: FieldShape::Char {
            max: None,
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "priority",
        shape: FieldShape::Choice {
            choices: PRIORITY_CHOICES,
            allow_blank: false,
        },
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "complexity_score",
        shape: FieldShape::Int {
            min: Some(0),
            max: Some(10),
        },
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "start_date",
        shape: FieldShape::Date,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "target_date",
        shape: FieldShape::Date,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "sequence_id",
        shape: FieldShape::Int {
            min: Some(-2147483648),
            max: Some(2147483647),
        },
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "sort_order",
        shape: FieldShape::Float,
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "completed_at",
        shape: FieldShape::DateTime,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "archived_at",
        shape: FieldShape::Date,
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "is_draft",
        shape: FieldShape::Bool,
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "external_source",
        shape: FieldShape::Char {
            max: Some(255),
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "external_id",
        shape: FieldShape::Char {
            max: Some(255),
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "git_work_branch",
        shape: FieldShape::Char {
            max: Some(128),
            allow_blank: true,
        },
        required: false,
        allow_null: false,
    },
    FieldSpec {
        name: "created_via",
        shape: FieldShape::Char {
            max: Some(32),
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "agent_executor",
        shape: FieldShape::Choice {
            choices: EXECUTOR_CHOICES,
            allow_blank: true,
        },
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "parent",
        shape: FieldShape::Pk(PkQueryset::IssuesVisible),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "state",
        shape: FieldShape::Pk(PkQueryset::States),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "estimate_point",
        shape: FieldShape::Pk(PkQueryset::Estimates),
        required: false,
        allow_null: true,
    },
    FieldSpec {
        name: "type",
        shape: FieldShape::Pk(PkQueryset::Types),
        required: false,
        allow_null: true,
    },
];

/// The two ways the field pass ends: serializer `errors` (400), or a
/// transport failure (500).
enum ShapeFailure {
    Errors(Map<String, Value>),
    Denial(Denial),
}

impl From<Denial> for ShapeFailure {
    fn from(denial: Denial) -> Self {
        ShapeFailure::Denial(denial)
    }
}

/// `Serializer.to_internal_value` over one table (`partial` for PATCH):
/// read-only fields never appear (unknown keys ignored); each writable field
/// runs `get_value` (HTML blank rules) then `run_validation`; errors key by
/// FIELD name in declaration order and EVERY field's errors are collected.
/// Pk existence probes run after the shape pass (a shape error anywhere
/// skips the probes — DRF raises per field before any queryset hits).
async fn validate_shape(
    pool: &sqlx::PgPool,
    input: &RequestData,
    table: &[FieldSpec],
    partial: bool,
    timezone: &Tz,
) -> Result<Vec<(String, FieldValue)>, ShapeFailure> {
    let mut values: Vec<(String, FieldValue)> = Vec::new();
    let mut errors: Map<String, Value> = Map::new();
    // Scalar pk probes deferred past the shape pass, in field order.
    let mut probes: Vec<(&str, PkQueryset, Uuid, Value)> = Vec::new();
    for spec in table {
        // `get_value` markers resolve here; everything else validates below.
        let value = match field_input(input, spec, partial) {
            None => continue,
            Some(FieldInput::Null) => {
                values.push((spec.name.to_owned(), FieldValue::Null));
                continue;
            }
            Some(FieldInput::MissingRequired) => {
                errors.insert(
                    spec.name.to_owned(),
                    Value::Array(vec![Value::String(MSG_REQUIRED.to_owned())]),
                );
                continue;
            }
            Some(FieldInput::UploadPk(filename)) => {
                // A files-only key reads as the file object: pks run
                // `queryset.get(pk=file)` → `to_python` fails → the Django
                // invalid message echoing `str(file)` (the filename).
                errors.insert(
                    spec.name.to_owned(),
                    Value::Array(vec![Value::String(django_uuid_invalid(&filename))]),
                );
                continue;
            }
            Some(FieldInput::UploadList) => {
                // The upload object is not a list (`FILE_UPLOAD_HANDLERS`
                // keep sub-2.5MB uploads in memory).
                errors.insert(
                    spec.name.to_owned(),
                    Value::Array(vec![Value::String(
                        "Expected a list of items but got type \"InMemoryUploadedFile\"."
                            .to_owned(),
                    )]),
                );
                continue;
            }
            Some(FieldInput::UploadJson) => {
                errors.insert(
                    spec.name.to_owned(),
                    Value::Array(vec![Value::String(MSG_JSON_INVALID.to_owned())]),
                );
                continue;
            }
            Some(FieldInput::Value(value)) => value,
        };
        if value == Value::Null && !spec.allow_null {
            // `validate_empty_values` null arm (BEFORE `to_internal_value`).
            errors.insert(
                spec.name.to_owned(),
                Value::Array(vec![Value::String(MSG_NULL.to_owned())]),
            );
            continue;
        }
        let outcome: Result<FieldValue, Vec<String>> = match &spec.shape {
            FieldShape::Char { max, allow_blank } => {
                // `CharField.run_validation` blank arm (BEFORE
                // `to_internal_value`): whitespace-only counts.
                if let Value::String(text) = &value {
                    if text.trim().is_empty() {
                        if !allow_blank {
                            Err(vec![MSG_BLANK.to_owned()])
                        } else {
                            Ok(FieldValue::Text(String::new()))
                        }
                    } else {
                        parse_char(&value, *max).map(FieldValue::Text)
                    }
                } else {
                    parse_char(&value, *max).map(FieldValue::Text)
                }
            }
            FieldShape::Choice {
                choices,
                allow_blank,
            } => parse_choice(&value, choices, *allow_blank).map(FieldValue::Text),
            FieldShape::Int { min, max } => match parse_integer(&value) {
                Err(messages) => Err(messages),
                Ok(IntegerValue::Int(int)) => {
                    if let Some(lo) = min {
                        if int < *lo {
                            errors.insert(
                                spec.name.to_owned(),
                                Value::Array(vec![Value::String(format!(
                                    "Ensure this value is greater than or equal to {lo}."
                                ))]),
                            );
                            continue;
                        }
                    }
                    if let Some(hi) = max {
                        if int > *hi {
                            errors.insert(
                                spec.name.to_owned(),
                                Value::Array(vec![Value::String(format!(
                                    "Ensure this value is less than or equal to {hi}."
                                ))]),
                            );
                            continue;
                        }
                    }
                    Ok(FieldValue::Int(int))
                }
                // Unbounded magnitudes flow to the min/max validators: a
                // field WITHOUT bounds cannot be reached out-of-range
                // through `int()` overflow (CPython), so an unbounded field
                // reports the DRF invalid message instead... except every
                // int field here carries bounds. Unreachable otherwise.
                Ok(IntegerValue::TooBig) => match max {
                    Some(hi) => {
                        errors.insert(
                            spec.name.to_owned(),
                            Value::Array(vec![Value::String(format!(
                                "Ensure this value is less than or equal to {hi}."
                            ))]),
                        );
                        continue;
                    }
                    None => Err(vec![MSG_INVALID_INT.to_owned()]),
                },
                Ok(IntegerValue::TooSmall) => match min {
                    Some(lo) => {
                        errors.insert(
                            spec.name.to_owned(),
                            Value::Array(vec![Value::String(format!(
                                "Ensure this value is greater than or equal to {lo}."
                            ))]),
                        );
                        continue;
                    }
                    None => Err(vec![MSG_INVALID_INT.to_owned()]),
                },
            },
            FieldShape::Float => parse_float(&value).map(FieldValue::Float),
            FieldShape::Bool => parse_boolean(&value).map(FieldValue::Bool),
            FieldShape::Date => match &value {
                Value::String(text) => parse_drf_date(text)
                    .map(FieldValue::Date)
                    .ok_or_else(|| vec![MSG_DATE_INVALID.to_owned()]),
                _ => Err(vec![MSG_DATE_INVALID.to_owned()]),
            },
            FieldShape::DateTime => match &value {
                Value::String(text) => parse_drf_datetime(text, timezone).map(FieldValue::DateTime),
                _ => Err(vec![MSG_DATETIME_INVALID.to_owned()]),
            },
            FieldShape::UuidField => parse_uuid_field(&value).map(FieldValue::Uuid),
            FieldShape::Pk(queryset) => match parse_uuid_pk(&value) {
                Ok(id) => {
                    probes.push((spec.name, *queryset, id, value.clone()));
                    continue;
                }
                Err(messages) => Err(messages),
            },
            FieldShape::PkList(_) => match parse_uuid_list(&value) {
                Ok(ids) => Ok(FieldValue::Ids(ids)),
                // Index-keyed errors (or the `not_a_list` message) slot in
                // verbatim — NOT list-wrapped again.
                Err(errors_value) => {
                    errors.insert(spec.name.to_owned(), errors_value);
                    continue;
                }
            },
            FieldShape::Json => parse_json_value(&value).map(FieldValue::Json),
        };
        match outcome {
            Ok(field_value) => {
                // `git_work_branch` runs the model regex validator after the
                // char pass (model `validators`, `issue.py`).
                if spec.name == "git_work_branch" {
                    if let FieldValue::Text(branch) = &field_value {
                        if !branch.is_empty() && !valid_branch_name(branch) {
                            errors.insert(
                                spec.name.to_owned(),
                                Value::Array(vec![Value::String(
                                    "Branch name may contain only letters, numbers, and . _ / -"
                                        .to_owned(),
                                )]),
                            );
                            continue;
                        }
                    }
                }
                values.push((spec.name.to_owned(), field_value));
            }
            Err(messages) => {
                errors.insert(
                    spec.name.to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if !errors.is_empty() {
        return Err(ShapeFailure::Errors(errors));
    }
    // Scalar pk existence probes (`queryset.get`), in field order.
    for (name, queryset, id, original) in &probes {
        let exists = pk_exists(pool, *queryset, id)
            .await
            .map_err(|_| Denial::ServerError)?;
        if !exists {
            errors.insert(
                (*name).to_owned(),
                Value::Array(vec![Value::String(format!(
                    "Invalid pk \"{}\" - object does not exist.",
                    does_not_exist_echo(original)
                ))]),
            );
        } else {
            values.push(((*name).to_owned(), FieldValue::Uuid(*id)));
        }
    }
    // List items probe in array order (shape-clean by now; original
    // spellings feed the `does_not_exist` echoes).
    for spec in table {
        if !matches!(spec.shape, FieldShape::PkList(_)) {
            continue;
        }
        let queryset = list_queryset(&spec.shape);
        let items = match input.get(spec.name) {
            Some(Value::Array(items)) => items,
            _ => continue,
        };
        let mut item_errors = Map::new();
        for (index, item) in items.iter().enumerate() {
            // Shape-clean (the pass above accepted the whole array).
            let id = parse_uuid_pk(item).map_err(|_| Denial::ServerError)?;
            let exists = pk_exists(pool, queryset, &id)
                .await
                .map_err(|_| Denial::ServerError)?;
            if !exists {
                item_errors.insert(
                    index.to_string(),
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        does_not_exist_echo(item)
                    ))]),
                );
            }
        }
        if !item_errors.is_empty() {
            errors.insert(spec.name.to_owned(), Value::Object(item_errors));
            values.retain(|(name, _)| name != spec.name);
        }
    }
    if !errors.is_empty() {
        return Err(ShapeFailure::Errors(errors));
    }
    Ok(values)
}

fn list_queryset(shape: &FieldShape) -> PkQueryset {
    match shape {
        FieldShape::PkList(queryset) => *queryset,
        _ => unreachable!("list queryset"),
    }
}

/// The branch-name model regex (`issue.py` validators).
fn valid_branch_name(branch: &str) -> bool {
    !branch.is_empty()
        && branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
}

/// One pk existence probe (`queryset.get` over the field's queryset).
async fn pk_exists(pool: &sqlx::PgPool, queryset: PkQueryset, id: &Uuid) -> Result<bool, DbErr> {
    let db = orm(pool);
    let hit: Option<Uuid> = match queryset {
        PkQueryset::StatesAll => {
            states::Entity::find()
                .select_only()
                .column(states::Column::Id)
                .filter(states::Column::Id.eq(*id))
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::States => {
            states::Entity::find()
                .select_only()
                .column(states::Column::Id)
                .filter(states::Column::Id.eq(*id))
                .filter(states::Column::Group.ne("triage"))
                .filter(states::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Issues => {
            issues::Entity::find()
                .select_only()
                .column(issues::Column::Id)
                .filter(issues::Column::Id.eq(*id))
                .filter(issues::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::IssuesVisible => {
            issues::Entity::find()
                .select_only()
                .column(issues::Column::Id)
                .join(JoinType::LeftJoin, issues::Relation::States.def())
                .join(JoinType::InnerJoin, issues::Relation::Projects.def())
                .filter(issues::Column::Id.eq(*id))
                .filter(issues::Column::DeletedAt.is_null())
                .filter(
                    Condition::all()
                        .add(states::Column::Group.eq("triage"))
                        .add(states::Column::Group.is_not_null())
                        .not(),
                )
                .filter(issues::Column::ArchivedAt.is_null())
                .filter(projects::Column::ArchivedAt.is_null())
                .filter(issues::Column::IsDraft.eq(false))
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::PodsAll => {
            pod::Entity::find()
                .select_only()
                .column(pod::Column::Id)
                .filter(pod::Column::Id.eq(*id))
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Labels => {
            labels::Entity::find()
                .select_only()
                .column(labels::Column::Id)
                .filter(labels::Column::Id.eq(*id))
                .filter(labels::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Users => {
            users::Entity::find()
                .select_only()
                .column(users::Column::Id)
                .filter(users::Column::Id.eq(*id))
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Estimates => {
            estimate_points::Entity::find()
                .select_only()
                .column(estimate_points::Column::Id)
                .filter(estimate_points::Column::Id.eq(*id))
                .filter(estimate_points::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Types => {
            issue_types::Entity::find()
                .select_only()
                .column(issue_types::Column::Id)
                .filter(issue_types::Column::Id.eq(*id))
                .filter(issue_types::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Projects => {
            projects::Entity::find()
                .select_only()
                .column(projects::Column::Id)
                .filter(projects::Column::Id.eq(*id))
                .filter(projects::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Favorites => {
            user_favorites::Entity::find()
                .select_only()
                .column(user_favorites::Column::Id)
                .filter(user_favorites::Column::Id.eq(*id))
                .filter(user_favorites::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
    };
    Ok(hit.is_some())
}

/// Render one serializer-`errors` map (field errors in declaration order;
/// object-leg errors appended after).
fn shape_errors_response(errors: &Map<String, Value>) -> Response {
    json_bad_request(&Value::Object(errors.clone()).to_string())
}

/// `Serializer.to_internal_value` on a non-dict body: DRF's `not_a_dict`
/// message names the Python type (`None` → `NoneType`, JSON numbers split
/// int/float).
fn non_dict_response(value: &Value) -> Response {
    let detail = format!(
        "Invalid data. Expected a dictionary, but got {}.",
        json_type_name(value)
    );
    let mut errors = Map::new();
    errors.insert(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(detail)]),
    );
    shape_errors_response(&errors)
}

/// Guard one write body: it must be a JSON object (forms always are).
#[allow(clippy::result_large_err)]
fn require_dict(input: &RequestData) -> Result<(), Response> {
    if input.value.as_object().is_some() {
        Ok(())
    } else {
        Err(non_dict_response(&input.value))
    }
}

/// Guard a body the view `.get()`s BEFORE any serializer runs
/// (draft create/patch, favorite create): a list/scalar body has no `.get`
/// → `AttributeError` → the generic 500 (probed live; the serializer's
/// `not_a_dict` 400 never runs because the view blows up first).
#[allow(clippy::result_large_err)]
fn require_data_get(input: &RequestData) -> Result<(), Response> {
    if input.value.as_object().is_some() {
        Ok(())
    } else {
        Err(json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            SERVER_ERROR_BODY.to_owned(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Validated-value accessors (`validated_data.get(key)`)
// ---------------------------------------------------------------------------

fn find_value<'a>(values: &'a [(String, FieldValue)], name: &str) -> Option<&'a FieldValue> {
    values
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// `True` when the key validated (present, possibly explicit `null`).
fn has_key(values: &[(String, FieldValue)], name: &str) -> bool {
    find_value(values, name).is_some()
}

/// Text-or-null: `None` when absent OR explicit `null`.
fn opt_text(values: &[(String, FieldValue)], name: &str) -> Option<String> {
    match find_value(values, name) {
        Some(FieldValue::Text(text)) => Some(text.clone()),
        _ => None,
    }
}

fn opt_uuid(values: &[(String, FieldValue)], name: &str) -> Option<Uuid> {
    match find_value(values, name) {
        Some(FieldValue::Uuid(id)) => Some(*id),
        _ => None,
    }
}

fn nullable_uuid(values: &[(String, FieldValue)], name: &str) -> Option<Option<Uuid>> {
    match find_value(values, name) {
        None => None,
        Some(FieldValue::Null) => Some(None),
        Some(FieldValue::Uuid(id)) => Some(Some(*id)),
        Some(_) => unreachable!("uuid field {name}"),
    }
}

fn opt_int(values: &[(String, FieldValue)], name: &str) -> Option<i64> {
    match find_value(values, name) {
        Some(FieldValue::Int(int)) => Some(*int),
        _ => None,
    }
}

fn opt_float(values: &[(String, FieldValue)], name: &str) -> Option<f64> {
    match find_value(values, name) {
        Some(FieldValue::Float(float)) => Some(*float),
        _ => None,
    }
}

fn opt_bool(values: &[(String, FieldValue)], name: &str) -> Option<bool> {
    match find_value(values, name) {
        Some(FieldValue::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

fn opt_date(values: &[(String, FieldValue)], name: &str) -> Option<NaiveDate> {
    match find_value(values, name) {
        Some(FieldValue::Date(date)) => Some(*date),
        _ => None,
    }
}

fn nullable_date(values: &[(String, FieldValue)], name: &str) -> Option<Option<NaiveDate>> {
    match find_value(values, name) {
        None => None,
        Some(FieldValue::Null) => Some(None),
        Some(FieldValue::Date(date)) => Some(Some(*date)),
        Some(_) => unreachable!("date field {name}"),
    }
}

fn opt_datetime(values: &[(String, FieldValue)], name: &str) -> Option<DateTime<Utc>> {
    match find_value(values, name) {
        Some(FieldValue::DateTime(moment)) => Some(*moment),
        _ => None,
    }
}

fn opt_json(values: &[(String, FieldValue)], name: &str) -> Option<Value> {
    match find_value(values, name) {
        Some(FieldValue::Json(value)) => Some(value.clone()),
        _ => None,
    }
}

/// Dual-source pk resolution (`state_id`→`state`, `parent_id`→`parent`):
/// the auto field runs LATER and wins on conflict — even an explicit `null`
/// there overwrites a `state_id` value. Returns absent/null/value.
fn dual_uuid(values: &[(String, FieldValue)], declared: &str, auto: &str) -> Option<Option<Uuid>> {
    if has_key(values, auto) {
        nullable_uuid(values, auto)
    } else if has_key(values, declared) {
        nullable_uuid(values, declared)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Shared row probes (`state` group, pod fetch)
// ---------------------------------------------------------------------------

/// One `states` row's group + project, save-path scope: the value Django
/// reads off the field-cached instance or the `_base_manager` (plain,
/// UNSCOPED — soft-deleted rows resolve; verified in Django 4.2's
/// `ForwardManyToOneDescriptor.get_queryset`).
struct StateRow {
    group: String,
}

async fn state_row(pool: &sqlx::PgPool, id: &Uuid) -> Result<Option<StateRow>, Denial> {
    let row: Option<String> = states::Entity::find()
        .select_only()
        .column(states::Column::Group)
        .filter(states::Column::Id.eq(*id))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|group| StateRow { group }))
}

/// One `projects` row's workspace + default assignee, save-path scope:
/// Django's `self.project` reads go through `_base_manager` (plain,
/// UNSCOPED — a soft-deleted project still re-points `workspace`).
struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
    default_assignee_id: Option<Uuid>,
}

async fn project_row(pool: &sqlx::PgPool, id: &Uuid) -> Result<Option<ProjectRow>, Denial> {
    #[derive(Debug, FromQueryResult)]
    struct ProjectRowDb {
        id: Uuid,
        workspace_id: Uuid,
        default_assignee_id: Option<Uuid>,
    }
    let row: Option<ProjectRowDb> = projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::WorkspaceId,
            projects::Column::DefaultAssigneeId,
        ])
        .filter(projects::Column::Id.eq(*id))
        .into_model()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| ProjectRow {
        id: row.id,
        workspace_id: row.workspace_id,
        default_assignee_id: row.default_assignee_id,
    }))
}

/// A JSON float: CPython `repr` for finite values (`1e+16`, `65535.0`),
/// `Infinity`/`-Infinity`/`NaN` tokens for the non-finite ones (`json.dumps`
/// emits the tokens; `serde_json` would mistranslate them to `null` — and its
/// exponent spelling (`1e16`) differs too, so floats NEVER render via serde).
fn json_float_str(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        };
    }
    py_float_str(value)
}

// ---------------------------------------------------------------------------
// Favorites (favorite.py:20-97, W32-W34)
// ---------------------------------------------------------------------------

/// One `user_favorites` row's rendered columns (owned text for the hand
/// assembler — see `render_favorite`).
pub(crate) struct FavoriteRow {
    id: String,
    pub(crate) entity_type: String,
    pub(crate) entity_identifier: Option<String>,
    name: Option<String>,
    is_folder: bool,
    sequence: f64,
    parent: Option<String>,
    workspace_id: String,
    project_id: Option<String>,
}

/// The SeaORM decode of one favorite row (rendered columns only).
#[derive(Debug, FromQueryResult)]
struct FavoriteDbRow {
    id: Uuid,
    entity_type: String,
    entity_identifier: Option<Uuid>,
    name: Option<String>,
    is_folder: bool,
    sequence: f64,
    parent_id: Option<Uuid>,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
}

impl From<FavoriteDbRow> for FavoriteRow {
    fn from(row: FavoriteDbRow) -> Self {
        FavoriteRow {
            id: row.id.to_string(),
            entity_type: row.entity_type,
            entity_identifier: row.entity_identifier.map(|id| id.to_string()),
            name: row.name,
            is_folder: row.is_folder,
            sequence: row.sequence,
            parent: row.parent_id.map(|id| id.to_string()),
            workspace_id: row.workspace_id.to_string(),
            project_id: row.project_id.map(|id| id.to_string()),
        }
    }
}

/// The rendered favorite columns over the ported scope (`favorite_list_sql`
/// without the `*`).
fn favorites_select(slug: &str, user_id: Uuid, parent: Option<Uuid>, top: bool) -> SelectStatement {
    let mut stmt = Query::select();
    stmt.columns([
        (a("user_favorites"), user_favorites::Column::Id),
        (a("user_favorites"), user_favorites::Column::EntityType),
        (
            a("user_favorites"),
            user_favorites::Column::EntityIdentifier,
        ),
        (a("user_favorites"), user_favorites::Column::Name),
        (a("user_favorites"), user_favorites::Column::IsFolder),
        (a("user_favorites"), user_favorites::Column::Sequence),
        (a("user_favorites"), user_favorites::Column::ParentId),
        (a("user_favorites"), user_favorites::Column::WorkspaceId),
        (a("user_favorites"), user_favorites::Column::ProjectId),
    ])
    .from_as(user_favorites::Entity, a("user_favorites"));
    if top {
        stmt.and_where(col("user_favorites", user_favorites::Column::UserId).eq(user_id))
            .and_where(
                col("user_favorites", user_favorites::Column::WorkspaceId)
                    .binary(BinOper::Equal, workspace_id_sub(slug)),
            )
            .and_where(col("user_favorites", user_favorites::Column::ParentId).is_null())
            .and_where(col("user_favorites", user_favorites::Column::DeletedAt).is_null());
    } else {
        stmt.and_where(col("user_favorites", user_favorites::Column::UserId).eq(user_id))
            .and_where(
                col("user_favorites", user_favorites::Column::WorkspaceId)
                    .binary(BinOper::Equal, workspace_id_sub(slug)),
            )
            .and_where(
                col("user_favorites", user_favorites::Column::ParentId)
                    .eq(parent.expect("group parent")),
            )
            .and_where(col("user_favorites", user_favorites::Column::DeletedAt).is_null());
    }
    // Branch: top-level is `(project-null AND NOT page) OR (project-member)`;
    // the group is `project-null OR (project-member)`.
    let mut member_probe = Query::select();
    member_probe
        .expr(Expr::val(1))
        .from_as(project_members::Entity, a("pm"))
        .and_where(
            col("pm", project_members::Column::ProjectId)
                .equals((a("user_favorites"), user_favorites::Column::ProjectId)),
        )
        .and_where(col("pm", project_members::Column::MemberId).eq(user_id))
        .and_where(col("pm", project_members::Column::IsActive).eq(true));
    let member = col("user_favorites", user_favorites::Column::ProjectId)
        .is_not_null()
        .and(Expr::exists(member_probe));
    if top {
        let escaped = col("user_favorites", user_favorites::Column::EntityType).ne("page");
        let left = col("user_favorites", user_favorites::Column::ProjectId)
            .is_null()
            .and(escaped);
        stmt.and_where(left.or(member));
    } else {
        stmt.and_where(
            col("user_favorites", user_favorites::Column::ProjectId)
                .is_null()
                .or(member),
        );
    }
    stmt.order_by(
        (a("user_favorites"), user_favorites::Column::CreatedAt),
        Order::Desc,
    );
    stmt
}

/// Render one favorite row in `USER_FAVORITE_WIRE_FIELDS` order. The
/// `sequence` float renders via [`json_float_str`] (never serde); the
/// `entity_data` sub-object comes from the merged lite ports.
pub(crate) fn render_favorite(row: &FavoriteRow, entity: Option<&Value>) -> String {
    let opt = |value: &Option<String>| match value {
        Some(text) => json_string(text),
        None => "null".to_owned(),
    };
    let entity_text = match entity {
        Some(value) => value.to_string(),
        None => "null".to_owned(),
    };
    format!(
        "{{\"id\":{},\"entity_type\":{},\"entity_identifier\":{},\"entity_data\":{},\"name\":{},\"is_folder\":{},\"sequence\":{},\"parent\":{},\"workspace_id\":{},\"project_id\":{}}}",
        json_string(&row.id),
        json_string(&row.entity_type),
        opt(&row.entity_identifier),
        entity_text,
        opt(&row.name),
        if row.is_folder { "true" } else { "false" },
        json_float_str(row.sequence),
        opt(&row.parent),
        json_string(&row.workspace_id),
        opt(&row.project_id),
    )
}

/// One entity row's lite columns.
struct EntityLiteRow {
    id: String,
    name: String,
    logo_props: Value,
    project_id: Option<String>,
}

async fn fetch_entity_lite(
    pool: &sqlx::PgPool,
    table: &str,
    id: &Uuid,
    with_project: bool,
) -> Result<Option<EntityLiteRow>, Denial> {
    // Only cycles/modules/views carry a `project_id` column — projects
    // are the top (the lite omits it) and pages link projects M2M (the
    // serializer reads `.first()`).
    let db = orm(pool);
    #[derive(Debug, FromQueryResult)]
    struct LiteNoProject {
        id: Uuid,
        name: String,
        logo_props: Value,
    }
    #[derive(Debug, FromQueryResult)]
    struct LiteWithProject {
        id: Uuid,
        name: String,
        logo_props: Value,
        project_id: Option<Uuid>,
    }
    #[derive(Debug, FromQueryResult)]
    struct CycleLite {
        id: Uuid,
        name: String,
        logo_props: Value,
        project_id: Uuid,
    }
    let row = match table {
        "projects" => {
            let found: Option<LiteNoProject> = projects::Entity::find()
                .select_only()
                .columns([
                    projects::Column::Id,
                    projects::Column::Name,
                    projects::Column::LogoProps,
                ])
                .filter(projects::Column::Id.eq(*id))
                .filter(projects::Column::DeletedAt.is_null())
                .into_model()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            found.map(|row| EntityLiteRow {
                id: row.id.to_string(),
                name: row.name,
                logo_props: row.logo_props,
                project_id: None,
            })
        }
        "pages" => {
            let found: Option<LiteNoProject> = pages::Entity::find()
                .select_only()
                .columns([
                    pages::Column::Id,
                    pages::Column::Name,
                    pages::Column::LogoProps,
                ])
                .filter(pages::Column::Id.eq(*id))
                .filter(pages::Column::DeletedAt.is_null())
                .into_model()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            found.map(|row| EntityLiteRow {
                id: row.id.to_string(),
                name: row.name,
                logo_props: row.logo_props,
                project_id: None,
            })
        }
        "cycles" => {
            let found: Option<CycleLite> = cycles::Entity::find()
                .select_only()
                .columns([
                    cycles::Column::Id,
                    cycles::Column::Name,
                    cycles::Column::LogoProps,
                    cycles::Column::ProjectId,
                ])
                .filter(cycles::Column::Id.eq(*id))
                .filter(cycles::Column::DeletedAt.is_null())
                .into_model()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            found.map(|row| EntityLiteRow {
                id: row.id.to_string(),
                name: row.name,
                logo_props: row.logo_props,
                project_id: Some(row.project_id.to_string()),
            })
        }
        "modules" => {
            let found: Option<CycleLite> = modules::Entity::find()
                .select_only()
                .columns([
                    modules::Column::Id,
                    modules::Column::Name,
                    modules::Column::LogoProps,
                    modules::Column::ProjectId,
                ])
                .filter(modules::Column::Id.eq(*id))
                .filter(modules::Column::DeletedAt.is_null())
                .into_model()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            found.map(|row| EntityLiteRow {
                id: row.id.to_string(),
                name: row.name,
                logo_props: row.logo_props,
                project_id: Some(row.project_id.to_string()),
            })
        }
        "issue_views" => {
            let found: Option<LiteWithProject> = issue_views::Entity::find()
                .select_only()
                .columns([
                    issue_views::Column::Id,
                    issue_views::Column::Name,
                    issue_views::Column::LogoProps,
                    issue_views::Column::ProjectId,
                ])
                .filter(issue_views::Column::Id.eq(*id))
                .filter(issue_views::Column::DeletedAt.is_null())
                .into_model()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            found.map(|row| EntityLiteRow {
                id: row.id.to_string(),
                name: row.name,
                logo_props: row.logo_props,
                project_id: row.project_id.map(|id| id.to_string()),
            })
        }
        _ => unreachable!("entity table {table}"),
    };
    let _ = with_project;
    Ok(row)
}

/// `Page.projects.first()`: the page's projects, `-created_at` first, over
/// the live-`Project` manager (the through rows carry no deletion filter —
/// the M2M manager only scopes the target model).
async fn page_first_project(pool: &sqlx::PgPool, page_id: &Uuid) -> Result<Option<String>, Denial> {
    #[derive(Debug, FromQueryResult)]
    struct ProjectIdRow {
        id: Uuid,
    }
    let mut stmt = Query::select();
    stmt.column((a("p"), projects::Column::Id))
        .from_as(project_pages::Entity, a("pp"))
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            a("p"),
            col("p", projects::Column::Id).equals((a("pp"), project_pages::Column::ProjectId)),
        )
        .and_where(col("pp", project_pages::Column::PageId).eq(*page_id))
        .and_where(col("p", projects::Column::DeletedAt).is_null())
        .order_by((a("p"), projects::Column::CreatedAt), Order::Desc)
        .limit(1);
    let rows = query_built::<ProjectIdRow>(&orm(pool), &stmt).await?;
    Ok(rows.into_iter().next().map(|row| row.id.to_string()))
}

/// `get_entity_data` (`favorite.py:83-88`): `None` for `issue`/`folder`/
/// unknown types, for a `NULL` identifier, and for a missed fetch; else the
/// merged lite port of the live entity row.
pub(crate) async fn fetch_favorite_entity(
    pool: &sqlx::PgPool,
    entity_type: &str,
    entity_identifier: Option<&str>,
) -> Result<Option<Value>, Denial> {
    let kind = match fav::favorite_entity_kind(entity_type) {
        Some(kind) => kind,
        // `issue` (serializer `None`), `folder`, and unknown types never
        // fetch (`favorite.py:46-56`, `:83-88`).
        None => return Ok(None),
    };
    let Some(identifier) = entity_identifier else {
        // `.get(pk=None)` matches nothing (`IS NULL` over a non-null pk).
        return Ok(None);
    };
    let id = Uuid::parse_str(identifier).map_err(|_| Denial::ServerError)?;
    match kind {
        fav::FavoriteEntityKind::Project => {
            let row = fetch_entity_lite(pool, "projects", &id, false).await?;
            Ok(row.map(|row| {
                serde_json::to_value(fav::project_favorite_lite_to_representation(
                    &fav::ProjectFavoriteLiteRow {
                        id: &row.id,
                        name: &row.name,
                        logo_props: &row.logo_props,
                    },
                ))
                .expect("lite json")
            }))
        }
        fav::FavoriteEntityKind::Page => {
            let row = fetch_entity_lite(pool, "pages", &id, false).await?;
            match row {
                None => Ok(None),
                Some(row) => {
                    let project_id = page_first_project(pool, &id).await?;
                    Ok(Some(
                        serde_json::to_value(fav::page_favorite_lite_to_representation(
                            &fav::PageFavoriteLiteRow {
                                id: &row.id,
                                name: &row.name,
                                logo_props: &row.logo_props,
                                project_id: project_id.as_deref(),
                            },
                        ))
                        .expect("lite json"),
                    ))
                }
            }
        }
        fav::FavoriteEntityKind::Cycle => {
            let row = fetch_entity_lite(pool, "cycles", &id, true).await?;
            Ok(row.map(|row| {
                serde_json::to_value(fav::cycle_favorite_lite_to_representation(
                    &fav::CycleFavoriteLiteRow {
                        id: &row.id,
                        name: &row.name,
                        logo_props: &row.logo_props,
                        project_id: row.project_id.as_deref(),
                    },
                ))
                .expect("lite json")
            }))
        }
        fav::FavoriteEntityKind::Module => {
            let row = fetch_entity_lite(pool, "modules", &id, true).await?;
            Ok(row.map(|row| {
                serde_json::to_value(fav::module_favorite_lite_to_representation(
                    &fav::ModuleFavoriteLiteRow {
                        id: &row.id,
                        name: &row.name,
                        logo_props: &row.logo_props,
                        project_id: row.project_id.as_deref(),
                    },
                ))
                .expect("lite json")
            }))
        }
        fav::FavoriteEntityKind::View => {
            let row = fetch_entity_lite(pool, "issue_views", &id, true).await?;
            Ok(row.map(|row| {
                serde_json::to_value(fav::view_favorite_to_representation(
                    &fav::ViewFavoriteRow {
                        id: &row.id,
                        name: &row.name,
                        logo_props: &row.logo_props,
                        project_id: row.project_id.as_deref(),
                    },
                ))
                .expect("lite json")
            }))
        }
    }
}

/// W32 GET (`favorite.py:23-35`): ADMIN+MEMBER gate, then scope + branch +
/// default order, each row rendered with its entity.
async fn favorite_list(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path(slug): axum::extract::Path<String>,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    let rows =
        query_built::<FavoriteDbRow>(&orm(&pool), &favorites_select(&slug, user_id, None, true))
            .await?;
    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let favorite = FavoriteRow::from(row);
        let entity = fetch_favorite_entity(
            &pool,
            &favorite.entity_type,
            favorite.entity_identifier.as_deref(),
        )
        .await?;
        rendered.push(render_favorite(&favorite, entity.as_ref()));
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", rendered.join(",")),
    ))
}

// ---------------------------------------------------------------------------
// Draft list filters (`issue_filters(query_params, "GET")` over DraftIssue)
// ---------------------------------------------------------------------------

/// Dispatcher order (`issue_filters.py:437-465`): presence-based, each filter
/// inserts its predicates; shared keys keep FIRST position with the LATER
/// value (`updated_at` overwrites `created_at`'s `created_at__date__*`;
/// `inbox_status` overwrites `intake_status`'s `issue_intake__status__in`).
/// `.filter(**filters)` then builds predicates in final-dict order and the
/// FIRST failure wins: `FieldError` → generic 500, `ValidationError` → 400
/// `{"error": "Please provide valid detail"}`.
const FILTER_DISPATCH_ORDER: &[&str] = &[
    "state",
    "state_group",
    "estimate_point",
    "priority",
    "parent",
    "labels",
    "assignees",
    "mentions",
    "created_by",
    "logged_by",
    "name",
    "created_at",
    "updated_at",
    "start_date",
    "target_date",
    "completed_at",
    "type",
    "project",
    "cycle",
    "module",
    "intake_status",
    "inbox_status",
    "sub_issue",
    "subscriber",
    "start_target_date",
];

/// One compiled predicate: a SeaORM condition, or the failure Django
/// raises while building it (`true` = `FieldError`/500, `false` =
/// `ValidationError`/400).
enum FilterPredicate {
    Cond(SimpleExpr),
    Fail(bool),
}

fn draft_col(column: draft_issues::Column) -> Expr {
    col("draft_issues", column)
}

fn draft_exists_states(groups: &[String]) -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(Expr::val(1))
        .from_as(states::Entity, a("s"))
        .and_where(
            col("s", states::Column::Id).equals((a("draft_issues"), draft_issues::Column::StateId)),
        )
        .and_where(col("s", states::Column::Group).is_in(groups.iter().cloned()));
    Expr::exists(stmt)
}

fn draft_zoned_date(column: draft_issues::Column, timezone: &Tz) -> SimpleExpr {
    draft_col(column)
        .binary(BinOper::Custom("AT TIME ZONE"), Expr::val(timezone.name()))
        .cast_as(a("date"))
}

fn draft_date_term(term: &str, timezone: &Tz) -> SimpleExpr {
    match term {
        "created_at__date" => draft_zoned_date(draft_issues::Column::CreatedAt, timezone),
        "completed_at__date" => draft_zoned_date(draft_issues::Column::CompletedAt, timezone),
        "start_date" => draft_col(draft_issues::Column::StartDate).into(),
        "target_date" => draft_col(draft_issues::Column::TargetDate).into(),
        other => unreachable!("date term {other}"),
    }
}

fn draft_date_bound(left: SimpleExpr, gte: bool, date: NaiveDate) -> SimpleExpr {
    let right = Expr::val(date.to_string()).cast_as(a("date"));
    if gte {
        left.gte(right)
    } else {
        left.lte(right)
    }
}

fn draft_like_pattern(text: &str) -> String {
    format!("%{}%", escape_like(text))
}

/// Compile the legacy filters for the draft list: `Ok(condition)` appends to
/// the window (and count) query; `Err(500)` on the first `FieldError`
/// predicate; `Err(400)` on the first `ValidationError` predicate.
fn compile_draft_filters(
    pairs: &[(String, String)],
    now: &DateTime<Utc>,
    timezone: &Tz,
) -> Result<Condition, StatusCode> {
    // Ordered map: key → predicate (later assignments overwrite the value,
    // keeping the first position).
    let mut predicates: Vec<(String, FilterPredicate)> = Vec::new();
    let mut insert = |key: String, predicate: FilterPredicate| {
        if let Some(slot) = predicates.iter_mut().find(|(name, _)| *name == key) {
            slot.1 = predicate;
        } else {
            predicates.push((key, predicate));
        }
    };
    for key in FILTER_DISPATCH_ORDER {
        let Some(raw) = query_get(pairs, key) else {
            continue;
        };
        match *key {
            "state" => {
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "state__in".to_owned(),
                        FilterPredicate::Cond(draft_col(draft_issues::Column::StateId).is_in(ids)),
                    );
                }
            }
            "state_group" => {
                let groups = raw_list(raw);
                if !groups.is_empty() {
                    let groups: Vec<String> =
                        groups.iter().map(|group| (*group).to_owned()).collect();
                    insert(
                        "state__group__in".to_owned(),
                        FilterPredicate::Cond(draft_exists_states(&groups)),
                    );
                }
            }
            "estimate_point" => {
                let points = raw_list(raw);
                if !points.is_empty() {
                    // Raw strings, converted at `filter()` time: an
                    // unparseable id is `ValidationError` → 400.
                    let mut ids = Vec::with_capacity(points.len());
                    let mut failed = false;
                    for point in &points {
                        match Uuid::parse_str(point) {
                            Ok(id) => ids.push(id),
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        }
                    }
                    if failed {
                        insert(
                            "estimate_point__in".to_owned(),
                            FilterPredicate::Fail(false),
                        );
                    } else {
                        insert(
                            "estimate_point__in".to_owned(),
                            FilterPredicate::Cond(
                                draft_col(draft_issues::Column::EstimatePointId).is_in(ids),
                            ),
                        );
                    }
                }
            }
            "priority" => {
                let priorities = raw_list(raw);
                if !priorities.is_empty() {
                    let priorities: Vec<String> = priorities
                        .iter()
                        .map(|priority| (*priority).to_owned())
                        .collect();
                    insert(
                        "priority__in".to_owned(),
                        FilterPredicate::Cond(
                            draft_col(draft_issues::Column::Priority).is_in(priorities),
                        ),
                    );
                }
            }
            "parent" => {
                let items: Vec<&str> = raw.split(',').filter(|item| *item != "null").collect();
                if items.contains(&"None") {
                    insert(
                        "parent__isnull".to_owned(),
                        FilterPredicate::Cond(draft_col(draft_issues::Column::ParentId).is_null()),
                    );
                }
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "parent__in".to_owned(),
                        FilterPredicate::Cond(draft_col(draft_issues::Column::ParentId).is_in(ids)),
                    );
                }
            }
            // The `label_issue` / `issue_assignee` guards are UNCONDITIONAL
            // (`filter_labels` / `filter_assignees` append outside the GET
            // branch): relations that do not exist on `DraftIssue` →
            // `FieldError` → 500 whenever the key is present.
            "labels" => {
                insert(
                    "label_issue__deleted_at__isnull".to_owned(),
                    FilterPredicate::Fail(true),
                );
            }
            "assignees" => {
                insert(
                    "issue_assignee__deleted_at__isnull".to_owned(),
                    FilterPredicate::Fail(true),
                );
            }
            "mentions" => {
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "issue_mention__mention__id__in".to_owned(),
                        FilterPredicate::Fail(true),
                    );
                }
            }
            "created_by" => {
                let items: Vec<&str> = raw.split(',').filter(|item| *item != "null").collect();
                if items.contains(&"None") {
                    insert(
                        "created_by__isnull".to_owned(),
                        FilterPredicate::Cond(
                            draft_col(draft_issues::Column::CreatedById).is_null(),
                        ),
                    );
                }
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "created_by__in".to_owned(),
                        FilterPredicate::Cond(
                            draft_col(draft_issues::Column::CreatedById).is_in(ids),
                        ),
                    );
                }
            }
            "logged_by" => {
                let items: Vec<&str> = raw.split(',').filter(|item| *item != "null").collect();
                if items.contains(&"None") {
                    insert("logged_by__isnull".to_owned(), FilterPredicate::Fail(true));
                }
                if !valid_uuid_list(raw).is_empty() {
                    insert("logged_by__in".to_owned(), FilterPredicate::Fail(true));
                }
            }
            "name" => {
                if !raw.is_empty() {
                    insert(
                        "name__icontains".to_owned(),
                        FilterPredicate::Cond(
                            draft_col(draft_issues::Column::Name).ilike(draft_like_pattern(raw)),
                        ),
                    );
                }
            }
            "created_at" => compile_date_param(&mut insert, "created_at__date", raw, now, timezone),
            // BUG: the `updated_at` param filters `created_at`
            // (`issue_filters.py:236`), overwriting that param's keys.
            "updated_at" => compile_date_param(&mut insert, "created_at__date", raw, now, timezone),
            "start_date" => compile_date_param(&mut insert, "start_date", raw, now, timezone),
            "target_date" => compile_date_param(&mut insert, "target_date", raw, now, timezone),
            "completed_at" => {
                compile_date_param(&mut insert, "completed_at__date", raw, now, timezone)
            }
            "type" => {
                let groups: &[&str] = match raw {
                    "backlog" => &["backlog"],
                    "active" => &["unstarted", "started", "review", "test"],
                    _ => &[
                        "backlog",
                        "unstarted",
                        "started",
                        "review",
                        "test",
                        "completed",
                        "cancelled",
                    ],
                };
                let groups: Vec<String> = groups.iter().map(|group| (*group).to_owned()).collect();
                insert(
                    "state__group__in".to_owned(),
                    FilterPredicate::Cond(draft_exists_states(&groups)),
                );
            }
            "project" => {
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "project__in".to_owned(),
                        FilterPredicate::Cond(
                            draft_col(draft_issues::Column::ProjectId).is_in(ids),
                        ),
                    );
                }
            }
            // `issue_cycle__deleted_at__isnull` is unconditional (same shape
            // as labels): presence alone 500s.
            "cycle" => {
                insert(
                    "issue_cycle__deleted_at__isnull".to_owned(),
                    FilterPredicate::Fail(true),
                );
            }
            "module" => {
                insert(
                    "issue_module__deleted_at__isnull".to_owned(),
                    FilterPredicate::Fail(true),
                );
            }
            "intake_status" => {
                let statuses = raw_list(raw);
                if !statuses.is_empty() {
                    insert(
                        "issue_intake__status__in".to_owned(),
                        FilterPredicate::Fail(true),
                    );
                }
            }
            "inbox_status" => {
                let statuses = raw_list(raw);
                if !statuses.is_empty() {
                    insert(
                        "issue_intake__status__in".to_owned(),
                        FilterPredicate::Fail(true),
                    );
                }
            }
            "sub_issue" => {
                if raw == "false" {
                    insert(
                        "parent__isnull".to_owned(),
                        FilterPredicate::Cond(draft_col(draft_issues::Column::ParentId).is_null()),
                    );
                }
            }
            "subscriber" => {
                insert(
                    "issue_subscribers__deleted_at__isnull".to_owned(),
                    FilterPredicate::Fail(true),
                );
            }
            "start_target_date" => {
                if raw == "true" {
                    insert(
                        "target_date__isnull".to_owned(),
                        FilterPredicate::Cond(
                            draft_col(draft_issues::Column::TargetDate).is_not_null(),
                        ),
                    );
                    insert(
                        "start_date__isnull".to_owned(),
                        FilterPredicate::Cond(
                            draft_col(draft_issues::Column::StartDate).is_not_null(),
                        ),
                    );
                }
            }
            _ => unreachable!("dispatcher key {key}"),
        }
    }
    let mut condition = Condition::all();
    for (_, predicate) in &predicates {
        match predicate {
            FilterPredicate::Cond(expr) => condition = condition.add(expr.clone()),
            FilterPredicate::Fail(field_error) => {
                return Err(if *field_error {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    StatusCode::BAD_REQUEST
                });
            }
        }
    }
    Ok(condition)
}

/// `filter_valid_uuids` over a comma list minus `"null"`: `uuid.UUID(item)`
/// leniency (braces/`urn:`/dashless accepted), failures silently dropped.
fn valid_uuid_list(raw: &str) -> Vec<Uuid> {
    raw.split(',')
        .filter(|item| *item != "null")
        .filter_map(|item| Uuid::parse_str(item).ok())
        .collect()
}

/// Raw-string comma lists minus `"null"`: the filter applies only when the
/// list is non-empty AND holds no `""`.
fn raw_list(raw: &str) -> Vec<&str> {
    let items: Vec<&str> = raw.split(',').filter(|item| *item != "null").collect();
    if items.is_empty() || items.contains(&"") {
        Vec::new()
    } else {
        items
    }
}

/// Escape LIKE specials for `__contains`/`__icontains` (backslash escape).
fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// One date param's `date_filter` (`issue_filters.py:52-81`): comma queries
/// (NO `"null"` removal), skipped wholesale when any is `""`; each query's
/// `;`-parts route to relative / `__gte` / `__lte` / `__contains`; later
/// queries overwrite earlier keys. `__gte`/`__lte` values convert as DATES
/// (fail → 400); `__contains` is a LIKE (never fails).
///
/// The `__date` terms truncate in the actor zone: Django's `__date` lookup
/// is `TruncDate`, whose `get_tzname()` returns the `TimezoneMixin`-activated
/// user zone, giving `(col AT TIME ZONE '<user tz>')::date`. The plain
/// `start_date` / `target_date` terms are date columns (no truncation), and
/// the relative-date bound (`today`) stays UTC (`timezone.now().date()`).
fn compile_date_param(
    insert: &mut impl FnMut(String, FilterPredicate),
    term: &str,
    raw: &str,
    now: &DateTime<Utc>,
    timezone: &Tz,
) {
    let queries: Vec<&str> = raw.split(',').collect();
    if queries.is_empty() || queries.contains(&"") {
        return;
    }
    let column = || draft_date_term(term, timezone);
    for query in queries {
        let parts: Vec<&str> = query.split(';').collect();
        if parts.len() >= 2 {
            if is_relative_query(parts[0]) {
                if parts.len() == 3 {
                    // `N_weeks|months;after|before;fromnow|…` → one computed
                    // date bound (`months` = 30 days, `now` = UTC today).
                    let (duration, unit) = parts[0].split_once('_').expect("relative shape");
                    let duration: i64 = duration.parse().unwrap_or(0);
                    let days = if unit == "months" {
                        duration * 30
                    } else {
                        duration * 7
                    };
                    let today = now.date_naive();
                    // `subsequent == "after"` picks `__gte` (else `__lte`);
                    // `offset == "fromnow"` picks `now + duration` (else `-`).
                    let gte = parts[1] == "after";
                    let bound = if parts[2] == "fromnow" {
                        today + chrono::Days::new(days.max(0) as u64)
                    } else {
                        today - chrono::Days::new(days.max(0) as u64)
                    };
                    let key = format!("{term}__{}", if gte { "gte" } else { "lte" });
                    insert(
                        key,
                        FilterPredicate::Cond(draft_date_bound(column(), gte, bound)),
                    );
                }
                // A relative head with != 3 parts writes NOTHING.
            } else if parts.contains(&"after") {
                match parse_drf_date(parts[0]) {
                    Some(date) => insert(
                        format!("{term}__gte"),
                        FilterPredicate::Cond(draft_date_bound(column(), true, date)),
                    ),
                    None => insert(format!("{term}__gte"), FilterPredicate::Fail(false)),
                }
            } else {
                match parse_drf_date(parts[0]) {
                    Some(date) => insert(
                        format!("{term}__lte"),
                        FilterPredicate::Cond(draft_date_bound(column(), false, date)),
                    ),
                    None => insert(format!("{term}__lte"), FilterPredicate::Fail(false)),
                }
            }
        } else {
            insert(
                format!("{term}__contains"),
                FilterPredicate::Cond(
                    column()
                        .cast_as(a("text"))
                        .like(draft_like_pattern(parts[0])),
                ),
            );
        }
    }
}

/// One draft row's list/detail columns (owned for the assemblers).
#[derive(Clone)]
struct DraftRow {
    id: String,
    name: Option<String>,
    state_id: Option<String>,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    estimate_point: Option<String>,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    project_id: Option<String>,
    parent_id: Option<String>,
    cycle_id: Option<String>,
    module_ids: Vec<String>,
    label_ids: Vec<String>,
    assignee_ids: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<String>,
    updated_by: Option<String>,
    type_id: Option<String>,
    description_html: String,
}

fn uuid_opt_text(value: Option<Uuid>) -> Option<String> {
    value.map(|id| id.to_string())
}

/// The SeaORM decode of one draft row (list/detail columns + the four
/// annotations).
#[derive(Debug, FromQueryResult)]
struct DraftDbRow {
    id: Uuid,
    name: Option<String>,
    state_id: Option<Uuid>,
    sort_order: f64,
    completed_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    estimate_point_id: Option<Uuid>,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    project_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    cycle_id: Option<Uuid>,
    module_ids: Vec<Uuid>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    created_at: chrono::DateTime<chrono::FixedOffset>,
    updated_at: chrono::DateTime<chrono::FixedOffset>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    type_id: Option<Uuid>,
    description_html: String,
}

fn draft_from_db(row: DraftDbRow) -> DraftRow {
    DraftRow {
        id: row.id.to_string(),
        name: row.name,
        state_id: uuid_opt_text(row.state_id),
        sort_order: row.sort_order,
        completed_at: row.completed_at.map(|moment| moment.with_timezone(&Utc)),
        estimate_point: uuid_opt_text(row.estimate_point_id),
        priority: row.priority,
        start_date: row.start_date,
        target_date: row.target_date,
        project_id: uuid_opt_text(row.project_id),
        parent_id: uuid_opt_text(row.parent_id),
        cycle_id: uuid_opt_text(row.cycle_id),
        module_ids: row.module_ids.iter().map(ToString::to_string).collect(),
        label_ids: row.label_ids.iter().map(ToString::to_string).collect(),
        assignee_ids: row.assignee_ids.iter().map(ToString::to_string).collect(),
        created_at: row.created_at.with_timezone(&Utc),
        updated_at: row.updated_at.with_timezone(&Utc),
        created_by: uuid_opt_text(row.created_by_id),
        updated_by: uuid_opt_text(row.updated_by_id),
        type_id: uuid_opt_text(row.type_id),
        description_html: row.description_html,
    }
}

/// `COALESCE(ARRAY_AGG(DISTINCT x), '{}'::uuid[])` (`list_engine::id_array`).
fn draft_id_array(x: Expr) -> SimpleExpr {
    Func::coalesce([
        PgFunc::array_agg_distinct(x).into(),
        Expr::val("{}").cast_as(a("uuid[]")),
    ])
    .into()
}

/// The draft cycle subquery (`draft_cycle_subquery_sql`): alive links for
/// this draft, `LIMIT 1` with NO `ORDER BY`.
fn draft_cycle_expr() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.column((a("dic"), draft_issue_cycles::Column::CycleId))
        .from_as(draft_issue_cycles::Entity, a("dic"))
        .and_where(
            col("dic", draft_issue_cycles::Column::DraftIssueId)
                .equals((a("draft_issues"), draft_issues::Column::Id)),
        )
        .and_where(col("dic", draft_issue_cycles::Column::DeletedAt).is_null())
        .limit(1);
    sub(stmt)
}

/// `label_ids` (`draft_label_ids_annotation_sql`): DISTINCT label ids over
/// alive links whose label row exists (`labels.id IS NOT NULL`).
fn draft_label_ids_expr() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(draft_id_array(col("labels", labels::Column::Id)))
        .from_as(draft_issue_labels::Entity, a("dil"))
        .join_as(
            JoinType::InnerJoin,
            labels::Entity,
            a("labels"),
            col("labels", labels::Column::Id)
                .equals((a("dil"), draft_issue_labels::Column::LabelId)),
        )
        .and_where(
            col("dil", draft_issue_labels::Column::DraftIssueId)
                .equals((a("draft_issues"), draft_issues::Column::Id)),
        )
        .and_where(col("dil", draft_issue_labels::Column::DeletedAt).is_null());
    sub(stmt)
}

/// `assignee_ids` (`draft_assignee_ids_annotation_sql`): DISTINCT assignee
/// ids over alive links where ANY active membership qualifies (no project
/// scoping, no `pm.deleted_at` guard).
fn draft_assignee_ids_expr() -> SimpleExpr {
    let mut member = Query::select();
    member
        .expr(Expr::val(1))
        .from_as(project_members::Entity, a("pm"))
        .and_where(
            col("pm", project_members::Column::MemberId).equals((a("users"), users::Column::Id)),
        )
        .and_where(col("pm", project_members::Column::IsActive).eq(true));
    let mut stmt = Query::select();
    stmt.expr(draft_id_array(col("users", users::Column::Id)))
        .from_as(draft_issue_assignees::Entity, a("dia"))
        .join_as(
            JoinType::InnerJoin,
            users::Entity,
            a("users"),
            col("users", users::Column::Id)
                .equals((a("dia"), draft_issue_assignees::Column::AssigneeId)),
        )
        .and_where(
            col("dia", draft_issue_assignees::Column::DraftIssueId)
                .equals((a("draft_issues"), draft_issues::Column::Id)),
        )
        .and_where(col("dia", draft_issue_assignees::Column::DeletedAt).is_null())
        .and_where(Expr::exists(member));
    sub(stmt)
}

/// `module_ids` (`draft_module_ids_annotation_sql`): DISTINCT module ids
/// over alive links whose module is unarchived (no `modules.deleted_at`
/// guard).
fn draft_module_ids_expr() -> SimpleExpr {
    let mut stmt = Query::select();
    stmt.expr(draft_id_array(col(
        "dim",
        draft_issue_modules::Column::ModuleId,
    )))
    .from_as(draft_issue_modules::Entity, a("dim"))
    .join_as(
        JoinType::InnerJoin,
        modules::Entity,
        a("modules"),
        col("modules", modules::Column::Id)
            .equals((a("dim"), draft_issue_modules::Column::ModuleId)),
    )
    .and_where(
        col("dim", draft_issue_modules::Column::DraftIssueId)
            .equals((a("draft_issues"), draft_issues::Column::Id)),
    )
    .and_where(col("dim", draft_issue_modules::Column::DeletedAt).is_null())
    .and_where(col("modules", modules::Column::ArchivedAt).is_null());
    sub(stmt)
}

/// The drafted read select: the 21 local columns plus the four
/// annotations, over the base scope (`draft_scope_where`). `own` adds the
/// `created_by` leg (list); `reread` pins one pk (create re-read / retrieve).
fn draft_rows_select(
    slug: &str,
    filters: Condition,
    own: Option<Uuid>,
    reread: Option<Uuid>,
    limit: u64,
    offset: u64,
    paged: bool,
) -> SelectStatement {
    let mut stmt = Query::select();
    stmt.columns([
        (a("draft_issues"), draft_issues::Column::Id),
        (a("draft_issues"), draft_issues::Column::Name),
        (a("draft_issues"), draft_issues::Column::StateId),
        (a("draft_issues"), draft_issues::Column::SortOrder),
        (a("draft_issues"), draft_issues::Column::CompletedAt),
        (a("draft_issues"), draft_issues::Column::EstimatePointId),
        (a("draft_issues"), draft_issues::Column::Priority),
        (a("draft_issues"), draft_issues::Column::StartDate),
        (a("draft_issues"), draft_issues::Column::TargetDate),
        (a("draft_issues"), draft_issues::Column::ProjectId),
        (a("draft_issues"), draft_issues::Column::ParentId),
        (a("draft_issues"), draft_issues::Column::CreatedAt),
        (a("draft_issues"), draft_issues::Column::UpdatedAt),
        (a("draft_issues"), draft_issues::Column::CreatedById),
        (a("draft_issues"), draft_issues::Column::UpdatedById),
        (a("draft_issues"), draft_issues::Column::TypeId),
        (a("draft_issues"), draft_issues::Column::DescriptionHtml),
    ])
    .expr_as(draft_cycle_expr(), a("cycle_id"))
    .expr_as(draft_module_ids_expr(), a("module_ids"))
    .expr_as(draft_label_ids_expr(), a("label_ids"))
    .expr_as(draft_assignee_ids_expr(), a("assignee_ids"))
    .from_as(draft_issues::Entity, a("draft_issues"))
    .and_where(
        col("draft_issues", draft_issues::Column::WorkspaceId)
            .binary(BinOper::Equal, workspace_id_sub(slug)),
    )
    .and_where(col("draft_issues", draft_issues::Column::DeletedAt).is_null());
    if let Some(user_id) = own {
        stmt.and_where(col("draft_issues", draft_issues::Column::CreatedById).eq(user_id));
    }
    if let Some(id) = reread {
        stmt.and_where(col("draft_issues", draft_issues::Column::Id).eq(id));
    }
    stmt.cond_where(filters);
    if paged {
        stmt.order_by(
            (a("draft_issues"), draft_issues::Column::CreatedAt),
            Order::Desc,
        )
        .limit(limit)
        .offset(offset);
    } else {
        stmt.limit(limit);
    }
    stmt
}

/// The list count: `COUNT(DISTINCT draft_issues.id)` over the same scope.
fn draft_count_select(slug: &str, filters: Condition, user_id: Uuid) -> SelectStatement {
    let mut stmt = Query::select();
    stmt.expr_as(
        Func::count_distinct(col("draft_issues", draft_issues::Column::Id)),
        a("n"),
    )
    .from_as(draft_issues::Entity, a("draft_issues"))
    .and_where(
        col("draft_issues", draft_issues::Column::WorkspaceId)
            .binary(BinOper::Equal, workspace_id_sub(slug)),
    )
    .and_where(col("draft_issues", draft_issues::Column::DeletedAt).is_null())
    .and_where(col("draft_issues", draft_issues::Column::CreatedById).eq(user_id));
    stmt.cond_where(filters);
    stmt
}

#[derive(Debug, FromQueryResult)]
struct CountDbRow {
    n: i64,
}

/// Render one draft row in `DraftIssueSerializer.Meta.fields` order
/// (`draft.py:166-189`, the same 21 keys and order the create re-read and
/// the detail serializer use): datetimes in the actor zone (DRF
/// `DateTimeField`), the float via [`json_float_str`], id arrays as UUID
/// strings.
fn render_draft(row: &DraftRow, timezone: &Tz) -> String {
    let opt = |value: &Option<String>| match value {
        Some(text) => json_string(text),
        None => "null".to_owned(),
    };
    let ids = |values: &[String]| {
        format!(
            "[{}]",
            values
                .iter()
                .map(|id| json_string(id))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let date = |value: &Option<NaiveDate>| match value {
        Some(date) => json_string(&date.format("%Y-%m-%d").to_string()),
        None => "null".to_owned(),
    };
    let moment = |value: &Option<DateTime<Utc>>| match value {
        Some(moment) => json_string(&render_drf_datetime(moment, timezone)),
        None => "null".to_owned(),
    };
    format!(
        "{{\"id\":{},\"name\":{},\"state_id\":{},\"sort_order\":{},\"completed_at\":{},\"estimate_point\":{},\"priority\":{},\"start_date\":{},\"target_date\":{},\"project_id\":{},\"parent_id\":{},\"cycle_id\":{},\"module_ids\":{},\"label_ids\":{},\"assignee_ids\":{},\"created_at\":{},\"updated_at\":{},\"created_by\":{},\"updated_by\":{},\"type_id\":{},\"description_html\":{}}}",
        json_string(&row.id),
        opt(&row.name),
        opt(&row.state_id),
        json_float_str(row.sort_order),
        moment(&row.completed_at),
        opt(&row.estimate_point),
        json_string(&row.priority),
        date(&row.start_date),
        date(&row.target_date),
        opt(&row.project_id),
        opt(&row.parent_id),
        opt(&row.cycle_id),
        ids(&row.module_ids),
        ids(&row.label_ids),
        ids(&row.assignee_ids),
        json_string(&render_drf_datetime(&row.created_at, timezone)),
        json_string(&render_drf_datetime(&row.updated_at, timezone)),
        opt(&row.created_by),
        opt(&row.updated_by),
        opt(&row.type_id),
        json_string(&row.description_html),
    )
}

/// W35 GET (`draft.py:96-111`): ADMIN+MEMBER+GUEST gate, legacy filters
/// (500/400 BEFORE pagination errors), offset page, 12-key envelope, all
/// wrapped in `gzip_page` (even denials — the decorator sits outside).
async fn draft_list(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path(slug): axum::extract::Path<String>,
    headers: HeaderMap,
    axum::extract::RawQuery(raw_query): axum::extract::RawQuery,
) -> HandlerResult {
    use crate::paginator::{
        apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor, Cursor,
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        let (parts, body) = denied.into_parts();
        let status = parts.status;
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|_| Denial::ServerError)?;
        let text = String::from_utf8(bytes.to_vec()).map_err(|_| Denial::ServerError)?;
        return Ok(gzip_body(&headers, status, text));
    }
    let pairs = query_pairs(raw_query.as_deref().unwrap_or(""));
    // The actor zone feeds the `__date` filter truncation (Django's
    // `TimezoneMixin.initial` activates it before the view runs), so it
    // resolves before the filters compile; the same value renders rows below.
    let timezone = actor_timezone(&pool, &user_id).await?;
    // `.filter(**filters)` runs BEFORE `paginate` (`:100-105`).
    let filters = match compile_draft_filters(&pairs, &utc_now_micros(), &timezone) {
        Ok(condition) => condition,
        Err(StatusCode::INTERNAL_SERVER_ERROR) => {
            return Ok(gzip_body(
                &headers,
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ))
        }
        Err(_) => {
            return Ok(gzip_body(
                &headers,
                StatusCode::BAD_REQUEST,
                INVALID_DETAIL_BODY.to_owned(),
            ))
        }
    };
    let page_denial = |error: crate::paginator::PageError| crate::app_issues::page_denial(error);
    let per_page = crate::paginator::parse_per_page(query_get(&pairs, "per_page"), 1000, 1000)
        .map_err(page_denial)?;
    let cursor = match query_get(&pairs, "cursor") {
        Some(raw) => Cursor::from_string(raw).map_err(page_denial)?,
        None => Cursor::default_for(per_page),
    };
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    // The window and count share one filter set: scope + own rows + legacy.
    // The `select_related` set is not executed (every rendered key is a
    // local column or a ported annotation); the annotations are correlated
    // subqueries (no joins, no `GROUP BY`, one statement).
    let db = orm(&pool);
    let window_limit =
        u64::try_from(window.stop - window.offset).map_err(|_| Denial::ServerError)?;
    let window_offset = u64::try_from(window.offset).map_err(|_| Denial::ServerError)?;
    let window_stmt = draft_rows_select(
        &slug,
        filters.clone(),
        Some(user_id),
        None,
        window_limit,
        window_offset,
        true,
    );
    let rows = query_built::<DraftDbRow>(&db, &window_stmt).await?;
    let has_more = rows.len() as i64 > limit;
    let decoded: Vec<DraftRow> = rows.into_iter().map(draft_from_db).collect();
    let page = apply_offset_window(&decoded, limit).map_err(page_denial)?;
    let total_count = {
        let count_stmt = draft_count_select(&slug, filters, user_id);
        let count_rows = query_built::<CountDbRow>(&db, &count_stmt).await?;
        count_rows
            .into_iter()
            .next()
            .map(|row| row.n)
            .ok_or(Denial::ServerError)?
    };
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let mut shaped = Vec::with_capacity(page.len());
    for row in &page {
        shaped.push(render_draft(row, &timezone));
    }
    let body = envelope(
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
    );
    Ok(gzip_body(&headers, StatusCode::OK, body))
}

/// The relative-date head pattern `\d+_(weeks|months)$`.
fn is_relative_query(head: &str) -> bool {
    let Some((digits, unit)) = head.split_once('_') else {
        return false;
    };
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (unit == "weeks" || unit == "months")
}

/// W32 POST (`favorite.py:37-67`): workspace get (404), entity dedupe (200
/// existing) else field validation + save (200), `IntegrityError` → 400.
async fn favorite_create(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path(slug): axum::extract::Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    // `Workspace.objects.get(slug)` runs BEFORE `request.data` is touched
    // (`favorite.py:40` vs `:43`): a missing slug 404s even for a malformed
    // body, and the `.get("entity_identifier")` on a non-dict body raises
    // `AttributeError` → the generic 500 (both probed live).
    // `Workspace.objects.get(slug=slug)` — the scoped manager 404s on a
    // missing or soft-deleted slug.
    let workspace_id: Option<Uuid> = workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .filter(workspaces::Column::Slug.eq(slug.clone()))
        .filter(workspaces::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(workspace_id) = workspace_id else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_data_get(&input) {
        return Ok(response);
    }
    // Entity dedupe (`:43-54`): a TRUTHY `entity_identifier` queries first.
    // The filter's UUID conversion raises `ValidationError` for uncoercible
    // values → 400 `{"error": "Please provide valid detail"}` BEFORE the
    // serializer ever runs (pinned by contract test).
    if let Some(identifier) = input.get("entity_identifier") {
        if json_is_truthy(identifier) {
            let entity_id = match django_coerce_uuid(identifier) {
                Some(id) => id,
                None => {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_DETAIL_BODY.to_owned(),
                    ))
                }
            };
            // `entity_type` rides the raw input (`None` when missing → the
            // `IS NULL` arm matches nothing over the non-null column);
            // composites stringify via `str()`.
            let entity_type = input.get("entity_type").map(char_filter_text);
            let dedupe_found: Option<FavoriteDbRow> = match &entity_type {
                None => None,
                Some(text) => user_favorites::Entity::find()
                    .select_only()
                    .columns([
                        user_favorites::Column::Id,
                        user_favorites::Column::EntityType,
                        user_favorites::Column::EntityIdentifier,
                        user_favorites::Column::Name,
                        user_favorites::Column::IsFolder,
                        user_favorites::Column::Sequence,
                        user_favorites::Column::ParentId,
                        user_favorites::Column::WorkspaceId,
                        user_favorites::Column::ProjectId,
                    ])
                    .filter(user_favorites::Column::WorkspaceId.eq(workspace_id))
                    .filter(user_favorites::Column::UserId.eq(user_id))
                    .filter(user_favorites::Column::EntityType.eq(text.clone()))
                    .filter(user_favorites::Column::EntityIdentifier.eq(entity_id))
                    .filter(user_favorites::Column::DeletedAt.is_null())
                    .order_by_desc(user_favorites::Column::CreatedAt)
                    .into_model()
                    .one(&orm(&pool))
                    .await
                    .map_err(|_| Denial::ServerError)?,
            };
            if let Some(row) = dedupe_found {
                let favorite = FavoriteRow::from(row);
                let entity = fetch_favorite_entity(
                    &pool,
                    &favorite.entity_type,
                    favorite.entity_identifier.as_deref(),
                )
                .await?;
                return Ok(json_response(
                    StatusCode::OK,
                    render_favorite(&favorite, entity.as_ref()),
                ));
            }
        }
    }
    // Field validation, then `save(user_id, workspace, project_id)` with the
    // RAW `project_id` (read-only on the serializer, unvalidated).
    let timezone = actor_timezone(&pool, &user_id).await?;
    let values = match validate_shape(&pool, &input, FAV_FIELDS, false, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    let entity_type = opt_text(&values, "entity_type").ok_or(Denial::ServerError)?;
    let entity_identifier = opt_uuid(&values, "entity_identifier");
    let name = opt_text(&values, "name");
    let is_folder = opt_bool(&values, "is_folder").unwrap_or(false);
    let sequence_input = opt_float(&values, "sequence");
    let parent = opt_uuid(&values, "parent");
    // `save(project_id=raw)`: `UserFavorite.save` touches `self.project`
    // first (`_base_manager`, unscoped) — an uncoercible id 400s
    // (`ValidationError`), a missing row 404s
    // (`RelatedObjectDoesNotExist`), and a surviving row re-points
    // `workspace` (soft-deleted resolves too).
    let (project_id, row_workspace_id) = match input.get("project_id") {
        None | Some(Value::Null) => (None, workspace_id),
        Some(raw) => match django_coerce_uuid(raw) {
            None => {
                return Ok(json_response(
                    StatusCode::BAD_REQUEST,
                    INVALID_DETAIL_BODY.to_owned(),
                ))
            }
            Some(id) => match project_row(&pool, &id).await? {
                None => {
                    return Ok(json_response(
                        StatusCode::NOT_FOUND,
                        OBJECT_NOT_FOUND_BODY.to_owned(),
                    ))
                }
                Some(project) => (Some(project.id), project.workspace_id),
            },
        },
    };
    // `sequence` (`db/models/favorite.py:56-68`): the WORKSPACE's largest
    // (project or slug workspace — no project/user scoping) + 10000 when
    // any row exists, else the input (or the 65535 default when absent).
    #[derive(Debug, FromQueryResult)]
    struct MaxSeqRow {
        max: Option<f64>,
    }
    let mut max_stmt = Query::select();
    max_stmt
        .expr_as(
            Func::max(col("user_favorites", user_favorites::Column::Sequence)),
            a("max"),
        )
        .from_as(user_favorites::Entity, a("user_favorites"))
        .and_where(col("user_favorites", user_favorites::Column::WorkspaceId).eq(row_workspace_id))
        .and_where(col("user_favorites", user_favorites::Column::DeletedAt).is_null());
    let largest = query_built::<MaxSeqRow>(&orm(&pool), &max_stmt).await?;
    let largest = largest.into_iter().next().map(|row| row.max);
    let sequence = match largest.flatten() {
        Some(largest) => largest + 10000.0,
        None => sequence_input.unwrap_or(65535.0),
    };
    let now = utc_now_micros();
    let now_db: DateTimeWithTimeZone = now.into();
    let id = Uuid::new_v4();
    let model = user_favorites::ActiveModel {
        id: Set(id),
        created_at: Set(now_db),
        updated_at: Set(now_db),
        created_by_id: Set(Some(user_id)),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        workspace_id: Set(row_workspace_id),
        project_id: Set(project_id),
        user_id: Set(user_id),
        entity_type: Set(entity_type.clone()),
        entity_identifier: Set(entity_identifier),
        name: Set(name.clone()),
        is_folder: Set(is_folder),
        sequence: Set(sequence),
        parent_id: Set(parent),
    };
    let insert = model.insert(&orm(&pool)).await;
    if let Err(error) = insert {
        if sea_is_integrity(&error) {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                FAVORITE_EXISTS_BODY.to_owned(),
            ));
        }
        return Err(Denial::ServerError);
    }
    let row = FavoriteRow {
        id: id.to_string(),
        entity_type: entity_type.clone(),
        entity_identifier: entity_identifier.map(|id| id.to_string()),
        name,
        is_folder,
        sequence,
        parent: parent.map(|id| id.to_string()),
        workspace_id: row_workspace_id.to_string(),
        project_id: project_id.map(|id| id.to_string()),
    };
    let entity =
        fetch_favorite_entity(&pool, &row.entity_type, row.entity_identifier.as_deref()).await?;
    Ok(json_response(
        StatusCode::OK,
        render_favorite(&row, entity.as_ref()),
    ))
}

/// Django `UUIDField.to_python` coercion for ORM FILTER values (as opposed
/// to DRF's own `UUIDField`): ints (bools included) ride `UUID(int=)`,
/// strings ride `UUID(hex=)`; floats/composites fail. `None` = the filter
/// raises `ValidationError`.
fn django_coerce_uuid(value: &Value) -> Option<Uuid> {
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return None;
                }
                return Some(Uuid::from_u128(int as u128));
            }
            if let Some(uint) = number.as_u64() {
                return Some(Uuid::from_u128(uint as u128));
            }
            if let Some(literal) = int_literal(number) {
                if literal.starts_with('-') {
                    return None;
                }
                return literal.parse::<u128>().ok().map(Uuid::from_u128);
            }
            None
        }
        Value::String(text) => Uuid::parse_str(text).ok(),
        Value::Bool(true) => Some(Uuid::from_u128(1)),
        Value::Bool(false) => Some(Uuid::from_u128(0)),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// `str(value)` for ORM `CharField` filter values: strings bare, numbers via
/// the JSON spelling, bools/null via CPython `repr`, composites via `repr`.
fn char_filter_text(value: &Value) -> String {
    choice_stringify(value)
}

/// W33 PATCH (`favorite.py:69-76`): `.get` (404), partial validation,
/// `save()` (no sequence recompute — not adding), 200; an `IntegrityError`
/// here is NOT the exists arm — it escapes to `handle_exception` → 400
/// `{"error": "The payload is not valid"}`.
async fn favorite_patch(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path((slug, favorite_id)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> HandlerResult {
    let favorite_pk = match parse_pk(&favorite_id) {
        Ok(pk) => pk,
        Err(response) => return Ok(response),
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    // `.get(...)` runs BEFORE `request.data` is touched (`favorite.py:71`
    // vs `:72`): a missing row 404s even for a malformed body (probed
    // live on the sibling draft route). The serializer still 400s
    // `not_a_dict` for non-dict bodies (no view-level `.get` here).
    let mut lookup_stmt = Query::select();
    lookup_stmt
        .columns([
            (a("user_favorites"), user_favorites::Column::Id),
            (a("user_favorites"), user_favorites::Column::EntityType),
            (
                a("user_favorites"),
                user_favorites::Column::EntityIdentifier,
            ),
            (a("user_favorites"), user_favorites::Column::Name),
            (a("user_favorites"), user_favorites::Column::IsFolder),
            (a("user_favorites"), user_favorites::Column::Sequence),
            (a("user_favorites"), user_favorites::Column::ParentId),
            (a("user_favorites"), user_favorites::Column::WorkspaceId),
            (a("user_favorites"), user_favorites::Column::ProjectId),
        ])
        .from_as(user_favorites::Entity, a("user_favorites"))
        .and_where(col("user_favorites", user_favorites::Column::UserId).eq(user_id))
        .and_where(
            col("user_favorites", user_favorites::Column::WorkspaceId)
                .binary(BinOper::Equal, workspace_id_sub(&slug)),
        )
        .and_where(col("user_favorites", user_favorites::Column::Id).eq(favorite_pk))
        .and_where(col("user_favorites", user_favorites::Column::DeletedAt).is_null())
        .limit(1);
    let lookup_rows = query_built::<FavoriteDbRow>(&orm(&pool), &lookup_stmt).await?;
    let found = lookup_rows.into_iter().next();
    let Some(row) = found else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_dict(&input) {
        return Ok(response);
    }
    let mut favorite = FavoriteRow::from(row);
    let timezone = actor_timezone(&pool, &user_id).await?;
    let values = match validate_shape(&pool, &input, FAV_FIELDS, true, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    // Default `ModelSerializer.update`: validated keys only, then `save()`
    // (audit `updated_by`, `auto_now` stamp; the sequence leg is add-only).
    let now = utc_now_micros();
    let now_db: DateTimeWithTimeZone = now.into();
    if has_key(&values, "entity_type") {
        favorite.entity_type = opt_text(&values, "entity_type").ok_or(Denial::ServerError)?;
    }
    if has_key(&values, "entity_identifier") {
        favorite.entity_identifier =
            opt_uuid(&values, "entity_identifier").map(|id| id.to_string());
    }
    if has_key(&values, "name") {
        favorite.name = opt_text(&values, "name");
    }
    if let Some(is_folder) = opt_bool(&values, "is_folder") {
        favorite.is_folder = is_folder;
    }
    if let Some(sequence) = opt_float(&values, "sequence") {
        favorite.sequence = sequence;
    }
    if has_key(&values, "parent") {
        favorite.parent = opt_uuid(&values, "parent").map(|id| id.to_string());
    }
    // `save()` always stamps (`auto_now` + audit), even for an empty patch.
    let mut update = user_favorites::Entity::update_many();
    update = update.col_expr(user_favorites::Column::UpdatedAt, Expr::value(now_db));
    update = update.col_expr(
        user_favorites::Column::UpdatedById,
        Expr::value(Some(user_id)),
    );
    if has_key(&values, "entity_type") {
        update = update.col_expr(
            user_favorites::Column::EntityType,
            Expr::value(favorite.entity_type.clone()),
        );
    }
    if has_key(&values, "entity_identifier") {
        let ident: Option<Uuid> = favorite
            .entity_identifier
            .as_deref()
            .map(|text| Uuid::parse_str(text).expect("favorite identifier"));
        update = update.col_expr(user_favorites::Column::EntityIdentifier, Expr::value(ident));
    }
    if has_key(&values, "name") {
        update = update.col_expr(
            user_favorites::Column::Name,
            Expr::value(favorite.name.clone()),
        );
    }
    if let Some(is_folder) = opt_bool(&values, "is_folder") {
        update = update.col_expr(user_favorites::Column::IsFolder, Expr::value(is_folder));
    }
    if let Some(sequence) = opt_float(&values, "sequence") {
        update = update.col_expr(user_favorites::Column::Sequence, Expr::value(sequence));
    }
    if has_key(&values, "parent") {
        let parent: Option<Uuid> = favorite
            .parent
            .as_deref()
            .map(|text| Uuid::parse_str(text).expect("favorite parent"));
        update = update.col_expr(user_favorites::Column::ParentId, Expr::value(parent));
    }
    update = update.filter(user_favorites::Column::Id.eq(favorite_pk));
    if let Err(error) = update.exec(&orm(&pool)).await {
        if sea_is_integrity(&error) {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                INVALID_PAYLOAD_BODY.to_owned(),
            ));
        }
        return Err(Denial::ServerError);
    }
    let entity = fetch_favorite_entity(
        &pool,
        &favorite.entity_type,
        favorite.entity_identifier.as_deref(),
    )
    .await?;
    Ok(json_response(
        StatusCode::OK,
        render_favorite(&favorite, entity.as_ref()),
    ))
}

/// W33 DELETE (`favorite.py:78-82`): `.get` (404), HARD delete, 204.
async fn favorite_delete(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path((slug, favorite_id)): axum::extract::Path<(String, String)>,
) -> HandlerResult {
    let favorite_pk = match parse_pk(&favorite_id) {
        Ok(pk) => pk,
        Err(response) => return Ok(response),
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    #[derive(Debug, FromQueryResult)]
    struct ExistsRow {
        id: Uuid,
    }
    let mut exists_stmt = Query::select();
    exists_stmt
        .column((a("user_favorites"), user_favorites::Column::Id))
        .from_as(user_favorites::Entity, a("user_favorites"))
        .and_where(col("user_favorites", user_favorites::Column::UserId).eq(user_id))
        .and_where(
            col("user_favorites", user_favorites::Column::WorkspaceId)
                .binary(BinOper::Equal, workspace_id_sub(&slug)),
        )
        .and_where(col("user_favorites", user_favorites::Column::Id).eq(favorite_pk))
        .and_where(col("user_favorites", user_favorites::Column::DeletedAt).is_null())
        .limit(1);
    let exists_rows = query_built::<ExistsRow>(&orm(&pool), &exists_stmt).await?;
    let exists = exists_rows.into_iter().next().map(|row| row.id);
    if exists.is_none() {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    }
    // Django's `Collector` emulates `CASCADE` in Python (the FK DDL carries
    // no `ON DELETE CASCADE`): deleting a folder deletes its whole subtree
    // — including soft-deleted descendants (probed live) — inside one
    // atomic block. The self-parent FK is the only edge into
    // `user_favorites`, so one recursive CTE is the full cascade.
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    // raw-sql: recursive CASCADE CTE the builder cannot express
    sqlx::query(
        "WITH RECURSIVE subtree(id) AS (
           SELECT id FROM user_favorites WHERE parent_id = $1
           UNION ALL
           SELECT child.id FROM user_favorites child JOIN subtree ON child.parent_id = subtree.id
         ) DELETE FROM user_favorites WHERE id IN (SELECT id FROM subtree)",
    )
    .bind(favorite_pk)
    .execute(&mut *tx)
    .await
    .map_err(|_| Denial::ServerError)?;
    {
        let conn = TxConn::new(&mut tx);
        user_favorites::Entity::delete_many()
            .filter(user_favorites::Column::Id.eq(favorite_pk))
            .exec(&conn)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    Ok(empty_response())
}

/// W34 GET (`favorite.py:85-97`): ADMIN+MEMBER gate, children of the folder
/// over project-null-or-member (NO page exclusion here), default order.
async fn favorite_group(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path((slug, favorite_id)): axum::extract::Path<(String, String)>,
) -> HandlerResult {
    let favorite_pk = match parse_pk(&favorite_id) {
        Ok(pk) => pk,
        Err(response) => return Ok(response),
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    let rows = query_built::<FavoriteDbRow>(
        &orm(&pool),
        &favorites_select(&slug, user_id, Some(favorite_pk), false),
    )
    .await?;
    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let favorite = FavoriteRow::from(row);
        let entity = fetch_favorite_entity(
            &pool,
            &favorite.entity_type,
            favorite.entity_identifier.as_deref(),
        )
        .await?;
        rendered.push(render_favorite(&favorite, entity.as_ref()));
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", rendered.join(",")),
    ))
}

/// One field's `get_value` (`fields.py`): `None` means skip (missing and not
/// required, HTML-blank collapse, or partial-missing); the variants resolve
/// without string sentinels (real input can spell anything).
enum FieldInput {
    Value(Value),
    Null,
    MissingRequired,
    UploadPk(String),
    UploadList,
    UploadJson,
}

fn field_input(input: &RequestData, spec: &FieldSpec, partial: bool) -> Option<FieldInput> {
    let required = spec.required && !partial;
    match input.input(spec.name) {
        InputRef::Missing => {
            if required {
                return Some(FieldInput::MissingRequired);
            }
            // HTML missing on create: `BooleanField.default_empty_html`
            // (`False`, or `None` with `allow_null`).
            if input.is_html && !partial && matches!(spec.shape, FieldShape::Bool) {
                if spec.allow_null {
                    return Some(FieldInput::Null);
                }
                return Some(FieldInput::Value(Value::Bool(false)));
            }
            None
        }
        InputRef::Files => {
            // A files-only key is present and reads as the file object:
            // scalars fail their shape (`str(file)` = the filename feeds
            // the same parsers); pks echo the filename as invalid UUID;
            // lists see a non-list upload object.
            let filename = input
                .files
                .get(spec.name)
                .and_then(|parts| parts.first())
                .map(|part| part.filename.clone())
                .unwrap_or_default();
            match &spec.shape {
                FieldShape::Pk(_) => Some(FieldInput::UploadPk(filename)),
                FieldShape::PkList(_) => Some(FieldInput::UploadList),
                // `json.dumps(file)` raises → the invalid message.
                FieldShape::Json => Some(FieldInput::UploadJson),
                _ => Some(FieldInput::Value(Value::String(filename))),
            }
        }
        InputRef::Json(Value::Null) => {
            if spec.allow_null {
                Some(FieldInput::Null)
            } else {
                Some(FieldInput::Value(Value::Null))
            }
        }
        InputRef::Json(value) => {
            // HTML blank collapse (`get_value`): `''` with `allow_null`
            // becomes `None` unless the field also allows blank; `''`
            // without `allow_null` becomes missing unless required or
            // blank-allowing.
            if input.is_html {
                if let Value::String(text) = value {
                    if text.is_empty() {
                        let blank_ok = matches!(
                            &spec.shape,
                            FieldShape::Char {
                                allow_blank: true,
                                ..
                            } | FieldShape::Choice {
                                allow_blank: true,
                                ..
                            }
                        );
                        if spec.allow_null {
                            if !blank_ok {
                                return Some(FieldInput::Null);
                            }
                        } else if !required && !blank_ok {
                            return None;
                        }
                    }
                }
            }
            Some(FieldInput::Value(value.clone()))
        }
    }
}

/// `ListField.to_internal_value` over a pk child (`label_ids`,
/// `assignee_ids`): non-lists fail with the type name; items parse in order
/// and each item's errors key by INDEX (`{"0": [...]}`).
fn parse_uuid_list(value: &Value) -> Result<Vec<Uuid>, Value> {
    let Value::Array(items) = value else {
        return Err(Value::Array(vec![Value::String(format!(
            "Expected a list of items but got type \"{}\".",
            json_type_name(value)
        ))]));
    };
    let mut ids = Vec::with_capacity(items.len());
    let mut errors = Map::new();
    for (index, item) in items.iter().enumerate() {
        match parse_uuid_pk(item) {
            Ok(id) => ids.push(id),
            Err(messages) => {
                errors.insert(
                    index.to_string(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        }
    }
    if errors.is_empty() {
        Ok(ids)
    } else {
        Err(Value::Object(errors))
    }
}

// ---------------------------------------------------------------------------
// Draft writes (`draft.py:14-165`, W35-W36)
// ---------------------------------------------------------------------------

/// `DraftIssueCreateSerializer.validate` (`draft.py:71-140`): sequential —
/// the FIRST failure raises (dates → html → assignees/labels filter →
/// state → parent → estimate). The binary arm is dead (`description_binary`
/// is read-only, so the key never validates).
struct DraftObjectAttrs {
    assignee_ids: Vec<Uuid>,
    label_ids: Vec<Uuid>,
    description_html: Option<String>,
}

#[allow(clippy::result_large_err)]
async fn draft_object_leg(
    pool: &sqlx::PgPool,
    values: &[(String, FieldValue)],
    project_id: Option<Uuid>,
    context_garbage: Option<String>,
) -> Result<DraftObjectAttrs, Response> {
    let bad = |errors: Map<String, Value>| shape_errors_response(&errors);
    let non_field = |message: &str| {
        let mut errors = Map::new();
        errors.insert(
            "non_field_errors".to_owned(),
            Value::Array(vec![Value::String(message.to_owned())]),
        );
        bad(errors)
    };
    // Start/target order (`:72-79`): presence-keyed — explicit nulls on
    // both sides `TypeError` (`None > None`) → the generic 500.
    if has_key(values, "start_date") && has_key(values, "target_date") {
        match (
            opt_date(values, "start_date"),
            opt_date(values, "target_date"),
        ) {
            (Some(start), Some(target)) => {
                if start > target {
                    return Err(non_field("Start date cannot exceed target date"));
                }
            }
            _ => {
                return Err(json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SERVER_ERROR_BODY.to_owned(),
                ));
            }
        }
    }
    // HTML sanitization (`:80-86`): truthy inputs only; the CLEAN output
    // replaces the input for storage.
    let mut description_html = opt_text(values, "description_html");
    if let Some(html) = opt_text(values, "description_html") {
        if !html.is_empty() {
            match sticky_kernel::sanitize_html(&html) {
                sticky_kernel::SanitizeOutcome::Clean(clean) => {
                    description_html = Some(clean);
                }
                sticky_kernel::SanitizeOutcome::Invalid => {
                    let mut errors = Map::new();
                    errors.insert(
                        "error".to_owned(),
                        Value::Array(vec![Value::String(
                            sticky_kernel::HTML_INVALID_MESSAGE.to_owned(),
                        )]),
                    );
                    return Err(bad(errors));
                }
            }
        }
    }
    // Assignees (`:92-103`): non-empty lists filter to active project
    // members (`role >= 15`); silently dropped otherwise. A `None` context
    // project renders `IS NULL` (no rows) — no error.
    // A garbage context id fails lazily, wherever first consumed: each
    // context arm below 400s on it — the ORM filter's `ValidationError`,
    // raised inside `validate()`, surfaces as `non_field_errors` carrying
    // the `to_python` UUID message (probed live) — in arm order. A request
    // that triggers no arm never notices it.
    let mut assignee_ids = Vec::new();
    if let Some(FieldValue::Ids(ids)) = find_value(values, "assignee_ids") {
        if !ids.is_empty() {
            if let Some(echo) = &context_garbage {
                return Err(non_field(&django_uuid_invalid(echo)));
            }
            assignee_ids = match project_id {
                None => Vec::new(),
                Some(project) => create_q::filter_assignee_members(&orm(pool), project, ids)
                    .await
                    .map_err(|_| {
                        json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            SERVER_ERROR_BODY.to_owned(),
                        )
                    })?,
            };
        }
    }
    // Labels (`:104-111`): non-empty lists filter to live labels of the
    // project — or of the NULL project (`IS NULL`: workspace-level labels).
    let mut label_ids = Vec::new();
    if let Some(FieldValue::Ids(ids)) = find_value(values, "label_ids") {
        if !ids.is_empty() {
            if let Some(echo) = &context_garbage {
                return Err(non_field(&django_uuid_invalid(echo)));
            }
            label_ids = create_q::filter_project_labels(&orm(pool), project_id, ids)
                .await
                .map_err(|_| {
                    json_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        SERVER_ERROR_BODY.to_owned(),
                    )
                })?;
        }
    }
    // State / parent / estimate project-membership (`:112-139`): present
    // and non-null ids must exist under the context project (default
    // managers); a `None` project matches nothing → the error.
    if let Some(state) = dual_uuid(values, "state_id", "state").flatten() {
        if let Some(echo) = &context_garbage {
            return Err(non_field(&django_uuid_invalid(echo)));
        }
        let exists = create_q::state_exists(&orm(pool), project_id, state)
            .await
            .map_err(|_| {
                json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SERVER_ERROR_BODY.to_owned(),
                )
            })?;
        if !exists {
            return Err(non_field(DRAFT_STATE_INVALID));
        }
    }
    if let Some(parent) = dual_uuid(values, "parent_id", "parent").flatten() {
        if let Some(echo) = &context_garbage {
            return Err(non_field(&django_uuid_invalid(echo)));
        }
        let exists = create_q::parent_exists(&orm(pool), project_id, parent)
            .await
            .map_err(|_| {
                json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SERVER_ERROR_BODY.to_owned(),
                )
            })?;
        if !exists {
            return Err(non_field(DRAFT_PARENT_INVALID));
        }
    }
    if let Some(estimate) = opt_uuid(values, "estimate_point") {
        if let Some(echo) = &context_garbage {
            return Err(non_field(&django_uuid_invalid(echo)));
        }
        let exists = create_q::estimate_exists(&orm(pool), project_id, estimate)
            .await
            .map_err(|_| {
                json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SERVER_ERROR_BODY.to_owned(),
                )
            })?;
        if !exists {
            return Err(non_field(DRAFT_ESTIMATE_INVALID));
        }
    }
    Ok(DraftObjectAttrs {
        assignee_ids,
        label_ids,
        description_html,
    })
}

/// The raw context `project_id` (`request.data.get(...)`, unvalidated):
/// missing/`null` → the `None` context; a coercible value resolves; garbage
/// stays lazy as its `to_python` echo — the consumer 400s wherever it first
/// touches it (a `validate()` arm reports the UUID message as
/// `non_field_errors`; the save path reports "valid detail").
fn raw_context_project(raw: Option<&Value>) -> (Option<Uuid>, Option<String>) {
    match raw {
        None | Some(Value::Null) => (None, None),
        Some(value) => match django_coerce_uuid(value) {
            Some(id) => (Some(id), None),
            None => (None, Some(pk_echo(value))),
        },
    }
}

/// The patch context `project_id` (`draft.py:168`,
/// `request.data.get("project_id", issue.project_id)`): a MISSING key keeps
/// the issue's own project, an explicit `null` means the `None` context
/// (arms error, labels go `IS NULL`, assignees drop), and garbage stays
/// lazy (see `raw_context_project`).
fn patch_context_project(
    raw: Option<&Value>,
    issue_project: Option<Uuid>,
) -> (Option<Uuid>, Option<String>) {
    match raw {
        None => (issue_project, None),
        Some(Value::Null) => (None, None),
        Some(value) => match django_coerce_uuid(value) {
            Some(id) => (Some(id), None),
            None => (None, Some(pk_echo(value))),
        },
    }
}

/// `DraftIssue.save` state legs (`db/models/draft.py:84-103`): a `None`
/// state resolves the project's default (then first) non-triage state over
/// `ORDER BY sequence` (a `None` project matches nothing) and leaves
/// `completed_at` untouched; a given state recomputes `completed_at` from
/// its group (`completed` → now, else `None`).
async fn draft_save_state(
    pool: &sqlx::PgPool,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
    input_completed: Option<DateTime<Utc>>,
    now: &DateTime<Utc>,
) -> Result<(Option<Uuid>, Option<DateTime<Utc>>), Denial> {
    match state_id {
        None => {
            let resolved = default_draft_state_opt(pool, project_id).await?;
            Ok((resolved, input_completed))
        }
        Some(state) => {
            // `self.state.group` off the cached row (create/patch) or the
            // `_base_manager` fetch (delete): a missing row raises → 404.
            let group = state_row(pool, &state)
                .await?
                .map(|row| row.group)
                .ok_or(Denial::NotFound)?;
            let completed = if group == "completed" {
                Some(*now)
            } else {
                None
            };
            Ok((Some(state), completed))
        }
    }
}

/// The draft default-state lookup: default-then-first non-triage state of
/// the project, `ORDER BY sequence` (the `State.objects` manager scope).
async fn default_draft_state(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    let db = orm(pool);
    for default_only in [true, false] {
        let mut query = states::Entity::find()
            .select_only()
            .column(states::Column::Id)
            .filter(states::Column::ProjectId.eq(*project_id))
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .filter(states::Column::IsTriage.eq(false))
            .order_by_asc(states::Column::Sequence);
        if default_only {
            query = query.filter(states::Column::Default.eq(true));
        }
        let row: Option<Uuid> = query
            .into_tuple()
            .one(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
        if let Some(id) = row {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// `DraftIssue.save` sort leg (`db/models/draft.py:105-112`): the sibling
/// max over `(project, state)` + 10000 when any sibling exists.
async fn draft_sort_order(
    pool: &sqlx::PgPool,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
) -> Result<Option<f64>, Denial> {
    #[derive(Debug, FromQueryResult)]
    struct MaxSortRow {
        max: Option<f64>,
    }
    let mut stmt = Query::select();
    stmt.expr_as(
        Func::max(col("draft_issues", draft_issues::Column::SortOrder)),
        a("max"),
    )
    .from_as(draft_issues::Entity, a("draft_issues"))
    .and_where(col("draft_issues", draft_issues::Column::DeletedAt).is_null());
    match project_id {
        Some(id) => {
            stmt.and_where(col("draft_issues", draft_issues::Column::ProjectId).eq(id));
        }
        None => {
            stmt.and_where(col("draft_issues", draft_issues::Column::ProjectId).is_null());
        }
    };
    match state_id {
        Some(id) => {
            stmt.and_where(col("draft_issues", draft_issues::Column::StateId).eq(id));
        }
        None => {
            stmt.and_where(col("draft_issues", draft_issues::Column::StateId).is_null());
        }
    };
    let rows = query_built::<MaxSortRow>(&orm(pool), &stmt).await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|row| row.max)
        .map(|max| max + 10000.0))
}

/// W35 POST (`draft.py:113-157`): ADMIN+MEMBER gate, workspace get (404),
/// field validation + object leg, INSERT + m2m + raw cycle/modules, 21-key
/// re-read (201) — datetimes UTC millis (no `user_timezone_converter`).
async fn draft_create(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path(slug): axum::extract::Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> HandlerResult {
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    // `Workspace.objects.get(slug)` runs BEFORE `request.data` is touched
    // (`draft.py:113` vs `:115-121`): a missing slug 404s even for a
    // malformed body, and the `.get("project_id")` on a non-dict body
    // raises `AttributeError` → the generic 500 (both probed live).
    // Both views share the lookup (`Workspace.objects.get(slug)`).
    let workspace_id: Option<Uuid> = workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .filter(workspaces::Column::Slug.eq(slug.clone()))
        .filter(workspaces::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(workspace_id) = workspace_id else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_data_get(&input) {
        return Ok(response);
    }
    let timezone = actor_timezone(&pool, &user_id).await?;
    // Field validation runs BEFORE the context id is consumed (`is_valid`
    // first, then `save`): field errors win over a garbage `project_id`.
    let values = match validate_shape(&pool, &input, DRAFT_FIELDS, false, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    // The context `project_id` is RAW (`request.data.get`, unvalidated)
    // and lazy: field validation above runs first, then the arms consume
    // it (`non_field_errors` UUID message on garbage), and garbage no arm
    // touched reaches `save()`, where `objects.create` raises OUTSIDE
    // `is_valid()` → `handle_exception` reports "valid detail" instead
    // (both shapes probed live; no row is created either way).
    let (context_project, context_garbage) = raw_context_project(input.get("project_id"));
    let object =
        match draft_object_leg(&pool, &values, context_project, context_garbage.clone()).await {
            Ok(object) => object,
            Err(response) => return Ok(response),
        };
    if context_garbage.is_some() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            INVALID_DETAIL_BODY.to_owned(),
        ));
    }
    // `create()`: the context project wins over any validated `project`
    // input (`DraftIssue(project=…, project_id=…)` keeps the latter —
    // probed live); `save()` fetches it (miss → 404) and re-points
    // `workspace` at the project's workspace.
    let row_project = context_project;
    let row_workspace = match row_project {
        None => workspace_id,
        Some(project) => match project_row(&pool, &project).await? {
            None => {
                return Ok(json_response(
                    StatusCode::NOT_FOUND,
                    OBJECT_NOT_FOUND_BODY.to_owned(),
                ))
            }
            Some(row) => row.workspace_id,
        },
    };
    let now = utc_now_micros();
    let input_state = dual_uuid(&values, "state_id", "state").flatten();
    let input_completed = opt_datetime(&values, "completed_at");
    let (state_id, completed_at) =
        draft_save_state(&pool, row_project, input_state, input_completed, &now).await?;
    let sort_order = match draft_sort_order(&pool, row_project, state_id).await? {
        Some(sort) => sort,
        None => opt_float(&values, "sort_order").unwrap_or(65535.0),
    };
    let description_html = object
        .description_html
        .clone()
        .unwrap_or_else(|| "<p></p>".to_owned());
    let description_stripped = sync_description_stripped(Some(&description_html));
    let description_json =
        opt_json(&values, "description_json").unwrap_or(Value::Object(Map::new()));
    let id = Uuid::new_v4();
    let name = opt_text(&values, "name");
    let deleted_at = opt_datetime(&values, "deleted_at");
    let priority = opt_text(&values, "priority").unwrap_or_else(|| "none".to_owned());
    let parent_id = dual_uuid(&values, "parent_id", "parent").flatten();
    let estimate_point_id = opt_uuid(&values, "estimate_point");
    let type_id = opt_uuid(&values, "type");
    let start_date = opt_date(&values, "start_date");
    let target_date = opt_date(&values, "target_date");
    let external_source = opt_text(&values, "external_source");
    let external_id = opt_text(&values, "external_id");
    let now_db: DateTimeWithTimeZone = now.into();
    let deleted_db: Option<DateTimeWithTimeZone> = deleted_at.map(Into::into);
    let completed_db: Option<DateTimeWithTimeZone> = completed_at.map(Into::into);
    let model = draft_issues::ActiveModel {
        id: Set(id),
        created_at: Set(now_db),
        updated_at: Set(now_db),
        created_by_id: Set(Some(user_id)),
        updated_by_id: Set(None),
        deleted_at: Set(deleted_db),
        workspace_id: Set(row_workspace),
        project_id: Set(row_project),
        parent_id: Set(parent_id),
        state_id: Set(state_id),
        estimate_point_id: Set(estimate_point_id),
        name: Set(name.clone()),
        description_json: Set(description_json.clone()),
        description_html: Set(description_html.clone()),
        description_stripped: Set(description_stripped.clone()),
        description_binary: Set(None),
        priority: Set(priority.clone()),
        start_date: Set(start_date),
        target_date: Set(target_date),
        sort_order: Set(sort_order),
        completed_at: Set(completed_db),
        external_source: Set(external_source.clone()),
        external_id: Set(external_id.clone()),
        type_id: Set(type_id),
    };
    let insert = model.insert(&orm(&pool)).await;
    if let Err(error) = insert {
        if sea_is_integrity(&error) {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                INVALID_PAYLOAD_BODY.to_owned(),
            ));
        }
        return Err(Denial::ServerError);
    }
    // M2M (`:124-143`): batched `bulk_create` (batch 10), audit inherited
    // from the fresh issue (created=actor, updated=None); NO
    // `ignore_conflicts` — an `IntegrityError` 400s.
    if let Err(response) = insert_draft_links(
        &pool,
        &id,
        &row_workspace,
        &row_project,
        &user_id,
        &None,
        &object.assignee_ids,
        &object.label_ids,
    )
    .await
    {
        return Ok(response);
    }
    // Cycle + modules (`:144-157`): RAW `initial_data`, unvalidated — a
    // present-but-malformed id 400s (`ValidationError` → "valid detail");
    // a missing row 400s (`IntegrityError`); a null module item NULL-inserts
    // → 400 (`IntegrityError`).
    if let Some(raw) = input.get("cycle_id") {
        if !matches!(raw, Value::Null) {
            let cycle_id = match coerce_link_uuid(raw) {
                Some(id) => id,
                None => {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_DETAIL_BODY.to_owned(),
                    ))
                }
            };
            let link = Uuid::new_v4();
            let cycle_now: DateTimeWithTimeZone = now.into();
            let cycle_model = draft_issue_cycles::ActiveModel {
                id: Set(link),
                created_at: Set(cycle_now),
                updated_at: Set(cycle_now),
                created_by_id: Set(Some(user_id)),
                updated_by_id: Set(None),
                deleted_at: Set(None),
                workspace_id: Set(row_workspace),
                project_id: Set(row_project),
                draft_issue_id: Set(id),
                cycle_id: Set(cycle_id),
            };
            if let Err(error) = cycle_model.insert(&orm(&pool)).await {
                if sea_is_integrity(&error) {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_PAYLOAD_BODY.to_owned(),
                    ));
                }
                return Err(Denial::ServerError);
            }
        }
    }
    if let Some(raw) = input.get("module_ids") {
        if !matches!(raw, Value::Null) {
            let items = match link_items(raw) {
                Some(items) => items,
                // `len()` / iteration over a number/bool → `TypeError` → 500.
                None => {
                    return Ok(json_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        SERVER_ERROR_BODY.to_owned(),
                    ))
                }
            };
            if !items.is_empty() {
                let mut module_ids = Vec::with_capacity(items.len());
                for item in &items {
                    if matches!(item, Value::Null) {
                        // `to_python(None)` passes `None` through — the NULL
                        // insert raises `IntegrityError` → 400 below.
                        module_ids.push(None);
                        continue;
                    }
                    match coerce_link_uuid(item) {
                        Some(id) => module_ids.push(Some(id)),
                        None => {
                            return Ok(json_response(
                                StatusCode::BAD_REQUEST,
                                INVALID_DETAIL_BODY.to_owned(),
                            ))
                        }
                    }
                }
                if let Err(error) = insert_draft_modules(
                    &pool,
                    &id,
                    &row_workspace,
                    &row_project,
                    &user_id,
                    &None,
                    &module_ids,
                )
                .await
                {
                    if sea_is_integrity(&error) {
                        return Ok(json_response(
                            StatusCode::BAD_REQUEST,
                            INVALID_PAYLOAD_BODY.to_owned(),
                        ));
                    }
                    return Err(Denial::ServerError);
                }
            }
        }
    }
    // The 21-key re-read (`:149-157`): UTC millis datetimes (no
    // `user_timezone_converter`); a scope miss (cross-workspace project)
    // answers 201 `null` (`.first()` → `None`).
    match reread_draft(&pool, &slug, &id).await? {
        Some(row) => Ok(json_response(
            StatusCode::CREATED,
            render_draft_reread(&row),
        )),
        None => Ok(json_response(StatusCode::CREATED, "null".to_owned())),
    }
}

/// Coerce one raw link id (`cycle_id`, `module_ids` items): the ORM
/// `create()` binds the raw value through `UUIDField.to_python` — ints ride
/// `UUID(int=)`, bools ride 0/1, strings ride `UUID(hex=)` (hyphenated,
/// simple, braced, urn, upper); floats/composites/`None` raise
/// `ValidationError` (probed live: `None` means the caller 400s
/// `INVALID_DETAIL_BODY`, never the 500).
fn coerce_link_uuid(value: &Value) -> Option<Uuid> {
    django_coerce_uuid(value)
}

/// `len(modules)` + iteration over raw `module_ids`: arrays itemize,
/// strings iterate CHARS, objects iterate KEYS; numbers/bools raise
/// `TypeError` (`None` here → the caller 500s).
fn link_items(value: &Value) -> Option<Vec<Value>> {
    match value {
        Value::Array(items) => Some(items.clone()),
        Value::String(text) => Some(
            text.chars()
                .map(|ch| Value::String(ch.to_string()))
                .collect(),
        ),
        Value::Object(map) => Some(map.keys().map(|key| Value::String(key.clone())).collect()),
        Value::Null => Some(Vec::new()),
        Value::Bool(_) | Value::Number(_) => None,
    }
}

/// Batched draft m2m INSERTs (batch 10): assignees then labels, audit ids
/// inherited from the (fresh or patch-target) issue row. Each leg runs in
/// its own transaction: `bulk_create` is atomic across batches (a late
/// chunk's `IntegrityError` rolls the whole leg back — probed live), but
/// the two legs are separate calls (assignees persist when labels fail).
#[allow(clippy::result_large_err)]
#[allow(clippy::too_many_arguments)]
async fn insert_draft_links(
    pool: &sqlx::PgPool,
    draft_id: &Uuid,
    workspace_id: &Uuid,
    project_id: &Option<Uuid>,
    created_by: &Uuid,
    updated_by: &Option<Uuid>,
    assignee_ids: &[Uuid],
    label_ids: &[Uuid],
) -> Result<(), Response> {
    let tx_error = || {
        json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            SERVER_ERROR_BODY.to_owned(),
        )
    };
    let mut tx = pool.begin().await.map_err(|_| tx_error())?;
    for chunk in assignee_ids.chunks(10) {
        let now: DateTimeWithTimeZone = utc_now_micros().into();
        let rows = chunk
            .iter()
            .map(|assignee| draft_issue_assignees::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(now),
                updated_at: Set(now),
                created_by_id: Set(Some(*created_by)),
                updated_by_id: Set(*updated_by),
                deleted_at: Set(None),
                workspace_id: Set(*workspace_id),
                project_id: Set(*project_id),
                draft_issue_id: Set(*draft_id),
                assignee_id: Set(*assignee),
            });
        let result = {
            let conn = TxConn::new(&mut tx);
            draft_issue_assignees::Entity::insert_many(rows)
                .exec(&conn)
                .await
        };
        if let Err(error) = result {
            // Drop without commit: the leg rolls back as one unit.
            if sea_is_integrity(&error) {
                return Err(json_response(
                    StatusCode::BAD_REQUEST,
                    INVALID_PAYLOAD_BODY.to_owned(),
                ));
            }
            return Err(json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ));
        }
    }
    tx.commit().await.map_err(|_| tx_error())?;
    let mut tx = pool.begin().await.map_err(|_| tx_error())?;
    for chunk in label_ids.chunks(10) {
        let now: DateTimeWithTimeZone = utc_now_micros().into();
        let rows = chunk.iter().map(|label| draft_issue_labels::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(now),
            updated_at: Set(now),
            created_by_id: Set(Some(*created_by)),
            updated_by_id: Set(*updated_by),
            deleted_at: Set(None),
            workspace_id: Set(*workspace_id),
            project_id: Set(*project_id),
            draft_issue_id: Set(*draft_id),
            label_id: Set(*label),
        });
        let result = {
            let conn = TxConn::new(&mut tx);
            draft_issue_labels::Entity::insert_many(rows)
                .exec(&conn)
                .await
        };
        if let Err(error) = result {
            // Drop without commit: the leg rolls back as one unit.
            if sea_is_integrity(&error) {
                return Err(json_response(
                    StatusCode::BAD_REQUEST,
                    INVALID_PAYLOAD_BODY.to_owned(),
                ));
            }
            return Err(json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ));
        }
    }
    tx.commit().await.map_err(|_| tx_error())?;
    Ok(())
}

/// Batched draft-module INSERTs (batch 10) in one transaction: like every
/// `bulk_create`, atomic across batches (probed live).
async fn insert_draft_modules(
    pool: &sqlx::PgPool,
    draft_id: &Uuid,
    workspace_id: &Uuid,
    project_id: &Option<Uuid>,
    created_by: &Uuid,
    updated_by: &Option<Uuid>,
    module_ids: &[Option<Uuid>],
) -> Result<(), DbErr> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| DbErr::Exec(sea_orm::RuntimeErr::SqlxError(error)))?;
    for chunk in module_ids.chunks(10) {
        let now: DateTimeWithTimeZone = utc_now_micros().into();
        // `module_id` is non-nullable: a `None` entry NULL-inserts through
        // the builder to raise the `IntegrityError` exactly as before.
        let mut insert = Query::insert();
        insert.into_table(draft_issue_modules::Entity);
        insert.columns([
            draft_issue_modules::Column::Id,
            draft_issue_modules::Column::CreatedAt,
            draft_issue_modules::Column::UpdatedAt,
            draft_issue_modules::Column::CreatedById,
            draft_issue_modules::Column::UpdatedById,
            draft_issue_modules::Column::DeletedAt,
            draft_issue_modules::Column::WorkspaceId,
            draft_issue_modules::Column::ProjectId,
            draft_issue_modules::Column::DraftIssueId,
            draft_issue_modules::Column::ModuleId,
        ]);
        for module in chunk {
            let link = Uuid::new_v4();
            insert
                .values([
                    link.into(),
                    now.into(),
                    now.into(),
                    Some(*created_by).into(),
                    (*updated_by).into(),
                    None::<Uuid>.into(),
                    (*workspace_id).into(),
                    (*project_id).into(),
                    (*draft_id).into(),
                    (*module).into(),
                ])
                .map_err(|e| DbErr::Exec(RuntimeErr::Internal(e.to_string())))?;
        }
        let stmt = insert;
        let result = {
            let conn = TxConn::new(&mut tx);
            conn.execute(DbBackend::Postgres.build(&stmt)).await
        };
        result?;
    }
    tx.commit()
        .await
        .map_err(|error| DbErr::Exec(sea_orm::RuntimeErr::SqlxError(error)))?;
    Ok(())
}

/// The create re-read (`draft.py:149-157`): one annotated row over the slug
/// scope (miss → `None` → 201 `null`).
async fn reread_draft(
    pool: &sqlx::PgPool,
    slug: &str,
    id: &Uuid,
) -> Result<Option<DraftRow>, Denial> {
    let empty = Condition::all();
    let stmt = draft_rows_select(slug, empty, None, Some(*id), 1, 0, false);
    let rows = query_built::<DraftDbRow>(&orm(pool), &stmt).await?;
    Ok(rows.into_iter().next().map(draft_from_db))
}

/// Render the create re-read (the same 21 keys as the list shape, but NOT
/// its order): the `.values()` compiler emits concrete columns first (in
/// `.values()` order) and annotations LAST (in `annotate()` order) —
/// `cycle_id, label_ids, assignee_ids, module_ids` — regardless of the
/// interleaving at `draft.py:127-149` (probed live on the wire). UTC MILLIS
/// datetimes (`DjangoJSONEncoder`, no `user_timezone_converter`).
fn render_draft_reread(row: &DraftRow) -> String {
    let opt = |value: &Option<String>| match value {
        Some(text) => json_string(text),
        None => "null".to_owned(),
    };
    let ids = |values: &[String]| {
        format!(
            "[{}]",
            values
                .iter()
                .map(|id| json_string(id))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let date = |value: &Option<NaiveDate>| match value {
        Some(date) => json_string(&date.format("%Y-%m-%d").to_string()),
        None => "null".to_owned(),
    };
    let moment = |value: &Option<DateTime<Utc>>| match value {
        Some(moment) => json_string(&render_encoder_datetime(moment)),
        None => "null".to_owned(),
    };
    format!(
        "{{\"id\":{},\"name\":{},\"state_id\":{},\"sort_order\":{},\"completed_at\":{},\"estimate_point\":{},\"priority\":{},\"start_date\":{},\"target_date\":{},\"project_id\":{},\"parent_id\":{},\"created_at\":{},\"updated_at\":{},\"created_by\":{},\"updated_by\":{},\"type_id\":{},\"description_html\":{},\"cycle_id\":{},\"label_ids\":{},\"assignee_ids\":{},\"module_ids\":{}}}",
        json_string(&row.id),
        opt(&row.name),
        opt(&row.state_id),
        json_float_str(row.sort_order),
        moment(&row.completed_at),
        opt(&row.estimate_point),
        json_string(&row.priority),
        date(&row.start_date),
        date(&row.target_date),
        opt(&row.project_id),
        opt(&row.parent_id),
        json_string(&render_encoder_datetime(&row.created_at)),
        json_string(&render_encoder_datetime(&row.updated_at)),
        opt(&row.created_by),
        opt(&row.updated_by),
        opt(&row.type_id),
        json_string(&row.description_html),
        opt(&row.cycle_id),
        ids(&row.label_ids),
        ids(&row.assignee_ids),
        ids(&row.module_ids),
    )
}

/// One draft row's full write columns (patch/destroy inputs).
struct DraftFullRow {
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
    created_by: Uuid,
    updated_by: Option<Uuid>,
    description_html: String,
    completed_at: Option<DateTime<Utc>>,
}

async fn draft_full_row(
    pool: &sqlx::PgPool,
    slug: &str,
    pk: &Uuid,
    own_only: Option<&Uuid>,
) -> Result<Option<DraftFullRow>, Denial> {
    #[derive(Debug, FromQueryResult)]
    struct FullDbRow {
        workspace_id: Uuid,
        project_id: Option<Uuid>,
        state_id: Option<Uuid>,
        created_by_id: Option<Uuid>,
        updated_by_id: Option<Uuid>,
        description_html: String,
        completed_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    }
    let mut stmt = Query::select();
    stmt.columns([
        (a("draft_issues"), draft_issues::Column::WorkspaceId),
        (a("draft_issues"), draft_issues::Column::ProjectId),
        (a("draft_issues"), draft_issues::Column::StateId),
        (a("draft_issues"), draft_issues::Column::CreatedById),
        (a("draft_issues"), draft_issues::Column::UpdatedById),
        (a("draft_issues"), draft_issues::Column::DescriptionHtml),
        (a("draft_issues"), draft_issues::Column::CompletedAt),
    ])
    .from_as(draft_issues::Entity, a("draft_issues"))
    .and_where(
        col("draft_issues", draft_issues::Column::WorkspaceId)
            .binary(BinOper::Equal, workspace_id_sub(slug)),
    )
    .and_where(col("draft_issues", draft_issues::Column::DeletedAt).is_null())
    .and_where(col("draft_issues", draft_issues::Column::Id).eq(*pk))
    .limit(1);
    if let Some(user) = own_only {
        stmt.and_where(col("draft_issues", draft_issues::Column::CreatedById).eq(*user));
    }
    let rows = query_built::<FullDbRow>(&orm(pool), &stmt).await?;
    Ok(rows.into_iter().next().map(|row| DraftFullRow {
        workspace_id: row.workspace_id,
        project_id: row.project_id,
        state_id: row.state_id,
        created_by: row.created_by_id.expect("draft created_by non-null"),
        updated_by: row.updated_by_id,
        description_html: row.description_html,
        completed_at: row.completed_at.map(|moment| moment.with_timezone(&Utc)),
    }))
}

/// W36 PATCH (`draft.py:158-183`): ADMIN+MEMBER + creator (dead `Issue`
/// branch), own-or-404 (`{"error": "Issue not found"}`), partial
/// validation + object leg, m2m/cycle/module replace, row save, 204.
async fn draft_patch(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path((slug, pk)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> HandlerResult {
    let draft_pk = match parse_pk(&pk) {
        Ok(pk) => pk,
        Err(response) => return Ok(response),
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    // The creator bypass looks the draft pk up in the ISSUE table
    // (`model=Issue`, `draft.py:159`) — ported as-is.
    let is_creator = creator_exists(&pool, "issues", &draft_pk, &user_id).await?;
    let gate = gates::Gate::WorkspaceCreator {
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
        model: gates::CreatorModel::Issue,
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, is_creator).await? {
        return Ok(denied);
    }
    // The own-or-404 fetch runs BEFORE `request.data` is touched
    // (`draft.py:163` vs `:168`): a missing draft 404s even for a malformed
    // body, and the `.get("project_id")` on a non-dict body raises
    // `AttributeError` → the generic 500 (both probed live).
    let Some(issue) = draft_full_row(&pool, &slug, &draft_pk, Some(&user_id)).await? else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            DRAFT_PATCH_404_BODY.to_owned(),
        ));
    };
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_data_get(&input) {
        return Ok(response);
    }
    // Context project (`:168`, see `patch_context_project`): resolved
    // here but consumed lazily — field validation below runs first, so
    // field errors win over garbage, and garbage no arm touches rides to
    // a 204 (the patch save never consumes the context; probed live).
    let (context_project, context_garbage) =
        patch_context_project(input.get("project_id"), issue.project_id);
    let timezone = actor_timezone(&pool, &user_id).await?;
    let values = match validate_shape(&pool, &input, DRAFT_FIELDS, true, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    let object = match draft_object_leg(&pool, &values, context_project, context_garbage).await {
        Ok(object) => object,
        Err(response) => return Ok(response),
    };
    let now = utc_now_micros();
    // `update()`: m2m legs run BEFORE the row save; link audit inherits
    // the issue's PRE-save ids.
    if has_key(&values, "assignee_ids") {
        soft_delete_draft_links(&pool, "draft_issue_assignees", &draft_pk, &now)
            .await
            .map_err(|_| Denial::ServerError)?;
        if !object.assignee_ids.is_empty() {
            if let Err(response) = insert_draft_links(
                &pool,
                &draft_pk,
                &issue.workspace_id,
                &issue.project_id,
                &issue.created_by,
                &issue.updated_by,
                &object.assignee_ids,
                &[],
            )
            .await
            {
                return Ok(response);
            }
        }
    }
    if has_key(&values, "label_ids") {
        soft_delete_draft_links(&pool, "draft_issue_labels", &draft_pk, &now)
            .await
            .map_err(|_| Denial::ServerError)?;
        if !object.label_ids.is_empty() {
            if let Err(response) = insert_draft_links(
                &pool,
                &draft_pk,
                &issue.workspace_id,
                &issue.project_id,
                &issue.created_by,
                &issue.updated_by,
                &[],
                &object.label_ids,
            )
            .await
            {
                return Ok(response);
            }
        }
    }
    // Cycle (`:196-204`): the `"not_provided"` sentinel skips everything;
    // any other value deletes all, then creates when truthy.
    let cycle_input = input.get("cycle_id");
    let cycle_provided = match cycle_input {
        None => false,
        Some(Value::String(text)) if text == "not_provided" => false,
        _ => true,
    };
    if cycle_provided {
        soft_delete_draft_links(&pool, "draft_issue_cycles", &draft_pk, &now)
            .await
            .map_err(|_| Denial::ServerError)?;
        if cycle_input.is_some_and(json_is_truthy) {
            let cycle_id = match cycle_input.and_then(coerce_link_uuid) {
                Some(id) => id,
                None => {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_DETAIL_BODY.to_owned(),
                    ))
                }
            };
            // `objects.create` runs `save()`, and `BaseModel.save` OVERWRITES
            // the passed audit ids with the actor/`None` on add
            // (`db/models/base.py:36-39`, probed live) — the instance ids
            // `update()` passes (`draft.py:269-276`) never reach the row.
            let link = Uuid::new_v4();
            let patch_now: DateTimeWithTimeZone = now.into();
            let cycle_model = draft_issue_cycles::ActiveModel {
                id: Set(link),
                created_at: Set(patch_now),
                updated_at: Set(patch_now),
                created_by_id: Set(Some(user_id)),
                updated_by_id: Set(None),
                deleted_at: Set(None),
                workspace_id: Set(issue.workspace_id),
                project_id: Set(issue.project_id),
                draft_issue_id: Set(draft_pk),
                cycle_id: Set(cycle_id),
            };
            if let Err(error) = cycle_model.insert(&orm(&pool)).await {
                if sea_is_integrity(&error) {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_PAYLOAD_BODY.to_owned(),
                    ));
                }
                return Err(Denial::ServerError);
            }
        }
    }
    // Modules (`:206-218`): a non-`None` raw value deletes all, then bulks
    // the iteration (which `TypeError`s on numbers/bools AFTER the
    // deletes — no transaction).
    if let Some(raw) = input.get("module_ids") {
        if !matches!(raw, Value::Null) {
            soft_delete_draft_links(&pool, "draft_issue_modules", &draft_pk, &now)
                .await
                .map_err(|_| Denial::ServerError)?;
            let items = match link_items(raw) {
                Some(items) => items,
                None => {
                    return Ok(json_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        SERVER_ERROR_BODY.to_owned(),
                    ))
                }
            };
            let mut module_ids = Vec::with_capacity(items.len());
            for item in &items {
                if matches!(item, Value::Null) {
                    // `to_python(None)` passes `None` through — the NULL
                    // insert raises `IntegrityError` → 400 below.
                    module_ids.push(None);
                    continue;
                }
                match coerce_link_uuid(item) {
                    Some(id) => module_ids.push(Some(id)),
                    None => {
                        return Ok(json_response(
                            StatusCode::BAD_REQUEST,
                            INVALID_DETAIL_BODY.to_owned(),
                        ))
                    }
                }
            }
            if let Err(error) = insert_draft_modules(
                &pool,
                &draft_pk,
                &issue.workspace_id,
                &issue.project_id,
                &issue.created_by,
                &issue.updated_by,
                &module_ids,
            )
            .await
            {
                if sea_is_integrity(&error) {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_PAYLOAD_BODY.to_owned(),
                    ));
                }
                return Err(Denial::ServerError);
            }
        }
    }
    // `super().update` (`:220-224`): setattr validated keys, then `save()`.
    // The `project` input key (writable pk) re-points `workspace` via
    // `WorkspaceBaseModel.save`; the state legs run over the POST-setattr
    // project/state.
    let mut post_project = issue.project_id;
    let mut post_workspace = issue.workspace_id;
    // The `project` key (writable pk): a value re-points `workspace` via
    // `WorkspaceBaseModel.save`; an explicit `null` clears the project
    // (the workspace stays — `if self.project` skips `None`).
    if let Some(resolved) = nullable_uuid(&values, "project") {
        post_project = resolved;
        if let Some(project) = resolved {
            // Validated live above (race aside); re-fetch for the workspace.
            match project_row(&pool, &project).await? {
                Some(row) => post_workspace = row.workspace_id,
                None => {
                    return Ok(json_response(
                        StatusCode::NOT_FOUND,
                        OBJECT_NOT_FOUND_BODY.to_owned(),
                    ))
                }
            }
        }
    }
    let post_state = match dual_uuid(&values, "state_id", "state") {
        Some(resolved) => resolved,
        None => issue.state_id,
    };
    // `setattr` runs before `save()`: an explicit `completed_at` input rides
    // the instance into the state legs (the `None`-state leg keeps it); only
    // an absent key leaves the stored value.
    let input_completed = if has_key(&values, "completed_at") {
        opt_datetime(&values, "completed_at")
    } else {
        issue.completed_at
    };
    let (state_id, completed_at) =
        draft_save_state(&pool, post_project, post_state, input_completed, &now).await?;
    let post_html = object
        .description_html
        .clone()
        .unwrap_or(issue.description_html.clone());
    let stripped = sync_description_stripped(Some(&post_html));
    // Assemble the UPDATE: validated keys + save halves + audit stamp.
    let now_db: DateTimeWithTimeZone = now.into();
    let mut update = draft_issues::Entity::update_many();
    update = update.col_expr(draft_issues::Column::UpdatedAt, Expr::value(now_db));
    update = update.col_expr(
        draft_issues::Column::UpdatedById,
        Expr::value(Some(user_id)),
    );
    if has_key(&values, "deleted_at") {
        let deleted: Option<DateTimeWithTimeZone> =
            opt_datetime(&values, "deleted_at").map(Into::into);
        update = update.col_expr(draft_issues::Column::DeletedAt, Expr::value(deleted));
    }
    if has_key(&values, "name") {
        update = update.col_expr(
            draft_issues::Column::Name,
            Expr::value(opt_text(&values, "name")),
        );
    }
    if has_key(&values, "description_json") {
        let doc = opt_json(&values, "description_json").unwrap_or(Value::Null);
        update = update.col_expr(draft_issues::Column::DescriptionJson, Expr::value(doc));
    }
    if has_key(&values, "description_html") {
        // `save()` recomputes `stripped` from the new html; when the html
        // is untouched the recompute is value-identical (write skipped).
        update = update.col_expr(
            draft_issues::Column::DescriptionHtml,
            Expr::value(post_html.clone()),
        );
        update = update.col_expr(
            draft_issues::Column::DescriptionStripped,
            Expr::value(stripped.clone()),
        );
    }
    if has_key(&values, "priority") {
        let priority = opt_text(&values, "priority").unwrap_or_else(|| "none".to_owned());
        update = update.col_expr(draft_issues::Column::Priority, Expr::value(priority));
    }
    if has_key(&values, "start_date") {
        update = update.col_expr(
            draft_issues::Column::StartDate,
            Expr::value(nullable_date(&values, "start_date").flatten()),
        );
    }
    if has_key(&values, "target_date") {
        update = update.col_expr(
            draft_issues::Column::TargetDate,
            Expr::value(nullable_date(&values, "target_date").flatten()),
        );
    }
    if has_key(&values, "sort_order") {
        if let Some(sort) = opt_float(&values, "sort_order") {
            update = update.col_expr(draft_issues::Column::SortOrder, Expr::value(sort));
        }
    }
    if has_key(&values, "completed_at") || post_state.is_some() {
        // Recomputed whenever the post-setattr state is non-null (even for
        // name-only patches); an explicit input only survives a null state.
        let completed: Option<DateTimeWithTimeZone> = completed_at.map(Into::into);
        update = update.col_expr(draft_issues::Column::CompletedAt, Expr::value(completed));
    }
    if has_key(&values, "external_source") {
        update = update.col_expr(
            draft_issues::Column::ExternalSource,
            Expr::value(opt_text(&values, "external_source")),
        );
    }
    if has_key(&values, "external_id") {
        update = update.col_expr(
            draft_issues::Column::ExternalId,
            Expr::value(opt_text(&values, "external_id")),
        );
    }
    if has_key(&values, "project") {
        update = update.col_expr(draft_issues::Column::ProjectId, Expr::value(post_project));
        update = update.col_expr(
            draft_issues::Column::WorkspaceId,
            Expr::value(post_workspace),
        );
    }
    if dual_uuid(&values, "parent_id", "parent").is_some() {
        update = update.col_expr(
            draft_issues::Column::ParentId,
            Expr::value(dual_uuid(&values, "parent_id", "parent").flatten()),
        );
    }
    if dual_uuid(&values, "state_id", "state").is_some() || state_id != issue.state_id {
        update = update.col_expr(draft_issues::Column::StateId, Expr::value(state_id));
    }
    if has_key(&values, "estimate_point") {
        update = update.col_expr(
            draft_issues::Column::EstimatePointId,
            Expr::value(opt_uuid(&values, "estimate_point")),
        );
    }
    if has_key(&values, "type") {
        update = update.col_expr(
            draft_issues::Column::TypeId,
            Expr::value(opt_uuid(&values, "type")),
        );
    }
    update = update.filter(draft_issues::Column::Id.eq(draft_pk));
    if let Err(error) = update.exec(&orm(&pool)).await {
        if sea_is_integrity(&error) {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                INVALID_PAYLOAD_BODY.to_owned(),
            ));
        }
        return Err(Denial::ServerError);
    }
    Ok(empty_response())
}

/// Bulk soft-delete one draft link table (`QuerySet.delete()` =
/// `UPDATE … SET deleted_at`, alive rows only).
async fn soft_delete_draft_links(
    pool: &sqlx::PgPool,
    table: &str,
    draft_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), DbErr> {
    let db = orm(pool);
    let now_db: DateTimeWithTimeZone = (*now).into();
    match table {
        "draft_issue_assignees" => {
            draft_issue_assignees::Entity::update_many()
                .col_expr(
                    draft_issue_assignees::Column::DeletedAt,
                    Expr::value(now_db),
                )
                .filter(draft_issue_assignees::Column::DraftIssueId.eq(*draft_id))
                .filter(draft_issue_assignees::Column::DeletedAt.is_null())
                .exec(&db)
                .await?;
        }
        "draft_issue_labels" => {
            draft_issue_labels::Entity::update_many()
                .col_expr(draft_issue_labels::Column::DeletedAt, Expr::value(now_db))
                .filter(draft_issue_labels::Column::DraftIssueId.eq(*draft_id))
                .filter(draft_issue_labels::Column::DeletedAt.is_null())
                .exec(&db)
                .await?;
        }
        "draft_issue_cycles" => {
            draft_issue_cycles::Entity::update_many()
                .col_expr(draft_issue_cycles::Column::DeletedAt, Expr::value(now_db))
                .filter(draft_issue_cycles::Column::DraftIssueId.eq(*draft_id))
                .filter(draft_issue_cycles::Column::DeletedAt.is_null())
                .exec(&db)
                .await?;
        }
        "draft_issue_modules" => {
            draft_issue_modules::Entity::update_many()
                .col_expr(draft_issue_modules::Column::DeletedAt, Expr::value(now_db))
                .filter(draft_issue_modules::Column::DraftIssueId.eq(*draft_id))
                .filter(draft_issue_modules::Column::DeletedAt.is_null())
                .exec(&db)
                .await?;
        }
        other => unreachable!("draft link table {other}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// W36 GET (`draft.py:186-197`): retrieve
// ---------------------------------------------------------------------------

/// Retrieve (`draft.py:186-197`): ADMIN + creator(`model=Issue` — the dead
/// branch: the pk is looked up in `issues`), own-or-404
/// (`{"error": ...}` 404), the 21-key detail shape (the detail serializer
/// only re-declares `description_html` over the list serializer, so the
/// rendered keys are identical).
async fn draft_retrieve(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path((slug, pk)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> HandlerResult {
    let _ = (&headers, &body);
    let draft_pk = match parse_pk(&pk) {
        Ok(pk) => pk,
        Err(response) => return Ok(response),
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    // The creator bypass looks the draft pk up in the ISSUE table
    // (`model=Issue`, `draft.py:186`) — ported as-is.
    let is_creator = creator_exists(&pool, "issues", &draft_pk, &user_id).await?;
    let gate = gates::Gate::WorkspaceCreator {
        roles: &[ROLE_ADMIN],
        model: gates::CreatorModel::Issue,
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, is_creator).await? {
        return Ok(denied);
    }
    let timezone = actor_timezone(&pool, &user_id).await?;
    let row = reread_draft(&pool, &slug, &draft_pk).await?;
    let Some(row) = row else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    if row.created_by.as_deref() != Some(user_id.to_string().as_str()) {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    }
    Ok(json_response(StatusCode::OK, render_draft(&row, &timezone)))
}

// ---------------------------------------------------------------------------
// W36 DELETE (`draft.py:199-203`): destroy (soft delete + emit)
// ---------------------------------------------------------------------------

/// The soft-delete emit behind every `SoftDeleteModel.delete()`
/// (`db/mixins.py:72-80`): `soft_delete_related_objects.delay(app_label,
/// model_name, pk, using=None)`.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, pk: &Uuid) {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    let message = TaskMessage::new(
        SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    );
    let job = NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `draft_issue.delete()` (`draft.py:202,308` + `db/mixins.py:72-80`): a
/// SOFT delete — `deleted_at` set, then the full `save()` legs (the state
/// default + `completed_at` recompute over the row's own project/state,
/// `updated_by` = the actor, `updated_at` = now), then the
/// `soft_delete_related_objects` emit. `save()` writes every column; only
/// the recomputed ones can differ, so only they are SET.
async fn soft_delete_draft(
    pool: &sqlx::PgPool,
    draft_id: &Uuid,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
    actor: &Uuid,
) -> Result<(), Denial> {
    let now = utc_now_micros();
    // The delete-time `save()` legs: a null state takes the project
    // default (which itself needs no touch — `default_draft_state` filters
    // by id); a set state recomputes `completed_at` off its group.
    let (resolved_state, completed_at) = match state_id {
        None => (default_draft_state_opt(pool, project_id).await?, None),
        Some(state) => {
            let group = state_row(pool, &state)
                .await?
                .map(|row| row.group)
                .ok_or(Denial::NotFound)?;
            let completed = if group == "completed" {
                Some(now)
            } else {
                None
            };
            (Some(state), completed)
        }
    };
    // NOTE: `delete()` sets `deleted_at` BEFORE `save()`; `save()` keeps
    // the input `completed_at` on the null-state leg — but the instance's
    // `completed_at` is whatever the row holds, not `None`. A null-state
    // draft keeps its stored `completed_at` through the delete.
    let keep_completed = state_id.is_none();
    let now_db: DateTimeWithTimeZone = now.into();
    if keep_completed {
        draft_issues::Entity::update_many()
            .col_expr(draft_issues::Column::DeletedAt, Expr::value(now_db))
            .col_expr(draft_issues::Column::UpdatedAt, Expr::value(now_db))
            .col_expr(draft_issues::Column::UpdatedById, Expr::value(Some(*actor)))
            .col_expr(draft_issues::Column::StateId, Expr::value(resolved_state))
            .filter(draft_issues::Column::Id.eq(*draft_id))
            .exec(&orm(pool))
            .await
            .map_err(|_| Denial::ServerError)?;
    } else {
        let completed_db: Option<DateTimeWithTimeZone> = completed_at.map(Into::into);
        draft_issues::Entity::update_many()
            .col_expr(draft_issues::Column::DeletedAt, Expr::value(now_db))
            .col_expr(draft_issues::Column::UpdatedAt, Expr::value(now_db))
            .col_expr(draft_issues::Column::UpdatedById, Expr::value(Some(*actor)))
            .col_expr(draft_issues::Column::CompletedAt, Expr::value(completed_db))
            .filter(draft_issues::Column::Id.eq(*draft_id))
            .exec(&orm(pool))
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    enqueue_soft_delete(pool, "draftissue", draft_id).await;
    Ok(())
}

/// The null-state default shared by the save/delete paths (the row's
/// own project; `None` project resolves to no state). `project=...` in
/// the default filter touches `self.project` first (`_base_manager`,
/// unscoped — verified in Django 4.2's `ForwardManyToOneDescriptor`): a
/// fully-missing row raises → the 404 branch.
async fn default_draft_state_opt(
    pool: &sqlx::PgPool,
    project_id: Option<Uuid>,
) -> Result<Option<Uuid>, Denial> {
    match project_id {
        None => Ok(None),
        Some(project) => {
            touch_save_project(pool, &project).await?;
            default_draft_state(pool, &project).await
        }
    }
}

/// `self.project` on a save (`_base_manager`, UNSCOPED — soft-deleted
/// rows resolve): a missing row raises `RelatedObjectDoesNotExist` → the
/// required-object 404.
async fn touch_save_project(pool: &sqlx::PgPool, project_id: &Uuid) -> Result<(), Denial> {
    let hit: Option<Uuid> = projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .filter(projects::Column::Id.eq(*project_id))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    hit.map(|_| ()).ok_or(Denial::NotFound)
}

/// Destroy (`draft.py:199-203`): ADMIN + creator(`model=DraftIssue`) gate,
/// then `.get(workspace__slug, pk)` (miss → 404 `{"error": ...}` — the
/// `ObjectDoesNotExist` branch, NOT the 500), the soft delete + emit, 204.
async fn draft_destroy(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path((slug, pk)): axum::extract::Path<(String, String)>,
) -> HandlerResult {
    let draft_pk = match parse_pk(&pk) {
        Ok(pk) => pk,
        Err(response) => return Ok(response),
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let is_creator = creator_exists(&pool, "draft_issues", &draft_pk, &user_id).await?;
    let gate = gates::Gate::WorkspaceCreator {
        roles: &[ROLE_ADMIN],
        model: gates::CreatorModel::DraftIssue,
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, is_creator).await? {
        return Ok(denied);
    }
    // `.get()` over the default (live) manager: miss → `DoesNotExist` →
    // the 404 branch.
    #[derive(Debug, FromQueryResult)]
    struct DestroyRow {
        project_id: Option<Uuid>,
        state_id: Option<Uuid>,
    }
    let mut destroy_stmt = Query::select();
    destroy_stmt
        .columns([
            (a("d"), draft_issues::Column::ProjectId),
            (a("d"), draft_issues::Column::StateId),
        ])
        .from_as(draft_issues::Entity, a("d"))
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            a("w"),
            col("w", workspaces::Column::Id).equals((a("d"), draft_issues::Column::WorkspaceId)),
        )
        .and_where(col("w", workspaces::Column::Slug).eq(slug.clone()))
        .and_where(col("d", draft_issues::Column::Id).eq(draft_pk))
        .and_where(col("d", draft_issues::Column::DeletedAt).is_null())
        .limit(1);
    let destroy_rows = query_built::<DestroyRow>(&orm(&pool), &destroy_stmt).await?;
    let destroy_row = destroy_rows
        .into_iter()
        .next()
        .map(|row| (row.project_id, row.state_id));
    let Some((project_id, state_id)) = destroy_row else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    soft_delete_draft(&pool, &draft_pk, project_id, state_id, &user_id).await?;
    Ok(empty_response())
}

// ---------------------------------------------------------------------------
// W37 POST (`draft.py:205-312`): draft-to-issue
// ---------------------------------------------------------------------------

/// Validated id-list access: `None` when absent or explicit `null` (the
/// lists reject `null`, so only absence yields `None` in practice).
fn opt_ids(values: &[(String, FieldValue)], name: &str) -> Option<Vec<Uuid>> {
    match find_value(values, name) {
        Some(FieldValue::Ids(ids)) => Some(ids.clone()),
        _ => None,
    }
}

/// `Pod.all_objects.get(pk)` for `assigned_pod_id` (`issue.py:153-155`):
/// NO soft-delete filter — tombstones resolve so `validate()` answers
/// "pod has been deleted". Field validation already proved existence; a
/// race miss 500s.
async fn fetch_pod_ref(pool: &sqlx::PgPool, id: &Uuid) -> Result<PodRef, Denial> {
    create_q::fetch_pod_ref(&orm(pool), *id)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)
}

/// `ProjectMember` assignee filter (`issue.py:338-344`): the silent drop,
/// `-created_at` order.
async fn filter_assignee_ids(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Denial> {
    create_q::filter_assignee_members(&orm(pool), *project_id, ids)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `Label` filter (`issue.py:347-354`): the silent drop, `-created_at`
/// order. The context project is never `None` here (the project guard),
/// so the null-project variant stays unissued.
async fn filter_label_ids(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Denial> {
    create_q::filter_project_labels(&orm(pool), Some(*project_id), ids)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `Project.objects.filter(pk).first()` for the managed arm
/// (`issue.py:296-299`).
async fn fetch_issue_project(
    pool: &sqlx::PgPool,
    id: &Uuid,
) -> Result<Option<ResolvedProject>, Denial> {
    create_q::fetch_resolved_project(&orm(pool), *id)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `managed_llm_profile` seam (`model_provider.py:67-68,85-91`): the
/// verdict is `bool(api_key_encrypted)`.
async fn assistant_has_api_key(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<bool, Denial> {
    let key: Option<Option<Vec<u8>>> = assistant_user_llm_config::Entity::find()
        .select_only()
        .column(assistant_user_llm_config::Column::ApiKeyEncrypted)
        .filter(assistant_user_llm_config::Column::UserId.eq(*user_id))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(key.flatten().is_some_and(|key| !key.is_empty()))
}

async fn enrolled_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, Denial> {
    let hit = admission_q::managed_runner_probe(
        &orm(pool),
        ManagedRunnerProbe::Enrolled,
        *user_id,
        *project_id,
        *workspace_id,
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(hit.is_some())
}

async fn online_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, Denial> {
    let bound = Utc::now() - chrono::Duration::seconds(HEARTBEAT_GRACE_SECS);
    let hit = admission_q::managed_runner_probe(
        &orm(pool),
        ManagedRunnerProbe::Online(bound),
        *user_id,
        *project_id,
        *workspace_id,
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(hit.is_some())
}

/// The paused tiebreaker (`policy.py:88-91`, `09738d222`).
async fn paused_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, Denial> {
    let hit = admission_q::managed_runner_probe(
        &orm(pool),
        ManagedRunnerProbe::Paused,
        *user_id,
        *project_id,
        *workspace_id,
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(hit.is_some())
}

/// `request.user` flags for the executor arm (in-memory on Django; one
/// read here, issued only past a managed pin + resolved project).
async fn viewer_user_flags(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<UserFlags, Denial> {
    #[derive(Debug, FromQueryResult)]
    struct FlagsRow {
        is_active: bool,
        is_bot: bool,
    }
    let row: Option<FlagsRow> = users::Entity::find()
        .select_only()
        .columns([users::Column::IsActive, users::Column::IsBot])
        .filter(users::Column::Id.eq(*user_id))
        .into_model()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ServerError)?;
    Ok(UserFlags {
        is_active: row.is_active,
        is_bot: row.is_bot,
    })
}

/// Map a `validate()` failure to its wire response. `ContextMissing` is
/// the assignee `KeyError`, which `handle_exception` renders as 400
/// `{"error": ...}` — unreachable here (the project guard pins the
/// context), mapped all the same. Anything else list-wraps through
/// `wire_body()` (dicts as-is, bare strings under `non_field_errors`).
fn issue_validate_error_response(error: &CreateValidateError) -> Response {
    match error {
        CreateValidateError::ContextMissing { .. } => json_bad_request(KEY_ERROR_BODY),
        _ => match error.wire_body() {
            Some(body) => json_bad_request(&body.to_string()),
            None => Denial::ServerError.into_response(),
        },
    }
}

/// The create-only `validate()` driver: attrs from the field pass (dual
/// sources resolved — the auto field wins on key presence), exactly the
/// reachable probes in Django's order, then the kernel verdict.
/// `instance=None` throughout: no sync lock, no pod-reassign /
/// mid-flight probes, no triage leg.
#[allow(clippy::result_large_err)]
async fn run_issue_create_validate(
    pool: &sqlx::PgPool,
    state: &AppState,
    values: &[(String, FieldValue)],
    project_id: &Uuid,
    actor: &Uuid,
    assigned_pod: Option<Option<PodRef>>,
) -> Result<ValidatedAttrs, Response> {
    let db_error = || Denial::ServerError.into_response();
    let text = |key: &str| match find_value(values, key) {
        Some(FieldValue::Text(text)) => Some(text.as_str()),
        _ => None,
    };
    let uuid_value = |key: &str| match find_value(values, key) {
        Some(FieldValue::Uuid(id)) => Some(*id),
        _ => None,
    };
    // Dual sources: the auto field wins whenever its KEY is present —
    // even when null (DRF loops fields in order, `set_value` overwrites).
    let state_value = dual_uuid(values, "state_id", "state").flatten();
    let parent_value = dual_uuid(values, "parent_id", "parent").flatten();
    let assignee_ids = opt_ids(values, "assignee_ids");
    let label_ids = opt_ids(values, "label_ids");
    let agent_executor: Option<Option<&str>> = match find_value(values, "agent_executor") {
        None => None,
        Some(FieldValue::Null) => Some(None),
        Some(FieldValue::Text(executor)) => Some(Some(executor.as_str())),
        _ => None,
    };
    let description_stripped: Option<Option<&str>> =
        match find_value(values, "description_stripped") {
            None => None,
            Some(FieldValue::Null) => Some(None),
            Some(FieldValue::Text(stripped)) => Some(Some(stripped.as_str())),
            _ => None,
        };
    let description_json: Option<&Value> = match find_value(values, "description_json") {
        Some(FieldValue::Json(value)) => Some(value),
        _ => None,
    };
    let attrs = ValidateAttrs {
        locked: LockedIssueAttrs {
            name: text("name"),
            description_html: text("description_html"),
            description_json,
            description_stripped,
            description_binary_present: false,
        },
        start_date: find_value(values, "start_date").and_then(|value| match value {
            FieldValue::Date(date) => Some(*date),
            _ => None,
        }),
        target_date: find_value(values, "target_date").and_then(|value| match value {
            FieldValue::Date(date) => Some(*date),
            _ => None,
        }),
        assigned_pod,
        // Drafts convert to issues without a runner (design §7: the
        // web app and v1 surfaces own the write path).
        assigned_runner: None,
        agent_executor,
        description_html: text("description_html"),
        description_binary: None,
        assignee_ids: assignee_ids.as_deref(),
        label_ids: label_ids.as_deref(),
        state: state_value,
        parent: parent_value,
        estimate_point: uuid_value("estimate_point"),
        attrs_project: None,
    };

    // Step 3 reachability (pure): the different-project / deleted legs
    // return before any executor probe.
    let pod_pure_fail = match assigned_pod {
        Some(Some(pod)) => pod.project_id != *project_id || pod.deleted,
        _ => false,
    };

    // Step 4 probes (unreached when step 3 returns, when the key is
    // absent/null, on unknown kinds, and past an unconfigured cloud or a
    // missing project — the predicates mirror the kernel arm-for-arm).
    let mut fetched_project: Option<ResolvedProject> = None;
    let mut llm_profile = LlmProfile {
        available: false,
        reason_code: ManagedRunnerReason::LLM_CONFIG_MISSING.to_owned(),
    };
    let mut enrolled_exists = false;
    let mut online_exists = false;
    let mut paused_exists = false;
    let mut viewer: Option<UserFlags> = None;
    if !pod_pure_fail {
        if let Some(Some(executor)) = agent_executor {
            if let Some(kind) = AgentExecutorKind::from_value(executor) {
                let cloud_blocked = kind == AgentExecutorKind::CloudAgent
                    && !cloud_agent_is_configured(&state.settings().cloud_agent);
                if !cloud_blocked && kind == AgentExecutorKind::ManagedRunner {
                    fetched_project = fetch_issue_project(pool, project_id)
                        .await
                        .map_err(|_| db_error())?;
                    if fetched_project.is_some() {
                        let flags = viewer_user_flags(pool, actor)
                            .await
                            .map_err(|_| db_error())?;
                        if managed_runner_is_enabled(&state.settings().managed_runner)
                            && flags.is_active
                            && !flags.is_bot
                        {
                            let keyed = assistant_has_api_key(pool, actor)
                                .await
                                .map_err(|_| db_error())?;
                            llm_profile = LlmProfile {
                                available: false,
                                reason_code: if keyed {
                                    ManagedRunnerReason::BYOK_UNSUPPORTED.to_owned()
                                } else {
                                    ManagedRunnerReason::LLM_CONFIG_MISSING.to_owned()
                                },
                            };
                            if llm_profile.available {
                                let resolved = fetched_project.ok_or_else(db_error)?;
                                enrolled_exists = enrolled_runner(
                                    pool,
                                    actor,
                                    &resolved.project_id,
                                    &resolved.workspace_id,
                                )
                                .await
                                .map_err(|_| db_error())?;
                                if enrolled_exists {
                                    online_exists = online_runner(
                                        pool,
                                        actor,
                                        &resolved.project_id,
                                        &resolved.workspace_id,
                                    )
                                    .await
                                    .map_err(|_| db_error())?;
                                    // `09738d222`: the paused tiebreaker
                                    // runs only when nothing is online.
                                    if !online_exists {
                                        paused_exists = paused_runner(
                                            pool,
                                            actor,
                                            &resolved.project_id,
                                            &resolved.workspace_id,
                                        )
                                        .await
                                        .map_err(|_| db_error())?;
                                    }
                                }
                            }
                        }
                        viewer = Some(flags);
                    }
                }
            }
        }
    }

    // Steps 7-8: non-empty lists only (empty skips both filters).
    let filtered_assignees = match assignee_ids.as_deref() {
        Some(ids) if !ids.is_empty() => Some(
            filter_assignee_ids(pool, project_id, ids)
                .await
                .map_err(|_| db_error())?,
        ),
        _ => None,
    };
    let filtered_labels = match label_ids.as_deref() {
        Some(ids) if !ids.is_empty() => Some(
            filter_label_ids(pool, project_id, ids)
                .await
                .map_err(|_| db_error())?,
        ),
        _ => None,
    };
    // Steps 9-11: touched + non-null only — a null clears without a probe.
    let state_ok = match state_value {
        Some(id) => Some(
            create_q::state_exists(&orm(pool), Some(*project_id), id)
                .await
                .map_err(|_| db_error())?,
        ),
        None => None,
    };
    let parent_ok = match parent_value {
        Some(id) => Some(
            create_q::parent_exists(&orm(pool), Some(*project_id), id)
                .await
                .map_err(|_| db_error())?,
        ),
        None => None,
    };
    let estimate_ok = match attrs.estimate_point {
        Some(id) => Some(
            create_q::estimate_exists(&orm(pool), Some(*project_id), id)
                .await
                .map_err(|_| db_error())?,
        ),
        None => None,
    };
    let policy = ExecutorPolicy {
        cloud: &state.settings().cloud_agent,
        managed: &state.settings().managed_runner,
        viewer: viewer.as_ref(),
    };
    let probes = ValidateProbes {
        // Unreached on create (`instance=None`): step 1 never runs, the
        // pod-reassign / mid-flight legs need an instance, the triage
        // manager leg is off.
        git_sync_exists: &|| false,
        github_sync_exists: &|| false,
        has_active_run: &|| false,
        filter_assignees: &|_| filtered_assignees.clone().unwrap_or_default(),
        filter_labels: &|_| filtered_labels.clone().unwrap_or_default(),
        state_exists: &|| state_ok.unwrap_or(false),
        triage_state_exists: &|| false,
        parent_exists: &|| parent_ok.unwrap_or(false),
        estimate_exists: &|| estimate_ok.unwrap_or(false),
        fetch_project: &|| fetched_project,
        llm_profile: &|| llm_profile.clone(),
        enrolled_exists: &|| enrolled_exists,
        online_exists: &|| online_exists,
        paused_exists: &|| paused_exists,
        runner_error: &|| None,
    };
    let ctx = ValidateContext {
        project_id: Some(*project_id),
        allow_triage_state: false,
    };
    match issue_create_validate(&attrs, &ctx, None, &policy, &probes) {
        Ok(validated) => Ok(validated),
        Err(error) => Err(issue_validate_error_response(&error)),
    }
}

// The save row is the domain's `IssueFieldSet` (imported at the top):
// every `issues` column as owned values. Assembly keeps building it
// here (validation-adjacent); the INSERT moved to `domain::issues`
// (ORM-03).
/// Map a domain-write failure onto the save arms: a dangling-row miss
/// 404s, an integrity violation is the invalid-payload 400, anything else
/// 500s.
fn domain_denial(error: domain::DomainError) -> Denial {
    if error.is_not_found() {
        Denial::NotFound
    } else if error.is_integrity_violation() {
        Denial::BadError("The payload is not valid".to_owned())
    } else {
        Denial::ServerError
    }
}

/// The fresh-row assembly: the row plus the raw m2m inputs.
type NewIssueAssembly = (IssueSaveRow, Option<Vec<Uuid>>, Option<Vec<Uuid>>);

/// Build the fresh row for `Issue.objects.create(...)`: model defaults
/// everywhere the input is absent, the sanitized `description_html`, and
/// placeholders for the transaction legs (`sequence_id`, `sort_order`,
/// `completed_at`, `description_stripped`). Returns the row plus the raw
/// m2m inputs.
fn assemble_new_issue(
    values: &[(String, FieldValue)],
    filtered: &ValidatedAttrs,
    pod: Option<Uuid>,
    project_id: &Uuid,
    workspace_id: &Uuid,
    actor: &Uuid,
) -> Result<NewIssueAssembly, Denial> {
    let name = opt_text(values, "name").ok_or(Denial::ServerError)?;
    let description_html = filtered
        .description_html
        .clone()
        .or_else(|| opt_text(values, "description_html"))
        .unwrap_or_else(|| "<p></p>".to_owned());
    let row = IssueSaveRow {
        id: Uuid::new_v4(),
        created_at: utc_now_micros(),
        updated_at: utc_now_micros(),
        created_by_id: Some(*actor),
        updated_by_id: None,
        deleted_at: opt_datetime(values, "deleted_at"),
        project_id: *project_id,
        workspace_id: *workspace_id,
        parent_id: dual_uuid(values, "parent_id", "parent").flatten(),
        state_id: dual_uuid(values, "state_id", "state").flatten(),
        point: opt_int(values, "point").map(|point| point as i32),
        estimate_point_id: opt_uuid(values, "estimate_point"),
        name,
        description_json: opt_json(values, "description_json").unwrap_or(Value::Object(Map::new())),
        description_html,
        description_stripped: None,
        description_binary: None,
        priority: opt_text(values, "priority").unwrap_or_else(|| "none".to_owned()),
        complexity_score: opt_int(values, "complexity_score").unwrap_or(0) as i32,
        start_date: opt_date(values, "start_date"),
        target_date: opt_date(values, "target_date"),
        sequence_id: 0,
        sort_order: opt_float(values, "sort_order").unwrap_or(65535.0),
        completed_at: opt_datetime(values, "completed_at"),
        archived_at: opt_date(values, "archived_at"),
        is_draft: opt_bool(values, "is_draft").unwrap_or(false),
        external_source: opt_text(values, "external_source"),
        external_id: opt_text(values, "external_id"),
        type_id: opt_uuid(values, "type"),
        git_work_branch: opt_text(values, "git_work_branch").unwrap_or_default(),
        workpad: String::new(),
        created_via: opt_text(values, "created_via"),
        assigned_pod_id: pod,
        assigned_runner_id: None,
        agent_executor: opt_text(values, "agent_executor"),
    };
    Ok((
        row,
        opt_ids(values, "assignee_ids"),
        opt_ids(values, "label_ids"),
    ))
}

fn host_settings_of(state: &AppState) -> HostSettings<'_> {
    let urls = &state.settings().urls;
    HostSettings {
        web_url: urls.web_url.as_deref(),
        app_base_url: urls.app_base_url.as_deref(),
        admin_base_url: None,
        space_base_url: None,
        admin_base_path: None,
        space_base_path: None,
    }
}

/// `base_host` raises past the save (the issue + no jobs persist on a
/// misconfigured host), so the origin resolves here, not up front.
fn request_origin(state: &AppState) -> Result<String, Denial> {
    let settings = host_settings_of(state);
    let origin = base_host(&settings, false, false, true);
    if origin.is_empty() {
        return Err(Denial::ServerError);
    }
    Ok(origin)
}

/// One draft `issue_activity.delay(...)` through the jobs kernel.
/// Enqueue failures log and the response stands (Django's broker publish
/// is equally fire-and-forget from the view's perspective).
async fn enqueue_activity_emit(pool: &sqlx::PgPool, emit: &wtask::WorkspaceIssueActivityEmit) {
    let message = TaskMessage::new(emit.task_name(), vec![], emit.kwargs());
    let job = NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// CPython `str()` over a parsed-JSON value (the module-ref dump's
/// `str(module)`, `draft.py:287`): strings bare, ints plain, floats via
/// `repr`, bools/`None` capitalized, composites via `repr`.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                number.to_string()
            } else {
                number
                    .as_f64()
                    .map(json_float_str)
                    .unwrap_or_else(|| number.to_string())
            }
        }
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        python_repr(&Value::String(key.clone())),
                        python_repr(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr()` over a parsed-JSON value (nested inside `str()` of a
/// composite): strings single-quoted with `\\'`/`\\\\` escapes.
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let mut out = String::with_capacity(text.len() + 2);
            out.push('\'');
            for ch in text.chars() {
                match ch {
                    '\'' => out.push_str("\\'"),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    ch => out.push(ch),
                }
            }
            out.push('\'');
            out
        }
        _ => python_str(value),
    }
}

/// `serializers.serialize("json", [cycle_issue])` (`draft.py:258-261`),
/// byte-exact (probed live): spaced separators, millis-truncated `Z`
/// datetimes (no fraction when zero), `created_by` = the ACTOR (the
/// `objects.create` → `save()` overwrites the passed draft ids),
/// `updated_by`/`deleted_at` null, field order
/// `created_at, updated_at, created_by, updated_by, deleted_at, project,
/// workspace, issue, cycle`.
#[allow(clippy::too_many_arguments)]
fn cycle_serialize_text(
    id: &Uuid,
    created_at: &DateTime<Utc>,
    updated_at: &DateTime<Utc>,
    actor: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    issue_id: &Uuid,
    cycle_id: &Uuid,
) -> String {
    format!(
        "[{{\"model\": \"db.cycleissue\", \"pk\": \"{id}\", \"fields\": {{\"created_at\": \"{created}\", \"updated_at\": \"{updated}\", \"created_by\": \"{actor}\", \"updated_by\": null, \"deleted_at\": null, \"project\": \"{project_id}\", \"workspace\": \"{workspace_id}\", \"issue\": \"{issue_id}\", \"cycle\": \"{cycle_id}\"}}}}]",
        created = render_encoder_datetime(created_at),
        updated = render_encoder_datetime(updated_at),
    )
}

/// The cycle branch's `current_instance`
/// (`draft.py:256-262`): `json.dumps({"updated_cycle_issues": None,
/// "created_cycle_issues": <serialize text>})` — insertion order
/// (`updated_` first), spaced separators.
fn cycle_current_instance(created_cycle_issues: &str) -> String {
    let mut inner = String::new();
    python_dump_str(&mut inner, created_cycle_issues);
    format!("{{\"updated_cycle_issues\": null, \"created_cycle_issues\": {inner}}}")
}

/// Fresh m2m members for the 201 body, in M2M-manager order (target
/// `-created_at`): `serializer.data` renders the links off the saved
/// instance. The through table carries NO soft-delete filter — the m2m
/// manager joins it raw, so replaced links still list; only the target
/// side (`labels.deleted_at`) filters.
async fn fetch_issue_m2m(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<(Vec<Uuid>, Vec<Uuid>), Denial> {
    #[derive(Debug, FromQueryResult)]
    struct IdRow {
        id: Uuid,
    }
    let db = orm(pool);
    let mut assignee_stmt = Query::select();
    assignee_stmt
        .column((a("u"), users::Column::Id))
        .from_as(users::Entity, a("u"))
        .join_as(
            JoinType::InnerJoin,
            issue_assignees::Entity,
            a("ia"),
            col("ia", issue_assignees::Column::AssigneeId).equals((a("u"), users::Column::Id)),
        )
        .and_where(col("ia", issue_assignees::Column::IssueId).eq(*issue_id))
        .order_by((a("u"), users::Column::CreatedAt), Order::Desc);
    let assignee_rows = query_built::<IdRow>(&db, &assignee_stmt).await?;
    let mut label_stmt = Query::select();
    label_stmt
        .column((a("l"), labels::Column::Id))
        .from_as(labels::Entity, a("l"))
        .join_as(
            JoinType::InnerJoin,
            issue_labels::Entity,
            a("il"),
            col("il", issue_labels::Column::LabelId).equals((a("l"), labels::Column::Id)),
        )
        .and_where(col("il", issue_labels::Column::IssueId).eq(*issue_id))
        .and_where(col("l", labels::Column::DeletedAt).is_null())
        .order_by((a("l"), labels::Column::CreatedAt), Order::Desc);
    let label_rows = query_built::<IdRow>(&db, &label_stmt).await?;
    Ok((
        assignee_rows.into_iter().map(|row| row.id).collect(),
        label_rows.into_iter().map(|row| row.id).collect(),
    ))
}

/// Render `serializer.data` after the draft-to-issue save: the 41-key
/// create representation off the in-memory row (no re-read) plus the
/// fresh m2m members, datetimes in DRF `Z` form in the actor zone,
/// `assignee_ids`/`label_ids` echoed from the raw input (falsy → `[]`).
async fn render_draft_to_issue_body(
    pool: &sqlx::PgPool,
    row: &IssueSaveRow,
    timezone: &Tz,
    initial_assignees: Option<&Value>,
    initial_labels: Option<&Value>,
) -> Result<String, Denial> {
    let (assignees, labels) = fetch_issue_m2m(pool, &row.id).await?;
    let id = row.id.to_string();
    let project = row.project_id.to_string();
    let workspace = row.workspace_id.to_string();
    let state = row.state_id.map(|value| value.to_string());
    let parent = row.parent_id.map(|value| value.to_string());
    let estimate_point = row.estimate_point_id.map(|value| value.to_string());
    let issue_type = row.type_id.map(|value| value.to_string());
    let assigned_pod_id = row.assigned_pod_id.map(|value| value.to_string());
    let assigned_runner_id = row.assigned_runner_id.map(|value| value.to_string());
    let created_at = render_datetime_in(&row.created_at, timezone);
    let updated_at = render_datetime_in(&row.updated_at, timezone);
    let deleted_at = row
        .deleted_at
        .map(|moment| render_datetime_in(&moment, timezone));
    let completed_at = row
        .completed_at
        .map(|moment| render_datetime_in(&moment, timezone));
    let archived_at = row.archived_at.map(|date| date.to_string());
    let start_date = row.start_date.map(|date| date.to_string());
    let target_date = row.target_date.map(|date| date.to_string());
    let created_by = row.created_by_id.map(|value| value.to_string());
    let updated_by = row.updated_by_id.map(|value| value.to_string());
    let assignee_strs: Vec<String> = assignees.iter().map(ToString::to_string).collect();
    let label_strs: Vec<String> = labels.iter().map(ToString::to_string).collect();
    let assignee_refs: Vec<&str> = assignee_strs.iter().map(String::as_str).collect();
    let label_refs: Vec<&str> = label_strs.iter().map(String::as_str).collect();
    let create_row = IssueCreateRow {
        id: id.as_str(),
        project: project.as_str(),
        workspace: workspace.as_str(),
        project_id: project.as_str(),
        workspace_id: workspace.as_str(),
        state: state.as_deref(),
        state_id: state.as_deref(),
        estimate_point: estimate_point.as_deref(),
        parent: parent.as_deref(),
        parent_id: parent.as_deref(),
        issue_type: issue_type.as_deref(),
        assigned_pod_id: assigned_pod_id.as_deref(),
        assigned_runner_id: assigned_runner_id.as_deref(),
        assignees: &assignee_refs,
        labels: &label_refs,
        name: row.name.as_str(),
        description_json: &row.description_json,
        description_html: row.description_html.as_str(),
        description_stripped: row.description_stripped.as_deref(),
        description_binary: row.description_binary.as_deref(),
        priority: row.priority.as_str(),
        complexity_score: row.complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft: row.is_draft,
        external_source: row.external_source.as_deref(),
        external_id: row.external_id.as_deref(),
        git_work_branch: row.git_work_branch.as_str(),
        created_via: row.created_via.as_deref(),
        agent_executor: row.agent_executor.as_deref(),
        point: row.point,
        deleted_at: deleted_at.as_deref(),
        created_at: created_at.as_str(),
        updated_at: updated_at.as_str(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_create_to_representation(&create_row, initial_assignees, initial_labels);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// W37 POST (`draft.py:205-312`): ADMIN+MEMBER gate, the draft lookup
/// (miss → 500 — `None.project_id` raises `AttributeError`), the
/// project-required 400, `IssueCreateSerializer` validation + save (the
/// advisory-locked sequence/sort legs), the `issue.activity.created`
/// emit, the raw `cycle_id` branch (create + `cycle.activity.created`
/// with the `"None"` project bug), the raw `module_ids` branch (bulk +
/// one `module.activity.created` per entry), the `FileAsset` re-point,
/// the draft soft delete, 201 `serializer.data`.
async fn draft_to_issue(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    axum::extract::Path((slug, draft_id)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> HandlerResult {
    let draft_pk = match parse_pk(&draft_id) {
        Ok(pk) => pk,
        Err(response) => return Ok(response),
    };
    let user_id = actor_user_id(extension)?;
    let pool = pool_of(&state)?;
    let gate = gates::Gate::Workspace {
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    // `.filter(pk=draft_id).first()` — NO `created_by` scoping (any
    // member's draft converts); a miss leaves `None`, and
    // `None.project_id` raises `AttributeError` → the generic 500.
    #[derive(Debug, FromQueryResult)]
    struct DraftConvertRow {
        project_id: Option<Uuid>,
        workspace_id: Uuid,
        created_by_id: Option<Uuid>,
        updated_by_id: Option<Uuid>,
        state_id: Option<Uuid>,
    }
    let mut convert_stmt = Query::select();
    convert_stmt
        .columns([
            (a("d"), draft_issues::Column::ProjectId),
            (a("d"), draft_issues::Column::WorkspaceId),
            (a("d"), draft_issues::Column::CreatedById),
            (a("d"), draft_issues::Column::UpdatedById),
            (a("d"), draft_issues::Column::StateId),
        ])
        .from_as(draft_issues::Entity, a("d"))
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            a("w"),
            col("w", workspaces::Column::Id).equals((a("d"), draft_issues::Column::WorkspaceId)),
        )
        .and_where(col("w", workspaces::Column::Slug).eq(slug.clone()))
        .and_where(col("d", draft_issues::Column::Id).eq(draft_pk))
        .and_where(col("d", draft_issues::Column::DeletedAt).is_null())
        .limit(1);
    let convert_rows = query_built::<DraftConvertRow>(&orm(&pool), &convert_stmt).await?;
    let draft = convert_rows.into_iter().next().map(|row| {
        (
            row.project_id,
            row.workspace_id,
            row.created_by_id,
            row.updated_by_id,
            row.state_id,
        )
    });
    let Some((draft_project_opt, draft_workspace, draft_created_by, draft_updated_by, draft_state)) =
        draft
    else {
        return Err(Denial::ServerError);
    };
    let Some(draft_project) = draft_project_opt else {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            PROJECT_REQUIRED_BODY.to_owned(),
        ));
    };
    // `draft_issue.project` (`_base_manager`, unscoped): a missing row
    // raises `RelatedObjectDoesNotExist` → the 404 branch.
    let project = match project_row(&pool, &draft_project).await? {
        Some(row) => row,
        None => {
            return Ok(json_response(
                StatusCode::NOT_FOUND,
                OBJECT_NOT_FOUND_BODY.to_owned(),
            ))
        }
    };
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_dict(&input) {
        return Ok(response);
    }
    let timezone = actor_timezone(&pool, &user_id).await?;
    let values = match validate_shape(&pool, &input, ISSUE_FIELDS, false, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    let assigned_pod: Option<Option<PodRef>> = match find_value(&values, "assigned_pod_id") {
        None => None,
        Some(FieldValue::Null) => Some(None),
        Some(FieldValue::Uuid(id)) => Some(Some(fetch_pod_ref(&pool, id).await?)),
        _ => None,
    };
    let filtered = match run_issue_create_validate(
        &pool,
        &state,
        &values,
        &draft_project,
        &user_id,
        assigned_pod,
    )
    .await
    {
        Ok(filtered) => filtered,
        Err(response) => return Ok(response),
    };
    let (row, raw_assignees, raw_labels) = assemble_new_issue(
        &values,
        &filtered,
        assigned_pod.flatten().map(|pod| pod.id),
        &draft_project,
        &project.workspace_id,
        &user_id,
    )?;
    // The m2m plan resolves pre-save (pure reads of validated input).
    let assignees = filtered
        .assignee_ids
        .clone()
        .or(raw_assignees)
        .unwrap_or_default();
    let labels = filtered
        .label_ids
        .clone()
        .or(raw_labels)
        .unwrap_or_default();
    // Task bytes render the raw QueryDict (PIDASHCONV-763): form list
    // fields dump as last-wins scalars. A projected COPY — `input`
    // still feeds the cycle/module branches below.
    let requested = if input.is_html {
        let mut projected = input.value.clone();
        if let Value::Object(map) = &mut projected {
            shared_body::project_list_scalars(map, BODY_SPEC.list_fields);
        }
        python_dumps(&projected)
    } else {
        python_dumps(&input.value)
    };
    // `base_host` raises past the save (the issue persists on a
    // misconfigured host): `request_origin` is pure over settings, so it
    // resolves here but only takes effect past the fire + m2m legs — the
    // chain stops ahead of the activity when it is `None`.
    let origin = request_origin(&state).ok();
    let actor_text = user_id.to_string();
    // The atomic sequence/sort INSERT (save hooks) plus the post-commit
    // chain — transition fire, m2m + default-assignee legs, created
    // activity — in handler order. Django's `post_save` fires inside
    // `Issue.objects.create`, before the assignee/label `bulk_create`s,
    // so the entry prompt sees empty m2m.
    let tx_orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = WriteCtx::new(WriteActor {
        user_id,
        timezone: Some(timezone.name().to_owned()),
    })
    .with_scope(WriteScope {
        workspace_id: Some(draft_workspace),
        project_id: Some(draft_project),
    });
    let transition: Arc<dyn IssueTransitionHook> = Arc::new(AppTransitionHook::new(&state, &pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pool));
    let outcome = domain::run(&ctx, tx_orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pool.clone();
        let ctx = ctx.clone();
        let origin = origin.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::create(
                &ctx,
                txn,
                &pool,
                domain_issues::CreateIssue {
                    fields: row,
                    assignees,
                    default_assignee: project.default_assignee_id,
                    labels,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        pod_once: false,

                        created_by: Some(user_id),
                        moved_by_run: None,
                        now: utc_now_micros(),
                    },
                    origin,
                    activity_requested: Value::String(requested),
                    activity_notification: Some(true),
                    activity_require_origin: true,
                    skip_activity: false,
                    skip_fire: false,
                    model_activity: None,
                    created_override: None,
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
    }
    // A `None` origin stopped the chain above, so this is `Some` — the
    // `ok_or` is unreachable, kept for the type.
    let origin = origin.ok_or(Denial::ServerError)?;
    // The 201 body renders off the saved row (hook outputs included),
    // exactly the in-memory row the direct save used to leave behind.
    let row = IssueSaveRow::from(&outcome.issue);
    let issue_text = row.id.to_string();
    let project_text = draft_project.to_string();
    // The cycle branch (`:240-265`): a truthy RAW `cycle_id`
    // (`objects.create` — `save()` overwrites the audit ids with the
    // actor/`None`), then `cycle.activity.created` with the `"None"`
    // project bug (the route carries no `project_id` kwarg).
    if input.get("cycle_id").is_some_and(json_is_truthy) {
        let raw = input.get("cycle_id").ok_or(Denial::ServerError)?;
        let cycle_id = match coerce_link_uuid(raw) {
            Some(id) => id,
            None => {
                return Ok(json_response(
                    StatusCode::BAD_REQUEST,
                    INVALID_DETAIL_BODY.to_owned(),
                ))
            }
        };
        let link = Uuid::new_v4();
        let link_created = utc_now_micros();
        let link_updated = utc_now_micros();
        let link_created_db: DateTimeWithTimeZone = link_created.into();
        let link_updated_db: DateTimeWithTimeZone = link_updated.into();
        let cycle_model = cycle_issues::ActiveModel {
            id: Set(link),
            created_at: Set(link_created_db),
            updated_at: Set(link_updated_db),
            created_by_id: Set(Some(user_id)),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            workspace_id: Set(draft_workspace),
            project_id: Set(draft_project),
            cycle_id: Set(cycle_id),
            issue_id: Set(row.id),
        };
        if let Err(error) = cycle_model.insert(&orm(&pool)).await {
            if sea_is_integrity(&error) {
                return Ok(json_response(
                    StatusCode::BAD_REQUEST,
                    INVALID_PAYLOAD_BODY.to_owned(),
                ));
            }
            return Err(Denial::ServerError);
        }
        let current = cycle_current_instance(&cycle_serialize_text(
            &link,
            &link_created,
            &link_updated,
            &user_id,
            &draft_project,
            &draft_workspace,
            &row.id,
            &cycle_id,
        ));
        enqueue_activity_emit(
            &pool,
            &wtask::draft_cycle_created_activity(
                &actor_text,
                None,
                current,
                Utc::now().timestamp(),
                &origin,
            ),
        )
        .await;
    }
    // The module branch (`:267-297`): a truthy RAW `module_ids`
    // `bulk_create`s rows that KEEP the draft's audit ids (no `save()`
    // runs), then fires one `module.activity.created` per entry over the
    // RAW values. A non-iterable (`TypeError`) 500s; a malformed entry
    // 400s (`ValidationError`); a null entry NULL-inserts → 400
    // (`IntegrityError`).
    if input.get("module_ids").is_some_and(json_is_truthy) {
        let raw = input.get("module_ids").ok_or(Denial::ServerError)?;
        let items = match link_items(raw) {
            Some(items) => items,
            None => {
                return Ok(json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SERVER_ERROR_BODY.to_owned(),
                ))
            }
        };
        let mut module_ids: Vec<Option<Uuid>> = Vec::with_capacity(items.len());
        for item in &items {
            if matches!(item, Value::Null) {
                module_ids.push(None);
                continue;
            }
            match coerce_link_uuid(item) {
                Some(id) => module_ids.push(Some(id)),
                None => {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_DETAIL_BODY.to_owned(),
                    ))
                }
            }
        }
        // One transaction: `bulk_create` is atomic across batches (a late
        // chunk's `IntegrityError` rolls the whole leg back — the issue row
        // itself committed pages ago and persists).
        let mut leg_tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
        for chunk in module_ids.chunks(10) {
            // `module_id` is non-nullable: a `None` entry NULL-inserts
            // through the builder to raise the `IntegrityError`.
            let mut insert = Query::insert();
            insert.into_table(module_issues::Entity);
            insert.columns([
                module_issues::Column::Id,
                module_issues::Column::CreatedAt,
                module_issues::Column::UpdatedAt,
                module_issues::Column::CreatedById,
                module_issues::Column::UpdatedById,
                module_issues::Column::DeletedAt,
                module_issues::Column::ProjectId,
                module_issues::Column::WorkspaceId,
                module_issues::Column::IssueId,
                module_issues::Column::ModuleId,
            ]);
            for member_id in chunk {
                let created: DateTimeWithTimeZone = utc_now_micros().into();
                let updated: DateTimeWithTimeZone = utc_now_micros().into();
                insert
                    .values([
                        Uuid::new_v4().into(),
                        created.into(),
                        updated.into(),
                        draft_created_by.into(),
                        draft_updated_by.into(),
                        None::<Uuid>.into(),
                        draft_project.into(),
                        draft_workspace.into(),
                        row.id.into(),
                        (*member_id).into(),
                    ])
                    .map_err(|_| Denial::ServerError)?;
            }
            let stmt = insert;
            let result = {
                let conn = TxConn::new(&mut leg_tx);
                conn.execute(DbBackend::Postgres.build(&stmt)).await
            };
            if let Err(error) = result {
                // Drop without commit: the leg rolls back as one unit.
                if sea_is_integrity(&error) {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_PAYLOAD_BODY.to_owned(),
                    ));
                }
                return Err(Denial::ServerError);
            }
        }
        leg_tx.commit().await.map_err(|_| Denial::ServerError)?;
        for item in &items {
            let mut ref_dump = String::from("{\"module_id\": ");
            python_dump_str(&mut ref_dump, &python_str(item));
            ref_dump.push('}');
            enqueue_activity_emit(
                &pool,
                &wtask::draft_module_created_activity(
                    ref_dump,
                    &actor_text,
                    &issue_text,
                    &project_text,
                    Utc::now().timestamp(),
                    &origin,
                ),
            )
            .await;
        }
    }
    // The `FileAsset` re-point (`:300-306`): a queryset `update()` — no
    // `save()`, so `updated_at` stays.
    file_assets::Entity::update_many()
        .col_expr(file_assets::Column::IssueId, Expr::value(Some(row.id)))
        .col_expr(
            file_assets::Column::EntityType,
            Expr::value(Some("ISSUE_DESCRIPTION".to_owned())),
        )
        .col_expr(file_assets::Column::DraftIssueId, Expr::value(None::<Uuid>))
        .filter(file_assets::Column::DraftIssueId.eq(draft_pk))
        .exec(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    // The draft delete (`:308`) — after all three publishes.
    soft_delete_draft(&pool, &draft_pk, Some(draft_project), draft_state, &user_id).await?;
    Ok(json_response(
        StatusCode::CREATED,
        render_draft_to_issue_body(
            &pool,
            &row,
            &timezone,
            input.get("assignee_ids"),
            input.get("label_ids"),
        )
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn uuid(text: &str) -> Uuid {
        Uuid::from_str(text).expect("uuid")
    }

    #[test]
    fn coerce_link_uuid_matrix() {
        // Strings: every `uuid.UUID` spelling (probed live).
        assert_eq!(
            coerce_link_uuid(&Value::String(
                "11111111-1111-1111-1111-111111111111".to_owned()
            )),
            Some(uuid("11111111-1111-1111-1111-111111111111"))
        );
        assert_eq!(
            coerce_link_uuid(&Value::String(
                "11111111111111111111111111111111".to_owned()
            )),
            Some(uuid("11111111-1111-1111-1111-111111111111"))
        );
        assert_eq!(
            coerce_link_uuid(&Value::String(
                "{11111111-1111-1111-1111-111111111111}".to_owned()
            )),
            Some(uuid("11111111-1111-1111-1111-111111111111"))
        );
        assert_eq!(
            coerce_link_uuid(&Value::String(
                "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA".to_owned()
            )),
            Some(uuid("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"))
        );
        // Ints ride `UUID(int=)`; bools ride 0/1.
        assert_eq!(
            coerce_link_uuid(&Value::Number(123.into())),
            Some(Uuid::from_u128(123))
        );
        assert_eq!(
            coerce_link_uuid(&Value::Bool(true)),
            Some(Uuid::from_u128(1))
        );
        assert_eq!(
            coerce_link_uuid(&Value::Bool(false)),
            Some(Uuid::from_u128(0))
        );
        // Everything else raises `ValidationError` (the caller 400s).
        assert_eq!(coerce_link_uuid(&Value::String("xyz".to_owned())), None);
        assert_eq!(coerce_link_uuid(&Value::Null), None);
        assert_eq!(
            coerce_link_uuid(&Value::Number(
                serde_json::Number::from_f64(1.5).expect("f64")
            )),
            None
        );
        assert_eq!(coerce_link_uuid(&Value::Number((-1).into())), None);
        assert_eq!(coerce_link_uuid(&Value::Array(vec![])), None);
        assert_eq!(coerce_link_uuid(&Value::Object(Map::new())), None);
    }

    #[test]
    fn link_items_matrix() {
        assert_eq!(
            link_items(&Value::Array(vec![Value::Number(1.into()), Value::Null])),
            Some(vec![Value::Number(1.into()), Value::Null])
        );
        // Strings iterate CHARS.
        assert_eq!(
            link_items(&Value::String("ab".to_owned())),
            Some(vec![
                Value::String("a".to_owned()),
                Value::String("b".to_owned())
            ])
        );
        // Objects iterate KEYS.
        let mut map = Map::new();
        map.insert("k".to_owned(), Value::Number(1.into()));
        assert_eq!(
            link_items(&Value::Object(map)),
            Some(vec![Value::String("k".to_owned())])
        );
        assert_eq!(link_items(&Value::Null), Some(vec![]));
        // Numbers/bools are not iterable (`TypeError` → the caller 500s).
        assert_eq!(link_items(&Value::Number(1.into())), None);
        assert_eq!(link_items(&Value::Bool(true)), None);
    }

    #[test]
    fn python_dumps_spacing_and_ascii() {
        // Insertion order survives (`preserve_order`), like CPython dicts.
        let mut map = Map::new();
        map.insert("b".to_owned(), Value::Number(1.into()));
        map.insert("a".to_owned(), Value::String("é\"\n".to_owned()));
        assert_eq!(
            python_dumps(&Value::Object(map)),
            "{\"b\": 1, \"a\": \"\\u00e9\\\"\\n\"}"
        );
        assert_eq!(
            python_dumps(&Value::Array(vec![Value::Null, Value::Bool(true)])),
            "[null, true]"
        );
    }

    #[test]
    fn python_str_matrix() {
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&Value::Bool(true)), "True");
        assert_eq!(python_str(&Value::Bool(false)), "False");
        assert_eq!(python_str(&Value::Number(5.into())), "5");
        assert_eq!(python_str(&Value::String("a".to_owned())), "a");
        assert_eq!(
            python_str(&Value::Array(vec![
                Value::Number(1.into()),
                Value::String("a".to_owned())
            ])),
            "[1, 'a']"
        );
        let mut map = Map::new();
        map.insert("k".to_owned(), Value::Bool(true));
        assert_eq!(python_str(&Value::Object(map)), "{'k': True}");
    }

    #[test]
    fn cycle_serialize_text_byte_exact() {
        // The `/tmp/probe_ser2.py` capture, verbatim.
        let created = chrono::DateTime::from_timestamp(1714979289, 123_456_000).expect("ts");
        let updated = chrono::DateTime::from_timestamp(1714979289, 654_321_000).expect("ts");
        assert_eq!(
            cycle_serialize_text(
                &uuid("11111111-1111-1111-1111-111111111111"),
                &created,
                &updated,
                &uuid("22222222-2222-2222-2222-222222222222"),
                &uuid("33333333-3333-3333-3333-333333333333"),
                &uuid("44444444-4444-4444-4444-444444444444"),
                &uuid("66666666-6666-6666-6666-666666666666"),
                &uuid("55555555-5555-5555-5555-555555555555"),
            ),
            "[{\"model\": \"db.cycleissue\", \"pk\": \"11111111-1111-1111-1111-111111111111\", \"fields\": {\"created_at\": \"2024-05-06T07:08:09.123Z\", \"updated_at\": \"2024-05-06T07:08:09.654Z\", \"created_by\": \"22222222-2222-2222-2222-222222222222\", \"updated_by\": null, \"deleted_at\": null, \"project\": \"33333333-3333-3333-3333-333333333333\", \"workspace\": \"44444444-4444-4444-4444-444444444444\", \"issue\": \"66666666-6666-6666-6666-666666666666\", \"cycle\": \"55555555-5555-5555-5555-555555555555\"}}]"
        );
    }

    #[test]
    fn cycle_current_instance_key_order() {
        // Insertion order (`updated_` first), spaced separators.
        assert_eq!(
            cycle_current_instance("[x]"),
            "{\"updated_cycle_issues\": null, \"created_cycle_issues\": \"[x]\"}"
        );
    }

    #[test]
    fn encoder_datetime_millis_edges() {
        // `/tmp/probe_ser3.py`: zero → no fraction; micros TRUNCATE.
        let base = chrono::DateTime::from_timestamp(1704164645, 0).expect("ts");
        assert_eq!(render_encoder_datetime(&base), "2024-01-02T03:04:05Z");
        let ms = chrono::DateTime::from_timestamp(1704164645, 999_999_000).expect("ts");
        assert_eq!(render_encoder_datetime(&ms), "2024-01-02T03:04:05.999Z");
        let one = chrono::DateTime::from_timestamp(1704164645, 1_000_000).expect("ts");
        assert_eq!(render_encoder_datetime(&one), "2024-01-02T03:04:05.001Z");
    }

    #[test]
    fn drf_datetime_iso8601_literal_fallback() {
        // PIDASHCONV-773: DRF `to_internal_value` falls through to
        // `strptime(value, 'iso-8601')` when `parse_datetime` returns None;
        // the literal matches case-insensitively and yields naive
        // 1900-01-01 in the actor zone (probed live both backends).
        let utc: Tz = "UTC".parse().unwrap();
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            let parsed = parse_drf_datetime(text, &utc).expect(text);
            assert_eq!(parsed.to_rfc3339(), "1900-01-01T00:00:00+00:00", "{text:?}");
        }
        // Near-misses stay invalid (exact match on the raw text, both
        // sides probed — padding fails even though this parser trims
        // some padded datetimes).
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            assert!(parse_drf_datetime(text, &utc).is_err(), "{text:?}");
        }
    }

    #[test]
    fn validate_error_response_shapes() {
        // `ContextMissing` is the `KeyError` 400.
        let response = issue_validate_error_response(&CreateValidateError::ContextMissing {
            key: "project_id",
        });
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        // Bare-string details list-wrap under `non_field_errors`.
        let response = issue_validate_error_response(&CreateValidateError::NonField {
            message: "State is not valid please pass a valid state_id",
        });
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        // Dict details render as-is, values list-wrapped.
        let response = issue_validate_error_response(&CreateValidateError::Field {
            field: "assigned_pod_id",
            message: std::borrow::Cow::Borrowed("x"),
        });
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn dual_uuid_auto_wins_on_presence() {
        let state_id = uuid("11111111-1111-1111-1111-111111111111");
        let state = uuid("22222222-2222-2222-2222-222222222222");
        // Auto value wins over the declared value.
        let values = vec![
            ("state_id".to_owned(), FieldValue::Uuid(state_id)),
            ("state".to_owned(), FieldValue::Uuid(state)),
        ];
        assert_eq!(dual_uuid(&values, "state_id", "state"), Some(Some(state)));
        // An explicit null auto still wins (clears, no fallback).
        let values = vec![
            ("state_id".to_owned(), FieldValue::Uuid(state_id)),
            ("state".to_owned(), FieldValue::Null),
        ];
        assert_eq!(dual_uuid(&values, "state_id", "state"), Some(None));
        // Declared alone resolves.
        let values = vec![("state_id".to_owned(), FieldValue::Uuid(state_id))];
        assert_eq!(
            dual_uuid(&values, "state_id", "state"),
            Some(Some(state_id))
        );
        assert_eq!(dual_uuid(&[], "state_id", "state"), None);
    }

    #[test]
    fn assemble_new_issue_defaults() {
        // `name` is required on the create shape.
        let filtered = ValidatedAttrs {
            description_html: None,
            assignee_ids: None,
            label_ids: None,
            agent_executor: None,
        };
        let project = uuid("33333333-3333-3333-3333-333333333333");
        let workspace = uuid("44444444-4444-4444-4444-444444444444");
        let actor = uuid("22222222-2222-2222-2222-222222222222");
        assert!(assemble_new_issue(&[], &filtered, None, &project, &workspace, &actor).is_err());
        let values = vec![("name".to_owned(), FieldValue::Text("n".to_owned()))];
        let (row, assignees, labels) =
            assemble_new_issue(&values, &filtered, None, &project, &workspace, &actor)
                .expect("row");
        assert_eq!(row.name, "n");
        assert_eq!(row.description_html, "<p></p>");
        assert_eq!(row.description_json, Value::Object(Map::new()));
        assert_eq!(row.priority, "none");
        assert_eq!(row.complexity_score, 0);
        assert_eq!(row.sort_order, 65535.0);
        assert_eq!(row.sequence_id, 0);
        assert!(!row.is_draft);
        assert_eq!(row.git_work_branch, "");
        assert_eq!(row.workpad, "");
        assert_eq!(row.created_by_id, Some(actor));
        assert_eq!(row.updated_by_id, None);
        assert_eq!(row.project_id, project);
        assert_eq!(row.workspace_id, workspace);
        assert_eq!(assignees, None);
        assert_eq!(labels, None);
        // The sanitized html wins over the input.
        let filtered = ValidatedAttrs {
            description_html: Some("<p>clean</p>".to_owned()),
            assignee_ids: None,
            label_ids: None,
            agent_executor: None,
        };
        let values = vec![
            ("name".to_owned(), FieldValue::Text("n".to_owned())),
            (
                "description_html".to_owned(),
                FieldValue::Text("<p>raw</p>".to_owned()),
            ),
        ];
        let (row, _, _) =
            assemble_new_issue(&values, &filtered, None, &project, &workspace, &actor)
                .expect("row");
        assert_eq!(row.description_html, "<p>clean</p>");
    }

    #[test]
    fn truthiness_matrix() {
        assert!(!json_is_truthy(&Value::Null));
        assert!(!json_is_truthy(&Value::Bool(false)));
        assert!(!json_is_truthy(&Value::Number(0.into())));
        assert!(!json_is_truthy(&Value::String(String::new())));
        assert!(!json_is_truthy(&Value::Array(vec![])));
        assert!(!json_is_truthy(&Value::Object(Map::new())));
        assert!(json_is_truthy(&Value::Bool(true)));
        assert!(json_is_truthy(&Value::Number(1.into())));
        assert!(json_is_truthy(&Value::String("x".to_owned())));
    }

    #[test]
    fn pk_parses_lowercase_only() {
        assert!(parse_pk("11111111-1111-1111-1111-111111111111").is_ok());
        assert!(parse_pk("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA").is_err());
        assert!(parse_pk("not-a-uuid").is_err());
    }

    #[test]
    fn draft_validate_messages_byte_exact() {
        // `app/serializers/draft.py:119,129,138`, verbatim (D3).
        assert_eq!(
            DRAFT_STATE_INVALID,
            "State is not valid please pass a valid state_id"
        );
        assert_eq!(
            DRAFT_PARENT_INVALID,
            "Parent is not valid issue_id please pass a valid issue_id"
        );
        assert_eq!(
            DRAFT_ESTIMATE_INVALID,
            "Estimate point is not valid please pass a valid estimate_point_id"
        );
    }

    #[test]
    fn patch_context_project_matrix() {
        // `request.data.get("project_id", issue.project_id)` (D5/D7).
        let own = uuid("11111111-1111-1111-1111-111111111111");
        let other = uuid("22222222-2222-2222-2222-222222222222");
        // Missing keeps the issue's own.
        assert_eq!(patch_context_project(None, Some(own)), (Some(own), None));
        assert_eq!(patch_context_project(None, None), (None, None));
        // Explicit null means the None context (NOT the fallback).
        assert_eq!(
            patch_context_project(Some(&Value::Null), Some(own)),
            (None, None)
        );
        // A valid id resolves cleanly.
        assert_eq!(
            patch_context_project(Some(&Value::String(other.to_string())), Some(own)),
            (Some(other), None)
        );
        // Garbage stays lazy as its `to_python` echo, never resolved here.
        assert_eq!(
            patch_context_project(Some(&Value::String("zzz".to_owned())), Some(own)),
            (None, Some("zzz".to_owned()))
        );
        assert_eq!(
            patch_context_project(
                Some(&Value::Number(
                    serde_json::Number::from_f64(1.5).expect("f64")
                )),
                Some(own)
            ),
            (None, Some("1.5".to_owned()))
        );
        // The create twin has no fallback: missing means None.
        assert_eq!(raw_context_project(None), (None, None));
        assert_eq!(
            raw_context_project(Some(&Value::String("zzz".to_owned()))),
            (None, Some("zzz".to_owned()))
        );
    }

    #[test]
    fn python_dumps_number_spellings() {
        // `json.dumps` over parsed floats: CPython `repr` spellings (D12);
        // integers echo verbatim however large.
        let num = |text: &str| serde_json::from_str::<Value>(text).expect("number");
        let mut map = Map::new();
        map.insert("exp".to_owned(), num("1e2"));
        assert_eq!(python_dumps(&Value::Object(map)), "{\"exp\": 100.0}");
        let mut map = Map::new();
        map.insert("big_exp".to_owned(), num("1E3"));
        map.insert("int".to_owned(), num("-5"));
        map.insert("huge".to_owned(), num("1267650600228229401496703205376"));
        map.insert("frac".to_owned(), num("0.5"));
        assert_eq!(
            python_dumps(&Value::Object(map)),
            "{\"big_exp\": 1000.0, \"int\": -5, \"huge\": 1267650600228229401496703205376, \"frac\": 0.5}"
        );
        // Scientific-threshold spellings ride `repr`, not the source text.
        let mut map = Map::new();
        map.insert("big".to_owned(), num("1e16"));
        map.insert("small".to_owned(), num("0.00001"));
        assert_eq!(
            python_dumps(&Value::Object(map)),
            "{\"big\": 1e+16, \"small\": 1e-05}"
        );
        // Non-finite floats use the `allow_nan` spellings.
        let mut map = Map::new();
        map.insert("inf".to_owned(), num("1e999"));
        assert_eq!(python_dumps(&Value::Object(map)), "{\"inf\": Infinity}");
    }

    #[test]
    fn python_dumps_negative_zero_int() {
        // `-0` is valid JSON but parses to int `0`: `dumps` renders the
        // value, not the source spelling (review 2).
        let num = |text: &str| serde_json::from_str::<Value>(text).expect("number");
        let mut map = Map::new();
        map.insert("negzero".to_owned(), num("-0"));
        map.insert("zero".to_owned(), num("0"));
        assert_eq!(
            python_dumps(&Value::Object(map)),
            "{\"negzero\": 0, \"zero\": 0}"
        );
    }

    #[test]
    fn form_arrays_dump_as_querydict_scalars() {
        // Task payloads render the raw QueryDict (PIDASHCONV-763): the
        // draft-create dump projects form `assignee_ids` / `label_ids`
        // arrays to last-wins scalars; validation keeps the arrays.
        let raw = b"title=t&assignee_ids=a&assignee_ids=b&label_ids=x";
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        headers.insert("content-length", raw.len().to_string().parse().unwrap());
        let input = match negotiate_input(&headers, raw) {
            Ok(input) => input,
            Err(_) => panic!("parses"),
        };
        assert!(input.is_html);
        assert_eq!(input.value["assignee_ids"], serde_json::json!(["a", "b"]));
        let mut projected = input.value.clone();
        let Value::Object(map) = &mut projected else {
            panic!("object");
        };
        shared_body::project_list_scalars(map, BODY_SPEC.list_fields);
        assert_eq!(projected["assignee_ids"], serde_json::json!("b"));
        assert_eq!(projected["label_ids"], serde_json::json!("x"));
    }

    #[tokio::test]
    async fn live_favorites_drafts_reads() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "workspaces",
            "users",
            "user_favorites",
            "project_members",
            "draft_issues",
            "draft_issue_labels",
            "labels",
            "draft_issue_assignees",
            "draft_issue_modules",
            "modules",
            "draft_issue_cycles",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (ws, user, fav, draft, label, assignee) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO user_favorites (id, name, is_folder, sequence, workspace_id, user_id, entity_type, deleted_at) VALUES ('{fav}', 'F', false, 1.0, '{ws}', '{user}', 'project', NULL)"
            ),
            format!(
                "INSERT INTO draft_issues (id, created_at, updated_at, name, priority, sort_order, workspace_id, created_by_id, description_html, deleted_at) VALUES ('{draft}', now(), now(), 'D', 'none', 65535.0, '{ws}', '{user}', '<p></p>', NULL)"
            ),
            format!(
                "INSERT INTO labels (id, name, deleted_at) VALUES ('{label}', 'L', NULL)"
            ),
            format!(
                "INSERT INTO users (id, username, first_name, last_name, avatar, date_joined, created_at, updated_at, last_location, created_location, is_superuser, is_managed, is_password_expired, is_active, is_staff, is_email_verified, is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, is_password_reset_required) VALUES ('{assignee}', 'u', 'A', 'B', '', now(), now(), now(), '', '', false, false, false, true, false, false, false, '', 'UTC', '', '', '', '', false, 'A B', false, false)"
            ),
            format!(
                "INSERT INTO draft_issue_labels (id, workspace_id, draft_issue_id, label_id, deleted_at) VALUES (gen_random_uuid(), '{ws}', '{draft}', '{label}', NULL)"
            ),
            format!(
                "INSERT INTO project_members (id, workspace_id, project_id, member_id, is_active, deleted_at) VALUES (gen_random_uuid(), '{ws}', gen_random_uuid(), '{assignee}', true, NULL)"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let fav_stmt = favorites_select("acme", user, None, true);
        let fav_rows = query_built::<FavoriteDbRow>(&db, &fav_stmt)
            .await
            .expect("favorites select");
        assert_eq!(fav_rows.len(), 1);
        assert_eq!(fav_rows[0].id, fav);
        let draft_stmt = draft_rows_select("acme", Condition::all(), Some(user), None, 10, 0, true);
        let draft_rows = query_built::<DraftDbRow>(&db, &draft_stmt)
            .await
            .expect("drafts select");
        assert_eq!(draft_rows.len(), 1);
        assert_eq!(draft_rows[0].id, draft);
        assert_eq!(draft_rows[0].label_ids, vec![label]);
        let count_stmt = draft_count_select("acme", Condition::all(), user);
        let count_rows = query_built::<CountDbRow>(&db, &count_stmt)
            .await
            .expect("count select");
        assert_eq!(count_rows.len(), 1);
        assert_eq!(count_rows[0].n, 1);
    }
}
