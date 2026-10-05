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
//! Layering: the services builders return `:named`-placeholder fragments; [`positional`]
//! rewrites them to `$n` (the `handlers_lists` precedent). Endpoint-specific SQL lives
//! here only where it names this endpoint's projection; merged predicates are spliced
//! verbatim, never re-ported. DRF field validation (`is_valid()` before `validate()`)
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
//! `Issue.save()` fires the orchestration `post_save` receiver (`capture_prior_state` +
//! `fire_state_transition`). This module calls the merged
//! `pidash_services::orchestration::entries` drivers with a minimal seam: the pre-save
//! snapshot and the transition's state lookups run live; deeper driver arms (run
//! creation, clock reconcile, scheduler, preflight) report unreachable and the fire
//! swallows them like a handler raise (counter + log line, save stands) — exactly the
//! truncation the D-26 handlers-A port ships, and the HTTP surface is unaffected either
//! way. The `github_signals` completion-comment leg fires only on updates
//! (`created → return`), never on this path.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::membership::ProjectRoleFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_db::app_pages::strip::sync_description_stripped;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_jobs::celery::CeleryTaskMessage;
use pidash_jobs::queue::{self, NewJob};
use pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK;
use pidash_services::app_issues::serializers_create::{
    assignee_member_filter_sql, issue_create_to_representation, issue_create_validate,
    label_filter_null_project_sql, label_filter_sql, m2m_batches, m2m_insert_sql,
    CreateValidateError, ExecutorPolicy, IssueCreateRow, LockedIssueAttrs, PodRef, ResolvedProject,
    ValidateAttrs, ValidateContext, ValidateProbes, ValidatedAttrs, DEFAULT_ASSIGNEE_EXISTS_SQL,
    ESTIMATE_EXISTS_SQL, PARENT_EXISTS_SQL, POD_FETCH_SQL, PROJECT_FETCH_SQL, STATE_EXISTS_SQL,
};
use pidash_services::app_issues::shape::envelope;
use pidash_services::app_workspace::queries_extras as qx;
use pidash_services::app_workspace::ser_account_token as fav;
use pidash_services::app_workspace::tasks as wtask;
use pidash_services::auth_session::shapes::{base_host, HostSettings};
use pidash_services::dispatch::admission::{
    LlmProfile, ENROLLED_MANAGED_RUNNERS_EXISTS_SQL, HEARTBEAT_GRACE_SECS,
    ONLINE_MANAGED_RUNNER_SQL,
};
use pidash_services::dispatch::policy::{
    cloud_agent_is_configured, managed_runner_is_enabled, UserFlags,
};
use pidash_services::orchestration::clock::ProjectClockPolicy;
use pidash_services::orchestration::creation::{
    AdmissionError, CreationError, CreationSeam, ExecutionError, ExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RenderedTurn, RunView, RunnerView, StateView, STATE_SELECT_SQL,
};
use pidash_services::orchestration::entries::{
    capture_prior_state, fire_state_transition, EntriesSeam, FireOutcome, FireRequest,
    PreflightSeam, PRIOR_STATE_SELECT_SQL,
};
use pidash_services::prompting::composer::OverrideRow;
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use pidash_types::v1_assets::sticky as sticky_kernel;
use pidash_types::WorkspaceId;

use super::gates;
use crate::app_issues::Denial;
use crate::middleware::gzip::{accepts_gzip, compress_if_shorter, MIN_COMPRESS_BYTES};
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json_cpython::{
    parse_request_data_spans, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};

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
                .options(crate::edge::proxy),
        )
        .route(
            DRAFTS_PATH,
            axum::routing::get(draft_list)
                .post(draft_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            DRAFT_DETAIL_PATH,
            axum::routing::get(draft_retrieve)
                .patch(draft_patch)
                .delete(draft_destroy)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
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
/// `handle_exception`'s generic 500 branch (`app/views/base.py:145-149`).
const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `KeyError` branch (`app/views/base.py:137-141`).
const KEY_ERROR_BODY: &str = r#"{"error":"The required key does not exist."}"#;
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

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

/// 204 with NO `Content-Type`: Django strips it on empty responses (verified live).
fn empty_response() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("204 response")
}

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

/// The actor's `user_timezone` (`users.user_timezone`), parsed for DRF
/// serializer rendering (`TimezoneMixin` activates the actor zone, so every
/// datetime renders shifted into it). Runs after the membership gate, so
/// memberless callers 403 before this is reached.
async fn actor_timezone(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
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
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
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
    let sql = format!(
        "SELECT 1 FROM {table} WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL LIMIT 1"
    );
    let row: Option<(i32,)> = sqlx::query_as(&sql)
        .bind(pk)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
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

/// Rewrite the services builders' `:named` placeholders to positional
/// `$n` binds, in `binds` order. Names are matched longest-first so
/// `:user` never eats the head of a longer placeholder.
fn positional(fragment: &str, binds: &[&str]) -> String {
    let mut ordered: Vec<(usize, &str)> = binds
        .iter()
        .enumerate()
        .map(|(index, name)| (index, *name))
        .collect();
    ordered.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));
    let mut out = fragment.to_owned();
    for (index, name) in ordered {
        out = out.replace(&format!(":{name}"), &format!("${}", index + 1));
    }
    out
}

/// Whether a `sqlx` failure is an `IntegrityError` (unique / FK / check /
/// not-null / exclusion): unique violations, foreign-key violations and
/// check violations map to the payload 400; everything else is the 500.
fn is_integrity_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.starts_with("23"))
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

fn python_dump_into(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
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

/// Strict ASCII-digits `u32` parse: Rust's `parse` accepts a leading `+`,
/// which `fromisoformat` (and the Django `date_re`s) reject.
fn parse_digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
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

/// ISO weekday 1-7 → `chrono::Weekday` (day 0 or 8+ reject).
fn iso_weekday(day: u32) -> Option<chrono::Weekday> {
    chrono::Weekday::try_from(u8::try_from(day).ok()?.checked_sub(1)?).ok()
}

/// The `fromisoformat` date half: `YYYY-MM-DD`, `YYYYMMDD`,
/// `YYYY-Www[-D]`, `YYYYWww[D]` (week without day → Monday). Ordinals reject.
fn parse_iso_date_part(text: &str) -> Option<NaiveDate> {
    if text.len() >= 4 && text.as_bytes().get(4) == Some(&b'W') {
        // `YYYYWww[D]` basic week.
        if !(text.len() == 7 || text.len() == 8) || !text.is_ascii() {
            return None;
        }
        let year = parse_digits(&text[0..4])? as i32;
        let week = parse_digits(&text[5..7])?;
        let weekday = if text.len() == 8 {
            parse_digits(&text[7..8])?
        } else {
            1
        };
        return NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?);
    }
    if text.contains('W') {
        // `YYYY-Www[-D]` extended week (a short year like `24W05` rejects).
        let mut parts = text.split('W');
        let year_text = parts.next()?;
        let year_digits = year_text.strip_suffix('-')?;
        if year_digits.len() != 4 {
            return None;
        }
        let year = parse_digits(year_digits)? as i32;
        let rest = parts.next()?;
        if parts.next().is_some() || !rest.is_ascii() {
            return None;
        }
        let (week_text, weekday) = match rest.split_once('-') {
            Some((week, day)) => (week, parse_digits(day)?),
            None if rest.len() == 2 => (rest, 1),
            None if rest.len() == 3 => (&rest[0..2], parse_digits(&rest[2..3])?),
            None => return None,
        };
        if week_text.len() != 2 {
            return None;
        }
        let week = parse_digits(week_text)?;
        return NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?);
    }
    if text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit()) {
        // `YYYYMMDD` basic.
        return NaiveDate::from_ymd_opt(
            text[0..4].parse().ok()?,
            text[4..6].parse().ok()?,
            text[6..8].parse().ok()?,
        );
    }
    if text.len() == 10 {
        // `YYYY-MM-DD` extended (strictly zero-padded here; 1-2-digit shapes
        // fall through to the `date_re`/`datetime_re` fallbacks).
        let bytes = text.as_bytes();
        if bytes[4] == b'-' && bytes[7] == b'-' {
            return NaiveDate::from_ymd_opt(
                parse_digits(&text[0..4])? as i32,
                parse_digits(&text[5..7])?,
                parse_digits(&text[8..10])?,
            );
        }
    }
    None
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

/// Split a trailing tz designator: `Z` (uppercase only), `±HH:MM[:SS[.f]]`,
/// `±HHMM`, `±HH`, with an optional single space before it. Returns the head
/// and the offset in seconds (fractional offset seconds truncate).
fn split_tz_suffix(text: &str) -> (&str, Option<i64>) {
    if let Some(head) = text.strip_suffix('Z') {
        if !head.is_empty() {
            return (head, Some(0));
        }
    }
    // The last `+`/`-` that starts the offset (not the date's dashes): scan
    // from the end for the last sign with only offset chars after it,
    // stopping at any time/date separator.
    let bytes = text.as_bytes();
    let mut sign_at = None;
    for (index, byte) in bytes.iter().enumerate().rev() {
        if *byte == b'+' || *byte == b'-' {
            let tail = &text[index + 1..];
            if !tail.is_empty()
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b':' || b == b'.')
            {
                sign_at = Some(index);
                break;
            }
        }
        if *byte == b'T' || *byte == b't' || *byte == b' ' || *byte == b'W' {
            break;
        }
    }
    let Some(at) = sign_at else {
        return (text, None);
    };
    let head = text[..at].trim_end();
    let tail = text[at + 1..].trim_start();
    if tail.is_empty() {
        return (text, None);
    }
    let sign: i64 = if bytes[at] == b'-' { -1 } else { 1 };
    let digits: String = tail.chars().filter(|ch| ch.is_ascii_digit()).collect();
    // Colon offsets are strict: exactly-two-digit hours/minutes, seconds of
    // exactly two digits plus an optional `.`/`,` fraction (truncated).
    let offset = if tail.contains(':') {
        let mut parts = tail.split(':');
        let (Some(hours_text), Some(minutes_text)) = (parts.next(), parts.next()) else {
            return (text, None);
        };
        let seconds_text = parts.next();
        if parts.next().is_some() || hours_text.len() != 2 || minutes_text.len() != 2 {
            return (text, None);
        }
        let (Some(hours), Some(minutes)) = (parse_digits(hours_text), parse_digits(minutes_text))
        else {
            return (text, None);
        };
        let seconds: i64 = match seconds_text {
            None => 0,
            Some(sec_text) => {
                let (int, frac) = match sec_text.split_once(['.', ',']) {
                    None => (sec_text, None),
                    Some((int, frac)) => (int, Some(frac)),
                };
                if int.len() != 2 {
                    return (text, None);
                }
                let Some(seconds) = parse_digits(int) else {
                    return (text, None);
                };
                if frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
                    return (text, None);
                }
                seconds as i64
            }
        };
        hours as i64 * 3600 + minutes as i64 * 60 + seconds
    } else if digits.len() == tail.len() && (tail.len() == 2 || tail.len() == 4) {
        let hours: i64 = tail[0..2].parse().unwrap_or(-1);
        let minutes: i64 = if tail.len() == 4 {
            tail[2..4].parse().unwrap_or(-1)
        } else {
            0
        };
        if hours < 0 || minutes < 0 {
            return (text, None);
        }
        hours * 3600 + minutes * 60
    } else {
        return (text, None);
    };
    // Offsets beyond ±23:59 raise in `fromisoformat` (surfacing as `invalid`
    // through DRF's suppress).
    if offset.abs() >= 24 * 3600 {
        return (head, Some(i64::MAX));
    }
    (head, Some(sign * offset))
}

/// The `fromisoformat` surface: extended/basic/week dates, any single-char
/// separator, `HH[:MM[:SS[.ffffff]]]` / basic `HHMM[SS]` times, comma-or-dot
/// fractions (truncated past 6 digits), optional tz.
fn parse_iso_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<i64>)> {
    use chrono::NaiveDateTime;
    let (head, offset) = split_tz_suffix(text);
    if offset == Some(i64::MAX) {
        return None;
    }
    // Date-only (midnight); a trailing bare separator (`'...T'`) rejects.
    if let Some(date) = parse_iso_date_part(head) {
        return Some((date.and_hms_opt(0, 0, 0)?, offset));
    }
    let (date_text, _sep, time_text) = split_date_time(head)?;
    let date = parse_iso_date_part(date_text)?;
    let naive_time = parse_iso_time(time_text)?;
    Some((NaiveDateTime::new(date, naive_time), offset))
}

/// Split `head` into date/separator/time: candidate date lengths
/// `YYYY-MM-DD` (10), `YYYYMMDD`/`YYYYWwwD` (8), `YYYY-Www` (7), `YYYYWww` (6);
/// the separator is any single (possibly multibyte) non-digit char.
fn split_date_time(head: &str) -> Option<(&str, char, &str)> {
    for len in [10usize, 8, 7, 6] {
        if head.len() <= len {
            continue;
        }
        let Some((date_text, rest)) = head.split_at_checked(len) else {
            continue;
        };
        if parse_iso_date_part(date_text).is_none() {
            continue;
        }
        let mut chars = rest.chars();
        let sep = chars.next()?;
        if sep.is_ascii_digit() {
            continue;
        }
        let time_text = &rest[sep.len_utf8()..];
        if time_text.is_empty() {
            continue;
        }
        return Some((date_text, sep, time_text));
    }
    None
}

/// `fromisoformat` time half: `HH[:MM[:SS[.ffffff]]]` (colons),
/// `HHMM[SS]` (basic), hour-only `HH`; comma-or-dot fractions truncate past
/// 6 digits. Colon fields must be zero-padded 2-digit (1-digit shapes fall
/// through to the `datetime_re` fallback).
fn parse_iso_time(text: &str) -> Option<chrono::NaiveTime> {
    use chrono::NaiveTime;
    let (clock, micros) = match text.find(['.', ',']) {
        Some(dot) => {
            let (clock, frac) = text.split_at(dot);
            let digits = &frac[1..];
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let padded = format!("{digits:0<6}");
            let micros: u32 = padded[..6].parse().ok()?;
            (clock, micros)
        }
        None => (text, 0),
    };
    if clock.contains(':') {
        let mut parts = clock.split(':');
        let hour: u32 = parse_digits(parts.next()?)?;
        let minute: u32 = parts.next().map(parse_digits).unwrap_or(Some(0))?;
        let second: u32 = parts.next().map(parse_digits).unwrap_or(Some(0))?;
        if parts.next().is_some() {
            return None;
        }
        for part in clock.split(':') {
            if part.len() != 2 {
                return None;
            }
        }
        NaiveTime::from_hms_micro_opt(hour, minute, second, micros)
    } else {
        if !clock.is_ascii() {
            return None;
        }
        match clock.len() {
            2 => NaiveTime::from_hms_micro_opt(parse_digits(clock)?, 0, 0, micros),
            4 => NaiveTime::from_hms_micro_opt(
                parse_digits(&clock[0..2])?,
                parse_digits(&clock[2..4])?,
                0,
                micros,
            ),
            6 => NaiveTime::from_hms_micro_opt(
                parse_digits(&clock[0..2])?,
                parse_digits(&clock[2..4])?,
                parse_digits(&clock[4..6])?,
                micros,
            ),
            _ => None,
        }
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
async fn pk_exists(
    pool: &sqlx::PgPool,
    queryset: PkQueryset,
    id: &Uuid,
) -> Result<bool, sqlx::Error> {
    let sql = match queryset {
        PkQueryset::StatesAll => "SELECT 1 FROM states WHERE id = $1 LIMIT 1",
        PkQueryset::States => {
            "SELECT 1 FROM states WHERE id = $1 AND \"group\" != 'triage' AND deleted_at IS NULL LIMIT 1"
        }
        PkQueryset::Issues => "SELECT 1 FROM issues WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
        PkQueryset::IssuesVisible => {
            "SELECT 1 FROM issues i LEFT JOIN states s ON s.id = i.state_id JOIN projects p ON p.id = i.project_id WHERE i.id = $1 AND i.deleted_at IS NULL AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft LIMIT 1"
        }
        PkQueryset::PodsAll => "SELECT 1 FROM pod WHERE id = $1 LIMIT 1",
        PkQueryset::Labels => "SELECT 1 FROM labels WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
        PkQueryset::Users => "SELECT 1 FROM users WHERE id = $1 LIMIT 1",
        PkQueryset::Estimates => {
            "SELECT 1 FROM estimate_points WHERE id = $1 AND deleted_at IS NULL LIMIT 1"
        }
        PkQueryset::Types => {
            "SELECT 1 FROM issue_types WHERE id = $1 AND deleted_at IS NULL LIMIT 1"
        }
        PkQueryset::Projects => "SELECT 1 FROM projects WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
        PkQueryset::Favorites => {
            "SELECT 1 FROM user_favorites WHERE id = $1 AND deleted_at IS NULL LIMIT 1"
        }
    };
    let row: Option<(i32,)> = sqlx::query_as(sql).bind(id).fetch_optional(pool).await?;
    Ok(row.is_some())
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
    let row: Option<(String,)> =
        sqlx::query_as("SELECT \"group\" FROM states WHERE id = $1 LIMIT 1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(group,)| StateRow { group }))
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
    let row: Option<(Uuid, Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT id, workspace_id, default_assignee_id FROM projects WHERE id = $1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(
        row.map(|(id, workspace_id, default_assignee_id)| ProjectRow {
            id,
            workspace_id,
            default_assignee_id,
        }),
    )
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
struct FavoriteRow {
    id: String,
    entity_type: String,
    entity_identifier: Option<String>,
    name: Option<String>,
    is_folder: bool,
    sequence: f64,
    parent: Option<String>,
    workspace_id: String,
    project_id: Option<String>,
}

fn favorite_row_from_pg(row: &sqlx::postgres::PgRow) -> Result<FavoriteRow, Denial> {
    use sqlx::Row;
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let entity_type: String = row
        .try_get("entity_type")
        .map_err(|_| Denial::ServerError)?;
    let entity_identifier: Option<Uuid> = row
        .try_get("entity_identifier")
        .map_err(|_| Denial::ServerError)?;
    let name: Option<String> = row.try_get("name").map_err(|_| Denial::ServerError)?;
    let is_folder: bool = row.try_get("is_folder").map_err(|_| Denial::ServerError)?;
    let sequence: f64 = row.try_get("sequence").map_err(|_| Denial::ServerError)?;
    let parent: Option<Uuid> = row.try_get("parent_id").map_err(|_| Denial::ServerError)?;
    let workspace_id: Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Option<Uuid> = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    Ok(FavoriteRow {
        id: id.to_string(),
        entity_type,
        entity_identifier: entity_identifier.map(|id| id.to_string()),
        name,
        is_folder,
        sequence,
        parent: parent.map(|id| id.to_string()),
        workspace_id: workspace_id.to_string(),
        project_id: project_id.map(|id| id.to_string()),
    })
}

/// Render one favorite row in `USER_FAVORITE_WIRE_FIELDS` order. The
/// `sequence` float renders via [`json_float_str`] (never serde); the
/// `entity_data` sub-object comes from the merged lite ports.
fn render_favorite(row: &FavoriteRow, entity: Option<&Value>) -> String {
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
    use sqlx::Row;
    // Only cycles/modules/views carry a `project_id` column — projects
    // are the top (the lite omits it) and pages link projects M2M (the
    // serializer reads `.first()`).
    let sql = if with_project {
        format!(
            "SELECT id, name, logo_props, project_id FROM {table} WHERE id = $1 AND deleted_at IS NULL LIMIT 1"
        )
    } else {
        format!(
            "SELECT id, name, logo_props FROM {table} WHERE id = $1 AND deleted_at IS NULL LIMIT 1"
        )
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let name: String = row.try_get("name").map_err(|_| Denial::ServerError)?;
    let logo_props: Value = row.try_get("logo_props").map_err(|_| Denial::ServerError)?;
    let project_id: Option<Uuid> = if with_project {
        row.try_get("project_id").map_err(|_| Denial::ServerError)?
    } else {
        None
    };
    Ok(Some(EntityLiteRow {
        id: id.to_string(),
        name,
        logo_props,
        project_id: project_id.map(|id| id.to_string()),
    }))
}

/// `Page.projects.first()`: the page's projects, `-created_at` first, over
/// the live-`Project` manager (the through rows carry no deletion filter —
/// the M2M manager only scopes the target model).
async fn page_first_project(pool: &sqlx::PgPool, page_id: &Uuid) -> Result<Option<String>, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT p.id FROM project_pages pp JOIN projects p ON p.id = pp.project_id WHERE pp.page_id = $1 AND p.deleted_at IS NULL ORDER BY p.created_at DESC LIMIT 1",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(id,)| id.to_string()))
}

/// `get_entity_data` (`favorite.py:83-88`): `None` for `issue`/`folder`/
/// unknown types, for a `NULL` identifier, and for a missed fetch; else the
/// merged lite port of the live entity row.
async fn fetch_favorite_entity(
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
    let sql = positional(
        &format!(
            "SELECT user_favorites.* FROM user_favorites WHERE {} AND {} ORDER BY {}",
            qx::favorite_scope_where(),
            qx::favorite_branch_where(),
            qx::FAVORITE_LIST_ORDER_SQL,
        ),
        &["slug", "user"],
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(&slug)
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut rendered = Vec::with_capacity(rows.len());
    for row in &rows {
        let favorite = favorite_row_from_pg(row)?;
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

/// One compiled predicate: SQL text, or the failure Django raises while
/// building it (`true` = `FieldError`/500, `false` = `ValidationError`/400).
enum FilterPredicate {
    Sql(String),
    Fail(bool),
}

/// Compile the legacy filters for the draft list: `Ok(sql)` appends to the
/// window (and count) query; `Err(500)` on the first `FieldError` predicate;
/// `Err(400)` on the first `ValidationError` predicate.
fn compile_draft_filters(
    pairs: &[(String, String)],
    now: &DateTime<Utc>,
) -> Result<String, StatusCode> {
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
                        FilterPredicate::Sql(in_uuid_list("draft_issues.state_id", &ids)),
                    );
                }
            }
            "state_group" => {
                let groups = raw_list(raw);
                if !groups.is_empty() {
                    insert(
                        "state__group__in".to_owned(),
                        FilterPredicate::Sql(format!(
                            r#"EXISTS(SELECT 1 FROM states s WHERE s.id = draft_issues.state_id AND s."group" IN ({}))"#,
                            groups
                                .iter()
                                .map(|group| push_text(group))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
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
                            FilterPredicate::Sql(in_uuid_list(
                                "draft_issues.estimate_point_id",
                                &ids,
                            )),
                        );
                    }
                }
            }
            "priority" => {
                let priorities = raw_list(raw);
                if !priorities.is_empty() {
                    insert(
                        "priority__in".to_owned(),
                        FilterPredicate::Sql(format!(
                            "draft_issues.priority IN ({})",
                            priorities
                                .iter()
                                .map(|priority| push_text(priority))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    );
                }
            }
            "parent" => {
                let items: Vec<&str> = raw.split(',').filter(|item| *item != "null").collect();
                if items.contains(&"None") {
                    insert(
                        "parent__isnull".to_owned(),
                        FilterPredicate::Sql("draft_issues.parent_id IS NULL".to_owned()),
                    );
                }
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "parent__in".to_owned(),
                        FilterPredicate::Sql(in_uuid_list("draft_issues.parent_id", &ids)),
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
                        FilterPredicate::Sql("draft_issues.created_by_id IS NULL".to_owned()),
                    );
                }
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "created_by__in".to_owned(),
                        FilterPredicate::Sql(in_uuid_list("draft_issues.created_by_id", &ids)),
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
                        FilterPredicate::Sql(format!(
                            "draft_issues.name ILIKE {} ESCAPE '\\'",
                            push_text(&format!("%{}%", escape_like(raw)))
                        )),
                    );
                }
            }
            "created_at" => compile_date_param(&mut insert, "created_at__date", raw, now),
            // BUG: the `updated_at` param filters `created_at`
            // (`issue_filters.py:236`), overwriting that param's keys.
            "updated_at" => compile_date_param(&mut insert, "created_at__date", raw, now),
            "start_date" => compile_date_param(&mut insert, "start_date", raw, now),
            "target_date" => compile_date_param(&mut insert, "target_date", raw, now),
            "completed_at" => compile_date_param(&mut insert, "completed_at__date", raw, now),
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
                insert(
                    "state__group__in".to_owned(),
                    FilterPredicate::Sql(format!(
                        r#"EXISTS(SELECT 1 FROM states s WHERE s.id = draft_issues.state_id AND s."group" IN ({}))"#,
                        groups
                            .iter()
                            .map(|group| push_text(group))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                );
            }
            "project" => {
                let ids = valid_uuid_list(raw);
                if !ids.is_empty() {
                    insert(
                        "project__in".to_owned(),
                        FilterPredicate::Sql(in_uuid_list("draft_issues.project_id", &ids)),
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
                        FilterPredicate::Sql("draft_issues.parent_id IS NULL".to_owned()),
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
                        FilterPredicate::Sql("draft_issues.target_date IS NOT NULL".to_owned()),
                    );
                    insert(
                        "start_date__isnull".to_owned(),
                        FilterPredicate::Sql("draft_issues.start_date IS NOT NULL".to_owned()),
                    );
                }
            }
            _ => unreachable!("dispatcher key {key}"),
        }
    }
    let mut fragments = Vec::new();
    for (_, predicate) in &predicates {
        match predicate {
            FilterPredicate::Sql(sql) => fragments.push(sql.clone()),
            FilterPredicate::Fail(field_error) => {
                return Err(if *field_error {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    StatusCode::BAD_REQUEST
                });
            }
        }
    }
    Ok(fragments.join(" AND "))
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

fn in_uuid_list(column: &str, ids: &[Uuid]) -> String {
    format!(
        "{column} IN ({})",
        ids.iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(", ")
    )
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
fn compile_date_param(
    insert: &mut impl FnMut(String, FilterPredicate),
    term: &str,
    raw: &str,
    now: &DateTime<Utc>,
) {
    let queries: Vec<&str> = raw.split(',').collect();
    if queries.is_empty() || queries.contains(&"") {
        return;
    }
    let column = match term {
        "created_at__date" => "CAST(draft_issues.created_at AS DATE)",
        "completed_at__date" => "CAST(draft_issues.completed_at AS DATE)",
        "start_date" => "draft_issues.start_date",
        "target_date" => "draft_issues.target_date",
        _ => unreachable!("date term {term}"),
    };
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
                        FilterPredicate::Sql(format!(
                            "{column} {} '{bound}'",
                            if gte { ">=" } else { "<=" }
                        )),
                    );
                }
                // A relative head with != 3 parts writes NOTHING.
            } else if parts.contains(&"after") {
                match parse_drf_date(parts[0]) {
                    Some(date) => insert(
                        format!("{term}__gte"),
                        FilterPredicate::Sql(format!("{column} >= '{date}'")),
                    ),
                    None => insert(format!("{term}__gte"), FilterPredicate::Fail(false)),
                }
            } else {
                match parse_drf_date(parts[0]) {
                    Some(date) => insert(
                        format!("{term}__lte"),
                        FilterPredicate::Sql(format!("{column} <= '{date}'")),
                    ),
                    None => insert(format!("{term}__lte"), FilterPredicate::Fail(false)),
                }
            }
        } else {
            insert(
                format!("{term}__contains"),
                FilterPredicate::Sql(format!(
                    "{column}::text LIKE {} ESCAPE '\\'",
                    push_text(&format!("%{}%", escape_like(parts[0])))
                )),
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

fn draft_row_from_pg(row: &sqlx::postgres::PgRow) -> Result<DraftRow, Denial> {
    use sqlx::Row;
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let name: Option<String> = row.try_get("name").map_err(|_| Denial::ServerError)?;
    let state_id: Option<Uuid> = row.try_get("state_id").map_err(|_| Denial::ServerError)?;
    let sort_order: f64 = row.try_get("sort_order").map_err(|_| Denial::ServerError)?;
    let completed_at: Option<DateTime<Utc>> = row
        .try_get("completed_at")
        .map_err(|_| Denial::ServerError)?;
    let estimate_point: Option<Uuid> = row
        .try_get("estimate_point_id")
        .map_err(|_| Denial::ServerError)?;
    let priority: String = row.try_get("priority").map_err(|_| Denial::ServerError)?;
    let start_date: Option<NaiveDate> =
        row.try_get("start_date").map_err(|_| Denial::ServerError)?;
    let target_date: Option<NaiveDate> = row
        .try_get("target_date")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Option<Uuid> = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let parent_id: Option<Uuid> = row.try_get("parent_id").map_err(|_| Denial::ServerError)?;
    let cycle_id: Option<Uuid> = row.try_get("cycle_id").map_err(|_| Denial::ServerError)?;
    let module_ids: Vec<Uuid> = row.try_get("module_ids").map_err(|_| Denial::ServerError)?;
    let label_ids: Vec<Uuid> = row.try_get("label_ids").map_err(|_| Denial::ServerError)?;
    let assignee_ids: Vec<Uuid> = row
        .try_get("assignee_ids")
        .map_err(|_| Denial::ServerError)?;
    let created_at: DateTime<Utc> = row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let created_by: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by: Option<Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let type_id: Option<Uuid> = row.try_get("type_id").map_err(|_| Denial::ServerError)?;
    let description_html: String = row
        .try_get("description_html")
        .map_err(|_| Denial::ServerError)?;
    Ok(DraftRow {
        id: id.to_string(),
        name,
        state_id: uuid_opt_text(state_id),
        sort_order,
        completed_at,
        estimate_point: uuid_opt_text(estimate_point),
        priority,
        start_date,
        target_date,
        project_id: uuid_opt_text(project_id),
        parent_id: uuid_opt_text(parent_id),
        cycle_id: uuid_opt_text(cycle_id),
        module_ids: module_ids.iter().map(ToString::to_string).collect(),
        label_ids: label_ids.iter().map(ToString::to_string).collect(),
        assignee_ids: assignee_ids.iter().map(ToString::to_string).collect(),
        created_at,
        updated_at,
        created_by: uuid_opt_text(created_by),
        updated_by: uuid_opt_text(updated_by),
        type_id: uuid_opt_text(type_id),
        description_html,
    })
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
    // `.filter(**filters)` runs BEFORE `paginate` (`:100-105`).
    let filters = match compile_draft_filters(&pairs, &utc_now_micros()) {
        Ok(sql) => sql,
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
    let timezone = actor_timezone(&pool, &user_id).await?;
    // The window and count share one filter text: scope + own rows + legacy.
    let where_sql = format!(
        "{} AND draft_issues.created_by_id = '{}'{}",
        positional(&qx::draft_scope_where(), &["slug"]),
        user_id,
        if filters.is_empty() {
            String::new()
        } else {
            format!(" AND ({filters})")
        },
    );
    let from_where = format!("FROM draft_issues WHERE {where_sql}");
    // The `select_related` set is not executed (every rendered key is a
    // local column or a ported annotation); the joins only exist to feed
    // the `ArrayAgg` annotations.
    let window_sql = format!(
        "SELECT DISTINCT draft_issues.*, {}, {}, {}, {} FROM draft_issues {} WHERE {where_sql} GROUP BY draft_issues.id ORDER BY {} LIMIT {} OFFSET {}",
        qx::draft_cycle_subquery_sql(),
        qx::draft_label_ids_annotation_sql(),
        qx::draft_assignee_ids_annotation_sql(),
        qx::draft_module_ids_annotation_sql(),
        draft_annotation_joins(),
        qx::DRAFT_LIST_ORDER_SQL,
        window.stop - window.offset,
        window.offset,
    );
    let window_sql = positional(&window_sql, &["slug"]);
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&window_sql)
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let has_more = rows.len() as i64 > limit;
    let mut decoded = Vec::with_capacity(rows.len());
    for row in &rows {
        decoded.push(draft_row_from_pg(row)?);
    }
    let page = apply_offset_window(&decoded, limit).map_err(page_denial)?;
    let total_count = {
        let sql = format!("SELECT COUNT(DISTINCT draft_issues.id) {from_where}");
        let sql = positional(&sql, &["slug"]);
        let row: (i64,) = sqlx::query_as(&sql)
            .bind(&slug)
            .fetch_one(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        row.0
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

/// The annotation joins behind the draft list's `ArrayAgg`s: alive label /
/// assignee / module links with their target rows (`pm` aliases the
/// unscoped `project_members` leg of the assignee guard).
fn draft_annotation_joins() -> String {
    "LEFT JOIN draft_issue_labels ON draft_issue_labels.draft_issue_id = draft_issues.id LEFT JOIN labels ON labels.id = draft_issue_labels.label_id LEFT JOIN draft_issue_assignees ON draft_issue_assignees.draft_issue_id = draft_issues.id LEFT JOIN users ON users.id = draft_issue_assignees.assignee_id LEFT JOIN project_members pm ON pm.member_id = users.id LEFT JOIN draft_issue_modules ON draft_issue_modules.draft_issue_id = draft_issues.id LEFT JOIN modules ON modules.id = draft_issue_modules.module_id".to_owned()
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
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_dict(&input) {
        return Ok(response);
    }
    // `Workspace.objects.get(slug=slug)` — the scoped manager 404s on a
    // missing or soft-deleted slug.
    let workspace: Option<(Uuid,)> = sqlx::query_as(&positional(
        &format!(
            "SELECT id FROM workspaces WHERE {}",
            qx::favorite_workspace_lookup_where()
        ),
        &["slug"],
    ))
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
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
            let sql = positional(
                &qx::favorite_dedupe_lookup_sql(),
                &["ws", "user", "entity_type", "entity_identifier"],
            );
            let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
                .bind(workspace_id)
                .bind(user_id)
                .bind(entity_type.as_deref())
                .bind(entity_id)
                .fetch_optional(&pool)
                .await
                .map_err(|_| Denial::ServerError)?;
            if let Some(row) = row {
                let favorite = favorite_row_from_pg(&row)?;
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
    let largest: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT MAX(sequence) FROM user_favorites WHERE workspace_id = $1 AND deleted_at IS NULL",
    )
    .bind(row_workspace_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sequence = match largest.and_then(|row| row.0) {
        Some(largest) => largest + 10000.0,
        None => sequence_input.unwrap_or(65535.0),
    };
    let now = utc_now_micros();
    let id = Uuid::new_v4();
    let insert = sqlx::query(
        "INSERT INTO user_favorites (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, user_id, entity_type, entity_identifier, name, is_folder, sequence, parent_id) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(row_workspace_id)
    .bind(project_id)
    .bind(user_id)
    .bind(&entity_type)
    .bind(entity_identifier)
    .bind(&name)
    .bind(is_folder)
    .bind(sequence)
    .bind(parent)
    .execute(&pool)
    .await;
    if let Err(error) = insert {
        if is_integrity_violation(&error) {
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
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_dict(&input) {
        return Ok(response);
    }
    let sql = positional(
        &format!(
            "SELECT user_favorites.* FROM user_favorites WHERE {}",
            qx::favorite_lookup_where()
        ),
        &["user", "slug", "pk"],
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(user_id)
        .bind(&slug)
        .bind(favorite_pk)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let mut favorite = favorite_row_from_pg(&row)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let values = match validate_shape(&pool, &input, FAV_FIELDS, true, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    // Default `ModelSerializer.update`: validated keys only, then `save()`
    // (audit `updated_by`, `auto_now` stamp; the sequence leg is add-only).
    let now = utc_now_micros();
    let mut sets: Vec<String> = Vec::new();
    if has_key(&values, "entity_type") {
        favorite.entity_type = opt_text(&values, "entity_type").ok_or(Denial::ServerError)?;
        sets.push(format!(
            "entity_type = {}",
            push_text(&favorite.entity_type)
        ));
    }
    if has_key(&values, "entity_identifier") {
        favorite.entity_identifier =
            opt_uuid(&values, "entity_identifier").map(|id| id.to_string());
        sets.push(format!(
            "entity_identifier = {}",
            push_uuid_opt(&favorite.entity_identifier)
        ));
    }
    if has_key(&values, "name") {
        favorite.name = opt_text(&values, "name");
        sets.push(format!("name = {}", push_text_opt(&favorite.name)));
    }
    if let Some(is_folder) = opt_bool(&values, "is_folder") {
        favorite.is_folder = is_folder;
        sets.push(format!("is_folder = {is_folder}"));
    }
    if let Some(sequence) = opt_float(&values, "sequence") {
        favorite.sequence = sequence;
        sets.push(format!("sequence = {}", push_float(sequence)));
    }
    if has_key(&values, "parent") {
        favorite.parent = opt_uuid(&values, "parent").map(|id| id.to_string());
        sets.push(format!("parent_id = {}", push_uuid_opt(&favorite.parent)));
    }
    // `save()` always stamps (`auto_now` + audit), even for an empty patch.
    let update = format!(
        "UPDATE user_favorites SET updated_at = '{}', updated_by_id = '{}'{} WHERE id = '{}'",
        now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        user_id,
        if sets.is_empty() {
            String::new()
        } else {
            format!(", {}", sets.join(", "))
        },
        favorite_pk,
    );
    if let Err(error) = sqlx::query(&update).execute(&pool).await {
        if is_integrity_violation(&error) {
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

/// Inline a validated `&str` as a SQL literal (quote-escaped; validated
/// text is trusted but quoting is still exact).
fn push_text(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn push_text_opt(value: &Option<String>) -> String {
    match value {
        Some(text) => push_text(text),
        None => "NULL".to_owned(),
    }
}

fn push_uuid_opt(value: &Option<String>) -> String {
    match value {
        Some(text) => format!("'{text}'"),
        None => "NULL".to_owned(),
    }
}

fn push_float(value: f64) -> String {
    if value.is_nan() {
        "'NaN'".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 {
            "'Infinity'".to_owned()
        } else {
            "'-Infinity'".to_owned()
        }
    } else {
        py_float_str(value)
    }
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
    let lookup = positional(
        &format!(
            "SELECT user_favorites.* FROM user_favorites WHERE {}",
            qx::favorite_lookup_where()
        ),
        &["user", "slug", "pk"],
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&lookup)
        .bind(user_id)
        .bind(&slug)
        .bind(favorite_pk)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if row.is_none() {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    }
    let delete = positional(&qx::favorite_hard_delete_sql(), &["pk"]);
    sqlx::query(&delete)
        .bind(favorite_pk)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
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
    let sql = positional(&qx::favorite_group_list_sql(), &["slug", "user", "fid"]);
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(&slug)
        .bind(user_id)
        .bind(favorite_pk)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut rendered = Vec::with_capacity(rows.len());
    for row in &rows {
        let favorite = favorite_row_from_pg(row)?;
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
    let mut assignee_ids = Vec::new();
    if let Some(FieldValue::Ids(ids)) = find_value(values, "assignee_ids") {
        if !ids.is_empty() {
            assignee_ids = match project_id {
                None => Vec::new(),
                Some(project) => {
                    let sql = assignee_member_filter_sql(ids.len());
                    let mut query = sqlx::query_as::<_, (Uuid,)>(&sql).bind(project);
                    for id in ids {
                        query = query.bind(id);
                    }
                    let rows: Vec<(Uuid,)> = query.fetch_all(pool).await.map_err(|_| {
                        json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            SERVER_ERROR_BODY.to_owned(),
                        )
                    })?;
                    rows.into_iter().map(|row| row.0).collect()
                }
            };
        }
    }
    // Labels (`:104-111`): non-empty lists filter to live labels of the
    // project — or of the NULL project (`IS NULL`: workspace-level labels).
    let mut label_ids = Vec::new();
    if let Some(FieldValue::Ids(ids)) = find_value(values, "label_ids") {
        if !ids.is_empty() {
            let sql = match project_id {
                Some(_) => label_filter_sql(ids.len()),
                None => label_filter_null_project_sql(ids.len()),
            };
            let mut query = sqlx::query_as::<_, (Uuid,)>(&sql);
            if let Some(project) = project_id {
                query = query.bind(project);
            }
            for id in ids {
                query = query.bind(id);
            }
            let rows: Vec<(Uuid,)> = query.fetch_all(pool).await.map_err(|_| {
                json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SERVER_ERROR_BODY.to_owned(),
                )
            })?;
            label_ids = rows.into_iter().map(|row| row.0).collect();
        }
    }
    // State / parent / estimate project-membership (`:112-139`): present
    // and non-null ids must exist under the context project (default
    // managers); a `None` project matches nothing → the error.
    if let Some(state) = dual_uuid(values, "state_id", "state").flatten() {
        let exists = match project_id {
            None => false,
            Some(project) => {
                let row: Option<(i32,)> = sqlx::query_as(STATE_EXISTS_SQL)
                    .bind(project)
                    .bind(state)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| {
                        json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            SERVER_ERROR_BODY.to_owned(),
                        )
                    })?;
                row.is_some()
            }
        };
        if !exists {
            return Err(non_field("State is not valid for the draft issue."));
        }
    }
    if let Some(parent) = dual_uuid(values, "parent_id", "parent").flatten() {
        let exists = match project_id {
            None => false,
            Some(project) => {
                let row: Option<(i32,)> = sqlx::query_as(PARENT_EXISTS_SQL)
                    .bind(project)
                    .bind(parent)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| {
                        json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            SERVER_ERROR_BODY.to_owned(),
                        )
                    })?;
                row.is_some()
            }
        };
        if !exists {
            return Err(non_field("Parent is not valid for the draft issue."));
        }
    }
    if let Some(estimate) = opt_uuid(values, "estimate_point") {
        let exists = match project_id {
            None => false,
            Some(project) => {
                let row: Option<(i32,)> = sqlx::query_as(ESTIMATE_EXISTS_SQL)
                    .bind(project)
                    .bind(estimate)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| {
                        json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            SERVER_ERROR_BODY.to_owned(),
                        )
                    })?;
                row.is_some()
            }
        };
        if !exists {
            return Err(non_field(
                "Estimate point is not valid for the draft issue.",
            ));
        }
    }
    Ok(DraftObjectAttrs {
        assignee_ids,
        label_ids,
        description_html,
    })
}

/// The context `project_id` for draft writes (`request.data.get(...)`,
/// RAW and unvalidated): missing/`null` → `None`; an uncoercible value →
/// 400 `{"error": "Please provide valid detail"}` (the ORM filter's
/// `ValidationError`, raised either by a `validate()` arm or by `save()`'s
/// project fetch — same body, same status).
#[allow(clippy::result_large_err)]
fn draft_context_project(input: &RequestData, key: &str) -> Result<Option<Uuid>, Response> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(raw) => match django_coerce_uuid(raw) {
            Some(id) => Ok(Some(id)),
            None => Err(json_response(
                StatusCode::BAD_REQUEST,
                INVALID_DETAIL_BODY.to_owned(),
            )),
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
    for default_only in [true, false] {
        let sql = format!(
            "SELECT id FROM states WHERE project_id = $1 AND deleted_at IS NULL AND \"group\" != 'triage' AND is_triage IS DISTINCT FROM TRUE{} ORDER BY sequence ASC LIMIT 1",
            if default_only { " AND \"default\" = TRUE" } else { "" },
        );
        let row: Option<(Uuid,)> = sqlx::query_as(&sql)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        if let Some((id,)) = row {
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
    let row: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT MAX(sort_order) FROM draft_issues WHERE project_id IS NOT DISTINCT FROM $1 AND state_id IS NOT DISTINCT FROM $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(state_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.and_then(|row| row.0).map(|max| max + 10000.0))
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
        roles: &[ROLE_ADMIN, ROLE_MEMBER],
    };
    if let Some(denied) = check_allow_gate(&pool, &gate, &slug, &user_id, false).await? {
        return Ok(denied);
    }
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_dict(&input) {
        return Ok(response);
    }
    let workspace: Option<(Uuid,)> = sqlx::query_as(&positional(
        &format!(
            // Both views share the lookup (`Workspace.objects.get(slug)`).
            "SELECT id FROM workspaces WHERE {}",
            qx::favorite_workspace_lookup_where()
        ),
        &["slug"],
    ))
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            OBJECT_NOT_FOUND_BODY.to_owned(),
        ));
    };
    // The context `project_id` is RAW (`request.data.get`, unvalidated).
    let context_project = match draft_context_project(&input, "project_id") {
        Ok(project) => project,
        Err(response) => return Ok(response),
    };
    let timezone = actor_timezone(&pool, &user_id).await?;
    let values = match validate_shape(&pool, &input, DRAFT_FIELDS, false, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    let object = match draft_object_leg(&pool, &values, context_project).await {
        Ok(object) => object,
        Err(response) => return Ok(response),
    };
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
    let insert = sqlx::query(
        "INSERT INTO draft_issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, parent_id, state_id, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, start_date, target_date, sort_order, completed_at, external_source, external_id, type_id) VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, NULL, $15, $16, $17, $18, $19, $20, $21, $22)",
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(deleted_at)
    .bind(row_workspace)
    .bind(row_project)
    .bind(parent_id)
    .bind(state_id)
    .bind(estimate_point_id)
    .bind(&name)
    .bind(&description_json)
    .bind(&description_html)
    .bind(&description_stripped)
    .bind(&priority)
    .bind(start_date)
    .bind(target_date)
    .bind(sort_order)
    .bind(completed_at)
    .bind(&external_source)
    .bind(&external_id)
    .bind(type_id)
    .execute(&pool)
    .await;
    if let Err(error) = insert {
        if is_integrity_violation(&error) {
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
            if let Err(error) = sqlx::query(
                "INSERT INTO draft_issue_cycles (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, draft_issue_id, cycle_id) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8)",
            )
            .bind(link)
            .bind(now)
            .bind(now)
            .bind(user_id)
            .bind(row_workspace)
            .bind(row_project)
            .bind(id)
            .bind(cycle_id)
            .execute(&pool)
            .await
            {
                if is_integrity_violation(&error) {
                    return Ok(json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()));
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
                    if is_integrity_violation(&error) {
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
/// inherited from the (fresh or patch-target) issue row.
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
    for chunk in assignee_ids.chunks(10) {
        let mut query = "INSERT INTO draft_issue_assignees (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, draft_issue_id, assignee_id) VALUES ".to_owned();
        let now = utc_now_micros();
        let now_text = now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let updated_text = match updated_by {
            Some(id) => format!("'{id}'"),
            None => "NULL".to_owned(),
        };
        let project_text = match project_id {
            Some(id) => format!("'{id}'"),
            None => "NULL".to_owned(),
        };
        let tuples: Vec<String> = chunk
            .iter()
            .map(|assignee| {
                let link = Uuid::new_v4();
                format!(
                    "('{link}', '{now_text}', '{now_text}', '{created_by}', {updated_text}, NULL, '{workspace_id}', {project_text}, '{draft_id}', '{assignee}')"
                )
            })
            .collect();
        query.push_str(&tuples.join(", "));
        if let Err(error) = sqlx::query(&query).execute(pool).await {
            if is_integrity_violation(&error) {
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
    for chunk in label_ids.chunks(10) {
        let mut query = "INSERT INTO draft_issue_labels (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, draft_issue_id, label_id) VALUES ".to_owned();
        let now = utc_now_micros();
        let now_text = now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let updated_text = match updated_by {
            Some(id) => format!("'{id}'"),
            None => "NULL".to_owned(),
        };
        let project_text = match project_id {
            Some(id) => format!("'{id}'"),
            None => "NULL".to_owned(),
        };
        let tuples: Vec<String> = chunk
            .iter()
            .map(|label| {
                let link = Uuid::new_v4();
                format!(
                    "('{link}', '{now_text}', '{now_text}', '{created_by}', {updated_text}, NULL, '{workspace_id}', {project_text}, '{draft_id}', '{label}')"
                )
            })
            .collect();
        query.push_str(&tuples.join(", "));
        if let Err(error) = sqlx::query(&query).execute(pool).await {
            if is_integrity_violation(&error) {
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
    Ok(())
}

/// Batched draft-module INSERTs (batch 10).
async fn insert_draft_modules(
    pool: &sqlx::PgPool,
    draft_id: &Uuid,
    workspace_id: &Uuid,
    project_id: &Option<Uuid>,
    created_by: &Uuid,
    updated_by: &Option<Uuid>,
    module_ids: &[Option<Uuid>],
) -> Result<(), sqlx::Error> {
    for chunk in module_ids.chunks(10) {
        let mut query = "INSERT INTO draft_issue_modules (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, draft_issue_id, module_id) VALUES ".to_owned();
        let now = utc_now_micros();
        let now_text = now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let updated_text = match updated_by {
            Some(id) => format!("'{id}'"),
            None => "NULL".to_owned(),
        };
        let project_text = match project_id {
            Some(id) => format!("'{id}'"),
            None => "NULL".to_owned(),
        };
        let tuples: Vec<String> = chunk
            .iter()
            .map(|module| {
                let link = Uuid::new_v4();
                let module_text = match module {
                    Some(id) => format!("'{id}'"),
                    None => "NULL".to_owned(),
                };
                format!(
                    "('{link}', '{now_text}', '{now_text}', '{created_by}', {updated_text}, NULL, '{workspace_id}', {project_text}, '{draft_id}', {module_text})"
                )
            })
            .collect();
        query.push_str(&tuples.join(", "));
        sqlx::query(&query).execute(pool).await?;
    }
    Ok(())
}

/// The create re-read (`draft.py:149-157`): one annotated row over the slug
/// scope (miss → `None` → 201 `null`).
async fn reread_draft(
    pool: &sqlx::PgPool,
    slug: &str,
    id: &Uuid,
) -> Result<Option<DraftRow>, Denial> {
    let sql = format!(
        "SELECT DISTINCT draft_issues.*, {}, {}, {}, {} FROM draft_issues {} WHERE {} AND draft_issues.id = '{}' GROUP BY draft_issues.id LIMIT 1",
        qx::draft_cycle_subquery_sql(),
        qx::draft_label_ids_annotation_sql(),
        qx::draft_assignee_ids_annotation_sql(),
        qx::draft_module_ids_annotation_sql(),
        draft_annotation_joins(),
        positional(&qx::draft_scope_where(), &["slug"]),
        id,
    );
    let sql = positional(&sql, &["slug"]);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| draft_row_from_pg(&row)).transpose()
}

/// Render the create re-read in `DRAFT_CREATE_READ_KEYS` order (the same 21
/// keys as the list shape): UTC MILLIS datetimes (`DjangoJSONEncoder`, no
/// `user_timezone_converter`).
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
        json_string(&render_encoder_datetime(&row.created_at)),
        json_string(&render_encoder_datetime(&row.updated_at)),
        opt(&row.created_by),
        opt(&row.updated_by),
        opt(&row.type_id),
        json_string(&row.description_html),
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
    use sqlx::Row;
    let mut sql = format!(
        "SELECT draft_issues.* FROM draft_issues WHERE {} AND draft_issues.id = '{}'",
        positional(&qx::draft_scope_where(), &["slug"]),
        pk,
    );
    if let Some(user) = own_only {
        sql.push_str(&format!(" AND draft_issues.created_by_id = '{user}'"));
    }
    sql.push_str(" LIMIT 1");
    let sql = positional(&sql, &["slug"]);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(DraftFullRow {
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|_| Denial::ServerError)?,
        project_id: row.try_get("project_id").map_err(|_| Denial::ServerError)?,
        state_id: row.try_get("state_id").map_err(|_| Denial::ServerError)?,
        created_by: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
        updated_by: row
            .try_get("updated_by_id")
            .map_err(|_| Denial::ServerError)?,
        description_html: row
            .try_get("description_html")
            .map_err(|_| Denial::ServerError)?,
        completed_at: row
            .try_get("completed_at")
            .map_err(|_| Denial::ServerError)?,
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
    let input = match negotiate_input(&headers, &body) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    if let Err(response) = require_dict(&input) {
        return Ok(response);
    }
    let Some(issue) = draft_full_row(&pool, &slug, &draft_pk, Some(&user_id)).await? else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            DRAFT_PATCH_404_BODY.to_owned(),
        ));
    };
    // Context project: raw input wins, else the issue's own (`:168`).
    let context_project = match input.get("project_id") {
        Some(Value::Null) | None => issue.project_id,
        Some(raw) => match django_coerce_uuid(raw) {
            Some(id) => Some(id),
            None => {
                return Ok(json_response(
                    StatusCode::BAD_REQUEST,
                    INVALID_DETAIL_BODY.to_owned(),
                ))
            }
        },
    };
    let timezone = actor_timezone(&pool, &user_id).await?;
    let values = match validate_shape(&pool, &input, DRAFT_FIELDS, true, &timezone).await {
        Ok(values) => values,
        Err(ShapeFailure::Errors(errors)) => return Ok(shape_errors_response(&errors)),
        Err(ShapeFailure::Denial(denial)) => return Err(denial),
    };
    let object = match draft_object_leg(&pool, &values, context_project).await {
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
            let link = Uuid::new_v4();
            if let Err(error) = sqlx::query(
                "INSERT INTO draft_issue_cycles (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, draft_issue_id, cycle_id) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8)",
            )
            .bind(link)
            .bind(now)
            .bind(now)
            .bind(user_id)
            .bind(issue.workspace_id)
            .bind(issue.project_id)
            .bind(draft_pk)
            .bind(cycle_id)
            .execute(&pool)
            .await
            {
                if is_integrity_violation(&error) {
                    return Ok(json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()));
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
                if is_integrity_violation(&error) {
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
    let (state_id, completed_at) =
        draft_save_state(&pool, post_project, post_state, issue.completed_at, &now).await?;
    let post_html = object
        .description_html
        .clone()
        .unwrap_or(issue.description_html.clone());
    let stripped = sync_description_stripped(Some(&post_html));
    // Assemble the UPDATE: validated keys + save halves + audit stamp.
    let mut sets: Vec<String> = Vec::new();
    if has_key(&values, "deleted_at") {
        sets.push(format!(
            "deleted_at = {}",
            push_moment(&opt_datetime(&values, "deleted_at"))
        ));
    }
    if has_key(&values, "name") {
        sets.push(format!(
            "name = {}",
            push_text_opt(&opt_text(&values, "name"))
        ));
    }
    if has_key(&values, "description_json") {
        sets.push(format!(
            "description_json = {}",
            push_text(
                &opt_json(&values, "description_json")
                    .unwrap_or(Value::Null)
                    .to_string()
            )
        ));
    }
    if has_key(&values, "description_html") {
        // `save()` recomputes `stripped` from the new html; when the html
        // is untouched the recompute is value-identical (write skipped).
        sets.push(format!("description_html = {}", push_text(&post_html)));
        sets.push(format!(
            "description_stripped = {}",
            push_text_opt(&stripped)
        ));
    }
    if has_key(&values, "priority") {
        sets.push(format!(
            "priority = {}",
            push_text(&opt_text(&values, "priority").unwrap_or_else(|| "none".to_owned()))
        ));
    }
    if has_key(&values, "start_date") {
        sets.push(format!(
            "start_date = {}",
            push_date(&nullable_date(&values, "start_date").flatten())
        ));
    }
    if has_key(&values, "target_date") {
        sets.push(format!(
            "target_date = {}",
            push_date(&nullable_date(&values, "target_date").flatten())
        ));
    }
    if has_key(&values, "sort_order") {
        if let Some(sort) = opt_float(&values, "sort_order") {
            sets.push(format!("sort_order = {}", push_float(sort)));
        }
    }
    if has_key(&values, "completed_at") || post_state.is_some() {
        // Recomputed whenever the post-setattr state is non-null (even for
        // name-only patches); an explicit input only survives a null state.
        sets.push(format!("completed_at = {}", push_moment(&completed_at)));
    }
    if has_key(&values, "external_source") {
        sets.push(format!(
            "external_source = {}",
            push_text_opt(&opt_text(&values, "external_source"))
        ));
    }
    if has_key(&values, "external_id") {
        sets.push(format!(
            "external_id = {}",
            push_text_opt(&opt_text(&values, "external_id"))
        ));
    }
    if has_key(&values, "project") {
        sets.push(format!(
            "project_id = {}, workspace_id = '{}'",
            push_uuid_ref(&post_project),
            post_workspace
        ));
    }
    if dual_uuid(&values, "parent_id", "parent").is_some() {
        sets.push(format!(
            "parent_id = {}",
            push_uuid_ref(&dual_uuid(&values, "parent_id", "parent").flatten())
        ));
    }
    if dual_uuid(&values, "state_id", "state").is_some() || state_id != issue.state_id {
        sets.push(format!("state_id = {}", push_uuid_ref(&state_id)));
    }
    if has_key(&values, "estimate_point") {
        sets.push(format!(
            "estimate_point_id = {}",
            push_uuid_ref(&opt_uuid(&values, "estimate_point"))
        ));
    }
    if has_key(&values, "type") {
        sets.push(format!(
            "type_id = {}",
            push_uuid_ref(&opt_uuid(&values, "type"))
        ));
    }
    let update = format!(
        "UPDATE draft_issues SET updated_at = '{}', updated_by_id = '{}'{} WHERE id = '{}'",
        now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        user_id,
        if sets.is_empty() {
            String::new()
        } else {
            format!(", {}", sets.join(", "))
        },
        draft_pk,
    );
    if let Err(error) = sqlx::query(&update).execute(&pool).await {
        if is_integrity_violation(&error) {
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
) -> Result<(), sqlx::Error> {
    let sql = format!(
        "UPDATE {table} SET deleted_at = '{}' WHERE draft_issue_id = '{draft_id}' AND deleted_at IS NULL",
        now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
    );
    sqlx::query(&sql).execute(pool).await?;
    Ok(())
}

fn push_moment(value: &Option<DateTime<Utc>>) -> String {
    match value {
        Some(moment) => format!(
            "'{}'",
            moment.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        ),
        None => "NULL".to_owned(),
    }
}

fn push_date(value: &Option<NaiveDate>) -> String {
    match value {
        Some(date) => format!("'{date}'"),
        None => "NULL".to_owned(),
    }
}

fn push_uuid_ref(value: &Option<Uuid>) -> String {
    match value {
        Some(id) => format!("'{id}'"),
        None => "NULL".to_owned(),
    }
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
    let message = CeleryTaskMessage::new(
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
    if keep_completed {
        sqlx::query(
            "UPDATE draft_issues SET deleted_at = $1, updated_at = $1, updated_by_id = $2, state_id = $3 WHERE id = $4",
        )
        .bind(now)
        .bind(actor)
        .bind(resolved_state)
        .bind(draft_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    } else {
        sqlx::query(
            "UPDATE draft_issues SET deleted_at = $1, updated_at = $1, updated_by_id = $2, completed_at = $3 WHERE id = $4",
        )
        .bind(now)
        .bind(actor)
        .bind(completed_at)
        .bind(draft_id)
        .execute(pool)
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
    let row: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM projects WHERE id = $1 LIMIT 1")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|_| ()).ok_or(Denial::NotFound)
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
    let row: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT d.project_id, d.state_id FROM draft_issues d JOIN workspaces w ON w.id = d.workspace_id WHERE w.slug = $1 AND d.id = $2 AND d.deleted_at IS NULL LIMIT 1",
    )
    .bind(&slug)
    .bind(draft_pk)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((project_id, state_id)) = row else {
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
    let row: Option<(Uuid, Uuid, Option<DateTime<Utc>>)> = sqlx::query_as(POD_FETCH_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (id, project_id, deleted_at) = row.ok_or(Denial::ServerError)?;
    Ok(PodRef {
        id,
        project_id,
        deleted: deleted_at.is_some(),
    })
}

/// `ProjectMember` assignee filter (`issue.py:338-344`): the silent drop,
/// `-created_at` order.
async fn filter_assignee_ids(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Denial> {
    let sql = assignee_member_filter_sql(ids.len());
    let mut query = sqlx::query_as::<_, (Uuid,)>(&sql).bind(project_id);
    for id in ids {
        query = query.bind(id);
    }
    let rows: Vec<(Uuid,)> = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

/// `Label` filter (`issue.py:347-354`): the silent drop, `-created_at`
/// order. The context project is never `None` here (the project guard),
/// so the null-project variant stays unissued.
async fn filter_label_ids(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Denial> {
    let sql = label_filter_sql(ids.len());
    let mut query = sqlx::query_as::<_, (Uuid,)>(&sql).bind(project_id);
    for id in ids {
        query = query.bind(id);
    }
    let rows: Vec<(Uuid,)> = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

/// `Project.objects.filter(pk).first()` for the managed arm
/// (`issue.py:296-299`).
async fn fetch_issue_project(
    pool: &sqlx::PgPool,
    id: &Uuid,
) -> Result<Option<ResolvedProject>, Denial> {
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(PROJECT_FETCH_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(project_id, workspace_id)| ResolvedProject {
        project_id,
        workspace_id,
    }))
}

/// `managed_llm_profile` seam (`model_provider.py:67-68,85-91`): the
/// verdict is `bool(api_key_encrypted)`.
async fn assistant_has_api_key(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<bool, Denial> {
    let row: Option<(Option<Vec<u8>>,)> = sqlx::query_as(
        "SELECT api_key_encrypted FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.and_then(|row| row.0).is_some_and(|key| !key.is_empty()))
}

async fn enrolled_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(ENROLLED_MANAGED_RUNNERS_EXISTS_SQL)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

async fn online_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, Denial> {
    let bound = Utc::now() - chrono::Duration::seconds(HEARTBEAT_GRACE_SECS);
    let row: Option<(Uuid,)> = sqlx::query_as(ONLINE_MANAGED_RUNNER_SQL)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(bound)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// `request.user` flags for the executor arm (in-memory on Django; one
/// read here, issued only past a managed pin + resolved project).
async fn viewer_user_flags(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<UserFlags, Denial> {
    let row: Option<(bool, bool)> =
        sqlx::query_as("SELECT is_active, is_bot FROM users WHERE id = $1 LIMIT 1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (is_active, is_bot) = row.ok_or(Denial::ServerError)?;
    Ok(UserFlags { is_active, is_bot })
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
        Some(id) => {
            let row: Option<(i32,)> = sqlx::query_as(STATE_EXISTS_SQL)
                .bind(project_id)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|_| db_error())?;
            Some(row.is_some())
        }
        None => None,
    };
    let parent_ok = match parent_value {
        Some(id) => {
            let row: Option<(i32,)> = sqlx::query_as(PARENT_EXISTS_SQL)
                .bind(project_id)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|_| db_error())?;
            Some(row.is_some())
        }
        None => None,
    };
    let estimate_ok = match attrs.estimate_point {
        Some(id) => {
            let row: Option<(i32,)> = sqlx::query_as(ESTIMATE_EXISTS_SQL)
                .bind(project_id)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|_| db_error())?;
            Some(row.is_some())
        }
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

/// Every `issues` column in `COLUMNS` order, as owned values — the
/// INSERT binds from here, and the 201 body renders off it.
struct IssueSaveRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    project_id: Uuid,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    state_id: Option<Uuid>,
    point: Option<i32>,
    estimate_point_id: Option<Uuid>,
    name: String,
    description_json: Value,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    priority: String,
    complexity_score: i32,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    type_id: Option<Uuid>,
    git_work_branch: String,
    workpad: String,
    created_via: Option<String>,
    assigned_pod_id: Option<Uuid>,
    agent_executor: Option<String>,
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
        agent_executor: opt_text(values, "agent_executor"),
    };
    Ok((
        row,
        opt_ids(values, "assignee_ids"),
        opt_ids(values, "label_ids"),
    ))
}

/// `convert_uuid_to_integer` (`utils/uuid.py:19-26`): sha256 of the
/// hyphenated-lowercase id, first 8 bytes big-endian signed.
fn advisory_lock_key(project_id: &Uuid) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(project_id.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// `Pod.default_for_project_id` (`runner/models.py:174-176`):
/// `Pod.objects` (live), project-default first (probed live:
/// `ORDER BY is_default DESC, created_at ASC`).
async fn default_pod_for_project(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM pod WHERE project_id = $1 AND is_default AND deleted_at IS NULL ORDER BY is_default DESC, created_at ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `Issue.objects.create(...)`: the 34-column INSERT in `COLUMNS` order.
/// An `IntegrityError` is the 400 payload branch; anything else 500s.
async fn insert_issue_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &IssueSaveRow,
) -> Result<(), Denial> {
    sqlx::query(
        "INSERT INTO issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, external_id, type_id, git_work_branch, workpad, created_via, assigned_pod_id, agent_executor) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, $34)",
    )
    .bind(row.id)
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(row.created_by_id)
    .bind(row.updated_by_id)
    .bind(row.deleted_at)
    .bind(row.project_id)
    .bind(row.workspace_id)
    .bind(row.parent_id)
    .bind(row.state_id)
    .bind(row.point)
    .bind(row.estimate_point_id)
    .bind(row.name.as_str())
    .bind(row.description_json.clone())
    .bind(row.description_html.as_str())
    .bind(row.description_stripped.as_deref())
    .bind(row.description_binary.as_deref())
    .bind(row.priority.as_str())
    .bind(row.complexity_score)
    .bind(row.start_date)
    .bind(row.target_date)
    .bind(row.sequence_id)
    .bind(row.sort_order)
    .bind(row.completed_at)
    .bind(row.archived_at)
    .bind(row.is_draft)
    .bind(row.external_source.as_deref())
    .bind(row.external_id.as_deref())
    .bind(row.type_id)
    .bind(row.git_work_branch.as_str())
    .bind(row.workpad.as_str())
    .bind(row.created_via.as_deref())
    .bind(row.assigned_pod_id)
    .bind(row.agent_executor.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        if is_integrity_violation(&error) {
            Denial::BadError("The payload is not valid".to_owned())
        } else {
            Denial::ServerError
        }
    })?;
    Ok(())
}

/// One m2m `bulk_create(batch_size=10)` leg: a multi-row INSERT per
/// 10-chunk in input order; an `IntegrityError` swallows the REST of the
/// batches (`except: pass` wraps the whole call), any other error 500s.
#[allow(clippy::too_many_arguments)]
async fn write_issue_m2m_batches(
    pool: &sqlx::PgPool,
    table: &str,
    member_column: &str,
    ids: &[Uuid],
    issue_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
) -> Result<(), Denial> {
    for batch in m2m_batches(ids) {
        let sql = m2m_insert_sql(table, member_column, batch.len(), false);
        let mut query = sqlx::query(&sql);
        for member_id in batch {
            // `auto_now_add` + `auto_now`: two `now()` calls per row, like
            // the per-object instantiation Django bulk-saves.
            let created = utc_now_micros();
            let updated = utc_now_micros();
            query = query
                .bind(Uuid::new_v4())
                .bind(created)
                .bind(updated)
                .bind(created_by_id)
                .bind(updated_by_id)
                .bind(project_id)
                .bind(workspace_id)
                .bind(issue_id)
                .bind(member_id);
        }
        match query.execute(pool).await {
            Ok(_) => {}
            Err(error) if is_integrity_violation(&error) => break,
            Err(_) => return Err(Denial::ServerError),
        }
    }
    Ok(())
}

/// The create default-assignee fallback (`serializers/issue.py:419-444`):
/// empty/missing assignees + a valid project-default member → one
/// `IssueAssignee` row, `IntegrityError` swallowed.
async fn write_default_assignee(
    pool: &sqlx::PgPool,
    default_assignee_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    created_by_id: Option<Uuid>,
) -> Result<(), Denial> {
    let member: Option<(i32,)> = sqlx::query_as(DEFAULT_ASSIGNEE_EXISTS_SQL)
        .bind(default_assignee_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if member.is_none() {
        return Ok(());
    }
    let sql = m2m_insert_sql("issue_assignees", "assignee_id", 1, false);
    let created = utc_now_micros();
    let updated = utc_now_micros();
    match sqlx::query(&sql)
        .bind(Uuid::new_v4())
        .bind(created)
        .bind(updated)
        .bind(created_by_id)
        .bind(None::<Uuid>)
        .bind(project_id)
        .bind(workspace_id)
        .bind(issue_id)
        .bind(default_assignee_id)
        .execute(pool)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if is_integrity_violation(&error) => Ok(()),
        Err(_) => Err(Denial::ServerError),
    }
}

/// The post-save fire: a non-transition answers `NoTransition` without
/// I/O; a failed handler is logged with the verbatim line (the counter
/// already bumped inside) and the save stands; only a *lookup* storage
/// failure propagates to the 500 — exactly the `try` placement in
/// `fire_state_transition`.
async fn fire_after_save(
    pool: &sqlx::PgPool,
    issue_id: Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
    dispatch_immediate: bool,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    let mut seam = DraftSignalSeam { pool };
    let mut preflight = DraftPreflight;
    let outcome = fire_state_transition(
        &mut seam,
        &mut preflight,
        &FireRequest {
            issue_id,
            prev_state_id,
            current_state_id,
            dispatch_immediate,
            moved_by_run: None,
            now,
            jitter_secs: 0.0,
            created_by,
        },
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    if let FireOutcome::Failed { log_line, .. } = outcome {
        tracing::error!("{log_line}");
    }
    Ok(())
}

/// The live seam for the draft-to-issue save. Only two methods run live:
/// [`EntriesSeam::prior_state_id`] (the pre-save snapshot) and
/// [`CreationSeam::state`] (the transition's state lookups) — the fire
/// short-circuits on equal states before any other seam use, and
/// transitions into non-trigger states return before the drivers run. A
/// transition that WOULD dispatch answers a store error, which the fire
/// swallows like a handler raise (counter + log line, save stands)
/// rather than crashing the request — the HTTP surface is unaffected
/// either way.
struct DraftSignalSeam<'a> {
    pool: &'a sqlx::PgPool,
}

/// The draft-to-issue save never dispatches through preflight, so it
/// never runs.
struct DraftPreflight;

fn seam_unreachable<T>(method: &str) -> Result<T, CreationError> {
    Err(CreationError::Db(format!(
        "draft signal seam: {method} runs only past a state change"
    )))
}

impl CreationSeam for DraftSignalSeam<'_> {
    async fn issue(&mut self, _issue_id: Uuid) -> Result<IssueView, CreationError> {
        seam_unreachable("issue")
    }

    async fn project(&mut self, _project_id: Uuid) -> Result<ProjectView, CreationError> {
        seam_unreachable("project")
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
            .bind(state_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        match row {
            None => Err(CreationError::MissingRow(format!(
                "states row {state_id} is gone"
            ))),
            Some((id, name, group)) => Ok(Some(StateView { id, name, group })),
        }
    }

    async fn latest_prior_run(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("latest_prior_run")
    }

    async fn active_run_for(&mut self, _issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("active_run_for")
    }

    async fn run(&mut self, _run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("run")
    }

    async fn runner(&mut self, _runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        seam_unreachable("runner")
    }

    async fn assigned_pod(&mut self, _pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("assigned_pod")
    }

    async fn default_pod_for_project(
        &mut self,
        _project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("default_pod_for_project")
    }

    async fn resume_parent_run_id(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        seam_unreachable("resume_parent_run_id")
    }

    async fn work_item_id_for_run(&mut self, _run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        seam_unreachable("work_item_id_for_run")
    }

    async fn lock_issue_for_handoff(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        seam_unreachable("lock_issue_for_handoff")
    }

    async fn lock_run_for_handoff(
        &mut self,
        _run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("lock_run_for_handoff")
    }

    async fn user_flags(&mut self, _user_id: Uuid) -> Result<UserFlags, CreationError> {
        seam_unreachable("user_flags")
    }

    async fn insert_run(&mut self, _row: &NewAgentRun) -> Result<RunView, CreationError> {
        seam_unreachable("insert_run")
    }

    async fn save_prompt(
        &mut self,
        _run_id: Uuid,
        _prompt: &str,
        _manifest: &Value,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_prompt")
    }

    async fn save_run_config(
        &mut self,
        _run_id: Uuid,
        _config: &Value,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_run_config")
    }

    async fn execution_fields(
        &mut self,
        _req: &ExecutionRequest,
    ) -> Result<ExecutionFields, ExecutionError> {
        Err(CreationError::Db(
            "draft signal seam: execution_fields runs only past a state change".to_owned(),
        )
        .into())
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        _workspace_id: Uuid,
        _executor_kind: AgentExecutorKind,
        _automatic: bool,
    ) -> Result<Option<AdmissionError>, CreationError> {
        seam_unreachable("lock_cloud_creation_capacity")
    }

    fn dispatch_after_commit(&mut self, _run_id: Uuid) {}

    async fn render_bundle(
        &mut self,
        _issue_id: Uuid,
        _run_id: Uuid,
        _parent_run_id: Option<Uuid>,
        _trigger: &str,
        _created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        seam_unreachable("render_bundle")
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        String::new()
    }
}

impl FinalizeAgentRunSeam for DraftSignalSeam<'_> {
    async fn finalize_failed_run(
        &mut self,
        _run_id: Uuid,
        _error_code: &str,
        _error: &str,
        _now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        seam_unreachable("finalize_failed_run")
    }
}

impl EntriesSeam for DraftSignalSeam<'_> {
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        Ok(row.and_then(|(_, state_id)| state_id))
    }

    async fn queued_follow_up(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("queued_follow_up")
    }

    async fn lock_ticker(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        seam_unreachable("lock_ticker")
    }

    async fn save_ticker(
        &mut self,
        _row: &IssueAgentTicker,
        _write: pidash_services::orchestration::clock::ClockWrite,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_ticker")
    }

    fn set_rollback(&mut self) {}

    async fn clock_policy(
        &mut self,
        _project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        seam_unreachable("clock_policy")
    }

    async fn binding(
        &mut self,
        _binding_id: Uuid,
    ) -> Result<pidash_services::orchestration::entries::BindingView, CreationError> {
        seam_unreachable("binding")
    }

    async fn scheduler_override_pod(
        &mut self,
        _pod_id: Uuid,
        _project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("scheduler_override_pod")
    }

    async fn workspace(
        &mut self,
        _workspace_id: Uuid,
    ) -> Result<pidash_services::orchestration::entries::WorkspaceView, CreationError> {
        seam_unreachable("workspace")
    }

    async fn scheduler_row(
        &mut self,
        _scheduler_id: Uuid,
    ) -> Result<pidash_services::orchestration::entries::SchedulerView, CreationError> {
        seam_unreachable("scheduler_row")
    }

    async fn scheduler_override_rows(
        &mut self,
        _workspace_id: Uuid,
    ) -> Result<Vec<OverrideRow>, CreationError> {
        seam_unreachable("scheduler_override_rows")
    }

    async fn project_role_facts(
        &mut self,
        _user_id: Uuid,
        _workspace_slug: &str,
        _project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        seam_unreachable("project_role_facts")
    }

    async fn has_usable_llm_config(&mut self, _user_id: Uuid) -> Result<bool, CreationError> {
        seam_unreachable("has_usable_llm_config")
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<
        Result<Uuid, pidash_db::orchestration::workpad::AgentUserCollisionError>,
        CreationError,
    > {
        seam_unreachable("agent_system_user")
    }

    async fn insert_scheduler_run(
        &mut self,
        _row: &pidash_services::orchestration::entries::NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        seam_unreachable("insert_scheduler_run")
    }

    fn compose_scheduler_turn(
        &mut self,
        _context: &Value,
        _index: &pidash_services::prompting::composer::OverrideIndex,
        _workspace_id: Option<&str>,
        _executor_kind: Option<&str>,
        _tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        Err("draft signal seam: compose_scheduler_turn runs only past a state change".to_owned())
    }
}

impl PreflightSeam for DraftPreflight {
    async fn preflight_eligibility_or_bounce(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
        _triggered_by: &str,
    ) -> Result<bool, CreationError> {
        seam_unreachable("preflight_eligibility_or_bounce")
    }
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
    let message = CeleryTaskMessage::new(emit.task_name(), vec![], emit.kwargs());
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
    let assignees: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT u.id FROM users AS u JOIN issue_assignees AS ia ON ia.assignee_id = u.id WHERE ia.issue_id = $1 ORDER BY u.created_at DESC",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let labels: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT l.id FROM labels AS l JOIN issue_labels AS il ON il.label_id = l.id WHERE il.issue_id = $1 AND l.deleted_at IS NULL ORDER BY l.created_at DESC",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok((
        assignees.into_iter().map(|row| row.0).collect(),
        labels.into_iter().map(|row| row.0).collect(),
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
    type DraftConvertRow = (Option<Uuid>, Uuid, Option<Uuid>, Option<Uuid>, Option<Uuid>);
    let draft: Option<DraftConvertRow> = sqlx::query_as(
        "SELECT d.project_id, d.workspace_id, d.created_by_id, d.updated_by_id, d.state_id FROM draft_issues d JOIN workspaces w ON w.id = d.workspace_id WHERE w.slug = $1 AND d.id = $2 AND d.deleted_at IS NULL LIMIT 1",
    )
    .bind(&slug)
    .bind(draft_pk)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
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
    let (mut row, raw_assignees, raw_labels) = assemble_new_issue(
        &values,
        &filtered,
        assigned_pod.flatten().map(|pod| pod.id),
        &draft_project,
        &project.workspace_id,
        &user_id,
    )?;
    if row.assigned_pod_id.is_none() {
        row.assigned_pod_id = default_pod_for_project(&pool, &draft_project).await?;
    }
    if row.state_id.is_none() {
        // The `self.project` touch runs inside the atomic block on the
        // state-given leg (the lock key); hoisted here — a `SELECT`'s
        // transaction membership is unobservable.
        touch_save_project(&pool, &draft_project).await?;
        row.state_id = default_draft_state_opt(&pool, Some(draft_project)).await?;
    } else {
        // The group rides the field-cached instance on Django (no query);
        // a race miss 500s (the state validated live seconds ago).
        let state_id = row.state_id.ok_or(Denial::ServerError)?;
        let group = state_row(&pool, &state_id)
            .await?
            .map(|row| row.group)
            .ok_or(Denial::ServerError)?;
        if group == "completed" {
            row.completed_at = Some(utc_now_micros());
        } else {
            row.completed_at = None;
        }
    }
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory_lock_key(&draft_project))
        .execute(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?;
    let max_sequence: (Option<i64>,) = sqlx::query_as(
        "SELECT MAX(sequence) FROM issue_sequences WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(draft_project)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.sequence_id = i32::try_from(max_sequence.0.map(|max| max + 1).unwrap_or(1))
        .map_err(|_| Denial::ServerError)?;
    let max_sort: (Option<f64>,) = match row.state_id {
        Some(state_id) => sqlx::query_as(
            "SELECT MAX(sort_order) FROM issues WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL",
        )
        .bind(draft_project)
        .bind(state_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?,
        None => sqlx::query_as(
            "SELECT MAX(sort_order) FROM issues WHERE project_id = $1 AND state_id IS NULL AND deleted_at IS NULL",
        )
        .bind(draft_project)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?,
    };
    if let Some(largest) = max_sort.0 {
        row.sort_order = largest + 10000.0;
    }
    row.description_stripped = sync_description_stripped(Some(&row.description_html));
    insert_issue_row(&mut tx, &row).await?;
    sqlx::query(
        "INSERT INTO issue_sequences (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, sequence, deleted) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(Uuid::new_v4())
    .bind(utc_now_micros())
    .bind(utc_now_micros())
    .bind(Some(user_id))
    .bind(None::<Uuid>)
    .bind(draft_project)
    .bind(project.workspace_id)
    .bind(row.id)
    .bind(i64::from(row.sequence_id))
    .bind(false)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if is_integrity_violation(&error) {
            Denial::BadError("The payload is not valid".to_owned())
        } else {
            Denial::ServerError
        }
    })?;
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    // The m2m legs run post-commit in autocommit (each `bulk_create`
    // wrapped in its own `except IntegrityError: pass`).
    let assignees = filtered
        .assignee_ids
        .clone()
        .or(raw_assignees)
        .unwrap_or_default();
    if assignees.is_empty() {
        if let Some(default) = project.default_assignee_id {
            write_default_assignee(
                &pool,
                &default,
                &row.id,
                &draft_project,
                &project.workspace_id,
                row.created_by_id,
            )
            .await?;
        }
    } else {
        write_issue_m2m_batches(
            &pool,
            "issue_assignees",
            "assignee_id",
            &assignees,
            &row.id,
            &draft_project,
            &project.workspace_id,
            row.created_by_id,
            None,
        )
        .await?;
    }
    let labels = filtered
        .label_ids
        .clone()
        .or(raw_labels)
        .unwrap_or_default();
    if !labels.is_empty() {
        write_issue_m2m_batches(
            &pool,
            "issue_labels",
            "label_id",
            &labels,
            &row.id,
            &draft_project,
            &project.workspace_id,
            row.created_by_id,
            None,
        )
        .await?;
    }
    let mut seam = DraftSignalSeam { pool: &pool };
    let prev = capture_prior_state(&mut seam, None)
        .await
        .map_err(|_| Denial::ServerError)?;
    fire_after_save(
        &pool,
        row.id,
        prev,
        row.state_id,
        true,
        Some(user_id),
        utc_now_micros(),
    )
    .await?;
    // `base_host` raises past the save (the issue persists on a
    // misconfigured host), so the origin resolves here, not up front.
    let origin = request_origin(&state)?;
    let actor_text = user_id.to_string();
    let issue_text = row.id.to_string();
    let project_text = draft_project.to_string();
    let requested = python_dumps(&input.value);
    enqueue_activity_emit(
        &pool,
        &wtask::draft_issue_created_activity(
            requested.clone(),
            &actor_text,
            &issue_text,
            &project_text,
            Utc::now().timestamp(),
            &origin,
        ),
    )
    .await;
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
        if let Err(error) = sqlx::query(
            "INSERT INTO cycle_issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, cycle_id, issue_id) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8)",
        )
        .bind(link)
        .bind(link_created)
        .bind(link_updated)
        .bind(user_id)
        .bind(draft_workspace)
        .bind(draft_project)
        .bind(cycle_id)
        .bind(row.id)
        .execute(&pool)
        .await
        {
            if is_integrity_violation(&error) {
                return Ok(json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()));
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
        for chunk in module_ids.chunks(10) {
            let sql = m2m_insert_sql("module_issues", "module_id", chunk.len(), false);
            let mut query = sqlx::query(&sql);
            for member_id in chunk {
                let created = utc_now_micros();
                let updated = utc_now_micros();
                query = query
                    .bind(Uuid::new_v4())
                    .bind(created)
                    .bind(updated)
                    .bind(draft_created_by)
                    .bind(draft_updated_by)
                    .bind(draft_project)
                    .bind(draft_workspace)
                    .bind(row.id)
                    .bind(member_id);
            }
            if let Err(error) = query.execute(&pool).await {
                if is_integrity_violation(&error) {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        INVALID_PAYLOAD_BODY.to_owned(),
                    ));
                }
                return Err(Denial::ServerError);
            }
        }
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
    sqlx::query(
        "UPDATE file_assets SET issue_id = $1, entity_type = 'ISSUE_DESCRIPTION', draft_issue_id = NULL WHERE draft_issue_id = $2",
    )
    .bind(row.id)
    .bind(draft_pk)
    .execute(&pool)
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
    fn advisory_lock_key_vectors() {
        assert_eq!(
            advisory_lock_key(&uuid("11111111-1111-1111-1111-111111111111")),
            -4972562656765536426
        );
        assert_eq!(advisory_lock_key(&Uuid::nil()), 1349170572285598868);
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
}
