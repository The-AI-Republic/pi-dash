#![forbid(unsafe_code)]

//! Quick-link / sticky / home-pref / sidebar-pref / recent-visit /
//! user-properties handlers (D-24, stage 5, PIDASHCONV-623).
//!
//! Ports (same URL paths, same JSON byte for byte):
//!
//! - `QuickLinkViewSet` (`app/views/workspace/quick_link.py:16-65`):
//!   owner-scoped CRUD over `WorkspaceUserLinkSerializer`
//!   (`serializers/workspace.py:196-247`); create 201/400, patch 200 (+404
//!   `{"detail": "Quick link not found."}`), retrieve 200/404 (`{"error":
//!   "Quick link not found."}`), destroy 204, list 200.
//! - `WorkspaceStickyViewSet` (`ws/sticky.py:16-60`): owner-scoped CRUD +
//!   list (`query` icontains filter, `-sort_order`, paginated
//!   `default_per_page=20`); patch/destroy creator-only via the stock
//!   actions; retrieve is the undecorated `ModelViewSet` default
//!   (auth-only). Reuses `db::v1_assets::Sticky` (D-21, merged) and the
//!   SER-C sticky transcription (PIDASHCONV-602).
//! - `WorkspaceHomePreferenceViewSet` (`ws/home.py:17-79`): get
//!   (autocreate-missing then `.values(key,is_enabled,config,sort_order)`),
//!   patch by key (`{"detail": "Preference not found"}` 400).
//! - `WorkspaceUserPreferenceViewSet` (`ws/user_preference.py:18-101`):
//!   get (autocreate-missing then key→`{is_pinned,sort_order}` dict),
//!   patch (per-key upserts, unknown keys skipped, `{"message":
//!   "Successfully updated"}`).
//! - `UserRecentVisitViewSet` (`ws/recent_visit.py:17-36`) +
//!   `WorkspaceUserPropertiesEndpoint` (`ws/user.py:253-279`): visits list
//!   (entity filter + `["issue","page","project"]` clamp, `[:20]`);
//!   props get/patch (`get_or_create`).
//!
//! Routes (`app/urls/workspace.py:188-192,244-285`): W27 user-properties,
//! W38/W39 quick-links, W40/W41 home-preferences, W42 recent-visits,
//! W43/W44 stickies, W45 sidebar-preferences. Every other method on those
//! paths proxies to Django.
//!
//! Fixture ids: F-W24-15
//! (`rust-api/fixtures/app_workspace/handlers/routes.golden.json`); shapes
//! via `pidash_services::app_workspace::{ser_extras, ser_invite}`
//! (F-W24-02/03, PIDASHCONV-601/602), plans via `queries_extras`
//! (F-W24-12, PIDASHCONV-611), gates via `super::gates` (F-W24-13,
//! PIDASHCONV-613), defaults via `models_prefs`.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * Quick-link 404 key asymmetry: partial_update answers `{"detail":
//!   "Quick link not found."} (:43) while retrieve answers `{"error":
//!   "Quick link not found."}` (:51-52).
//! * Quick-link destroy has no try/except, so a missing row answers the
//!   generic `{"error": "The required object does not exist."}` 404
//!   (`views/base.py:132-136`), not a quick-link message.
//! * Sticky retrieve is the auth-only `ModelViewSet` default (no
//!   `@allow_permission`); only patch/destroy are creator-gated.
//! * Home PATCH without `<key>` and GET with `<key>` die with `TypeError`
//!   (500) after the gate passes (`home.py:24,68` take `slug` /
//!   `slug,key` respectively).
//! * Home PATCH missing answers 400 `{"detail": "Preference not
//!   found"}` (`home.py:79`): 400, not 404.
//! * Home autocreate re-inserts the whole growing list per missing key
//!   with `ignore_conflicts` (first insert wins); the response order is
//!   `-created_at`, not `sort_order`.
//! * Sidebar PATCH has no user filter (`user_preference.py:88`): a member
//!   can match and rewrite another user's row. It always answers 200
//!   `{"message": "Successfully updated"}`, silently skipping bad keys.
//! * Sidebar PATCH writes only `is_pinned`/`sort_order` (`update_fields`);
//!   `updated_at` is not stamped.
//! * `description_binary` input never validates (DRF maps `BinaryField`
//!   to read-only); the binary arm is dead through the serializer.
//! * Sticky create ignores input `sort_order` whenever the workspace has
//!   rows (`max + 10000` wins); an explicit `project` on quick-link
//!   create re-points `workspace` at the project's workspace
//!   (`WorkspaceBaseModel.save`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Datelike, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::workspace::WorkspaceFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_db::v1_assets::model::sticky as sticky_model;
use pidash_services::app_workspace::models_prefs::workspace_user_properties as user_props_model;
use pidash_services::app_workspace::models_prefs::{
    backfill_workspace_id, NavigationControlPreference,
};
use pidash_services::app_workspace::{queries_extras, ser_extras, ser_invite};
use pidash_types::v1_assets::sticky as sticky_kernel;
use pidash_types::WorkspaceId;

use super::gates;
use crate::app_issues::{query_last, Denial, QueryMap};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// W38 collection (`app/urls/workspace.py:244-248`).
pub const QUICK_LINKS_PATH: &str = "/api/workspaces/{slug}/quick-links/";
/// W39 detail (`:249-253`).
pub const QUICK_LINK_PATH: &str = "/api/workspaces/{slug}/quick-links/{pk}/";
/// W40 collection (`:255-259`).
pub const HOME_PREFS_PATH: &str = "/api/workspaces/{slug}/home-preferences/";
/// W41 keyed (`:260-264`).
pub const HOME_PREF_KEY_PATH: &str = "/api/workspaces/{slug}/home-preferences/{key}/";
/// W42 (`:265-269`).
pub const RECENT_VISITS_PATH: &str = "/api/workspaces/{slug}/recent-visits/";
/// W43 collection (`:270-274`).
pub const STICKIES_PATH: &str = "/api/workspaces/{slug}/stickies/";
/// W44 detail (`:275-279`).
pub const STICKY_PATH: &str = "/api/workspaces/{slug}/stickies/{pk}/";
/// W45 (`:281-285`).
pub const SIDEBAR_PREFS_PATH: &str = "/api/workspaces/{slug}/sidebar-preferences/";
/// W27 (`:188-192`).
pub const USER_PROPS_PATH: &str = "/api/workspaces/{slug}/user-properties/";

/// Owned methods per path (mirroring `urls/workspace.py`); every other
/// method proxies to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            QUICK_LINKS_PATH,
            axum::routing::get(quick_link_list)
                .post(quick_link_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            QUICK_LINK_PATH,
            axum::routing::get(quick_link_retrieve)
                .patch(quick_link_partial_update)
                .delete(quick_link_destroy)
                .put(crate::edge::proxy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            HOME_PREFS_PATH,
            axum::routing::get(home_prefs_get)
                .patch(home_prefs_patch_no_key)
                .put(crate::edge::proxy)
                .post(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            HOME_PREF_KEY_PATH,
            axum::routing::get(home_pref_get_with_key)
                .patch(home_pref_patch)
                .put(crate::edge::proxy)
                .post(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            RECENT_VISITS_PATH,
            axum::routing::get(recent_visits_list)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            STICKIES_PATH,
            axum::routing::get(sticky_list)
                .post(sticky_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            STICKY_PATH,
            axum::routing::get(sticky_retrieve)
                .patch(sticky_partial_update)
                .delete(sticky_destroy)
                .put(crate::edge::proxy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            SIDEBAR_PREFS_PATH,
            axum::routing::get(sidebar_prefs_get)
                .patch(sidebar_prefs_patch)
                .put(crate::edge::proxy)
                .post(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_PROPS_PATH,
            axum::routing::get(user_props_get)
                .patch(user_props_patch)
                .put(crate::edge::proxy)
                .post(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

fn gate_for(method: &str, path: &str) -> gates::Gate {
    gates::gate_for(method, path)
        .unwrap_or_else(|| panic!("D-24 gate for {method} {path}"))
        .gate
}

/// The role list one allow-gate checks: `Workspace`/`WorkspaceCreator`
/// check their own roles; class/auth rows check none here.
fn gate_roles(gate: &gates::Gate) -> &[i32] {
    match gate {
        gates::Gate::Workspace { roles } | gates::Gate::WorkspaceCreator { roles, .. } => roles,
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// Shared request plumbing (pilot-2 / D-27 precedent)
// ---------------------------------------------------------------------------

type HandlerResult = Result<Response, Denial>;

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(Vec::new()))
        .expect("empty response")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// `request.user` from the Django session: missing session, missing key,
/// or a non-UUID id is anonymous → 401.
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

fn pool_of(state: &AppState) -> Result<PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Badly-formed UUIDs in paths render the `ValidationError` branch
/// (`app/views/base.py:126-130`): 400 `{"error": "Please provide valid
/// detail"}`.
const INVALID_DETAIL_MSG: &str = "Please provide valid detail";

fn parse_uuid_or_invalid(raw: &str) -> Result<Uuid, Denial> {
    raw.parse::<Uuid>()
        .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))
}

/// Membership facts for one `(user, slug)` over the same rows the
/// decorator reads (`app/permissions/base.py:19-86`): the active-row
/// filters and the `workspace__slug=` scoping are this SQL; the scope
/// check denies facts fetched for another tenant. `is_creator` is the
/// tagged creator lookup (sticky rows only); every other row passes
/// `false`.
async fn fetch_allow_facts(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    allowed: &[i32],
    is_creator: bool,
) -> Result<AllowFacts, Denial> {
    let workspace_role: Option<(i16,)> = sqlx::query_as(
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
    Ok(AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role
            .map(|(role,)| allowed.contains(&i32::from(role)))
            .unwrap_or(false),
        is_creator,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: workspace_role
            .map(|(role,)| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
    })
}

/// Facts for the `WorkspaceViewerPermission` rows (user-properties):
/// any active membership passes every method.
async fn fetch_viewer_facts(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
) -> Result<WorkspaceFacts, Denial> {
    let workspace_role: Option<(i16,)> = sqlx::query_as(
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
    let role = workspace_role.map(|(role,)| i32::from(role));
    Ok(WorkspaceFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        has_admin_or_member_role: role
            .is_some_and(|role| role == ROLE_ADMIN || role == ROLE_MEMBER),
        has_admin_role: role.is_some_and(|role| role == ROLE_ADMIN),
        is_member: role.is_some(),
        is_admin_unfiltered: false,
    })
}

/// Run one `super::gates` decorator/default row: `Allow` runs the body,
/// `Deny` answers the decorator 403, anonymous already 401'd.
fn check_gate(gate: &gates::Gate, slug: &str, facts: &AllowFacts) -> Result<(), Denial> {
    match gates::decide_gate(gate, &gates::tenant_context(slug), facts) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(Denial::Forbidden),
        gates::GateOutcome::DenyClass => Err(Denial::Forbidden),
        gates::GateOutcome::MissingSlug => Err(Denial::ServerError),
        gates::GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

/// Run the `WorkspaceViewerPermission` row: `Allow` runs the body, the
/// class denial answers the DRF-default 403 body
/// ([`gates::CLASS_DENIED_BODY`]).
enum ViewerGate {
    Allow,
    DenyClass,
    Unauthorized,
}

fn check_viewer_gate(slug: &str, facts: &WorkspaceFacts) -> ViewerGate {
    match gates::decide_class_viewer(&gates::tenant_context(slug), facts) {
        gates::GateOutcome::Allow => ViewerGate::Allow,
        gates::GateOutcome::DenyClass => ViewerGate::DenyClass,
        _ => ViewerGate::Unauthorized,
    }
}

/// `request.user.user_timezone` (`TimezoneMixin`): unknown zones 500
/// through the same branch Django's `zoneinfo` activation raises into.
async fn actor_timezone(pool: &PgPool, user_id: &Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT u.user_timezone FROM users u WHERE u.id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// `Workspace.objects.get(slug=slug)`: a miss (including a soft-deleted
/// row) answers the `ObjectDoesNotExist` 404.
async fn resolve_workspace_id(pool: &PgPool, slug: &str) -> Result<Uuid, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFound)
}

/// DRF datetime rendering through the request zone
/// (`TimezoneMixin` + `user_timezone_converter`). Shifts past year
/// 9999/0001 are `OverflowError` → 500, like `enforce_timezone` on read.
fn render_dt(value: &DateTime<Utc>, timezone: &Tz) -> Result<String, Denial> {
    let shifted = value.with_timezone(timezone);
    if shifted.date_naive().year() > 9999 || shifted.date_naive().year() < 1 {
        return Err(Denial::ServerError);
    }
    Ok(crate::serializer::render_datetime_in(value, timezone))
}

/// Parse the request body the way DRF does for JSON writes: empty → `{}`;
/// malformed → `ParseError` 400; non-object JSON → the attribute errors
/// the view/serializer code hits (500 envelope).
fn parse_body_object(raw: &[u8]) -> Result<Map<String, Value>, Denial> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Denial::ServerError),
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}"))),
    }
}

/// Parse the request body for the sidebar PATCH: empty → `{}` (which
/// iterates zero times → 200); malformed → `ParseError` 400.
fn parse_sidebar_body(raw: &[u8]) -> Result<Value, Denial> {
    if raw.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice::<Value>(raw)
        .map_err(|error| Denial::BadDetail(format!("JSON parse error - {error}")))
}

fn page_denial(error: crate::paginator::PageError) -> Denial {
    use crate::paginator::PageError as E;
    match error {
        E::InvalidPerPage
        | E::PerPageTooLarge(_)
        | E::InvalidCursor
        | E::OffsetTooLarge
        | E::NegativeOffset => Denial::BadDetail(error.detail()),
        E::ZeroLimit | E::NegativeSlice | E::NonFiniteCursor | E::MissingOrderKey => {
            Denial::ServerError
        }
    }
}

/// Envelope for the 12-key `BasePaginator.paginate` body
/// (`utils/paginator.py:714-731`), keys in order via the kernel.
fn envelope(
    total_count: i64,
    per_page: i64,
    next: &crate::paginator::Cursor,
    prev: &crate::paginator::Cursor,
    results: Value,
) -> Result<Response, Denial> {
    use crate::paginator::{max_hits, PageResponse};
    // `max_hits = ceil(count / limit)` with the request `per_page`
    // (`utils/paginator.py:183`); `0` divides by zero into the 500.
    let total_pages = max_hits(total_count, per_page).map_err(page_denial)?;
    let page = PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count,
        next_cursor: next.to_string(),
        prev_cursor: prev.to_string(),
        next_page_results: next.has_results_or_false(),
        prev_page_results: prev.has_results_or_false(),
        count: results.as_array().map(|a| a.len()).unwrap_or(0),
        total_pages,
        total_results: total_count,
        extra_stats: None,
        results,
    };
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&page.to_json_value()).expect("page response"),
    ))
}

/// Soft-delete fan-out (`db/mixins.py:72-78`): instance `delete()` stamps
/// `deleted_at` + `save()` (so `updated_at`/`updated_by` move) and posts
/// `soft_delete_related_objects.delay("db", <model>, <pk>, using=None)`.
/// Best-effort post-commit enqueue — without it the response still stands.
async fn enqueue_soft_delete(pool: &PgPool, model: &str, pk: &Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
            Value::Null,
        ],
        Default::default(),
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, "soft-delete sweep enqueue failed; response stands");
    }
}

/// A write that dies in the database: unique/FK violations are Django's
/// `IntegrityError` → 400 `{"error": "The payload is not valid"}`;
/// anything else is the generic 500.
fn map_write_error(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db) = &error {
        if let Some(code) = db.code() {
            if code.as_ref() == "23505" || code.as_ref() == "23503" {
                return Denial::BadError("The payload is not valid".to_owned());
            }
        }
    }
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// DRF field ladders (shared by the write paths)
// ---------------------------------------------------------------------------

/// One field's error value, in writable-field order. Values are
/// message lists, except field-validator dicts (quick-link `url`), which
/// nest as objects.
type FieldErrors = Vec<(String, Value)>;

/// `serializer.errors` rendering: fields in declaration order
/// (`{"f":["m"],"g":{...}}`).
fn render_field_errors(errors: &FieldErrors) -> String {
    let mut out = String::from("{");
    for (index, (field, value)) in errors.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(field));
        out.push(':');
        out.push_str(&value.to_string());
    }
    out.push('}');
    out
}

fn push_field_msg(errors: &mut FieldErrors, field: &str, message: String) {
    errors.push((field.to_owned(), Value::Array(vec![Value::String(message)])));
}

const REQUIRED_MSG: &str = "This field is required.";
const NULL_MSG: &str = "This field may not be null.";
const BLANK_MSG: &str = "This field may not be blank.";
const INVALID_STR_MSG: &str = "Not a valid string.";
const BOOL_MSG: &str = "Must be a valid boolean.";
const INT_MSG: &str = "A valid integer is required.";
const FLOAT_MSG: &str = "A valid number is required.";
const TOO_LARGE_MSG: &str = "String value too large.";
const INT_TO_FLOAT_MSG: &str = "Integer value too large to convert to float";
const DATETIME_HINT: &str = "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]";

fn max_length_msg(max: usize) -> String {
    format!("Ensure this field has no more than {max} characters.")
}

fn datetime_invalid_msg() -> String {
    format!("Datetime has wrong format. Use one of these formats instead: {DATETIME_HINT}.")
}

/// Python truthiness over a JSON input value (CPython rules:
/// `None`/`False`/`0`/`""`/`[]`/`{}` are falsy, everything else truthy).
fn is_python_truthy(value: &Value) -> bool {
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

/// A JSON number split by syntax: plain integers keep their digits
/// (arbitrary precision survives the parse), float syntax goes through
/// `repr` — including overflowing exponents (`1e1000` → `inf`, which
/// Python accepts).
enum JsonNum {
    Int(String),
    Float(f64),
}

fn split_json_number(number: &serde_json::Number) -> JsonNum {
    if let Some(int) = number.as_i64() {
        return JsonNum::Int(int.to_string());
    }
    if let Some(uint) = number.as_u64() {
        return JsonNum::Int(uint.to_string());
    }
    let text = number.to_string();
    if text.chars().any(|c| c == '.' || c == 'e' || c == 'E') {
        // Float syntax from parsed JSON always parses (`NaN` fallback is
        // unreachable).
        JsonNum::Float(text.parse::<f64>().unwrap_or(f64::NAN))
    } else {
        JsonNum::Int(text)
    }
}

/// `CharField.to_internal_value` (DRF 3.15.2 `fields.py`): bools and
/// non-strings-that-aren't-numbers fail; numbers stringify; the result is
/// whitespace-trimmed. The blank check runs before this
/// (`validate_empty_values`), so `""` never reaches here.
fn drf_char_to_internal(value: &Value) -> Result<String, ()> {
    match value {
        Value::String(text) => Ok(text.trim().to_owned()),
        Value::Number(number) => match split_json_number(number) {
            JsonNum::Int(digits) => Ok(digits),
            JsonNum::Float(float) => Ok(crate::paginator::py_float_str(float)),
        },
        _ => Err(()),
    }
}

/// `BooleanField.to_internal_value`: case-insensitive string match plus
/// numeric `== 1` / `== 0` (so `1.0` is true and `0.0` is false, exactly
/// like the `in TRUE_VALUES` / `in FALSE_VALUES` tests). `None` never
/// reaches here (the null check runs first; no bool field here allows
/// null).
fn drf_bool_to_internal(value: &Value) -> Result<bool, ()> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                match int {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(()),
                }
            } else if let Some(uint) = number.as_u64() {
                match uint {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(()),
                }
            } else if let Some(float) = number.as_f64() {
                if float == 1.0 {
                    Ok(true)
                } else if float == 0.0 {
                    Ok(false)
                } else {
                    Err(())
                }
            } else {
                Err(())
            }
        }
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            match lower.as_str() {
                "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
                "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
                _ => Err(()),
            }
        }
        _ => Err(()),
    }
}

/// Strict Python-`int()` shape check over ASCII digits (sign, surrounding
/// whitespace, single underscores between digits). Unicode decimal digits
/// (which CPython also accepts) are not covered.
fn python_int_digits(text: &str) -> Option<String> {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace());
    let (_, rest) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    if rest.is_empty() {
        return None;
    }
    let mut digits = String::with_capacity(rest.len());
    let mut prev_underscore = true;
    for ch in rest.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            digits.push(ch);
            prev_underscore = false;
        } else {
            return None;
        }
    }
    if prev_underscore || digits.is_empty() {
        return None;
    }
    Some(digits)
}

/// `IntegerField.to_internal_value`: the 1000-char guard, the `\.0*\s*$`
/// strip (so `"1.0"` is 1 but `"1.2"` is invalid), then `int()`. Huge
/// magnitudes validate (Python ints are unbounded) and die at the
/// database instead — `TooBig` carries that branch.
enum IntCheck {
    Value(i128),
    TooBig,
    Invalid,
    TooLarge,
}

fn drf_int_to_internal(value: &Value) -> IntCheck {
    if matches!(value, Value::String(text) if text.len() > 1000) {
        return IntCheck::TooLarge;
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => match split_json_number(number) {
            JsonNum::Int(digits) => digits,
            JsonNum::Float(float) => crate::paginator::py_float_str(float),
        },
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        _ => return IntCheck::Invalid,
    };
    // `re_decimal = re.compile(r'\.0*\s*$')`.
    let stripped = match text.find('.') {
        Some(dot)
            if text[dot + 1..]
                .chars()
                .all(|c| c == '0' || c.is_whitespace()) =>
        {
            text[..dot].to_owned()
        }
        _ => text,
    };
    // Keep the sign for the magnitude parse.
    let trimmed = stripped.trim_matches(|c: char| c.is_whitespace());
    let (negative, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let Some(digits) = python_int_digits(rest) else {
        return IntCheck::Invalid;
    };
    let signed = if negative {
        format!("-{digits}")
    } else {
        digits
    };
    match signed.parse::<i128>() {
        Ok(int) => IntCheck::Value(int),
        Err(_) => IntCheck::TooBig,
    }
}

/// Strict Python-`float()` string grammar (ASCII): surrounding
/// whitespace, caseless `inf`/`infinity`/`nan`, single underscores
/// between digits, optional fraction and exponent.
fn python_float_from_str(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => return Some(f64::INFINITY),
        "-inf" | "-infinity" => return Some(f64::NEG_INFINITY),
        "nan" | "+nan" | "-nan" => return Some(f64::NAN),
        _ => {}
    }
    // Validate the numeric grammar (underscores single and between
    // digits), then delegate the value parse to Rust (same IEEE-754
    // result, including `inf` for overflowing exponents).
    let body = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if body.is_empty() {
        return None;
    }
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(pos) => (&body[..pos], Some(&body[pos + 1..])),
        None => (body, None),
    };
    if let Some(exp) = exponent {
        let exp = exp.strip_prefix(['+', '-']).unwrap_or(exp);
        if !is_strict_digit_run(exp) {
            return None;
        }
    }
    if mantissa.is_empty() {
        return None;
    }
    match mantissa.find('.') {
        None => {
            if !is_strict_digit_run(mantissa) {
                return None;
            }
        }
        Some(dot) => {
            let (int_part, frac_part) = (&mantissa[..dot], &mantissa[dot + 1..]);
            if frac_part.contains('.') {
                return None;
            }
            if int_part.is_empty() && frac_part.is_empty() {
                return None;
            }
            if !int_part.is_empty() && !is_strict_digit_run(int_part) {
                return None;
            }
            if !frac_part.is_empty() && !is_strict_digit_run(frac_part) {
                return None;
            }
        }
    }
    let cleaned: String = trimmed.chars().filter(|c| *c != '_').collect();
    cleaned.parse::<f64>().ok()
}

/// Non-empty ASCII digits with single underscores between digits.
fn is_strict_digit_run(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut prev_underscore = true;
    for ch in text.chars() {
        if ch == '_' {
            if prev_underscore {
                return false;
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            prev_underscore = false;
        } else {
            return false;
        }
    }
    !prev_underscore
}

/// `FloatField.to_internal_value`: the 1000-char guard, then `float()`.
/// An integer literal past `u64` is the `overflow` branch (`float(huge)`
/// raises `OverflowError`, while an overflowing exponent string parses to
/// `inf` and stays valid).
enum FloatCheck {
    Value(f64),
    Invalid,
    TooLarge,
    Overflow,
}

fn drf_float_to_internal(value: &Value) -> FloatCheck {
    if matches!(value, Value::String(text) if text.len() > 1000) {
        return FloatCheck::TooLarge;
    }
    match value {
        Value::Bool(true) => FloatCheck::Value(1.0),
        Value::Bool(false) => FloatCheck::Value(0.0),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                FloatCheck::Value(int as f64)
            } else if let Some(uint) = number.as_u64() {
                FloatCheck::Value(uint as f64)
            } else {
                match split_json_number(number) {
                    // Beyond int range: integer syntax overflows
                    // (`float(huge)` raises), float syntax parses
                    // (possibly to `inf`, accepted).
                    JsonNum::Int(_) => FloatCheck::Overflow,
                    JsonNum::Float(float) => FloatCheck::Value(float),
                }
            }
        }
        Value::String(text) => match python_float_from_str(text) {
            Some(float) => FloatCheck::Value(float),
            None => FloatCheck::Invalid,
        },
        _ => FloatCheck::Invalid,
    }
}

/// Raw `float()` for the sidebar PATCH (no serializer, no length guard):
/// `None`/lists/dicts are `TypeError`, bad strings are `ValueError` —
/// both 500; huge ints are `OverflowError` — also 500.
fn raw_float_to_f64(value: &Value) -> Result<f64, ()> {
    match value {
        Value::Bool(true) => Ok(1.0),
        Value::Bool(false) => Ok(0.0),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(int as f64)
            } else if let Some(uint) = number.as_u64() {
                Ok(uint as f64)
            } else {
                match split_json_number(number) {
                    JsonNum::Int(_) => Err(()),
                    JsonNum::Float(float) => Ok(float),
                }
            }
        }
        Value::String(text) => python_float_from_str(text).ok_or(()),
        _ => Err(()),
    }
}

/// `BooleanField.to_python` (`django/db/models/fields/__init__.py:1102`)
/// for the non-nullable sidebar `is_pinned`: `True`/`False`/`1`/`0`
/// (numeric equality, so `1.0`/`0.0` too) and the exact strings
/// `t`/`True`/`1` / `f`/`False`/`0`; everything else is
/// `ValidationError` → 400 valid-detail. `None` never reaches here (the
/// prep returns `None` first → `IntegrityError` → 400 payload-not-valid).
fn raw_bool_to_python(value: &Value) -> Result<bool, ()> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                match int {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(()),
                }
            } else if let Some(uint) = number.as_u64() {
                match uint {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(()),
                }
            } else if let Some(float) = number.as_f64() {
                if float == 1.0 {
                    Ok(true)
                } else if float == 0.0 {
                    Ok(false)
                } else {
                    Err(())
                }
            } else {
                Err(())
            }
        }
        Value::String(text) => match text.as_str() {
            "t" | "True" | "1" => Ok(true),
            "f" | "False" | "0" => Ok(false),
            _ => Err(()),
        },
        _ => Err(()),
    }
}

// ---------------------------------------------------------------------------
// `parse_datetime` port (probed live against CPython 3.12 + Django 4.2)
// ---------------------------------------------------------------------------

/// A parsed wall time plus an optional fixed UTC offset in seconds.
struct ParsedDt {
    naive: chrono::NaiveDateTime,
    offset_secs: Option<i32>,
}

/// `django.utils.dateparse.parse_datetime`: `fromisoformat` first, then
/// the anchored fallback regex. `None` is "not well formatted" (DRF
/// answers the invalid-detail 400); range failures also surface as
/// `None` here (DRF suppresses the `ValueError` into the same body).
fn parse_drf_datetime(text: &str) -> Option<ParsedDt> {
    if text.is_empty() {
        return None;
    }
    if let Some(parsed) = parse_isoformat(text) {
        return Some(parsed);
    }
    parse_fallback_datetime(text)
}

/// CPython 3.12 `datetime.fromisoformat` over the probed grammar:
/// extended/basic calendar dates, week dates (extended + basic), any
/// single non-digit date/time separator, basic/extended times, `,`/`.`
/// fractions (truncated to 6 digits), `Z`/offset suffixes (hour-only,
/// `HHMM`, `HH:MM`, `HHMMSS`, `HH:MM:SS`). No leading whitespace; one
/// trailing space before the offset is allowed.
fn parse_isoformat(text: &str) -> Option<ParsedDt> {
    if !text.is_ascii() || text.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let bytes = text.as_bytes();
    // Date part: `YYYY-MM-DD`, `YYYYMMDD`, `YYYY-Www-d`, `YYYYWwwd`.
    let (year, month, day, rest) = parse_iso_date(bytes)?;
    if year < 1 {
        // `datetime` rejects year 0 where chrono would accept it.
        return None;
    }
    if rest.is_empty() {
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some(ParsedDt {
            naive: date.and_hms_opt(0, 0, 0)?,
            offset_secs: None,
        });
    }
    // Exactly one separator character, and it must not be a digit.
    let sep = rest.chars().next()?;
    if sep.is_ascii_digit() {
        return None;
    }
    let after_sep = &rest[sep.len_utf8()..];
    if after_sep.is_empty() {
        return None;
    }
    // A second separator-shaped char means a multi-char separator.
    let (time_part, tz_part) = split_iso_tz(after_sep)?;
    let (hour, minute, second, micro) = parse_iso_time(time_part)?;
    let naive = chrono::NaiveDate::from_ymd_opt(year, month, day)?
        .and_hms_micro_opt(hour, minute, second, micro)?;
    let offset_secs = match tz_part {
        None => None,
        Some(tz) => Some(parse_iso_offset(tz)?),
    };
    Some(ParsedDt { naive, offset_secs })
}

/// Split a time+offset tail at the `Z`/offset suffix, allowing whitespace
/// between the time and the suffix. Returns the time part and the raw
/// suffix (if any).
fn split_iso_tz(tail: &str) -> Option<(&str, Option<&str>)> {
    if let Some(stripped) = tail.strip_suffix('Z') {
        if stripped.is_empty() {
            return None;
        }
        return Some((stripped.trim_end(), Some("Z")));
    }
    // An offset starts at the last `+`/`-` past the time's own colons:
    // scan for a trailing `[+-]\d` run.
    let bytes = tail.as_bytes();
    let mut split = None;
    for (index, &byte) in bytes.iter().enumerate() {
        if (byte == b'+' || byte == b'-')
            && tail[index + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            split = Some(index);
        }
    }
    match split {
        None => Some((tail, None)),
        Some(pos) => {
            let (time, tz) = (&tail[..pos], &tail[pos..]);
            if time.is_empty() {
                return None;
            }
            Some((time.trim_end(), Some(tz)))
        }
    }
}

/// ISO weekday number (1 = Monday) to chrono's weekday.
fn iso_weekday(weekday: u32) -> chrono::Weekday {
    use chrono::Weekday;
    match weekday {
        1 => Weekday::Mon,
        2 => Weekday::Tue,
        3 => Weekday::Wed,
        4 => Weekday::Thu,
        5 => Weekday::Fri,
        6 => Weekday::Sat,
        _ => Weekday::Sun,
    }
}

/// Strict calendar/week-date head. Returns the resolved
/// year/month/day plus the unconsumed tail.
fn parse_iso_date(bytes: &[u8]) -> Option<(i32, u32, u32, &str)> {
    let text = std::str::from_utf8(bytes).ok()?;
    // Week dates first (`YYYY-Www-d`, `YYYYWwwd`).
    if bytes.len() >= 8 && bytes[4] == b'W' {
        // Basic week date `YYYYWwwd`.
        let year: i32 = text[0..4].parse().ok()?;
        let week: u32 = text[5..7].parse().ok()?;
        let weekday: u32 = text[7..8].parse().ok()?;
        if !(1..=53).contains(&week) || !(1..=7).contains(&weekday) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday))?;
        return Some((date.year(), date.month(), date.day(), &text[8..]));
    }
    if bytes.len() >= 10 && bytes[4] == b'-' && bytes[5] == b'W' {
        // Extended week date `YYYY-Www-d`.
        let year: i32 = text[0..4].parse().ok()?;
        if !text[0..4].chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let week: u32 = text[6..8].parse().ok()?;
        if text.len() < 10 || text.as_bytes()[8] != b'-' {
            // `YYYY-Www` without a day: fromisoformat requires the day.
            return None;
        }
        let weekday: u32 = text[9..10].parse().ok()?;
        if !(1..=53).contains(&week) || !(1..=7).contains(&weekday) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday))?;
        return Some((date.year(), date.month(), date.day(), &text[10..]));
    }
    if bytes.len() >= 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        // Extended calendar date (strictly two digits per part here; the
        // fallback covers one-digit parts).
        let (year, month, day) = (
            text[0..4].parse::<i32>().ok()?,
            text[5..7].parse::<u32>().ok()?,
            text[8..10].parse::<u32>().ok()?,
        );
        if !text[0..4].chars().all(|c| c.is_ascii_digit())
            || !text[5..7].chars().all(|c| c.is_ascii_digit())
            || !text[8..10].chars().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        if month == 0 || month > 12 || day == 0 || day > 31 {
            return None;
        }
        return Some((year, month, day, &text[10..]));
    }
    if bytes.len() >= 8 && text[0..8].chars().all(|c| c.is_ascii_digit()) {
        // Basic calendar date.
        let (year, month, day) = (
            text[0..4].parse::<i32>().ok()?,
            text[4..6].parse::<u32>().ok()?,
            text[6..8].parse::<u32>().ok()?,
        );
        if month == 0 || month > 12 || day == 0 || day > 31 {
            return None;
        }
        return Some((year, month, day, &text[8..]));
    }
    None
}

/// `HH[:MM[:SS[.ffffff]]]` / `HHMM[SS[.ffffff]]` / `HH`, with `,` or `.`
/// fractions truncated (never rounded) to 6 digits.
fn parse_iso_time(text: &str) -> Option<(u32, u32, u32, u32)> {
    if text.is_empty() {
        return None;
    }
    // Split the fraction first.
    let (head, frac) = match text.find(['.', ',']) {
        Some(pos) => (&text[..pos], Some(&text[pos + 1..])),
        None => (text, None),
    };
    let micro = match frac {
        None => 0,
        Some(digits) => {
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let mut six = digits.to_owned();
            six.truncate(6);
            while six.len() < 6 {
                six.push('0');
            }
            six.parse::<u32>().ok()?
        }
    };
    if head.contains(':') {
        let mut parts = head.split(':');
        let hour: u32 = parts.next()?.parse().ok()?;
        let minute: u32 = parts.next()?.parse().ok()?;
        let second: u32 = match parts.next() {
            Some(part) => part.parse().ok()?,
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        if head
            .split(':')
            .any(|part| part.len() != 2 || !part.chars().all(|c| c.is_ascii_digit()))
        {
            return None;
        }
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        Some((hour, minute, second, micro))
    } else {
        if !head.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        match head.len() {
            2 => {
                let hour: u32 = head.parse().ok()?;
                if hour > 23 {
                    return None;
                }
                Some((hour, 0, 0, micro))
            }
            4 => {
                let (hour, minute) = (
                    head[0..2].parse::<u32>().ok()?,
                    head[2..4].parse::<u32>().ok()?,
                );
                if hour > 23 || minute > 59 {
                    return None;
                }
                Some((hour, minute, 0, micro))
            }
            6 => {
                let (hour, minute, second) = (
                    head[0..2].parse::<u32>().ok()?,
                    head[2..4].parse::<u32>().ok()?,
                    head[4..6].parse::<u32>().ok()?,
                );
                if hour > 23 || minute > 59 || second > 59 {
                    return None;
                }
                Some((hour, minute, second, micro))
            }
            _ => None,
        }
    }
}

/// `Z` or `±HH[[:]MM[[:]SS]]` / `±HHMM[SS]` / `±HH`, strictly under 24
/// hours (else `ValueError` → invalid).
fn parse_iso_offset(text: &str) -> Option<i32> {
    if text == "Z" {
        return Some(0);
    }
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Colons, when present, must sit on the `HH:MM[:SS]` joints.
    if rest.contains(':') {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() > 3 || parts.iter().any(|part| part.len() != 2) {
            return None;
        }
    } else if !matches!(digits.len(), 2 | 4 | 6) {
        return None;
    }
    let hours: i32 = digits[0..2].parse().ok()?;
    let minutes: i32 = if digits.len() >= 4 {
        digits[2..4].parse().ok()?
    } else {
        0
    };
    let seconds: i32 = if digits.len() == 6 {
        digits[4..6].parse().ok()?
    } else {
        0
    };
    let total = hours * 3600 + minutes * 60 + seconds;
    if total >= 24 * 3600 {
        return None;
    }
    Some(if negative { -total } else { total })
}

/// The anchored fallback regex
/// (`\d{4}-\d{1,2}-\d{1,2}[T ]\d{1,2}:\d{1,2}(?::\d{1,2}(?:[.,]\d{1,6}\d{0,6})?)?\s*(Z|[+-]\d{2}(?::?\d{2})?)?$`).
/// Only reached when `fromisoformat` fails, so in practice this covers
/// one-digit parts.
fn parse_fallback_datetime(text: &str) -> Option<ParsedDt> {
    if !text.is_ascii() {
        return None;
    }
    let bytes = text.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    if !text[0..4].chars().all(|c| c.is_ascii_digit()) || year < 1 {
        return None;
    }
    let month_end = text[5..].find('-')? + 5;
    let month: u32 = text[5..month_end].parse().ok()?;
    if month == 0 || month > 12 || month_end - 5 > 2 {
        return None;
    }
    let day_start = month_end + 1;
    let day_end = text[day_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(text.len(), |pos| pos + day_start);
    if day_end - day_start == 0 || day_end - day_start > 2 {
        return None;
    }
    let day: u32 = text[day_start..day_end].parse().ok()?;
    if day == 0 || day > 31 {
        return None;
    }
    let sep = text[day_end..].chars().next()?;
    if sep != 'T' && sep != ' ' {
        return None;
    }
    let time = &text[day_end + 1..];
    let (hour, minute, second, micro, rest) = parse_fallback_time(time)?;
    let rest = rest.trim_start();
    let offset_secs = if rest.is_empty() {
        None
    } else if rest == "Z" {
        Some(0)
    } else {
        Some(parse_fallback_offset(rest)?)
    };
    let naive = chrono::NaiveDate::from_ymd_opt(year, month, day)?
        .and_hms_micro_opt(hour, minute, second, micro)?;
    Some(ParsedDt { naive, offset_secs })
}

fn parse_fallback_time(text: &str) -> Option<(u32, u32, u32, u32, &str)> {
    let hour_end = text.find(':')?;
    if hour_end == 0 || hour_end > 2 {
        return None;
    }
    let hour: u32 = text[..hour_end].parse().ok()?;
    let after_hour = &text[hour_end + 1..];
    let minute_end = after_hour
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after_hour.len());
    if minute_end == 0 || minute_end > 2 {
        return None;
    }
    let minute: u32 = after_hour[..minute_end].parse().ok()?;
    let mut rest = &after_hour[minute_end..];
    let mut second = 0u32;
    let mut micro = 0u32;
    if let Some(tail) = rest.strip_prefix(':') {
        let second_end = tail
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(tail.len());
        if second_end == 0 || second_end > 2 {
            return None;
        }
        second = tail[..second_end].parse().ok()?;
        rest = &tail[second_end..];
        if let Some(frac) = rest.strip_prefix(['.', ',']) {
            let frac_end = frac
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(frac.len());
            if frac_end == 0 || frac_end > 12 {
                return None;
            }
            // `\d{1,6}\d{0,6}`: up to 12 digits, first 6 kept.
            let mut six = frac[..frac_end.min(6)].to_owned();
            while six.len() < 6 {
                six.push('0');
            }
            micro = six.parse::<u32>().ok()?;
            rest = &frac[frac_end..];
        }
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second, micro, rest))
}

fn parse_fallback_offset(text: &str) -> Option<i32> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if rest.len() < 2 || !rest[0..2].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = rest[0..2].parse().ok()?;
    let tail = &rest[2..];
    let minutes: i32 = if tail.is_empty() {
        0
    } else {
        let tail = tail.strip_prefix(':').unwrap_or(tail);
        if tail.len() != 2 || !tail.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        tail.parse::<i32>().ok()?
    };
    let total = hours * 3600 + minutes * 60;
    if total >= 24 * 3600 {
        return None;
    }
    Some(if negative { -total } else { total })
}

/// `DateTimeField.to_internal_value` + `enforce_timezone` (`USE_TZ`,
/// current zone = the actor's): naive values attach the actor zone
/// (gaps fail `make_aware`, folds take the first side); aware values
/// keep their instant. Shifts past year 9999/0001 are the `overflow`
/// branch.
#[derive(Debug)]
enum DtCheck {
    Value(DateTime<Utc>),
    Invalid,
    MakeAware,
    Overflow,
}

fn drf_datetime_to_internal(value: &Value, timezone: &Tz) -> DtCheck {
    let Value::String(text) = value else {
        return DtCheck::Invalid;
    };
    let Some(parsed) = parse_drf_datetime(text) else {
        return DtCheck::Invalid;
    };
    match parsed.offset_secs {
        Some(offset) => {
            let instant = parsed.naive.and_utc() - chrono::Duration::seconds(i64::from(offset));
            // `astimezone(actor zone)` must stay representable.
            let shifted = instant.with_timezone(timezone);
            if shifted.date_naive().year() > 9999 || shifted.date_naive().year() < 1 {
                return DtCheck::Overflow;
            }
            DtCheck::Value(instant)
        }
        None => {
            use chrono::{LocalResult, TimeZone};
            match timezone.from_local_datetime(&parsed.naive) {
                LocalResult::Single(aware) => DtCheck::Value(aware.with_timezone(&Utc)),
                LocalResult::Ambiguous(first, _) => DtCheck::Value(first.with_timezone(&Utc)),
                LocalResult::None => DtCheck::MakeAware,
            }
        }
    }
}

fn make_aware_msg(timezone: &Tz) -> String {
    format!("Invalid datetime for the timezone \"{timezone}\".")
}

// ---------------------------------------------------------------------------
// PK + choice ladders
// ---------------------------------------------------------------------------

/// `PrimaryKeyRelatedField.to_internal_value` shape (`relations.py`):
/// bools are `incorrect_type`; strings must be UUIDs (malformed ones
/// raise Django `ValidationError` → the whole-request 400, not a field
/// error); `0` dies in the database (500); other ints convert via
/// `uuid.UUID(int=…)` (out of range → `incorrect_type`); floats and
/// containers raise `AttributeError` (500).
enum PkCheck {
    Uuid(Uuid),
    Blank,
    IncorrectType(String),
    PayloadNotValid,
    ServerError,
}

fn check_pk_shape(value: &Value) -> PkCheck {
    match value {
        Value::Null => PkCheck::Blank,
        Value::Bool(_) => PkCheck::IncorrectType("bool".to_owned()),
        Value::String(text) => {
            if text.is_empty() {
                return PkCheck::Blank;
            }
            match text.parse::<Uuid>() {
                Ok(id) => PkCheck::Uuid(id),
                Err(_) => PkCheck::PayloadNotValid,
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int == 0 {
                    return PkCheck::ServerError;
                }
                if int < 0 {
                    return PkCheck::IncorrectType("int".to_owned());
                }
                PkCheck::Uuid(Uuid::from_u128(int as u128))
            } else if let Some(uint) = number.as_u64() {
                if uint == 0 {
                    return PkCheck::ServerError;
                }
                PkCheck::Uuid(Uuid::from_u128(u128::from(uint)))
            } else if number.is_f64() {
                PkCheck::ServerError
            } else {
                PkCheck::IncorrectType("int".to_owned())
            }
        }
        Value::Array(_) => PkCheck::ServerError,
        Value::Object(_) => PkCheck::ServerError,
    }
}

fn pk_does_not_exist_msg(pk: &Value) -> String {
    let rendered = pk_json_scalar(pk);
    format!("Invalid pk \"{rendered}\" - object does not exist.")
}

fn pk_incorrect_type_msg(data_type: &str) -> String {
    format!("Incorrect type. Expected pk value, received {data_type}.")
}

/// The `pk_value` rendering inside `does_not_exist` (the raw input as
/// DRF interpolates it).
fn pk_json_scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// `ChoiceField.to_internal_value`: `str(data)` looked up in the choice
/// strings; a miss fails `invalid_choice` with the Python-`str` input.
fn check_choice<'a>(value: &Value, choices: &[&'a str]) -> Result<&'a str, String> {
    let rendered = py_str(value);
    for choice in choices {
        if rendered == *choice {
            return Ok(choice);
        }
    }
    Err(format!("\"{rendered}\" is not a valid choice."))
}

/// Python `str()` over a JSON value (ASCII-exact; container strings use
/// single quotes with minimal escaping — exotic control/unicode content
/// in error text may diverge in an uncovered corner).
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => match split_json_number(number) {
            JsonNum::Int(digits) => digits,
            JsonNum::Float(float) => crate::paginator::py_float_str(float),
        },
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, val)| {
                    format!("{}: {}", py_repr(&Value::String(key.clone())), py_repr(val))
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` over a JSON value (same caveat as [`py_str`]).
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr_string(text),
        Value::Array(_) | Value::Object(_) => py_str(value),
        _ => py_str(value),
    }
}

fn py_repr_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

// ---------------------------------------------------------------------------
// JSON render helpers (byte-exact floats)
// ---------------------------------------------------------------------------

/// A JSON float exactly like CPython's `json.dumps`: `repr()` digits
/// ([`crate::paginator::py_float_str`]) with the `NaN`/`Infinity`
/// literals for non-finite values (where `str()` would print lowercase).
fn render_json_float(value: f64) -> String {
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
    crate::paginator::py_float_str(value)
}

fn opt_json_string(value: Option<&str>) -> String {
    match value {
        Some(text) => json_string(text),
        None => "null".to_owned(),
    }
}

/// Push one `key: rendered` pair into a hand-shaped object.
fn push_pair(out: &mut String, first: &mut bool, key: &str, rendered: &str) {
    if !*first {
        out.push(',');
    }
    *first = false;
    out.push_str(&json_string(key));
    out.push(':');
    out.push_str(rendered);
}

// ---------------------------------------------------------------------------
// Quick links (quick_link.py:16-65)
// ---------------------------------------------------------------------------

/// PATCH miss (`:43`, fixture error #25): lowercase `detail`
/// (hand-written, not DRF-raised).
pub const QUICK_LINK_PATCH_MISSING_BODY: &str = r#"{"detail":"Quick link not found."}"#;
/// Retrieve miss (`:51-52`, fixture error #26).
pub const QUICK_LINK_RETRIEVE_MISSING_BODY: &str = r#"{"error":"Quick link not found."}"#;
/// Django model name for the delete fan-out (`db/mixins.py:78`).
const QUICK_LINK_MODEL: &str = "workspaceuserlink";

/// Validated quick-link write input: `None` = key absent (model default
/// on create, keep on update). `created_by` validates but `BaseModel.save`
/// overwrites it on create (kept on update); `updated_by` validates and
/// is always overwritten.
struct QuickLinkInput {
    deleted_at: Option<Option<DateTime<Utc>>>,
    title: Option<Option<String>>,
    url: Option<String>,
    metadata: Option<Value>,
    created_by: Option<Option<Uuid>>,
    updated_by: Option<Option<Uuid>>,
    project: Option<Option<Uuid>>,
}

/// What write validation can fail with: field errors (400), an
/// object-level `validate()` body (400), Django `ValidationError` from a
/// malformed FK (whole-request 400), or an uncaught error (500).
enum WriteInvalid {
    Fields(FieldErrors),
    Object(Value),
    PayloadNotValid,
    ServerError,
}

impl WriteInvalid {
    fn into_response(self) -> Response {
        match self {
            WriteInvalid::Fields(errors) => {
                json_response(StatusCode::BAD_REQUEST, render_field_errors(&errors))
            }
            WriteInvalid::Object(body) => json_response(StatusCode::BAD_REQUEST, body.to_string()),
            WriteInvalid::PayloadNotValid => {
                Denial::BadError(INVALID_DETAIL_MSG.to_owned()).into_response()
            }
            WriteInvalid::ServerError => Denial::ServerError.into_response(),
        }
    }
}

/// `WorkspaceUserLinkSerializer` write path (`workspace.py:196-246`):
/// the scheme-prefix mutation, then per-field validation in writable
/// order (`deleted_at`, `title`, `url`, `metadata`, `created_by`,
/// `updated_by`, `project`), then the FK existence checks. `partial`
/// skips the required checks.
async fn validate_quick_link_body(
    pool: &PgPool,
    body: &Map<String, Value>,
    partial: bool,
    timezone: &Tz,
) -> Result<QuickLinkInput, WriteInvalid> {
    let mut data = body.clone();
    // `to_internal_value` scheme step (`:202-207`); a truthy non-string
    // `url` is `AttributeError` → 500.
    ser_extras::prefix_user_link_url(&mut data).map_err(|_| WriteInvalid::ServerError)?;
    let mut errors: FieldErrors = Vec::new();
    let mut deleted_at = None;
    let mut title = None;
    let mut url = None;
    let mut metadata = None;
    let mut created_by: Option<Option<Uuid>> = None;
    let mut updated_by: Option<Option<Uuid>> = None;
    let mut project: Option<Option<Uuid>> = None;

    if let Some(value) = data.get("deleted_at") {
        if value.is_null() {
            deleted_at = Some(None);
        } else {
            match drf_datetime_to_internal(value, timezone) {
                DtCheck::Value(dt) => deleted_at = Some(Some(dt)),
                DtCheck::Invalid => {
                    push_field_msg(&mut errors, "deleted_at", datetime_invalid_msg())
                }
                DtCheck::MakeAware => {
                    push_field_msg(&mut errors, "deleted_at", make_aware_msg(timezone));
                }
                DtCheck::Overflow => {
                    push_field_msg(
                        &mut errors,
                        "deleted_at",
                        "Datetime value out of range.".to_owned(),
                    );
                }
            }
        }
    }
    if let Some(value) = data.get("title") {
        if value.is_null() {
            title = Some(None);
        } else if matches!(value, Value::String(text) if text.is_empty()) {
            title = Some(Some(String::new()));
        } else {
            match drf_char_to_internal(value) {
                Ok(text) => {
                    if text.chars().count() > 255 {
                        push_field_msg(&mut errors, "title", max_length_msg(255));
                    } else {
                        title = Some(Some(text));
                    }
                }
                Err(()) => push_field_msg(&mut errors, "title", INVALID_STR_MSG.to_owned()),
            }
        }
    }
    match data.get("url") {
        None => {
            if !partial {
                push_field_msg(&mut errors, "url", REQUIRED_MSG.to_owned());
            }
        }
        Some(value) if value.is_null() => push_field_msg(&mut errors, "url", NULL_MSG.to_owned()),
        Some(Value::String(text)) if text.is_empty() => {
            push_field_msg(&mut errors, "url", BLANK_MSG.to_owned());
        }
        Some(value) => match drf_char_to_internal(value) {
            Ok(text) => match ser_extras::validate_user_link_url(&text) {
                Ok(()) => url = Some(text),
                Err(_) => errors.push((
                    "url".to_owned(),
                    Value::Object(
                        [(
                            "error".to_owned(),
                            Value::String(ser_extras::USER_LINK_INVALID_URL_MESSAGE.to_owned()),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                )),
            },
            Err(()) => push_field_msg(&mut errors, "url", INVALID_STR_MSG.to_owned()),
        },
    }
    if let Some(value) = data.get("metadata") {
        if value.is_null() {
            push_field_msg(&mut errors, "metadata", NULL_MSG.to_owned());
        } else {
            metadata = Some(value.clone());
        }
    }
    // FK fields in order, each fully validated (shape + existence)
    // before the next: a malformed UUID aborts the whole request
    // (Django `ValidationError`, not a field error), discarding any
    // earlier field errors — exactly like `is_valid()`.
    for field in ["created_by", "updated_by", "project"] {
        let Some(value) = data.get(field) else {
            continue;
        };
        // Null is allowed (`null=True`).
        if value.is_null() {
            match field {
                "created_by" => created_by = Some(None),
                "updated_by" => updated_by = Some(None),
                _ => project = Some(None),
            }
            continue;
        }
        if matches!(value, Value::String(text) if text.is_empty()) {
            push_field_msg(&mut errors, field, BLANK_MSG.to_owned());
            continue;
        }
        let id = match check_pk_shape(value) {
            PkCheck::Uuid(id) => id,
            PkCheck::Blank => {
                push_field_msg(&mut errors, field, BLANK_MSG.to_owned());
                continue;
            }
            PkCheck::IncorrectType(dtype) => {
                push_field_msg(&mut errors, field, pk_incorrect_type_msg(&dtype));
                continue;
            }
            PkCheck::PayloadNotValid => return Err(WriteInvalid::PayloadNotValid),
            PkCheck::ServerError => return Err(WriteInvalid::ServerError),
        };
        // Existence through the default manager (projects soft-delete
        // scoped, users plain).
        let exists: bool = if field == "project" {
            let row: Option<(Uuid,)> = sqlx::query_as(
                r#"SELECT p.id FROM projects p WHERE p.id = $1 AND p.deleted_at IS NULL"#,
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| WriteInvalid::ServerError)?;
            row.is_some()
        } else {
            let row: Option<(Uuid,)> =
                sqlx::query_as(r#"SELECT u.id FROM users u WHERE u.id = $1"#)
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| WriteInvalid::ServerError)?;
            row.is_some()
        };
        if !exists {
            push_field_msg(&mut errors, field, pk_does_not_exist_msg(value));
        } else {
            match field {
                "created_by" => created_by = Some(Some(id)),
                "updated_by" => updated_by = Some(Some(id)),
                _ => project = Some(Some(id)),
            }
        }
    }
    if !errors.is_empty() {
        return Err(WriteInvalid::Fields(errors));
    }
    Ok(QuickLinkInput {
        deleted_at,
        title,
        url,
        metadata,
        created_by,
        updated_by,
        project,
    })
}

type QuickLinkRow = (
    Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<String>,
    String,
    Value,
    Option<Uuid>,
    Option<Uuid>,
    Uuid,
    Option<Uuid>,
    Uuid,
);

/// One `workspace_user_links` row rendered through
/// `WorkspaceUserLinkSerializer` in [`ser_extras::USER_LINK_WIRE_FIELDS`]
/// order.
fn render_quick_link(row: &QuickLinkRow, timezone: &Tz) -> Result<String, Denial> {
    let (
        id,
        created_at,
        updated_at,
        deleted_at,
        title,
        url,
        metadata,
        created_by,
        updated_by,
        workspace,
        project,
        owner,
    ) = row;
    let id = id.to_string();
    let created_at = render_dt(created_at, timezone)?;
    let updated_at = render_dt(updated_at, timezone)?;
    let deleted_at = match deleted_at {
        Some(dt) => Some(render_dt(dt, timezone)?),
        None => None,
    };
    let created_by = (*created_by).map(|id| id.to_string());
    let updated_by = (*updated_by).map(|id| id.to_string());
    let workspace = workspace.to_string();
    let project = (*project).map(|id| id.to_string());
    let owner = owner.to_string();
    let shaped = ser_extras::UserLinkRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        title: title.as_deref(),
        url,
        metadata,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        workspace: &workspace,
        project: project.as_deref(),
        owner: &owner,
    };
    let view = ser_extras::user_link_to_representation(&shaped);
    Ok(serde_json::to_string(&view).expect("link view"))
}

async fn quick_link_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("POST", "workspaces/<slug>/quick-links/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let workspace_id = resolve_workspace_id(&pool, &slug).await?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let data = match parse_body_object(&body) {
        Ok(data) => data,
        Err(denial) => return Err(denial),
    };
    let input = match validate_quick_link_body(&pool, &data, false, &timezone).await {
        Ok(input) => input,
        Err(invalid) => return Ok(invalid.into_response()),
    };
    // Dup guard (`workspace.py:218-232`, [`ser_extras::user_link_create_lookup`]):
    // `url` is always present (required).
    let url = input.url.expect("required url");
    let dup: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT l.id FROM workspace_user_links l
           WHERE l.url = $1 AND l.workspace_id = $2 AND l.owner_id = $3
           AND l.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(&url)
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if dup.is_some() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            ser_extras::duplicate_user_link_body().to_string(),
        ));
    }
    // `WorkspaceBaseModel.save`: an explicit project re-points the row's
    // workspace at the project's workspace.
    let workspace_id = match input.project {
        Some(Some(project_id)) => {
            let row: Option<(Uuid,)> =
                sqlx::query_as(r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1"#)
                    .bind(project_id)
                    .fetch_optional(&pool)
                    .await
                    .map_err(|_| Denial::ServerError)?;
            backfill_workspace_id(workspace_id, row.map(|row| row.0))
        }
        _ => workspace_id,
    };
    // Validated, then overwritten by `BaseModel.save` on create
    // (`created_by=user`, `updated_by=None`).
    let _ = (input.created_by, input.updated_by);
    let id = Uuid::new_v4();
    // `auto_now_add` and `auto_now` each call `timezone.now()`
    // separately, so the two stamps can differ by a microsecond.
    let created_at = Utc::now();
    let updated_at = Utc::now();
    let deleted_at = input.deleted_at.flatten();
    let title = input.title.flatten();
    let project_id = input.project.flatten();
    let metadata = input.metadata.unwrap_or(Value::Object(Map::new()));
    sqlx::query(
        r#"INSERT INTO workspace_user_links
           (id, created_at, updated_at, deleted_at, title, url, metadata,
            created_by_id, updated_by_id, workspace_id, project_id, owner_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"#,
    )
    .bind(id)
    .bind(created_at)
    .bind(updated_at)
    .bind(deleted_at)
    .bind(&title)
    .bind(&url)
    .bind(&metadata)
    .bind(user_id)
    .bind(Option::<Uuid>::None)
    .bind(workspace_id)
    .bind(project_id)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    let row: QuickLinkRow = (
        id,
        created_at,
        updated_at,
        deleted_at,
        title,
        url,
        metadata,
        Some(user_id),
        None,
        workspace_id,
        project_id,
        user_id,
    );
    Ok(json_response(
        StatusCode::CREATED,
        render_quick_link(&row, &timezone)?,
    ))
}

async fn quick_link_partial_update(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let gate = gate_for("PATCH", "workspaces/<slug>/quick-links/<pk>/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let row: Option<QuickLinkRow> = sqlx::query_as(
        r#"SELECT l.id, l.created_at, l.updated_at, l.deleted_at, l.title, l.url,
                  l.metadata, l.created_by_id, l.updated_by_id, l.workspace_id,
                  l.project_id, l.owner_id
           FROM workspace_user_links l JOIN workspaces w ON w.id = l.workspace_id
           WHERE l.id = $1 AND w.slug = $2 AND l.owner_id = $3 AND l.deleted_at IS NULL
           ORDER BY l.created_at DESC LIMIT 1"#,
    )
    .bind(pk)
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(current) = row else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            QUICK_LINK_PATCH_MISSING_BODY.to_owned(),
        ));
    };
    let timezone = actor_timezone(&pool, &user_id).await?;
    let data = match parse_body_object(&body) {
        Ok(data) => data,
        Err(denial) => return Err(denial),
    };
    let input = match validate_quick_link_body(&pool, &data, true, &timezone).await {
        Ok(input) => input,
        Err(invalid) => return Ok(invalid.into_response()),
    };
    // Dup guard (`workspace.py:234-246`): an omitted `url` passes `None`,
    // which matches nothing on the `NOT NULL` column.
    if input.url.is_some() {
        let dup: Option<(Uuid,)> = sqlx::query_as(
            r#"SELECT l.id FROM workspace_user_links l
               WHERE l.url = $1 AND l.workspace_id = $2 AND l.owner_id = $3
               AND l.id != $4 AND l.deleted_at IS NULL LIMIT 1"#,
        )
        .bind(input.url.as_deref().expect("some url"))
        .bind(current.9)
        .bind(current.11)
        .bind(pk)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if dup.is_some() {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                ser_extras::duplicate_user_link_body().to_string(),
            ));
        }
    }
    let workspace_id = match input.project {
        Some(Some(project_id)) => {
            let prow: Option<(Uuid,)> =
                sqlx::query_as(r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1"#)
                    .bind(project_id)
                    .fetch_optional(&pool)
                    .await
                    .map_err(|_| Denial::ServerError)?;
            backfill_workspace_id(current.9, prow.map(|row| row.0))
        }
        _ => current.9,
    };
    // Validated, then overwritten (`updated_by=user`).
    let _ = input.updated_by;
    let updated_at = Utc::now();
    // `update()` writes the validated keys; `save()` stamps
    // `updated_at`/`updated_by` (and keeps a validated `created_by`).
    let deleted_at = input.deleted_at.unwrap_or(current.3);
    let title = input.title.unwrap_or(current.4);
    let url = input.url.unwrap_or(current.5);
    let metadata = input.metadata.unwrap_or(current.6);
    let created_by = input.created_by.unwrap_or(current.7);
    let project = input.project.unwrap_or(current.10);
    sqlx::query(
        r#"UPDATE workspace_user_links SET deleted_at = $1, title = $2, url = $3,
                  metadata = $4, created_by_id = $5, updated_by_id = $6,
                  workspace_id = $7, project_id = $8, updated_at = $9
           WHERE id = $10"#,
    )
    .bind(deleted_at)
    .bind(&title)
    .bind(&url)
    .bind(&metadata)
    .bind(created_by)
    .bind(user_id)
    .bind(workspace_id)
    .bind(project)
    .bind(updated_at)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    let row: QuickLinkRow = (
        pk,
        current.1,
        updated_at,
        deleted_at,
        title,
        url,
        metadata,
        created_by,
        Some(user_id),
        workspace_id,
        project,
        current.11,
    );
    Ok(json_response(
        StatusCode::OK,
        render_quick_link(&row, &timezone)?,
    ))
}

async fn quick_link_retrieve(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let gate = gate_for("GET", "workspaces/<slug>/quick-links/<pk>/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let row: Option<QuickLinkRow> = sqlx::query_as(
        r#"SELECT l.id, l.created_at, l.updated_at, l.deleted_at, l.title, l.url,
                  l.metadata, l.created_by_id, l.updated_by_id, l.workspace_id,
                  l.project_id, l.owner_id
           FROM workspace_user_links l JOIN workspaces w ON w.id = l.workspace_id
           WHERE l.id = $1 AND w.slug = $2 AND l.owner_id = $3 AND l.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(pk)
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            QUICK_LINK_RETRIEVE_MISSING_BODY.to_owned(),
        ));
    };
    let timezone = actor_timezone(&pool, &user_id).await?;
    Ok(json_response(
        StatusCode::OK,
        render_quick_link(&row, &timezone)?,
    ))
}

async fn quick_link_destroy(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let gate = gate_for("DELETE", "workspaces/<slug>/quick-links/<pk>/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    // No try/except in Python (`:56`): a miss funnels into the generic
    // `ObjectDoesNotExist` 404.
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT l.id FROM workspace_user_links l JOIN workspaces w ON w.id = l.workspace_id
           WHERE l.id = $1 AND w.slug = $2 AND l.owner_id = $3 AND l.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(pk)
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if row.is_none() {
        return Err(Denial::NotFound);
    }
    let deleted_at = Utc::now();
    let updated_at = Utc::now();
    sqlx::query(
        r#"UPDATE workspace_user_links SET deleted_at = $1, updated_at = $2, updated_by_id = $3
           WHERE id = $4"#,
    )
    .bind(deleted_at)
    .bind(updated_at)
    .bind(user_id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    enqueue_soft_delete(&pool, QUICK_LINK_MODEL, &pk).await;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

async fn quick_link_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("GET", "workspaces/<slug>/quick-links/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    // No `Workspace.objects.get` on this path (`:62` filters the slug
    // directly), so there is no 404 — only the gate's 403.
    let timezone = actor_timezone(&pool, &user_id).await?;
    let rows: Vec<QuickLinkRow> = sqlx::query_as(
        r#"SELECT l.id, l.created_at, l.updated_at, l.deleted_at, l.title, l.url,
                  l.metadata, l.created_by_id, l.updated_by_id, l.workspace_id,
                  l.project_id, l.owner_id
           FROM workspace_user_links l JOIN workspaces w ON w.id = l.workspace_id
           WHERE w.slug = $1 AND l.owner_id = $2 AND l.deleted_at IS NULL
           ORDER BY l.created_at DESC"#,
    )
    .bind(&slug)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut parts = Vec::with_capacity(rows.len());
    for row in &rows {
        parts.push(render_quick_link(row, &timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", parts.join(",")),
    ))
}

// ---------------------------------------------------------------------------
// Stickies (sticky.py:16-60)
// ---------------------------------------------------------------------------

/// Stock `get_object()` miss (`ModelViewSet`, DRF `NotFound`).
pub const STICKY_NOT_FOUND_BODY: &str = r#"{"Detail":"Not found."}"#;
/// Django model name for the delete fan-out (`db/mixins.py:78`).
const STICKY_MODEL: &str = "sticky";

/// Validated sticky write input: `None` = key absent (model default on
/// create, keep on update). `description_stripped` validates but
/// `Sticky.save` always recomputes it (validated-then-discarded);
/// `created_by` validates but `BaseModel.save` overwrites it on create
/// (kept on update); `updated_by` validates and is always overwritten.
struct StickyInput {
    deleted_at: Option<Option<DateTime<Utc>>>,
    name: Option<Option<String>>,
    description: Option<Value>,
    description_html: Option<String>,
    description_stripped: Option<Option<String>>,
    logo_props: Option<Value>,
    color: Option<Option<String>>,
    background_color: Option<Option<String>>,
    sort_order: Option<f64>,
    created_by: Option<Option<Uuid>>,
    updated_by: Option<Option<Uuid>>,
}

/// `StickySerializer` write path (`workspace.py:354-377`): read-only
/// keys stripped, per-field validation in writable order
/// (`deleted_at`, `name`, `description`, `description_html`,
/// `description_stripped`, `logo_props`, `color`, `background_color`,
/// `sort_order`, `created_by`, `updated_by`), FK existence, then the
/// object-level `validate()`.
async fn validate_sticky_body(
    pool: &PgPool,
    body: &Map<String, Value>,
    partial: bool,
    timezone: &Tz,
) -> Result<StickyInput, WriteInvalid> {
    // Read-only keys are silently dropped (`to_internal_value` iterates
    // `_writable_fields`).
    let data = sticky_kernel::strip_ignored_sticky_keys(body);
    let mut errors: FieldErrors = Vec::new();
    let mut deleted_at = None;
    let mut name = None;
    let mut description = None;
    let mut description_html = None;
    let mut description_stripped = None;
    let mut logo_props = None;
    let mut color = None;
    let mut background_color = None;
    let mut sort_order = None;
    let mut created_by: Option<Option<Uuid>> = None;
    let mut updated_by: Option<Option<Uuid>> = None;

    if let Some(value) = data.get("deleted_at") {
        if value.is_null() {
            deleted_at = Some(None);
        } else {
            match drf_datetime_to_internal(value, timezone) {
                DtCheck::Value(dt) => deleted_at = Some(Some(dt)),
                DtCheck::Invalid => {
                    push_field_msg(&mut errors, "deleted_at", datetime_invalid_msg())
                }
                DtCheck::MakeAware => {
                    push_field_msg(&mut errors, "deleted_at", make_aware_msg(timezone));
                }
                DtCheck::Overflow => {
                    push_field_msg(
                        &mut errors,
                        "deleted_at",
                        "Datetime value out of range.".to_owned(),
                    );
                }
            }
        }
    }
    // `name` is not required (`extra_kwargs`, also `blank=True`); no
    // `max_length` (a `TextField`).
    if let Some(value) = data.get("name") {
        if value.is_null() {
            name = Some(None);
        } else if matches!(value, Value::String(text) if text.is_empty()) {
            name = Some(Some(String::new()));
        } else {
            match drf_char_to_internal(value) {
                Ok(text) => name = Some(Some(text)),
                Err(()) => push_field_msg(&mut errors, "name", INVALID_STR_MSG.to_owned()),
            }
        }
    }
    if let Some(value) = data.get("description") {
        if value.is_null() {
            push_field_msg(&mut errors, "description", NULL_MSG.to_owned());
        } else {
            description = Some(value.clone());
        }
    }
    if let Some(value) = data.get("description_html") {
        if value.is_null() {
            push_field_msg(&mut errors, "description_html", NULL_MSG.to_owned());
        } else if matches!(value, Value::String(text) if text.is_empty()) {
            description_html = Some(String::new());
        } else {
            match drf_char_to_internal(value) {
                Ok(text) => description_html = Some(text),
                Err(()) => {
                    push_field_msg(&mut errors, "description_html", INVALID_STR_MSG.to_owned())
                }
            }
        }
    }
    if let Some(value) = data.get("description_stripped") {
        if value.is_null() {
            description_stripped = Some(None);
        } else if matches!(value, Value::String(text) if text.is_empty()) {
            description_stripped = Some(Some(String::new()));
        } else {
            match drf_char_to_internal(value) {
                Ok(text) => description_stripped = Some(Some(text)),
                Err(()) => {
                    push_field_msg(
                        &mut errors,
                        "description_stripped",
                        INVALID_STR_MSG.to_owned(),
                    );
                }
            }
        }
    }
    if let Some(value) = data.get("logo_props") {
        if value.is_null() {
            push_field_msg(&mut errors, "logo_props", NULL_MSG.to_owned());
        } else {
            logo_props = Some(value.clone());
        }
    }
    for field in ["color", "background_color"] {
        let Some(value) = data.get(field) else {
            continue;
        };
        if value.is_null() {
            match field {
                "color" => color = Some(None),
                _ => background_color = Some(None),
            }
            continue;
        }
        if matches!(value, Value::String(text) if text.is_empty()) {
            match field {
                "color" => color = Some(Some(String::new())),
                _ => background_color = Some(Some(String::new())),
            }
            continue;
        }
        match drf_char_to_internal(value) {
            Ok(text) => {
                if text.chars().count() > 255 {
                    push_field_msg(&mut errors, field, max_length_msg(255));
                } else {
                    match field {
                        "color" => color = Some(Some(text)),
                        _ => background_color = Some(Some(text)),
                    }
                }
            }
            Err(()) => push_field_msg(&mut errors, field, INVALID_STR_MSG.to_owned()),
        }
    }
    if let Some(value) = data.get("sort_order") {
        if value.is_null() {
            push_field_msg(&mut errors, "sort_order", NULL_MSG.to_owned());
        } else {
            match drf_float_to_internal(value) {
                FloatCheck::Value(float) => sort_order = Some(float),
                FloatCheck::Invalid => {
                    push_field_msg(&mut errors, "sort_order", FLOAT_MSG.to_owned())
                }
                FloatCheck::TooLarge => {
                    push_field_msg(&mut errors, "sort_order", TOO_LARGE_MSG.to_owned());
                }
                FloatCheck::Overflow => {
                    push_field_msg(&mut errors, "sort_order", INT_TO_FLOAT_MSG.to_owned());
                }
            }
        }
    }
    for field in ["created_by", "updated_by"] {
        let Some(value) = data.get(field) else {
            continue;
        };
        if value.is_null() {
            match field {
                "created_by" => created_by = Some(None),
                _ => updated_by = Some(None),
            }
            continue;
        }
        if matches!(value, Value::String(text) if text.is_empty()) {
            push_field_msg(&mut errors, field, BLANK_MSG.to_owned());
            continue;
        }
        let id = match check_pk_shape(value) {
            PkCheck::Uuid(id) => id,
            PkCheck::Blank => {
                push_field_msg(&mut errors, field, BLANK_MSG.to_owned());
                continue;
            }
            PkCheck::IncorrectType(dtype) => {
                push_field_msg(&mut errors, field, pk_incorrect_type_msg(&dtype));
                continue;
            }
            PkCheck::PayloadNotValid => return Err(WriteInvalid::PayloadNotValid),
            PkCheck::ServerError => return Err(WriteInvalid::ServerError),
        };
        let row: Option<(Uuid,)> = sqlx::query_as(r#"SELECT u.id FROM users u WHERE u.id = $1"#)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| WriteInvalid::ServerError)?;
        if row.is_none() {
            push_field_msg(&mut errors, field, pk_does_not_exist_msg(value));
        } else {
            match field {
                "created_by" => created_by = Some(Some(id)),
                _ => updated_by = Some(Some(id)),
            }
        }
    }
    if !errors.is_empty() {
        return Err(WriteInvalid::Fields(errors));
    }
    // Object-level `validate()` (`:361-376`) runs only when every field
    // passed. `description_binary` never reaches it (read-only, so the
    // arm is dead — `None` here).
    let html_present = description_html.as_deref().filter(|html| !html.is_empty());
    if html_present.is_some() {
        match ser_extras::sticky_validate_descriptions(html_present, None) {
            Ok(sanitized) => {
                if let Some(clean) = sanitized {
                    description_html = Some(clean);
                }
            }
            Err(raised) => {
                // The raised dicts list-wrap on the wire
                // (`as_serializer_error`); the live bodies live in the
                // sticky kernel (single owner).
                let body = if raised.get("description_binary").is_some() {
                    sticky_kernel::binary_invalid_body()
                } else {
                    sticky_kernel::html_invalid_body()
                };
                return Err(WriteInvalid::Object(body));
            }
        }
    }
    // Every sticky field is optional, so `partial` changes nothing.
    let _ = partial;
    Ok(StickyInput {
        deleted_at,
        name,
        description,
        description_html,
        description_stripped,
        logo_props,
        color,
        background_color,
        sort_order,
        created_by,
        updated_by,
    })
}

/// One `stickies` row (17 columns — past the tuple `FromRow` arity,
/// so a named struct).
#[derive(sqlx::FromRow, Clone, Debug)]
struct StickyRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    name: Option<String>,
    description: Value,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    logo_props: Value,
    color: Option<String>,
    background_color: Option<String>,
    sort_order: f64,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    workspace_id: Uuid,
    owner_id: Uuid,
}

/// One `stickies` row in [`ser_extras::STICKY_WIRE_FIELDS`] order. The
/// object is hand-shaped (not the SER-C view) so `sort_order` renders
/// through [`render_json_float`] — serde would print `NaN` as `null`
/// and exponents without Python's `+`/padding.
fn render_sticky(row: &StickyRow, timezone: &Tz) -> Result<String, Denial> {
    let created_at = render_dt(&row.created_at, timezone)?;
    let updated_at = render_dt(&row.updated_at, timezone)?;
    let deleted_at = match &row.deleted_at {
        Some(dt) => Some(render_dt(dt, timezone)?),
        None => None,
    };
    let binary = row
        .description_binary
        .as_deref()
        .map(ser_extras::sticky_binary_to_string);
    let created_by = row.created_by_id.map(|id| id.to_string());
    let updated_by = row.updated_by_id.map(|id| id.to_string());
    let mut out = String::from("{");
    let mut first = true;
    push_pair(
        &mut out,
        &mut first,
        "id",
        &json_string(&row.id.to_string()),
    );
    push_pair(
        &mut out,
        &mut first,
        "created_at",
        &json_string(&created_at),
    );
    push_pair(
        &mut out,
        &mut first,
        "updated_at",
        &json_string(&updated_at),
    );
    push_pair(
        &mut out,
        &mut first,
        "deleted_at",
        &opt_json_string(deleted_at.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "name",
        &opt_json_string(row.name.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "description",
        &row.description.to_string(),
    );
    push_pair(
        &mut out,
        &mut first,
        "description_html",
        &json_string(&row.description_html),
    );
    push_pair(
        &mut out,
        &mut first,
        "description_stripped",
        &opt_json_string(row.description_stripped.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "description_binary",
        &opt_json_string(binary.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "logo_props",
        &row.logo_props.to_string(),
    );
    push_pair(
        &mut out,
        &mut first,
        "color",
        &opt_json_string(row.color.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "background_color",
        &opt_json_string(row.background_color.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "sort_order",
        &render_json_float(row.sort_order),
    );
    push_pair(
        &mut out,
        &mut first,
        "created_by",
        &opt_json_string(created_by.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "updated_by",
        &opt_json_string(updated_by.as_deref()),
    );
    push_pair(
        &mut out,
        &mut first,
        "workspace",
        &json_string(&row.workspace_id.to_string()),
    );
    push_pair(
        &mut out,
        &mut first,
        "owner",
        &json_string(&row.owner_id.to_string()),
    );
    out.push('}');
    Ok(out)
}

const STICKY_SELECT: &str = "s.id, s.created_at, s.updated_at, s.deleted_at, s.name, s.description, s.description_html, s.description_stripped, s.description_binary, s.logo_props, s.color, s.background_color, s.sort_order, s.created_by_id, s.updated_by_id, s.workspace_id, s.owner_id";

async fn sticky_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("POST", "workspaces/<slug>/stickies/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let workspace_id = resolve_workspace_id(&pool, &slug).await?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let data = match parse_body_object(&body) {
        Ok(data) => data,
        Err(denial) => return Err(denial),
    };
    let input = match validate_sticky_body(&pool, &data, false, &timezone).await {
        Ok(input) => input,
        Err(invalid) => return Ok(invalid.into_response()),
    };
    // `Sticky.save` sequence rule: `max + 10000` wins whenever the
    // workspace has rows (input `sort_order` ignored); the input (or the
    // 65535 default) survives only on an empty table.
    let max_row: (Option<f64>,) = sqlx::query_as(
        r#"SELECT MAX(s.sort_order) FROM stickies s
           WHERE s.workspace_id = $1 AND s.deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sort_order = match max_row.0 {
        None => input.sort_order.unwrap_or(sticky_model::DEFAULT_SORT_ORDER),
        Some(max) => sticky_model::sort_order_on_create(Some(max)),
    };
    // Validated, then discarded: `Sticky.save` recomputes the stripped
    // text, and `BaseModel.save` overwrites the audit pair on create.
    let _ = (
        &input.description_stripped,
        input.created_by,
        input.updated_by,
    );
    let id = Uuid::new_v4();
    let created_at = Utc::now();
    let updated_at = Utc::now();
    let description_html = input.description_html.unwrap_or("<p></p>".to_owned());
    // `Sticky.save` recomputes `description_stripped` unconditionally —
    // validated input for that key is discarded.
    let description_stripped = sticky_model::stripped_description(Some(description_html.as_str()));
    let description = input.description.unwrap_or(Value::Object(Map::new()));
    let logo_props = input.logo_props.unwrap_or(Value::Object(Map::new()));
    let deleted_at = input.deleted_at.flatten();
    let name = input.name.flatten();
    let color = input.color.flatten();
    let background_color = input.background_color.flatten();
    sqlx::query(
        r#"INSERT INTO stickies
           (id, created_at, updated_at, deleted_at, name, description, description_html,
            description_stripped, description_binary, logo_props, color, background_color,
            sort_order, created_by_id, updated_by_id, workspace_id, owner_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)"#,
    )
    .bind(id)
    .bind(created_at)
    .bind(updated_at)
    .bind(deleted_at)
    .bind(&name)
    .bind(&description)
    .bind(&description_html)
    .bind(&description_stripped)
    .bind(Option::<Vec<u8>>::None)
    .bind(&logo_props)
    .bind(&color)
    .bind(&background_color)
    .bind(sort_order)
    .bind(user_id)
    .bind(Option::<Uuid>::None)
    .bind(workspace_id)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    let row = StickyRow {
        id,
        created_at,
        updated_at,
        deleted_at,
        name,
        description,
        description_html,
        description_stripped,
        description_binary: None,
        logo_props,
        color,
        background_color,
        sort_order,
        created_by_id: Some(user_id),
        updated_by_id: None,
        workspace_id,
        owner_id: user_id,
    };
    Ok(json_response(
        StatusCode::CREATED,
        render_sticky(&row, &timezone)?,
    ))
}

async fn sticky_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("GET", "workspaces/<slug>/stickies/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let per_page = crate::paginator::parse_per_page(
        query_last(&query, "per_page").as_deref(),
        queries_extras::STICKY_LIST_PER_PAGE.into(),
        1000,
    )
    .map_err(page_denial)?;
    let cursor = match query_last(&query, "cursor") {
        None => crate::paginator::Cursor::default_for(per_page),
        Some(raw) => crate::paginator::Cursor::from_string(&raw).map_err(page_denial)?,
    };
    let limit = crate::paginator::clamp_limit(per_page, crate::paginator::MAX_LIMIT);
    let window =
        crate::paginator::offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
            .map_err(page_denial)?;
    // `?query=` applies only when truthy (`:42,44`).
    let filter = query_last(&query, "query").filter(|term| !term.is_empty());
    let pattern = filter.map(|term| format!("%{}%", queries_extras::escape_icontains(&term)));
    let total: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM stickies s JOIN workspaces w ON w.id = s.workspace_id
           WHERE w.slug = $1 AND s.owner_id = $2 AND s.deleted_at IS NULL
           AND ($3::text IS NULL OR s.description_stripped ILIKE $3)"#,
    )
    .bind(&slug)
    .bind(user_id)
    .bind(pattern.as_deref())
    .fetch_one(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let fetch: Vec<StickyRow> = sqlx::query_as(&format!(
        r#"SELECT {STICKY_SELECT} FROM stickies s JOIN workspaces w ON w.id = s.workspace_id
           WHERE w.slug = $1 AND s.owner_id = $2 AND s.deleted_at IS NULL
           AND ($3::text IS NULL OR s.description_stripped ILIKE $3)
           ORDER BY {} LIMIT $4 OFFSET $5"#,
        queries_extras::STICKY_LIST_ORDER_SQL,
    ))
    .bind(&slug)
    .bind(user_id)
    .bind(pattern.as_deref())
    .bind(window.stop - window.offset)
    .bind(window.offset)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let has_more = i64::try_from(fetch.len()).unwrap_or(i64::MAX) > limit;
    let next = crate::paginator::next_cursor(limit, window.page, has_more);
    let prev = crate::paginator::prev_cursor(limit, window.page);
    let page_rows = crate::paginator::apply_offset_window(&fetch, limit).map_err(page_denial)?;
    let mut parts = Vec::with_capacity(page_rows.len());
    for row in &page_rows {
        parts.push(render_sticky(row, &timezone)?);
    }
    let mut rendered = Vec::with_capacity(parts.len());
    for part in &parts {
        rendered.push(serde_json::from_str(part).map_err(|_| Denial::ServerError)?);
    }
    envelope(total.0, per_page, &next, &prev, Value::Array(rendered))
}

async fn sticky_retrieve(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    // No decorator on this action: any signed-in user reaches the body.
    let user_id = actor_user_id(extension)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let row: Option<StickyRow> = sqlx::query_as(&format!(
        r#"SELECT {STICKY_SELECT} FROM stickies s JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.id = $1 AND w.slug = $2 AND s.owner_id = $3 AND s.deleted_at IS NULL
           LIMIT 1"#,
    ))
    .bind(pk)
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            STICKY_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let timezone = actor_timezone(&pool, &user_id).await?;
    Ok(json_response(
        StatusCode::OK,
        render_sticky(&row, &timezone)?,
    ))
}

/// The creator fact for the sticky patch/destroy rows
/// (`app/permissions/base.py:36`): `Sticky.objects.filter(id=pk,
/// created_by=user).exists()` — unscoped by workspace/owner, unlike the
/// handler lookups.
async fn sticky_creator_fact(pool: &PgPool, pk: &Uuid, user_id: &Uuid) -> Result<bool, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT s.id FROM stickies s
           WHERE s.id = $1 AND s.created_by_id = $2 AND s.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(pk)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

async fn sticky_partial_update(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let gate = gate_for("PATCH", "workspaces/<slug>/stickies/<pk>/");
    let is_creator = sticky_creator_fact(&pool, &pk, &user_id).await?;
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), is_creator).await?;
    check_gate(&gate, &slug, &facts)?;
    let row: Option<StickyRow> = sqlx::query_as(&format!(
        r#"SELECT {STICKY_SELECT} FROM stickies s JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.id = $1 AND w.slug = $2 AND s.owner_id = $3 AND s.deleted_at IS NULL
           LIMIT 1"#,
    ))
    .bind(pk)
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(current) = row else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            STICKY_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let timezone = actor_timezone(&pool, &user_id).await?;
    let data = match parse_body_object(&body) {
        Ok(data) => data,
        Err(denial) => return Err(denial),
    };
    let input = match validate_sticky_body(&pool, &data, true, &timezone).await {
        Ok(input) => input,
        Err(invalid) => return Ok(invalid.into_response()),
    };
    // Validated, then discarded (`save()` recomputes/overwrites).
    let _ = (&input.description_stripped, input.updated_by);
    let updated_at = Utc::now();
    let deleted_at = input.deleted_at.unwrap_or(current.deleted_at);
    let name = input.name.unwrap_or(current.name);
    let description = input.description.unwrap_or(current.description);
    let description_html = input.description_html.unwrap_or(current.description_html);
    // `Sticky.save` recomputes the stripped text from the final HTML on
    // every save.
    let description_stripped = sticky_model::stripped_description(Some(description_html.as_str()));
    let logo_props = input.logo_props.unwrap_or(current.logo_props);
    let color = input.color.unwrap_or(current.color);
    let background_color = input.background_color.unwrap_or(current.background_color);
    let sort_order = input.sort_order.unwrap_or(current.sort_order);
    let created_by = input.created_by.unwrap_or(current.created_by_id);
    sqlx::query(
        r#"UPDATE stickies SET deleted_at = $1, name = $2, description = $3,
                  description_html = $4, description_stripped = $5, logo_props = $6,
                  color = $7, background_color = $8, sort_order = $9,
                  created_by_id = $10, updated_by_id = $11, updated_at = $12
           WHERE id = $13"#,
    )
    .bind(deleted_at)
    .bind(&name)
    .bind(&description)
    .bind(&description_html)
    .bind(&description_stripped)
    .bind(&logo_props)
    .bind(&color)
    .bind(&background_color)
    .bind(sort_order)
    .bind(created_by)
    .bind(user_id)
    .bind(updated_at)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    let row = StickyRow {
        id: pk,
        created_at: current.created_at,
        updated_at,
        deleted_at,
        name,
        description,
        description_html,
        description_stripped,
        description_binary: current.description_binary,
        logo_props,
        color,
        background_color,
        sort_order,
        created_by_id: created_by,
        updated_by_id: Some(user_id),
        workspace_id: current.workspace_id,
        owner_id: current.owner_id,
    };
    Ok(json_response(
        StatusCode::OK,
        render_sticky(&row, &timezone)?,
    ))
}

async fn sticky_destroy(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let gate = gate_for("DELETE", "workspaces/<slug>/stickies/<pk>/");
    let is_creator = sticky_creator_fact(&pool, &pk, &user_id).await?;
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), is_creator).await?;
    check_gate(&gate, &slug, &facts)?;
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT s.id FROM stickies s JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.id = $1 AND w.slug = $2 AND s.owner_id = $3 AND s.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(pk)
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if row.is_none() {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            STICKY_NOT_FOUND_BODY.to_owned(),
        ));
    }
    let deleted_at = Utc::now();
    let updated_at = Utc::now();
    sqlx::query(
        r#"UPDATE stickies SET deleted_at = $1, updated_at = $2, updated_by_id = $3 WHERE id = $4"#,
    )
    .bind(deleted_at)
    .bind(updated_at)
    .bind(user_id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    enqueue_soft_delete(&pool, STICKY_MODEL, &pk).await;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// Home preferences (home.py:17-79)
// ---------------------------------------------------------------------------

/// PATCH miss (`:79`, fixture error #27): 400, not 404.
pub const HOME_PREF_PATCH_MISSING_BODY: &str = r#"{"detail":"Preference not found"}"#;

/// Validated home-pref patch input (`partial=True` always, so nothing
/// is required). Pure — no FK fields.
#[derive(Debug)]
struct HomePrefInput {
    key: Option<String>,
    is_enabled: Option<bool>,
    sort_order: Option<f64>,
}

fn validate_home_pref_body(body: &Map<String, Value>) -> Result<HomePrefInput, FieldErrors> {
    let mut errors: FieldErrors = Vec::new();
    let mut key = None;
    let mut is_enabled = None;
    let mut sort_order = None;
    if let Some(value) = body.get("key") {
        if value.is_null() {
            push_field_msg(&mut errors, "key", NULL_MSG.to_owned());
        } else if matches!(value, Value::String(text) if text.is_empty()) {
            push_field_msg(&mut errors, "key", BLANK_MSG.to_owned());
        } else {
            match drf_char_to_internal(value) {
                Ok(text) => {
                    if text.chars().count() > 255 {
                        push_field_msg(&mut errors, "key", max_length_msg(255));
                    } else {
                        key = Some(text);
                    }
                }
                Err(()) => push_field_msg(&mut errors, "key", INVALID_STR_MSG.to_owned()),
            }
        }
    }
    if let Some(value) = body.get("is_enabled") {
        if value.is_null() {
            push_field_msg(&mut errors, "is_enabled", NULL_MSG.to_owned());
        } else {
            match drf_bool_to_internal(value) {
                Ok(flag) => is_enabled = Some(flag),
                Err(()) => push_field_msg(&mut errors, "is_enabled", BOOL_MSG.to_owned()),
            }
        }
    }
    if let Some(value) = body.get("sort_order") {
        if value.is_null() {
            push_field_msg(&mut errors, "sort_order", NULL_MSG.to_owned());
        } else {
            match drf_float_to_internal(value) {
                FloatCheck::Value(float) => sort_order = Some(float),
                FloatCheck::Invalid => {
                    push_field_msg(&mut errors, "sort_order", FLOAT_MSG.to_owned())
                }
                FloatCheck::TooLarge => {
                    push_field_msg(&mut errors, "sort_order", TOO_LARGE_MSG.to_owned());
                }
                FloatCheck::Overflow => {
                    push_field_msg(&mut errors, "sort_order", INT_TO_FLOAT_MSG.to_owned());
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(HomePrefInput {
        key,
        is_enabled,
        sort_order,
    })
}

/// One `.values("key", "is_enabled", "config", "sort_order")` row (`:63`).
fn render_home_pref_row(key: &str, is_enabled: bool, config: &Value, sort_order: f64) -> String {
    let config = config.to_string();
    format!(
        "{{\"key\":{},\"is_enabled\":{},\"config\":{},\"sort_order\":{}}}",
        json_string(key),
        if is_enabled { "true" } else { "false" },
        config,
        render_json_float(sort_order),
    )
}

/// The patch response (`WorkspaceHomePreferenceSerializer`: `key`,
/// `is_enabled`, `sort_order`).
fn render_home_pref_patch(key: &str, is_enabled: bool, sort_order: f64) -> String {
    format!(
        "{{\"key\":{},\"is_enabled\":{},\"sort_order\":{}}}",
        json_string(key),
        if is_enabled { "true" } else { "false" },
        render_json_float(sort_order),
    )
}

async fn home_prefs_get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("GET", "workspaces/<slug>/home-preferences/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let workspace_id = resolve_workspace_id(&pool, &slug).await?;
    let existing: Vec<(String,)> = sqlx::query_as(
        r#"SELECT p.key FROM workspace_home_preferences p
           WHERE p.user_id = $1 AND p.workspace_id = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let borrowed: Vec<&str> = existing.iter().map(|row| row.0.as_str()).collect();
    // The autocreate loop (`:37-58`, [`queries_extras::home_autocreate_plan`]):
    // one insert per missing key in plan order, each with its own clock
    // read (the `-created_at` response order depends on it).
    for (key, sort_order) in queries_extras::home_autocreate_plan(&borrowed) {
        sqlx::query(
            r#"INSERT INTO workspace_home_preferences
               (id, created_at, updated_at, "key", user_id, workspace_id, sort_order)
               VALUES ($1,$2,$3,$4,$5,$6,$7)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(key)
        .bind(user_id)
        .bind(workspace_id)
        .bind(f64::from(sort_order))
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    let rows: Vec<(String, bool, Value, f64)> = sqlx::query_as(
        r#"SELECT p."key", p.is_enabled, p.config, p.sort_order
           FROM workspace_home_preferences p
           WHERE p.user_id = $1 AND p.workspace_id = $2 AND p.deleted_at IS NULL
           ORDER BY p.created_at DESC"#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let parts: Vec<String> = rows
        .iter()
        .map(|row| render_home_pref_row(&row.0, row.1, &row.2, row.3))
        .collect();
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", parts.join(",")),
    ))
}

async fn home_pref_patch(
    State(state): State<AppState>,
    Path((slug, key)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("PATCH", "workspaces/<slug>/home-preferences/<key>/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let row: Option<(Uuid, String, bool, f64)> = sqlx::query_as(
        r#"SELECT p.id, p."key", p.is_enabled, p.sort_order
           FROM workspace_home_preferences p JOIN workspaces w ON w.id = p.workspace_id
           WHERE p."key" = $1 AND w.slug = $2 AND p.user_id = $3 AND p.deleted_at IS NULL
           ORDER BY p.created_at DESC LIMIT 1"#,
    )
    .bind(&key)
    .bind(&slug)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, current_key, current_enabled, current_sort)) = row else {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            HOME_PREF_PATCH_MISSING_BODY.to_owned(),
        ));
    };
    let data = match parse_body_object(&body) {
        Ok(data) => data,
        Err(denial) => return Err(denial),
    };
    let input = match validate_home_pref_body(&data) {
        Ok(input) => input,
        Err(errors) => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                render_field_errors(&errors),
            ));
        }
    };
    let new_key = input.key.unwrap_or(current_key);
    let new_enabled = input.is_enabled.unwrap_or(current_enabled);
    let new_sort = input.sort_order.unwrap_or(current_sort);
    // `serializer.save()` → full `save()` (`updated_at`/`updated_by`
    // move); a clashing `key` is `IntegrityError` → 400.
    sqlx::query(
        r#"UPDATE workspace_home_preferences
           SET "key" = $1, is_enabled = $2, sort_order = $3, updated_at = $4, updated_by_id = $5
           WHERE id = $6"#,
    )
    .bind(&new_key)
    .bind(new_enabled)
    .bind(new_sort)
    .bind(Utc::now())
    .bind(user_id)
    .bind(id)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    Ok(json_response(
        StatusCode::OK,
        render_home_pref_patch(&new_key, new_enabled, new_sort),
    ))
}

/// PATCH on the unkeyed path: the gate passes, then `patch(request,
/// slug)` misses its `key` argument → `TypeError` → 500.
async fn home_prefs_patch_no_key(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("PATCH", "workspaces/<slug>/home-preferences/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    Ok(json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        crate::app_issues::SERVER_ERROR_BODY.to_owned(),
    ))
}

/// GET on the keyed path: the gate passes, then `get(request, slug)`
/// chokes on the unexpected `key` kwarg → `TypeError` → 500.
async fn home_pref_get_with_key(
    State(state): State<AppState>,
    Path((slug, _key)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("GET", "workspaces/<slug>/home-preferences/<key>/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    Ok(json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        crate::app_issues::SERVER_ERROR_BODY.to_owned(),
    ))
}

// ---------------------------------------------------------------------------
// Sidebar preferences (user_preference.py:18-101)
// ---------------------------------------------------------------------------

/// The PATCH answer: always 200, whatever was skipped (`:101`).
pub const SIDEBAR_PATCH_OK_BODY: &str = r#"{"message":"Successfully updated"}"#;

/// The sidebar PATCH body shape (`for data in request.data`).
enum SidebarBody<'a> {
    Items(&'a Vec<Value>),
    Empty,
    Invalid,
}

fn sidebar_body_shape(data: &Value) -> SidebarBody<'_> {
    match data {
        Value::Array(items) => SidebarBody::Items(items),
        Value::Object(map) if map.is_empty() => SidebarBody::Empty,
        _ => SidebarBody::Invalid,
    }
}

/// One `{key: {is_pinned, sort_order}}` entry in `sort_order` order.
fn render_sidebar_prefs(rows: &[(String, bool, f64)]) -> String {
    let mut out = String::from("{");
    for (index, (key, is_pinned, sort_order)) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(key));
        out.push(':');
        out.push_str(&format!(
            "{{\"is_pinned\":{},\"sort_order\":{}}}",
            if *is_pinned { "true" } else { "false" },
            render_json_float(*sort_order),
        ));
    }
    out.push('}');
    out
}

async fn sidebar_prefs_get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("GET", "workspaces/<slug>/sidebar-preferences/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    let workspace_id = resolve_workspace_id(&pool, &slug).await?;
    let existing: Vec<(String,)> = sqlx::query_as(
        r#"SELECT p."key" FROM workspace_user_preferences p
           WHERE p.user_id = $1 AND p.workspace_id = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let borrowed: Vec<&str> = existing.iter().map(|row| row.0.as_str()).collect();
    // The autocreate loop (`:35-61`,
    // [`queries_extras::sidebar_autocreate_plan`]).
    for (key, is_pinned, sort_order) in queries_extras::sidebar_autocreate_plan(&borrowed) {
        sqlx::query(
            r#"INSERT INTO workspace_user_preferences
               (id, created_at, updated_at, "key", user_id, workspace_id, sort_order, is_pinned)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(key)
        .bind(user_id)
        .bind(workspace_id)
        .bind(f64::from(sort_order))
        .bind(is_pinned)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    let rows: Vec<(String, bool, f64)> = sqlx::query_as(
        r#"SELECT p."key", p.is_pinned, p.sort_order
           FROM workspace_user_preferences p
           WHERE p.user_id = $1 AND p.workspace_id = $2 AND p.deleted_at IS NULL
           ORDER BY p.sort_order ASC"#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, render_sidebar_prefs(&rows)))
}

async fn sidebar_prefs_patch(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("PATCH", "workspaces/<slug>/sidebar-preferences/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    // No `Workspace.objects.get` on this path (`:82-88` filters the slug
    // directly); a bogus slug just matches nothing → 200.
    let data = match parse_sidebar_body(&body) {
        Ok(data) => data,
        Err(denial) => return Err(denial),
    };
    // `for data in request.data`: an empty body parses to `{}` (zero
    // iterations → 200); any non-list shape — a non-empty dict (its keys
    // have no `.pop`), a string, a number, a bool, `null` — dies with
    // `AttributeError`/`TypeError` → 500.
    let items = match sidebar_body_shape(&data) {
        SidebarBody::Items(items) => items.clone(),
        SidebarBody::Empty => {
            return Ok(json_response(
                StatusCode::OK,
                SIDEBAR_PATCH_OK_BODY.to_owned(),
            ));
        }
        SidebarBody::Invalid => return Err(Denial::ServerError),
    };
    for item in &items {
        let Value::Object(row) = item else {
            return Err(Denial::ServerError);
        };
        let Some(key) = row.get("key") else {
            continue;
        };
        if !is_python_truthy(key) {
            continue;
        };
        // The lookup has NO user filter (`:88`, ported bug): a member can
        // match another user's row. Non-string keys stringify
        // (`CharField.get_prep_value`) and miss.
        let key_text = py_str(key);
        let found: Option<(Uuid, bool, f64)> = sqlx::query_as(
            r#"SELECT p.id, p.is_pinned, p.sort_order
               FROM workspace_user_preferences p JOIN workspaces w ON w.id = p.workspace_id
               WHERE p."key" = $1 AND w.slug = $2 AND p.deleted_at IS NULL
               ORDER BY p.created_at DESC LIMIT 1"#,
        )
        .bind(&key_text)
        .bind(&slug)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        let Some((id, current_pinned, current_sort)) = found else {
            continue;
        };
        // Raw assignment, converted at `save()`: `is_pinned` preps first
        // (`ValidationError` → 400 valid-detail), then `sort_order`
        // (re-raised `TypeError`/`ValueError` → 500); only then does the
        // write run (`NULL` → `IntegrityError` → 400 payload-not-valid).
        let mut new_pinned: Option<bool> = None;
        let mut new_sort: Option<f64> = None;
        let mut null_write = false;
        if let Some(raw) = row.get("is_pinned") {
            if raw.is_null() {
                null_write = true;
            } else {
                match raw_bool_to_python(raw) {
                    Ok(flag) => new_pinned = Some(flag),
                    Err(()) => return Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned())),
                }
            }
        }
        if let Some(raw) = row.get("sort_order") {
            if raw.is_null() {
                null_write = true;
            } else {
                match raw_float_to_f64(raw) {
                    Ok(float) => new_sort = Some(float),
                    Err(()) => return Err(Denial::ServerError),
                }
            }
        }
        if null_write {
            return Err(Denial::BadError("The payload is not valid".to_owned()));
        }
        // `save(update_fields=["is_pinned", "sort_order"])`: only these
        // two columns move — `updated_at` is NOT stamped.
        sqlx::query(r#"UPDATE workspace_user_preferences SET is_pinned = $1, sort_order = $2 WHERE id = $3"#)
            .bind(new_pinned.unwrap_or(current_pinned))
            .bind(new_sort.unwrap_or(current_sort))
            .bind(id)
            .execute(&pool)
            .await
            .map_err(map_write_error)?;
    }
    Ok(json_response(
        StatusCode::OK,
        SIDEBAR_PATCH_OK_BODY.to_owned(),
    ))
}

// ---------------------------------------------------------------------------
// Recent visits (recent_visit.py:17-36)
// ---------------------------------------------------------------------------

async fn recent_visits_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let gate = gate_for("GET", "workspaces/<slug>/recent-visits/");
    let facts = fetch_allow_facts(&pool, &slug, &user_id, gate_roles(&gate), false).await?;
    check_gate(&gate, &slug, &facts)?;
    // No `Workspace.objects.get` on this path (`:26` filters the slug
    // directly). The `?entity_name=` narrowing applies only when truthy
    // (`:30`); the allowlist clamp applies regardless (`:33`).
    let timezone = actor_timezone(&pool, &user_id).await?;
    let entity = query_last(&query, "entity_name").filter(|name| !name.is_empty());
    let rows: Vec<(Uuid, String, Option<Uuid>, DateTime<Utc>)> = sqlx::query_as(
        r#"SELECT v.id, v.entity_name, v.entity_identifier, v.visited_at
           FROM user_recent_visits v JOIN workspaces w ON w.id = v.workspace_id
           WHERE w.slug = $1 AND v.user_id = $2 AND v.deleted_at IS NULL
           AND ($3::text IS NULL OR v.entity_name = $3)
           AND v.entity_name IN ('issue', 'page', 'project')
           ORDER BY v.created_at DESC LIMIT 20"#,
    )
    .bind(&slug)
    .bind(user_id)
    .bind(entity.as_deref())
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut parts = Vec::with_capacity(rows.len());
    for (id, entity_name, entity_identifier, visited_at) in &rows {
        let fetched = match ser_extras::recent_visit_entity(entity_name) {
            Some(ser_extras::RecentVisitEntity::Issue) => {
                fetch_issue_visit(&pool, entity_identifier).await?
            }
            Some(ser_extras::RecentVisitEntity::Project) => {
                fetch_project_visit(&pool, entity_identifier).await?
            }
            Some(ser_extras::RecentVisitEntity::Page) => {
                fetch_page_visit(&pool, entity_identifier).await?
            }
            None => None,
        };
        let entity_data = ser_extras::recent_visit_entity_data(entity_name, fetched);
        let id = id.to_string();
        let entity_identifier = entity_identifier.map(|id| id.to_string());
        let visited_at = render_dt(visited_at, &timezone)?;
        let shaped = ser_extras::RecentVisitRow {
            id: &id,
            entity_name,
            entity_identifier: entity_identifier.as_deref(),
            entity_data,
            visited_at: &visited_at,
        };
        let view = ser_extras::recent_visit_to_representation(&shaped);
        parts.push(serde_json::to_string(&view).expect("visit view"));
    }
    Ok(json_response(
        StatusCode::OK,
        format!("[{}]", parts.join(",")),
    ))
}

type IssueVisitQueryRow = (
    Uuid,
    String,
    Option<Uuid>,
    Option<String>,
    Option<Uuid>,
    i32,
    Uuid,
);

/// `Issue.objects.get(pk=…)` + `IssueRecentVisitSerializer`
/// (`workspace.py:249-273`): a `NULL` identifier or a missing (or
/// soft-deleted) row renders `entity_data` as `None`.
async fn fetch_issue_visit(
    pool: &PgPool,
    entity_identifier: &Option<Uuid>,
) -> Result<Option<Value>, Denial> {
    let Some(identifier) = entity_identifier else {
        return Ok(None);
    };
    let row: Option<IssueVisitQueryRow> = sqlx::query_as(
        r#"SELECT i.id, i.name, i.state_id, i.priority, i.type_id, i.sequence_id, i.project_id
               FROM issues i WHERE i.id = $1 AND i.deleted_at IS NULL"#,
    )
    .bind(identifier)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, name, state, priority, issue_type, sequence_id, project_id)) = row else {
        return Ok(None);
    };
    // `get_assignees` (`:271-272`): live through-rows, `-created_at`.
    let assignees: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT a.assignee_id FROM issue_assignees a
           WHERE a.issue_id = $1 AND a.deleted_at IS NULL ORDER BY a.created_at DESC"#,
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let assignee_ids: Vec<String> = assignees.iter().map(|row| row.0.to_string()).collect();
    let assignee_refs: Vec<&str> = assignee_ids.iter().map(String::as_str).collect();
    // `get_project_identifier` (`:267-269`): the FK fetch runs through
    // `_base_manager` (unfiltered), so a soft-deleted project still
    // yields its identifier.
    let project_row: Option<(String,)> =
        sqlx::query_as(r#"SELECT p.identifier FROM projects p WHERE p.id = $1"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let project_identifier = project_row.map(|row| row.0);
    let id = id.to_string();
    let state = state.map(|id| id.to_string());
    let issue_type = issue_type.map(|id| id.to_string());
    let project_id = project_id.to_string();
    let shaped = ser_extras::IssueVisitRow {
        id: &id,
        name: &name,
        state: state.as_deref(),
        priority: priority.as_deref(),
        assignees: &assignee_refs,
        issue_type: issue_type.as_deref(),
        sequence_id: i64::from(sequence_id),
        project_id: &project_id,
        project_identifier: project_identifier.as_deref(),
    };
    let view = ser_extras::issue_visit_to_representation(&shaped);
    Ok(Some(serde_json::to_value(&view).expect("issue visit")))
}

/// `Project.objects.get(pk=…)` + `ProjectRecentVisitSerializer`
/// (`workspace.py:275-287`).
async fn fetch_project_visit(
    pool: &PgPool,
    entity_identifier: &Option<Uuid>,
) -> Result<Option<Value>, Denial> {
    let Some(identifier) = entity_identifier else {
        return Ok(None);
    };
    let row: Option<(Uuid, String, Value, String)> = sqlx::query_as(
        r#"SELECT p.id, p.name, p.logo_props, p.identifier FROM projects p
           WHERE p.id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(identifier)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, name, logo_props, identifier)) = row else {
        return Ok(None);
    };
    // `get_project_members` (`:282-287`): non-bot active members over the
    // default (soft-delete scoped) manager, `-created_at`.
    let members: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT pm.member_id FROM project_members pm JOIN users u ON u.id = pm.member_id
           WHERE pm.project_id = $1 AND u.is_bot = FALSE AND pm.is_active
           AND pm.deleted_at IS NULL ORDER BY pm.created_at DESC"#,
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let member_ids: Vec<String> = members.iter().map(|row| row.0.to_string()).collect();
    let member_refs: Vec<&str> = member_ids.iter().map(String::as_str).collect();
    let id = id.to_string();
    let shaped = ser_extras::ProjectVisitRow {
        id: &id,
        name: &name,
        logo_props: &logo_props,
        project_members: &member_refs,
        identifier: &identifier,
    };
    let view = ser_extras::project_visit_to_representation(&shaped);
    Ok(Some(serde_json::to_value(&view).expect("project visit")))
}

/// `Page.objects.get(pk=…)` + `PageRecentVisitSerializer`
/// (`workspace.py:290-311`). The row is never annotated here, so
/// `get_project_id` always takes the first-related-project fallback.
async fn fetch_page_visit(
    pool: &PgPool,
    entity_identifier: &Option<Uuid>,
) -> Result<Option<Value>, Denial> {
    let Some(identifier) = entity_identifier else {
        return Ok(None);
    };
    let row: Option<(Uuid, String, Value, Uuid)> = sqlx::query_as(
        r#"SELECT p.id, p.name, p.logo_props, p.owned_by_id FROM pages p
           WHERE p.id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(identifier)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, name, logo_props, owned_by)) = row else {
        return Ok(None);
    };
    // `obj.projects.first()`: live targets, newest first; the through
    // join carries no soft-delete filter (Django core does not know the
    // through model is soft-deletable).
    let first: Option<(Uuid, String)> = sqlx::query_as(
        r#"SELECT p.id, p.identifier FROM projects p
           JOIN project_pages pp ON pp.project_id = p.id
           WHERE pp.page_id = $1 AND p.deleted_at IS NULL
           ORDER BY p.created_at DESC LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (first_id, first_identifier) = match first {
        Some((first_id, first_identifier)) => (Some(first_id.to_string()), Some(first_identifier)),
        None => (None, None),
    };
    let project_id = ser_extras::page_visit_project_id(None, first_id.as_deref());
    let id = id.to_string();
    let owned_by = owned_by.to_string();
    let shaped = ser_extras::PageVisitRow {
        id: &id,
        name: &name,
        logo_props: &logo_props,
        project_id,
        owned_by: &owned_by,
        project_identifier: first_identifier.as_deref(),
    };
    let view = ser_extras::page_visit_to_representation(&shaped);
    Ok(Some(serde_json::to_value(&view).expect("page visit")))
}

// ---------------------------------------------------------------------------
// User properties (user.py:253-279)
// ---------------------------------------------------------------------------

/// Validated user-properties patch input (`partial=True` always).
struct UserPropsInput {
    deleted_at: Option<Option<DateTime<Utc>>>,
    filters: Option<Value>,
    display_filters: Option<Value>,
    display_properties: Option<Value>,
    rich_filters: Option<Value>,
    navigation_project_limit: Option<UserPropsLimit>,
    navigation_control_preference: Option<String>,
    created_by: Option<Option<Uuid>>,
    updated_by: Option<Option<Uuid>>,
}

/// An `IntegerField` outcome: a value, or a magnitude that validates but
/// dies at the database (Python ints are unbounded).
enum UserPropsLimit {
    Value(i32),
    TooBig,
}

async fn validate_user_props_body(
    pool: &PgPool,
    body: &Map<String, Value>,
    timezone: &Tz,
) -> Result<UserPropsInput, WriteInvalid> {
    let mut errors: FieldErrors = Vec::new();
    let mut deleted_at = None;
    let mut filters = None;
    let mut display_filters = None;
    let mut display_properties = None;
    let mut rich_filters = None;
    let mut navigation_project_limit = None;
    let mut navigation_control_preference = None;
    let mut created_by: Option<Option<Uuid>> = None;
    let mut updated_by: Option<Option<Uuid>> = None;

    if let Some(value) = body.get("deleted_at") {
        if value.is_null() {
            deleted_at = Some(None);
        } else {
            match drf_datetime_to_internal(value, timezone) {
                DtCheck::Value(dt) => deleted_at = Some(Some(dt)),
                DtCheck::Invalid => {
                    push_field_msg(&mut errors, "deleted_at", datetime_invalid_msg())
                }
                DtCheck::MakeAware => {
                    push_field_msg(&mut errors, "deleted_at", make_aware_msg(timezone));
                }
                DtCheck::Overflow => {
                    push_field_msg(
                        &mut errors,
                        "deleted_at",
                        "Datetime value out of range.".to_owned(),
                    );
                }
            }
        }
    }
    for field in [
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
    ] {
        let Some(value) = body.get(field) else {
            continue;
        };
        if value.is_null() {
            push_field_msg(&mut errors, field, NULL_MSG.to_owned());
            continue;
        }
        match field {
            "filters" => filters = Some(value.clone()),
            "display_filters" => display_filters = Some(value.clone()),
            "display_properties" => display_properties = Some(value.clone()),
            _ => rich_filters = Some(value.clone()),
        }
    }
    if let Some(value) = body.get("navigation_project_limit") {
        if value.is_null() {
            push_field_msg(&mut errors, "navigation_project_limit", NULL_MSG.to_owned());
        } else {
            match drf_int_to_internal(value) {
                IntCheck::Value(int) => match i32::try_from(int) {
                    Ok(limit) => navigation_project_limit = Some(UserPropsLimit::Value(limit)),
                    Err(_) => navigation_project_limit = Some(UserPropsLimit::TooBig),
                },
                IntCheck::TooBig => navigation_project_limit = Some(UserPropsLimit::TooBig),
                IntCheck::Invalid => {
                    push_field_msg(&mut errors, "navigation_project_limit", INT_MSG.to_owned());
                }
                IntCheck::TooLarge => {
                    push_field_msg(
                        &mut errors,
                        "navigation_project_limit",
                        TOO_LARGE_MSG.to_owned(),
                    );
                }
            }
        }
    }
    if let Some(value) = body.get("navigation_control_preference") {
        if value.is_null() {
            push_field_msg(
                &mut errors,
                "navigation_control_preference",
                NULL_MSG.to_owned(),
            );
        } else if matches!(value, Value::String(text) if text.is_empty()) {
            push_field_msg(
                &mut errors,
                "navigation_control_preference",
                BLANK_MSG.to_owned(),
            );
        } else {
            // A `ChoiceField` (the model declares `choices=`): `str(data)`
            // looked up verbatim — no trimming, no length check.
            let choices = [
                NavigationControlPreference::Accordion.as_str(),
                NavigationControlPreference::Tabbed.as_str(),
            ];
            match check_choice(value, &choices) {
                Ok(choice) => navigation_control_preference = Some(choice.to_owned()),
                Err(message) => {
                    push_field_msg(&mut errors, "navigation_control_preference", message);
                }
            }
        }
    }
    for field in ["created_by", "updated_by"] {
        let Some(value) = body.get(field) else {
            continue;
        };
        if value.is_null() {
            match field {
                "created_by" => created_by = Some(None),
                _ => updated_by = Some(None),
            }
            continue;
        }
        if matches!(value, Value::String(text) if text.is_empty()) {
            push_field_msg(&mut errors, field, BLANK_MSG.to_owned());
            continue;
        }
        let id = match check_pk_shape(value) {
            PkCheck::Uuid(id) => id,
            PkCheck::Blank => {
                push_field_msg(&mut errors, field, BLANK_MSG.to_owned());
                continue;
            }
            PkCheck::IncorrectType(dtype) => {
                push_field_msg(&mut errors, field, pk_incorrect_type_msg(&dtype));
                continue;
            }
            PkCheck::PayloadNotValid => return Err(WriteInvalid::PayloadNotValid),
            PkCheck::ServerError => return Err(WriteInvalid::ServerError),
        };
        let row: Option<(Uuid,)> = sqlx::query_as(r#"SELECT u.id FROM users u WHERE u.id = $1"#)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| WriteInvalid::ServerError)?;
        if row.is_none() {
            push_field_msg(&mut errors, field, pk_does_not_exist_msg(value));
        } else {
            match field {
                "created_by" => created_by = Some(Some(id)),
                _ => updated_by = Some(Some(id)),
            }
        }
    }
    if !errors.is_empty() {
        return Err(WriteInvalid::Fields(errors));
    }
    Ok(UserPropsInput {
        deleted_at,
        filters,
        display_filters,
        display_properties,
        rich_filters,
        navigation_project_limit,
        navigation_control_preference,
        created_by,
        updated_by,
    })
}

type UserPropsRow = (
    Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Value,
    Value,
    Value,
    Value,
    i32,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Uuid,
    Uuid,
);

fn render_user_props(row: &UserPropsRow, timezone: &Tz) -> Result<String, Denial> {
    let (
        id,
        created_at,
        updated_at,
        deleted_at,
        filters,
        display_filters,
        display_properties,
        rich_filters,
        navigation_project_limit,
        navigation_control_preference,
        created_by,
        updated_by,
        workspace,
        user,
    ) = row;
    let id = id.to_string();
    let created_at = render_dt(created_at, timezone)?;
    let updated_at = render_dt(updated_at, timezone)?;
    let deleted_at = match deleted_at {
        Some(dt) => Some(render_dt(dt, timezone)?),
        None => None,
    };
    let created_by = created_by.map(|id| id.to_string());
    let updated_by = updated_by.map(|id| id.to_string());
    let workspace = workspace.to_string();
    let user = user.to_string();
    let shaped = ser_invite::UserPropertiesRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        filters,
        display_filters,
        display_properties,
        rich_filters,
        navigation_project_limit: *navigation_project_limit,
        navigation_control_preference,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        workspace: &workspace,
        user: &user,
    };
    let view = ser_invite::user_properties_to_representation(&shaped);
    Ok(serde_json::to_string(&view).expect("props view"))
}

const USER_PROPS_SELECT: &str = "p.id, p.created_at, p.updated_at, p.deleted_at, p.filters, p.display_filters, p.display_properties, p.rich_filters, p.navigation_project_limit, p.navigation_control_preference, p.created_by_id, p.updated_by_id, p.workspace_id, p.user_id";

/// `WorkspaceUserProperties.objects.get_or_create(user, workspace)`:
/// the live row, or a fresh insert with the Django-side defaults
/// (`models_prefs::workspace_user_properties`). `ON CONFLICT DO NOTHING`
/// plus a re-read is the net effect of the create-or-retry race path.
async fn get_or_create_user_props(
    pool: &PgPool,
    user_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<UserPropsRow, Denial> {
    let row: Option<UserPropsRow> = sqlx::query_as(&format!(
        r#"SELECT {USER_PROPS_SELECT} FROM workspace_user_properties p
           WHERE p.user_id = $1 AND p.workspace_id = $2 AND p.deleted_at IS NULL LIMIT 1"#,
    ))
    .bind(user_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if let Some(row) = row {
        return Ok(row);
    }
    let id = Uuid::new_v4();
    let created_at = Utc::now();
    let updated_at = Utc::now();
    let filters = user_props_model::default_filters();
    let display_filters = user_props_model::default_display_filters();
    let display_properties = user_props_model::default_display_properties();
    sqlx::query(
        r#"INSERT INTO workspace_user_properties
           (id, created_at, updated_at, workspace_id, user_id, filters, display_filters,
            display_properties, rich_filters, navigation_project_limit,
            navigation_control_preference, created_by_id, updated_by_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
           ON CONFLICT (workspace_id, user_id) WHERE deleted_at IS NULL DO NOTHING"#,
    )
    .bind(id)
    .bind(created_at)
    .bind(updated_at)
    .bind(workspace_id)
    .bind(user_id)
    .bind(&filters)
    .bind(&display_filters)
    .bind(&display_properties)
    .bind(Value::Object(Map::new()))
    .bind(user_props_model::DEFAULT_NAVIGATION_PROJECT_LIMIT)
    .bind(user_props_model::DEFAULT_NAVIGATION_CONTROL_PREFERENCE)
    .bind(user_id)
    .bind(Option::<Uuid>::None)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row: Option<UserPropsRow> = sqlx::query_as(&format!(
        r#"SELECT {USER_PROPS_SELECT} FROM workspace_user_properties p
           WHERE p.user_id = $1 AND p.workspace_id = $2 AND p.deleted_at IS NULL LIMIT 1"#,
    ))
    .bind(user_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::ServerError)
}

async fn user_props_get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let facts = fetch_viewer_facts(&pool, &slug, &user_id).await?;
    match check_viewer_gate(&slug, &facts) {
        ViewerGate::Allow => {}
        ViewerGate::DenyClass => {
            return Ok(json_response(
                StatusCode::FORBIDDEN,
                gates::CLASS_DENIED_BODY.to_owned(),
            ));
        }
        ViewerGate::Unauthorized => return Err(Denial::Unauthorized),
    }
    let workspace_id = resolve_workspace_id(&pool, &slug).await?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let row = get_or_create_user_props(&pool, &user_id, &workspace_id).await?;
    Ok(json_response(
        StatusCode::OK,
        render_user_props(&row, &timezone)?,
    ))
}

async fn user_props_patch(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let facts = fetch_viewer_facts(&pool, &slug, &user_id).await?;
    match check_viewer_gate(&slug, &facts) {
        ViewerGate::Allow => {}
        ViewerGate::DenyClass => {
            return Ok(json_response(
                StatusCode::FORBIDDEN,
                gates::CLASS_DENIED_BODY.to_owned(),
            ));
        }
        ViewerGate::Unauthorized => return Err(Denial::Unauthorized),
    }
    let workspace_id = resolve_workspace_id(&pool, &slug).await?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let current = get_or_create_user_props(&pool, &user_id, &workspace_id).await?;
    let data = match parse_body_object(&body) {
        Ok(data) => data,
        Err(denial) => return Err(denial),
    };
    let input = match validate_user_props_body(&pool, &data, &timezone).await {
        Ok(input) => input,
        Err(invalid) => return Ok(invalid.into_response()),
    };
    // Validated, then overwritten (`updated_by=user`).
    let _ = input.updated_by;
    // An out-of-`i32` limit validates (unbounded Python ints) and dies at
    // the database instead.
    let navigation_project_limit = match input.navigation_project_limit {
        None => current.8,
        Some(UserPropsLimit::Value(limit)) => limit,
        Some(UserPropsLimit::TooBig) => return Err(Denial::ServerError),
    };
    let deleted_at = input.deleted_at.unwrap_or(current.3);
    let filters = input.filters.unwrap_or(current.4);
    let display_filters = input.display_filters.unwrap_or(current.5);
    let display_properties = input.display_properties.unwrap_or(current.6);
    let rich_filters = input.rich_filters.unwrap_or(current.7);
    let navigation_control_preference = input.navigation_control_preference.unwrap_or(current.9);
    let created_by = input.created_by.unwrap_or(current.10);
    let updated_at = Utc::now();
    sqlx::query(
        r#"UPDATE workspace_user_properties
           SET deleted_at = $1, filters = $2, display_filters = $3, display_properties = $4,
               rich_filters = $5, navigation_project_limit = $6,
               navigation_control_preference = $7, created_by_id = $8,
               updated_by_id = $9, updated_at = $10
           WHERE id = $11"#,
    )
    .bind(deleted_at)
    .bind(&filters)
    .bind(&display_filters)
    .bind(&display_properties)
    .bind(&rich_filters)
    .bind(navigation_project_limit)
    .bind(&navigation_control_preference)
    .bind(created_by)
    .bind(user_id)
    .bind(updated_at)
    .bind(current.0)
    .execute(&pool)
    .await
    .map_err(map_write_error)?;
    let row: UserPropsRow = (
        current.0,
        current.1,
        updated_at,
        deleted_at,
        filters,
        display_filters,
        display_properties,
        rich_filters,
        navigation_project_limit,
        navigation_control_preference,
        created_by,
        Some(user_id),
        current.12,
        current.13,
    );
    Ok(json_response(
        StatusCode::OK,
        render_user_props(&row, &timezone)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture_routes() -> Value {
        let path = format!(
            "{}/../../fixtures/app_workspace/handlers/routes.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("routes fixture");
        serde_json::from_str(&text).expect("routes json")
    }

    /// F-W24-15: the eight owned paths match the route table entries
    /// (Django patterns, `<slug>`/`<pk>`/`<key>`).
    #[test]
    fn routes_fixture_matches_owned_paths() {
        let fixture = fixture_routes();
        let routes = fixture["routes"].as_array().expect("routes list");
        let texts: Vec<&str> = routes
            .iter()
            .map(|entry| entry.as_str().expect("route string"))
            .collect();
        // (W-id, documented methods, axum path const).
        let owned = [
            ("W27", "GET+PATCH", USER_PROPS_PATH),
            ("W38", "GET+POST", QUICK_LINKS_PATH),
            ("W39", "GET+PATCH+DELETE", QUICK_LINK_PATH),
            ("W40", "GET", HOME_PREFS_PATH),
            ("W41", "get+patch", HOME_PREF_KEY_PATH),
            ("W42", "GET", RECENT_VISITS_PATH),
            ("W43", "GET+POST", STICKIES_PATH),
            ("W44", "GET+PATCH+DELETE", STICKY_PATH),
            ("W45", "GET+PATCH", SIDEBAR_PREFS_PATH),
        ];
        for (id, methods, path) in owned {
            let entry = texts
                .iter()
                .find(|text| text.starts_with(id))
                .unwrap_or_else(|| panic!("{id}"));
            assert!(
                entry.contains(methods),
                "{id}: {entry} should document {methods}"
            );
            let django_path = path
                .trim_start_matches("/api/")
                .replace("{slug}", "<slug>")
                .replace("{pk}", "<pk>")
                .replace("{key}", "<key>");
            assert!(
                entry.contains(&django_path),
                "{id}: {entry} should contain {django_path}"
            );
        }
        // The un-dispatched home methods (PATCH without `<key>`, GET with
        // `<key>`) die with `TypeError` → 500; the fixture records the
        // W40 half as a bug, ported by the mismatch handlers.
        let bugs = fixture["bugs"].as_array().expect("bugs list");
        assert!(
            bugs.iter()
                .any(|bug| bug.to_string().contains("W40 PATCH without <key>")),
            "W40 kwarg-mismatch bug entry"
        );
    }

    /// F-W24-15 errors #25/#26/#27: the exact status + body bytes.
    #[test]
    fn fixture_errors_match_bodies() {
        let fixture = fixture_routes();
        let errors = fixture["errors"].as_array().expect("errors list");
        let find = |source: &str| {
            errors
                .iter()
                .find(|entry| entry["source"] == source)
                .unwrap_or_else(|| panic!("{source}"))
        };
        let patch_404 = find("quick_link.py:43");
        assert_eq!(patch_404["status"], 404);
        assert_eq!(patch_404["body"].to_string(), QUICK_LINK_PATCH_MISSING_BODY);
        let retrieve_404 = find("quick_link.py:51-52");
        assert_eq!(retrieve_404["status"], 404);
        assert_eq!(
            retrieve_404["body"].to_string(),
            QUICK_LINK_RETRIEVE_MISSING_BODY
        );
        let home_400 = find("home.py:79");
        assert_eq!(home_400["status"], 400);
        assert_eq!(home_400["body"].to_string(), HOME_PREF_PATCH_MISSING_BODY);
        let sidebar_ok = SIDEBAR_PATCH_OK_BODY;
        assert_eq!(sidebar_ok, r#"{"message":"Successfully updated"}"#);
    }

    #[test]
    fn char_ladder() {
        assert_eq!(
            drf_char_to_internal(&json!("  padded  ")).unwrap(),
            "padded"
        );
        assert_eq!(drf_char_to_internal(&json!(5)).unwrap(), "5");
        assert_eq!(drf_char_to_internal(&json!(4.5)).unwrap(), "4.5");
        assert!(drf_char_to_internal(&json!(true)).is_err());
        assert!(drf_char_to_internal(&json!([1])).is_err());
        assert!(drf_char_to_internal(&json!({"a": 1})).is_err());
        assert_eq!(
            max_length_msg(255),
            "Ensure this field has no more than 255 characters."
        );
    }

    #[test]
    fn bool_ladder() {
        // (`in TRUE_VALUES` / `in FALSE_VALUES`, numeric `==` included.)
        for truthy in [
            json!(true),
            json!(1),
            json!(1.0),
            json!("t"),
            json!("TRUE"),
            json!("Yes"),
            json!("on"),
            json!("1"),
        ] {
            assert_eq!(drf_bool_to_internal(&truthy), Ok(true), "{truthy}");
        }
        for falsy in [
            json!(false),
            json!(0),
            json!(0.0),
            json!("f"),
            json!("FALSE"),
            json!("No"),
            json!("off"),
            json!("0"),
        ] {
            assert_eq!(drf_bool_to_internal(&falsy), Ok(false), "{falsy}");
        }
        for invalid in [
            json!(2),
            json!(0.5),
            json!("yes please"),
            json!(""),
            json!("  true  "),
            json!([1]),
            json!({"a": 1}),
        ] {
            assert!(drf_bool_to_internal(&invalid).is_err(), "{invalid}");
        }
        assert_eq!(BOOL_MSG, "Must be a valid boolean.");
    }

    #[test]
    fn int_ladder() {
        let value = |input: &Value| match drf_int_to_internal(input) {
            IntCheck::Value(int) => int.to_string(),
            IntCheck::TooBig => "big".to_owned(),
            IntCheck::Invalid => "invalid".to_owned(),
            IntCheck::TooLarge => "too-large".to_owned(),
        };
        assert_eq!(value(&json!("1.0")), "1");
        assert_eq!(value(&json!("1.000 ")), "1");
        assert_eq!(value(&json!(1.0)), "1");
        assert_eq!(value(&json!("1.2")), "invalid");
        assert_eq!(value(&json!(1.5)), "invalid");
        assert_eq!(value(&json!(true)), "invalid");
        assert_eq!(value(&json!(" 10 ")), "10");
        assert_eq!(value(&json!("1_0")), "10");
        assert_eq!(value(&json!("1__0")), "invalid");
        assert_eq!(value(&json!("+5")), "5");
        assert_eq!(value(&json!("0x10")), "invalid");
        assert_eq!(value(&json!("9".repeat(40))), "big");
        assert_eq!(value(&json!("1".repeat(1001))), "too-large");
        assert_eq!(value(&json!("1".repeat(1000))), "big");
        assert_eq!(value(&json!([1])), "invalid");
        assert_eq!(INT_MSG, "A valid integer is required.");
        assert_eq!(TOO_LARGE_MSG, "String value too large.");
    }

    #[test]
    fn float_ladder() {
        let value = |input: &Value| match drf_float_to_internal(input) {
            FloatCheck::Value(float) => format!("{float:?}"),
            FloatCheck::Invalid => "invalid".to_owned(),
            FloatCheck::TooLarge => "too-large".to_owned(),
            FloatCheck::Overflow => "overflow".to_owned(),
        };
        assert_eq!(value(&json!(true)), "1.0");
        assert_eq!(value(&json!(false)), "0.0");
        assert_eq!(value(&json!("nan")), "NaN");
        assert_eq!(value(&json!("+INF")), "inf");
        assert_eq!(value(&json!("1_000.5")), "1000.5");
        assert_eq!(value(&json!("1__0")), "invalid");
        assert_eq!(value(&json!(".5e3")), "500.0");
        assert_eq!(value(&json!("1.")), "1.0");
        let inf_exp: Value = serde_json::from_str("1e1000").unwrap();
        assert_eq!(value(&inf_exp), "inf");
        assert_eq!(value(&json!("x".repeat(1001))), "too-large");
        let huge: Value = serde_json::from_str(&"9".repeat(60)).unwrap();
        assert_eq!(value(&huge), "overflow");
        assert_eq!(value(&json!([1.0])), "invalid");
        assert_eq!(FLOAT_MSG, "A valid number is required.");
        assert_eq!(
            INT_TO_FLOAT_MSG,
            "Integer value too large to convert to float"
        );
    }

    /// The `parse_datetime` probe table (CPython 3.12 + Django 4.2):
    /// accepted inputs with their UTC instant, rejected inputs as `None`.
    #[test]
    fn datetime_probe_table() {
        let utc: Tz = "UTC".parse().unwrap();
        let instant = |text: &str| match drf_datetime_to_internal(&json!(text), &utc) {
            DtCheck::Value(dt) => dt.to_string(),
            DtCheck::Invalid => "invalid".to_owned(),
            DtCheck::MakeAware => "make-aware".to_owned(),
            DtCheck::Overflow => "overflow".to_owned(),
        };
        // Accepted (wall time == UTC instant under the UTC zone).
        assert_eq!(instant("2024-01-01T10:00:00"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-1-1T1:2"), "2024-01-01 01:02:00 UTC");
        assert_eq!(instant("2024-01-01 10:00:00"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01X10:00"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("20240101T100000"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01"), "2024-01-01 00:00:00 UTC");
        assert_eq!(instant("20240101"), "2024-01-01 00:00:00 UTC");
        assert_eq!(instant("2024-01-01T10:00"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01T10"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01T100000"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01T10:00:00Z"), "2024-01-01 10:00:00 UTC");
        assert_eq!(
            instant("2024-01-01T10:00:00+05:30"),
            "2024-01-01 04:30:00 UTC"
        );
        assert_eq!(
            instant("2024-01-01T10:00:00+0530"),
            "2024-01-01 04:30:00 UTC"
        );
        assert_eq!(instant("2024-01-01T10:00:00+05"), "2024-01-01 05:00:00 UTC");
        assert_eq!(
            instant("2024-01-01T10:00:00+00:61"),
            "2024-01-01 08:59:00 UTC"
        );
        assert_eq!(
            instant("2024-01-01T10:00:00+2359"),
            "2023-12-31 10:01:00 UTC"
        );
        assert_eq!(
            instant("2024-01-01T10:00:00-00:00"),
            "2024-01-01 10:00:00 UTC"
        );
        assert_eq!(
            instant("2024-01-01T10:00:00.123456789"),
            "2024-01-01 10:00:00.123456 UTC"
        );
        assert_eq!(
            instant("2024-01-01T10:00:00,5"),
            "2024-01-01 10:00:00.500 UTC"
        );
        assert_eq!(instant("2024-01-01t10:00:00"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01T10:00:00 "), "2024-01-01 10:00:00 UTC");
        assert_eq!(
            instant("2024-01-01T10:00:00.5+05:30"),
            "2024-01-01 04:30:00.500 UTC"
        );
        assert_eq!(instant("20240101T100000Z"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01T10Z"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-01-01T10+05:00"), "2024-01-01 05:00:00 UTC");
        assert_eq!(
            instant("2024-01-01T10:00:00 +05:00"),
            "2024-01-01 05:00:00 UTC"
        );
        assert_eq!(
            instant("2024-01-01 10:00:00 +05:00"),
            "2024-01-01 05:00:00 UTC"
        );
        assert_eq!(instant("2024-01-01T1:2:3.4"), "2024-01-01 01:02:03.400 UTC");
        assert_eq!(instant("2024-W01-1T10:00:00"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("2024-W01-1"), "2024-01-01 00:00:00 UTC");
        assert_eq!(instant("2024W011T100000"), "2024-01-01 10:00:00 UTC");
        assert_eq!(instant("0001-01-01T00:00:00"), "0001-01-01 00:00:00 UTC");
        assert_eq!(
            instant("9999-12-31T23:59:59.999999"),
            "9999-12-31 23:59:59.999999 UTC"
        );
        // Rejected.
        for bad in [
            "2024-01-01T10:00:00+5:30",
            "2024-01-01T10:00GARBAGE",
            "2024-01-01T10:00:00+05:30junk",
            "2024-01-01T10:00:00Zjunk",
            "2024-01-01T10:00:00+24:00",
            "2024-01-01T10:00:60",
            "2024-02-30T10:00:00",
            "2024-13-01T10:00",
            "2024-01-01T25:00",
            "2024-01-01T24:00",
            "2024-01-01XX10:00",
            "2024-01-0110:00",
            "2024-01-01Z",
            "2024-01-01t10:00:00z",
            " 2024-01-01T10:00:00",
            "2024-01-01T10:00:00.",
            "2024-01-01T10:00:00,",
            "2024-01-01T 10:00:00",
            "T10:00:00",
            "10:00:00",
            "",
            "2024-1-1",
            "2024-123",
            "2024-123T10:00:00",
            "2024-01-01T10:00:00+053045x",
            "0000-01-01",
            "+2024-01-01T10:00:00",
            "24-01-01T10:00:00",
        ] {
            assert_eq!(instant(bad), "invalid", "{bad}");
        }
        assert!(matches!(
            drf_datetime_to_internal(&json!(5), &utc),
            DtCheck::Invalid
        ));
        assert_eq!(
            datetime_invalid_msg(),
            "Datetime has wrong format. Use one of these formats instead: \
             YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        );
    }

    #[test]
    fn datetime_zones_and_make_aware() {
        let york: Tz = "America/New_York".parse().unwrap();
        // Naive wall time attaches the actor zone (`make_aware`).
        match drf_datetime_to_internal(&json!("2024-01-01T10:00:00"), &york) {
            DtCheck::Value(dt) => assert_eq!(dt.to_string(), "2024-01-01 15:00:00 UTC"),
            other => panic!("expected value, got {other:?}"),
        }
        // The spring-forward gap fails `make_aware`.
        match drf_datetime_to_internal(&json!("2024-03-10T02:30:00"), &york) {
            DtCheck::MakeAware => {}
            other => panic!("expected make-aware, got {other:?}"),
        }
        assert_eq!(
            make_aware_msg(&york),
            "Invalid datetime for the timezone \"America/New_York\"."
        );
        // The fall-back fold takes the first side (EDT).
        match drf_datetime_to_internal(&json!("2024-11-03T01:30:00"), &york) {
            DtCheck::Value(dt) => assert_eq!(dt.to_string(), "2024-11-03 05:30:00 UTC"),
            other => panic!("expected value, got {other:?}"),
        }
        // Aware shifts past year 9999 are the `overflow` branch.
        let kiribati: Tz = "Pacific/Kiritimati".parse().unwrap();
        match drf_datetime_to_internal(&json!("9999-12-31T20:00:00Z"), &kiribati) {
            DtCheck::Overflow => {}
            other => panic!("expected overflow, got {other:?}"),
        }
    }

    #[test]
    fn pk_ladder() {
        let shape = |input: &Value| match check_pk_shape(input) {
            PkCheck::Uuid(id) => format!("uuid:{id}"),
            PkCheck::Blank => "blank".to_owned(),
            PkCheck::IncorrectType(dtype) => format!("type:{dtype}"),
            PkCheck::PayloadNotValid => "payload".to_owned(),
            PkCheck::ServerError => "server".to_owned(),
        };
        assert_eq!(
            shape(&json!("12345678-1234-5678-1234-567812345678")),
            "uuid:12345678-1234-5678-1234-567812345678"
        );
        assert_eq!(shape(&json!("xyz")), "payload");
        assert_eq!(shape(&json!(true)), "type:bool");
        assert_eq!(shape(&json!("")), "blank");
        assert_eq!(shape(&json!(0)), "server");
        assert_eq!(
            shape(&json!(5)),
            "uuid:00000000-0000-0000-0000-000000000005"
        );
        assert_eq!(shape(&json!(-1)), "type:int");
        let huge: Value = serde_json::from_str(&"9".repeat(40)).unwrap();
        assert_eq!(shape(&huge), "type:int");
        assert_eq!(shape(&json!(4.5)), "server");
        assert_eq!(shape(&json!([1])), "server");
        assert_eq!(shape(&json!({"a": 1})), "server");
        assert_eq!(
            pk_does_not_exist_msg(&json!("abc")),
            "Invalid pk \"abc\" - object does not exist."
        );
        assert_eq!(
            pk_does_not_exist_msg(&json!(5)),
            "Invalid pk \"5\" - object does not exist."
        );
        assert_eq!(
            pk_incorrect_type_msg("bool"),
            "Incorrect type. Expected pk value, received bool."
        );
    }

    #[test]
    fn choice_ladder() {
        let choices = ["ACCORDION", "TABBED"];
        assert_eq!(check_choice(&json!("ACCORDION"), &choices), Ok("ACCORDION"));
        assert_eq!(
            check_choice(&json!("accordion"), &choices),
            Err("\"accordion\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            check_choice(&json!(5), &choices),
            Err("\"5\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            check_choice(&json!(true), &choices),
            Err("\"True\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            check_choice(&json!(" ACCORDION"), &choices),
            Err("\" ACCORDION\" is not a valid choice.".to_owned())
        );
    }

    #[test]
    fn raw_bool_ladder() {
        // `BooleanField.to_python`: exact strings, numeric `==`.
        for (input, expected) in [
            (json!(true), true),
            (json!(1), true),
            (json!(1.0), true),
            (json!("t"), true),
            (json!("True"), true),
            (json!("1"), true),
            (json!(false), false),
            (json!(0), false),
            (json!(0.0), false),
            (json!("f"), false),
            (json!("False"), false),
            (json!("0"), false),
        ] {
            assert_eq!(raw_bool_to_python(&input), Ok(expected), "{input}");
        }
        for invalid in [
            json!("false"),
            json!("TRUE"),
            json!("yes"),
            json!(""),
            json!(2),
            json!([]),
            json!({}),
        ] {
            assert!(raw_bool_to_python(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn raw_float_ladder() {
        assert_eq!(raw_float_to_f64(&json!(true)), Ok(1.0));
        assert!(raw_float_to_f64(&json!("nan")).unwrap().is_nan());
        let inf_exp: Value = serde_json::from_str("1e1000").unwrap();
        assert_eq!(raw_float_to_f64(&inf_exp), Ok(f64::INFINITY));
        let huge: Value = serde_json::from_str(&"9".repeat(60)).unwrap();
        assert!(raw_float_to_f64(&huge).is_err());
        for invalid in [json!("abc"), json!([1.0]), json!({"a": 1})] {
            assert!(raw_float_to_f64(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn python_str_shapes() {
        assert_eq!(py_str(&json!(null)), "None");
        assert_eq!(py_str(&json!(true)), "True");
        assert_eq!(py_str(&json!(5)), "5");
        assert_eq!(py_str(&json!(4.5)), "4.5");
        assert_eq!(py_str(&json!("x")), "x");
        assert_eq!(py_str(&json!([1, "a"])), "[1, 'a']");
        assert_eq!(py_str(&json!({"k": 1})), "{'k': 1}");
        assert_eq!(py_repr_string("a'b"), "\"a'b\"");
        assert_eq!(py_repr_string("say \"hi\""), "'say \"hi\"'");
        assert_eq!(py_repr_string("a\nb"), "'a\\nb'");
        assert!(!is_python_truthy(&json!(null)));
        assert!(!is_python_truthy(&json!(0)));
        assert!(!is_python_truthy(&json!("")));
        assert!(!is_python_truthy(&json!([])));
        assert!(is_python_truthy(&json!("false")));
        assert!(is_python_truthy(&json!(0.5)));
    }

    #[test]
    fn json_float_renders_like_cpython() {
        assert_eq!(render_json_float(999.0), "999.0");
        assert_eq!(render_json_float(65535.0), "65535.0");
        assert_eq!(render_json_float(0.0), "0.0");
        assert_eq!(render_json_float(-0.0), "-0.0");
        assert_eq!(render_json_float(1e300), "1e+300");
        assert_eq!(render_json_float(1e-5), "1e-05");
        assert_eq!(render_json_float(1e16), "1e+16");
        assert_eq!(render_json_float(1e15), "1000000000000000.0");
        assert_eq!(render_json_float(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(render_json_float(5e-324), "5e-324");
        assert_eq!(render_json_float(1.7976931348623157e308), "1.7976931348623157e+308");
        assert_eq!(render_json_float(2.5e-7), "2.5e-07");
        assert_eq!(render_json_float(1e21), "1e+21");
        assert_eq!(render_json_float(123456789012345680.0), "1.2345678901234568e+17");
        assert_eq!(render_json_float(1.23456789012345), "1.23456789012345");
        assert_eq!(render_json_float(123456.789), "123456.789");
        assert_eq!(render_json_float(0.0001), "0.0001");
        assert_eq!(render_json_float(f64::NAN), "NaN");
        assert_eq!(render_json_float(f64::INFINITY), "Infinity");
        assert_eq!(render_json_float(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn field_errors_render_in_order() {
        let errors: FieldErrors = vec![
            (
                "title".to_owned(),
                json!(["Ensure this field has no more than 255 characters."]),
            ),
            ("url".to_owned(), json!({"error": "Invalid URL format."})),
        ];
        assert_eq!(
            render_field_errors(&errors),
            r#"{"title":["Ensure this field has no more than 255 characters."],"url":{"error":"Invalid URL format."}}"#
        );
    }

    #[test]
    fn home_pref_validation() {
        // Empty patch validates (partial); unknown keys are ignored.
        let body: Map<String, Value> = serde_json::from_str(r#"{"config":{"x":1}}"#).unwrap();
        let input = validate_home_pref_body(&body).unwrap();
        assert!(input.key.is_none() && input.is_enabled.is_none() && input.sort_order.is_none());
        let body: Map<String, Value> =
            serde_json::from_str(r#"{"is_enabled":false,"sort_order":999}"#).unwrap();
        let input = validate_home_pref_body(&body).unwrap();
        assert_eq!(input.is_enabled, Some(false));
        assert_eq!(input.sort_order, Some(999.0));
        // Errors render in declaration order, not input order.
        let body: Map<String, Value> =
            serde_json::from_str(r#"{"sort_order":"x","key":null}"#).unwrap();
        let errors = validate_home_pref_body(&body).unwrap_err();
        assert_eq!(
            render_field_errors(&errors),
            r#"{"key":["This field may not be null."],"sort_order":["A valid number is required."]}"#
        );
        // Numerics coerce to key strings.
        let body: Map<String, Value> = serde_json::from_str(r#"{"key":5}"#).unwrap();
        assert_eq!(
            validate_home_pref_body(&body).unwrap().key,
            Some("5".to_owned())
        );
    }

    #[test]
    fn home_and_sidebar_shapes() {
        assert_eq!(
            render_home_pref_row("my_stickies", true, &json!({}), 997.0),
            r#"{"key":"my_stickies","is_enabled":true,"config":{},"sort_order":997.0}"#
        );
        assert_eq!(
            render_home_pref_patch("recents", false, 998.0),
            r#"{"key":"recents","is_enabled":false,"sort_order":998.0}"#
        );
        let rows = vec![
            ("views".to_owned(), false, 65535.0),
            ("drafts".to_owned(), true, 75535.0),
        ];
        assert_eq!(
            render_sidebar_prefs(&rows),
            r#"{"views":{"is_pinned":false,"sort_order":65535.0},"drafts":{"is_pinned":true,"sort_order":75535.0}}"#
        );
    }

    #[test]
    fn sticky_renders_all_seventeen_keys_in_order() {
        let utc: Tz = "UTC".parse().unwrap();
        let row = StickyRow {
            id: Uuid::nil(),
            created_at: "2024-05-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            updated_at: "2024-05-02T10:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            deleted_at: None,
            name: Some("S1".to_owned()),
            description: json!({"a": 1}),
            description_html: "<p>x</p>".to_owned(),
            description_stripped: Some("x".to_owned()),
            description_binary: Some(vec![1, 2, 3]),
            logo_props: json!({}),
            color: None,
            background_color: Some("#fff".to_owned()),
            sort_order: 75535.0,
            created_by_id: Some(Uuid::nil()),
            updated_by_id: None,
            workspace_id: Uuid::nil(),
            owner_id: Uuid::nil(),
        };
        let rendered = render_sticky(&row, &utc).unwrap();
        let value: Value = serde_json::from_str(&rendered).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_extras::STICKY_WIRE_FIELDS);
        assert!(rendered.contains(r#""description_binary":"AQID""#));
        assert!(rendered.contains(r#""sort_order":75535.0"#));
        assert!(rendered.contains(r#""created_at":"2024-05-01T10:00:00Z""#));
    }

    #[test]
    fn link_and_props_render_in_wire_order() {
        let utc: Tz = "UTC".parse().unwrap();
        let link: QuickLinkRow = (
            Uuid::nil(),
            "2024-05-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            "2024-05-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            None,
            Some("T".to_owned()),
            "https://x.test".to_owned(),
            json!({}),
            Some(Uuid::nil()),
            None,
            Uuid::nil(),
            None,
            Uuid::nil(),
        );
        let rendered = render_quick_link(&link, &utc).unwrap();
        let value: Value = serde_json::from_str(&rendered).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_extras::USER_LINK_WIRE_FIELDS);
        let props: UserPropsRow = (
            Uuid::nil(),
            "2024-05-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            "2024-05-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            None,
            json!({}),
            json!({}),
            json!({}),
            json!({}),
            10,
            "ACCORDION".to_owned(),
            Some(Uuid::nil()),
            None,
            Uuid::nil(),
            Uuid::nil(),
        );
        let rendered = render_user_props(&props, &utc).unwrap();
        let value: Value = serde_json::from_str(&rendered).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_invite::USER_PROPERTIES_WIRE_FIELDS);
    }

    #[test]
    fn sidebar_body_shapes() {
        assert!(matches!(
            sidebar_body_shape(&json!([{"key": "views"}])),
            SidebarBody::Items(_)
        ));
        assert!(matches!(sidebar_body_shape(&json!({})), SidebarBody::Empty));
        for invalid in [
            json!({"a": 1}),
            json!("x"),
            json!(5),
            json!(true),
            json!(null),
        ] {
            assert!(matches!(sidebar_body_shape(&invalid), SidebarBody::Invalid));
        }
    }

    #[test]
    fn body_and_page_plumbing() {
        assert!(parse_body_object(&[]).unwrap().is_empty());
        let map = parse_body_object(br#"{"a":1}"#).unwrap();
        assert_eq!(map["a"], json!(1));
        assert!(matches!(
            parse_body_object(br#"[1]"#),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            parse_body_object(br#"{bad"#),
            Err(Denial::BadDetail(_))
        ));
        assert_eq!(parse_sidebar_body(&[]).unwrap(), Value::Object(Map::new()));
        use crate::paginator::PageError as E;
        assert!(matches!(
            page_denial(E::InvalidPerPage),
            Denial::BadDetail(_)
        ));
        assert!(matches!(
            page_denial(E::NegativeOffset),
            Denial::BadDetail(_)
        ));
        assert!(matches!(page_denial(E::ZeroLimit), Denial::ServerError));
        assert!(matches!(
            parse_uuid_or_invalid("xyz"),
            Err(Denial::BadError(_))
        ));
    }

    /// Every owned method resolves a gate row (no `panic!` at request
    /// time); the sticky detail GET is auth-only.
    #[test]
    fn gate_rows_cover_owned_methods() {
        let pairs = [
            ("POST", "workspaces/<slug>/quick-links/"),
            ("GET", "workspaces/<slug>/quick-links/"),
            ("GET", "workspaces/<slug>/quick-links/<pk>/"),
            ("PATCH", "workspaces/<slug>/quick-links/<pk>/"),
            ("DELETE", "workspaces/<slug>/quick-links/<pk>/"),
            ("POST", "workspaces/<slug>/stickies/"),
            ("GET", "workspaces/<slug>/stickies/"),
            ("GET", "workspaces/<slug>/stickies/<pk>/"),
            ("PATCH", "workspaces/<slug>/stickies/<pk>/"),
            ("DELETE", "workspaces/<slug>/stickies/<pk>/"),
            ("GET", "workspaces/<slug>/home-preferences/"),
            ("PATCH", "workspaces/<slug>/home-preferences/"),
            ("GET", "workspaces/<slug>/home-preferences/<key>/"),
            ("PATCH", "workspaces/<slug>/home-preferences/<key>/"),
            ("GET", "workspaces/<slug>/sidebar-preferences/"),
            ("PATCH", "workspaces/<slug>/sidebar-preferences/"),
            ("GET", "workspaces/<slug>/recent-visits/"),
            ("GET", "workspaces/<slug>/user-properties/"),
            ("PATCH", "workspaces/<slug>/user-properties/"),
        ];
        for (method, path) in pairs {
            let gate = gate_for(method, path);
            if (method, path) == ("GET", "workspaces/<slug>/stickies/<pk>/") {
                assert!(matches!(gate, gates::Gate::Authenticated));
            }
        }
    }
}
