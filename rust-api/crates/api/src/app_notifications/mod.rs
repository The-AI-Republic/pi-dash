//! D-34 app-notification viewset-core handlers (stage 5, PIDASHCONV-301).
//!
//! Ports the five viewset-core units of
//! `apps/api/pi_dash/app/views/notification/base.py` with routes from
//! `app/urls/notification.py:17-31`:
//!
//! * `get_queryset` (`base.py:32-45`): `workspace__slug` + `receiver_id`
//!   scoping; every handler below threads the same conjuncts, so no
//!   handler ever reads outside the receiver's workspace slice.
//! * `list` (`base.py:48-149`): query params
//!   `snoozed/archived/read/type/mentioned`, the `per_page`+`cursor`
//!   pagination branch (`BasePaginator.paginate` envelope) vs the
//!   full-serializer branch (`base.py:140-149`).
//! * `retrieve` + `destroy`: inherited `BaseViewSet` mixins scoped by
//!   `get_queryset` (no local lines) — the DRF-default `get_object`
//!   lookup plus the full serializer (200) / the soft delete (204).
//! * `partial_update` (`base.py:151-161`): only `snoozed_till` is
//!   honoured (`request.data.get("snoozed_till", None)`); the
//!   serializer-errors 400 on invalid input.
//!
//! Only routes 1-3 are owned here (`GET` the collection,
//! `GET|PATCH|DELETE` the detail); every other method on those paths
//! proxies to Django, and sibling D-34 routes (read/archive/unread/
//! mark-all-read/preferences, PIDASHCONV-302/303) keep proxying until
//! their own issues land — route registration is the cutover
//! granularity, no flag needed.
//!
//! Layering: the permission gates live in [`gate`] (PIDASHCONV-300);
//! the queryset shapes in `pidash_services::app_notifications::queries`
//! (PIDASHCONV-299); the serializer key sets in
//! `::shape` (PIDASHCONV-298). This module owns the HTTP shell
//! (routes, session auth, the `@allow_permission` gate), the SQL text
//! for the handler-owned lookups/writes, the DRF field-validation
//! mirrors, and the row rendering.
//!
//! # Ported bugs and quirks (translate, don't redesign — also in the PR)
//!
//! * BUG-snoozed-true (`base.py:81`): `snoozed=true` matches every row
//!   whose `snoozed_till` is set (past *and* future); ported verbatim.
//! * BUG-double-exists (`base.py:66-67`): `is_inbox_issue` and
//!   `is_intake_issue` annotate the *same* intake `Exists` subquery.
//! * QUIRK-mentioned-truthiness (`base.py:54,100-103`): any *present*
//!   non-empty `mentioned` value — including `"false"` — takes the
//!   `icontains` arm; only an absent (or empty) param takes `exclude`.
//! * QUIRK-snoozed-archived-keyerror (`base.py:80-92`): any `snoozed` /
//!   `archived` value besides `"true"` / `"false"` answers 400
//!   `{"error":"The required key does not exist."}` (verified live;
//!   the queries-layer doc says 500, which is wrong — see the PR).
//! * QUIRK-read-silent (`base.py:52,94-98`): `read` only filters on the
//!   exact strings `"true"` / `"false"`; anything else is ignored.
//! * QUIRK-created-none (`base.py:126-129`): with `type=created`, a
//!   workspace member whose role is sub-15 sees *nothing* (`.none()`),
//!   answered without touching the database.
//! * QUIRK-partial-only-snoozed (`base.py:154-155`): the comment says
//!   "Only read_at and snoozed_till can be updated" but
//!   `notification_data` carries only `snoozed_till` — every other body
//!   key (including `read_at`) is silently dropped.
//! * QUIRK-paginate-reorders (`base.py:140-146`): the pagination branch
//!   re-orders by `order_by` (default `"-created_at"`), dropping the
//!   base `(snoozed_till, -created_at)` ordering; ported as-is.
//! * QUIRK-wire-order: the live serializer emits `[pk] + declared +
//!   body` (`id` first); the services `shape` kernel's
//!   `NOTIFICATION_WIRE_FIELDS` lists the declared block first. The
//!   local [`NotificationBody`] below emits the live order; the
//!   foundation order is left untouched (read-only) and noted in the PR.
//!
//! # Task delivery
//!
//! `destroy` performs Django's `SoftDeleteModel.delete()` inline (row
//! stays, `deleted_at` set) and enqueues the same
//! `soft_delete_related_objects` sweep the model fires, best-effort
//! after commit like the D-32 intake handlers: a missing queue table
//! must not turn the 204 into a 500, so failures are traced and the
//! response stands.

pub mod gate;

pub use gate::{
    decide_gate, gate_for, tenant_context, Gate, GateOutcome, RouteGate, GATES,
    UNAUTHENTICATED_BODY,
};

use std::collections::HashMap;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_types::WorkspaceId;

use crate::middleware::SessionHandle;
use crate::paginator::{self, Cursor, PageError, PageResponse};
use crate::serializer::render_datetime_in;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire constants (all verified live against Django, 2026-09-30)
// ---------------------------------------------------------------------------

/// `handle_exception`'s `get_object` miss: DRF's `Http404("No
/// Notification matches the given query.")` rendered as
/// `{"detail": ...}`. Answers `retrieve` and `destroy` misses
/// (`base.py` has no local retrieve/destroy lines).
pub const NOTIFICATION_NOT_FOUND_BODY: &str =
    r#"{"detail":"No Notification matches the given query."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`app/views/base.py`): answers `partial_update` misses, whose lookup
/// is a bare `.get()` (`base.py:152`).
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `KeyError` branch: answers unknown
/// `snoozed` / `archived` values (`base.py:80-92` dict lookup).
pub const KEY_ERROR_BODY: &str = r#"{"error":"The required key does not exist."}"#;
/// DRF 3.15 `DateTimeField` invalid-input message: answers
/// `partial_update` with an unparseable `snoozed_till`.
pub const INVALID_SNOOZED_MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
/// `BasePaginator.get_per_page` ceiling (`default_per_page=1000`,
/// `max_per_page=1000`).
pub const MAX_PER_PAGE: i64 = 1000;

// ---------------------------------------------------------------------------
// Query map (Django QueryDict: repeats legal, .get returns the last)
// ---------------------------------------------------------------------------

/// One query value, repeated or not.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every list handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None` when absent.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => one.clone(),
        OneOrMany::Many(many) => many.last().cloned().unwrap_or_default(),
    })
}

/// Django truthiness of `request.GET.get(key, False)`: absent is falsy;
/// a present value is truthy unless it is the empty string
/// (`?mentioned=` behaves like an absent param).
pub fn query_truthy(query: &QueryMap, key: &str) -> bool {
    query_last(query, key).is_some_and(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// Denials (exact status + body)
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` fallthrough.
    Forbidden,
    /// 404, DRF `get_object` miss on notifications.
    NotificationNotFound,
    /// 404, `ObjectDoesNotExist` branch (bare `.get()` miss).
    ObjectNotFound,
    /// 400, `{"detail": ...}` (`ParseError`, JSON parse errors).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline / `KeyError` branch).
    BadError(String),
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(Value),
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
            Denial::NotificationNotFound => (
                StatusCode::NOT_FOUND,
                NOTIFICATION_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("serializable denial"),
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

pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a 200 JSON response with exact bytes.
fn json_ok(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Map a [`PageError`] the way the views do: the documented 400-class
/// variants carry their `ParseError` detail; the rest mirror uncaught
/// Python exceptions, so the handlers answer 500 for them.
fn page_denial(error: PageError) -> Denial {
    match error {
        PageError::InvalidPerPage
        | PageError::PerPageTooLarge(_)
        | PageError::InvalidCursor
        | PageError::OffsetTooLarge
        | PageError::NegativeOffset => Denial::BadDetail(error.detail()),
        _ => Denial::ServerError,
    }
}

// ---------------------------------------------------------------------------
// Request context: auth + workspace gate
// ---------------------------------------------------------------------------

/// Authenticated actor plus time zone (`TimezoneMixin.initial`
/// activates the user's zone; datetimes render in it).
pub struct Actor {
    pub id: Uuid,
    pub timezone: Tz,
}

/// `request.user` through Django-session auth.
///
/// `BaseSessionAuthentication` + `IsAuthenticated`
/// (`app/views/base.py`): anonymous answers the DRF `NotAuthenticated`
/// body before anything else runs.
pub async fn actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    let resolved =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    Ok(Actor {
        id: resolved.id,
        timezone: resolved.timezone,
    })
}

pub fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// Active workspace role for `(user, slug)`, or `None` (no row).
/// Mirrors the `allow_permission` workspace lookup (`is_active=True`,
/// soft-deleted rows excluded, `app/permissions/base.py:44-51`).
async fn workspace_role(pool: &PgPool, user_id: &Uuid, slug: &str) -> Result<Option<i32>, Denial> {
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
    Ok(row.map(|(role,)| i32::from(role)))
}

/// Membership facts for the D-34 gate: ADMIN/MEMBER/GUEST all pass
/// (`base.py:47,151`); anything else denies.
fn workspace_facts(slug: &str, role: Option<i32>) -> AllowFacts {
    let allowed = matches!(
        role,
        Some(ROLE_ADMIN) | Some(ROLE_MEMBER) | Some(ROLE_GUEST)
    );
    AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: allowed,
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(ROLE_ADMIN),
    }
}

/// Enforce one [`gate`] row: allow runs, deny answers the allow-style
/// 403. Anonymous never reaches here ([`actor`] 401s first).
#[allow(clippy::result_large_err)]
fn enforce(outcome: GateOutcome) -> Result<(), Response> {
    match outcome {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Deny => Err(crate::permissions::PermissionDenied.into_response()),
        GateOutcome::Unauthenticated => Err(Denial::Unauthorized.into_response()),
    }
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An owned path: the owned methods serve from Rust, everything else
/// falls through to Django (its 405-after-auth and metadata responses
/// live there). `HEAD` rides axum's `get` handling like Django's
/// `GET`-backed `HEAD`.
pub fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
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

/// Register the viewset-core routes (routes 1-3,
/// `app/urls/notification.py:17-31`). Sibling D-34 handler issues
/// extend this router with their own paths; merges keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/users/notifications/",
            owned(
                axum::routing::get(list),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/users/notifications/{pk}/",
            owned(
                axum::routing::get(retrieve)
                    .patch(partial_update)
                    .delete(destroy),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
}

// ---------------------------------------------------------------------------
// SQL: notification rows (get_queryset + list annotations)
// ---------------------------------------------------------------------------

/// Every `Notification` column in audit-then-definition order
/// (`db/models/base.py:18`, `db/mixins.py`, `db/models/notification.py:14-33`).
const NOTIFICATION_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "workspace_id",
    "project_id",
    "data",
    "entity_identifier",
    "entity_name",
    "title",
    "message",
    "message_html",
    "message_stripped",
    "sender",
    "triggered_by_id",
    "receiver_id",
    "read_at",
    "snoozed_till",
    "archived_at",
];

/// `"n"."c1", "n"."c2", …` in [`NOTIFICATION_COLUMNS`] order.
fn select_list(alias: &str) -> String {
    NOTIFICATION_COLUMNS
        .iter()
        .map(|c| format!("\"{alias}\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The intake `Exists` subquery (`base.py:57-61`), emitted once per
/// annotation (BUG-double-exists, `base.py:66-67`). Captured verbatim
/// from Django's compiled SQL: the `issues` leg carries its
/// soft-delete conjunct, the `intake_issues` leg carries none, and the
/// workspace leg reuses the scope slug placeholder.
fn intake_exists_sql(notif_alias: &str, slug_param: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM \"issues\" U0 \
         INNER JOIN \"intake_issues\" U1 ON (U0.\"id\" = U1.\"issue_id\") \
         INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
         WHERE (U0.\"deleted_at\" IS NULL \
         AND U1.\"status\" IN (0, 2, -2) \
         AND U0.\"id\" = \"{notif_alias}\".\"entity_identifier\" \
         AND U2.\"slug\" = {slug_param}))"
    )
}

/// The `is_mentioned_notification` annotation (`base.py:68-74`):
/// `Case(When(sender__icontains="mentioned", …))` — `icontains`
/// renders `ILIKE '%mentioned%'`.
fn mentioned_annotation_sql(notif_alias: &str) -> String {
    format!(
        "CASE WHEN \"{notif_alias}\".\"sender\" ILIKE '%mentioned%' THEN true ELSE false END \
         AS \"is_mentioned_notification\""
    )
}

/// The `triggered_by_details` join legs: the user row plus its avatar
/// asset (the `avatar_url` property, `db/models/user.py:142-151`).
const TRIGGERED_BY_SELECT: &str = r#"tu."id" AS "tu_id", tu."first_name" AS "tu_first_name", tu."last_name" AS "tu_last_name", tu."avatar" AS "tu_avatar", tu."avatar_asset_id" AS "tu_avatar_asset_id", fa."entity_type" AS "tu_avatar_asset_entity", tu."is_bot" AS "tu_is_bot", tu."display_name" AS "tu_display_name""#;

const TRIGGERED_BY_JOINS: &str = r#"LEFT JOIN "users" tu ON (tu."id" = n."triggered_by_id") LEFT JOIN "file_assets" fa ON (fa."id" = tu."avatar_asset_id" AND fa."deleted_at" IS NULL)"#;

/// Full row SELECT: the notification columns, the double `Exists`
/// annotations, the mentioned `Case/When`, and the `triggered_by`
/// projection. `$slug` / `$user` bind the scope (conventionally
/// `$1` / `$2`).
fn row_select_sql(slug_param: &str, _user_param: &str) -> String {
    let exists = intake_exists_sql("n", slug_param);
    let mentioned = mentioned_annotation_sql("n");
    format!(
        "SELECT {cols}, {exists} AS \"is_inbox_issue\", {exists} AS \"is_intake_issue\", {mentioned}, {TRIGGERED_BY_SELECT} \
         FROM \"notifications\" n \
         INNER JOIN \"workspaces\" w ON (n.\"workspace_id\" = w.\"id\") \
         {TRIGGERED_BY_JOINS}",
        cols = select_list("n"),
    )
}

/// `get_queryset` scope (`base.py:32-45`): `workspace__slug` +
/// `receiver_id`, soft-delete-scoped. `$slug` / `$user` bind the scope.
fn base_where(slug_param: &str, user_param: &str) -> String {
    format!(
        "w.\"slug\" = {slug_param} \
         AND n.\"receiver_id\" = {user_param} \
         AND n.\"deleted_at\" IS NULL"
    )
}

/// `snoozed` filter (`base.py:80-85`; default `"false"`). The `"true"`
/// branch is BUG-snoozed-true (`snoozed_till < now OR snoozed_till IS
/// NOT NULL` — matches every set `snoozed_till`); ported verbatim.
/// `$now` binds `timezone.now()` at evaluation time. Anything besides
/// `"true"` / `"false"` is the `KeyError` 400.
fn snoozed_clause(param: &str, now_param: &str) -> Result<String, Denial> {
    match param {
        "true" => Ok(format!(
            "(n.\"snoozed_till\" < {now_param} OR n.\"snoozed_till\" IS NOT NULL)"
        )),
        "false" => Ok(format!(
            "(n.\"snoozed_till\" >= {now_param} OR n.\"snoozed_till\" IS NULL)"
        )),
        _ => Err(Denial::BadError(
            "The required key does not exist.".to_owned(),
        )),
    }
}

/// `archived` filter (`base.py:87-92`; default `"false"`).
fn archived_clause(param: &str) -> Result<&'static str, Denial> {
    match param {
        "true" => Ok("n.\"archived_at\" IS NOT NULL"),
        "false" => Ok("n.\"archived_at\" IS NULL"),
        _ => Err(Denial::BadError(
            "The required key does not exist.".to_owned(),
        )),
    }
}

/// `read` filter (`base.py:52,94-98`): only the exact strings
/// `"true"` / `"false"` filter; anything else (including a present
/// but odd value) is silently ignored (QUIRK-read-silent).
fn read_clause(param: Option<&str>) -> Option<&'static str> {
    match param {
        Some("false") => Some("n.\"read_at\" IS NULL"),
        Some("true") => Some("n.\"read_at\" IS NOT NULL"),
        _ => None,
    }
}

/// `mentioned` filter (`base.py:54,100-103`): the branch signal is
/// param *presence* with a non-empty value
/// (QUIRK-mentioned-truthiness) — any present non-empty value,
/// including `"false"`, takes the `icontains` arm.
fn mentioned_clause(param_present: bool) -> &'static str {
    if param_present {
        "n.\"sender\" ILIKE '%mentioned%'"
    } else {
        "NOT (n.\"sender\" ILIKE '%mentioned%')"
    }
}

/// `subscribed` type arm (list, `base.py:107-115`): the subscriber's
/// issue ids minus issues they created or are assigned to. Captured
/// from Django's compiled SQL, including the inner soft-delete
/// conjuncts and the `pk = issue_id` assignee correlation (as-is).
fn subscribed_arm(slug_param: &str, user_param: &str) -> String {
    format!(
        "n.\"entity_identifier\" IN (SELECT U0.\"issue_id\" FROM \"issue_subscribers\" U0 \
         INNER JOIN \"workspaces\" wsub ON (U0.\"workspace_id\" = wsub.\"id\") \
         WHERE (U0.\"deleted_at\" IS NULL \
         AND U0.\"subscriber_id\" = {user_param} \
         AND wsub.\"slug\" = {slug_param} \
         AND NOT EXISTS (SELECT 1 FROM \"issues\" WHERE \"deleted_at\" IS NULL AND \"created_by_id\" = {user_param} AND \"id\" = U0.\"issue_id\") \
         AND NOT EXISTS (SELECT 1 FROM \"issue_assignees\" WHERE \"deleted_at\" IS NULL AND \"id\" = U0.\"issue_id\" AND \"assignee_id\" = {user_param})))"
    )
}

/// `assigned` type arm (list, `base.py:118-122`).
fn assigned_arm(slug_param: &str, user_param: &str) -> String {
    format!(
        "n.\"entity_identifier\" IN (SELECT ia.\"issue_id\" FROM \"issue_assignees\" ia \
         INNER JOIN \"workspaces\" wa ON (ia.\"workspace_id\" = wa.\"id\") \
         WHERE (ia.\"deleted_at\" IS NULL AND ia.\"assignee_id\" = {user_param} AND wa.\"slug\" = {slug_param}))"
    )
}

/// `created` type arm (list, `base.py:131-133`): issues the user created.
/// The select is alias-qualified (`ci."id"`): a bare `"id"` would be
/// ambiguous against the outer notification/workspace/user legs.
fn created_arm(slug_param: &str, user_param: &str) -> String {
    format!(
        "n.\"entity_identifier\" IN (SELECT ci.\"id\" FROM \"issues\" ci \
         INNER JOIN \"workspaces\" wc ON (ci.\"workspace_id\" = wc.\"id\") \
         WHERE (ci.\"deleted_at\" IS NULL AND ci.\"created_by_id\" = {user_param} AND wc.\"slug\" = {slug_param}))"
    )
}

/// Outcome of compiling the list `type` param (`base.py:105-137`).
enum TypeFilter {
    /// Extra `AND (...)` fragment over `entity_identifier`.
    Where(String),
    /// `.none()`: answer `[]` without querying (QUIRK-created-none).
    Empty,
    /// No known branch (`type=all` default): `.filter(Q())` no-op.
    None,
}

/// Compile the list `type` param: comma-split (`:105`), known
/// branches OR together (`:115,122,134`). `created_member` is the
/// evaluated guard below. Unknown tokens (including the default
/// `"all"`) match no branch.
fn list_type_filter(
    param: &str,
    slug_param: &str,
    user_param: &str,
    created_member: bool,
) -> TypeFilter {
    let mut arms = Vec::new();
    let mut saw_created = false;
    for token in param.split(',') {
        match token {
            "subscribed" => arms.push(subscribed_arm(slug_param, user_param)),
            "assigned" => arms.push(assigned_arm(slug_param, user_param)),
            "created" => saw_created = true,
            _ => {}
        }
    }
    if saw_created {
        if created_member {
            return TypeFilter::Empty;
        }
        arms.push(created_arm(slug_param, user_param));
    }
    if arms.is_empty() {
        TypeFilter::None
    } else {
        TypeFilter::Where(format!("({})", arms.join(" OR ")))
    }
}

/// The `created` member guard (list `:126-128`): a sub-15 active
/// workspace membership (soft-delete-scoped, from Django's compiled
/// SQL). When it hits, the whole list becomes `.none()`.
async fn created_member_guard(pool: &PgPool, slug: &str, user_id: &Uuid) -> Result<bool, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT wm.id FROM workspace_members wm
           INNER JOIN workspaces w ON (wm.workspace_id = w.id)
           WHERE (wm.deleted_at IS NULL AND wm.is_active
             AND wm.member_id = $1 AND wm.role < 15 AND w.slug = $2)
           LIMIT 1"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// Map the `paginate` `order_by` param onto a notification column.
/// `OffsetPaginator.get_result` re-orders by `(key dir NULLS LAST,
/// -created_at)`; an unknown field raises Django-side (`FieldError`
/// → the 500 envelope, verified live).
fn order_column(order_by: &str) -> Result<(&'static str, bool), Denial> {
    let (name, descending) = match order_by.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (order_by, false),
    };
    let column = match name {
        "created_at" => "created_at",
        "updated_at" => "updated_at",
        "snoozed_till" => "snoozed_till",
        "read_at" => "read_at",
        "archived_at" => "archived_at",
        "title" => "title",
        _ => return Err(Denial::ServerError),
    };
    Ok((column, descending))
}

// ---------------------------------------------------------------------------
// Rows + rendering (live wire order: [pk] + declared + body)
// ---------------------------------------------------------------------------

/// One notification row with every column the responses render, plus
/// the list annotations and the `triggered_by` projection.
struct NotificationRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    data: Option<Value>,
    entity_identifier: Option<Uuid>,
    entity_name: String,
    title: String,
    message: Option<Value>,
    message_html: String,
    message_stripped: Option<String>,
    sender: String,
    triggered_by_id: Option<Uuid>,
    receiver_id: Uuid,
    read_at: Option<DateTime<Utc>>,
    snoozed_till: Option<DateTime<Utc>>,
    archived_at: Option<DateTime<Utc>>,
    is_inbox_issue: bool,
    is_intake_issue: bool,
    is_mentioned_notification: bool,
    tu_id: Option<Uuid>,
    tu_first_name: Option<String>,
    tu_last_name: Option<String>,
    tu_avatar: Option<String>,
    tu_avatar_asset_id: Option<Uuid>,
    tu_avatar_asset_entity: Option<String>,
    tu_is_bot: Option<bool>,
    tu_display_name: Option<String>,
}

impl NotificationRow {
    fn get(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            deleted_at: row.try_get("deleted_at")?,
            workspace_id: row.try_get("workspace_id")?,
            project_id: row.try_get("project_id")?,
            data: row.try_get("data")?,
            entity_identifier: row.try_get("entity_identifier")?,
            entity_name: row.try_get("entity_name")?,
            title: row.try_get("title")?,
            message: row.try_get("message")?,
            message_html: row.try_get("message_html")?,
            message_stripped: row.try_get("message_stripped")?,
            sender: row.try_get("sender")?,
            triggered_by_id: row.try_get("triggered_by_id")?,
            receiver_id: row.try_get("receiver_id")?,
            read_at: row.try_get("read_at")?,
            snoozed_till: row.try_get("snoozed_till")?,
            archived_at: row.try_get("archived_at")?,
            is_inbox_issue: row.try_get("is_inbox_issue")?,
            is_intake_issue: row.try_get("is_intake_issue")?,
            is_mentioned_notification: row.try_get("is_mentioned_notification")?,
            tu_id: row.try_get("tu_id")?,
            tu_first_name: row.try_get("tu_first_name")?,
            tu_last_name: row.try_get("tu_last_name")?,
            tu_avatar: row.try_get("tu_avatar")?,
            tu_avatar_asset_id: row.try_get("tu_avatar_asset_id")?,
            tu_avatar_asset_entity: row.try_get("tu_avatar_asset_entity")?,
            tu_is_bot: row.try_get("tu_is_bot")?,
            tu_display_name: row.try_get("tu_display_name")?,
        })
    }
}

/// The `avatar_url` property (`db/models/user.py:142-151`): the
/// asset's static URL wins for the four static entity types, else the
/// raw `avatar` string when non-empty, else null. Same rule as the
/// license handlers' port.
fn avatar_url(asset_id: Option<Uuid>, asset_entity: Option<&str>, avatar: &str) -> Option<String> {
    if asset_id.is_some()
        && matches!(
            asset_entity,
            Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER")
        )
    {
        return asset_id.map(|asset| format!("/api/assets/v2/static/{asset}/"));
    }
    if avatar.is_empty() {
        None
    } else {
        Some(avatar.to_owned())
    }
}

/// `NotificationSerializer` output in the live wire order: `[pk] +
/// declared + body` (`id`, `triggered_by_details`,
/// `is_inbox_issue`, `is_intake_issue`, `is_mentioned_notification`,
/// then the `__all__` body). Struct order is the byte order.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct NotificationBody {
    id: String,
    triggered_by_details: Option<UserLiteBody>,
    is_inbox_issue: bool,
    is_intake_issue: bool,
    is_mentioned_notification: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    data: Option<Value>,
    entity_identifier: Option<String>,
    entity_name: String,
    title: String,
    message: Option<Value>,
    message_html: String,
    message_stripped: Option<String>,
    sender: String,
    read_at: Option<String>,
    snoozed_till: Option<String>,
    archived_at: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    workspace: String,
    project: Option<String>,
    triggered_by: Option<String>,
    receiver: String,
}

/// `UserLiteSerializer` output (`app/serializers/user.py:141-153`),
/// in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct UserLiteBody {
    id: String,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

/// Single-instance output (`retrieve`, `partial_update`): the same
/// serializer over an *unannotated* instance (`get_queryset` carries
/// no annotations — they live only on the `list` queryset), so the
/// three read-only annotation fields are skipped (`SkipField`,
/// verified live) and the wire carries 22 keys.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct DetailBody {
    id: String,
    triggered_by_details: Option<UserLiteBody>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    data: Option<Value>,
    entity_identifier: Option<String>,
    entity_name: String,
    title: String,
    message: Option<Value>,
    message_html: String,
    message_stripped: Option<String>,
    sender: String,
    read_at: Option<String>,
    snoozed_till: Option<String>,
    archived_at: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    workspace: String,
    project: Option<String>,
    triggered_by: Option<String>,
    receiver: String,
}

/// Render the `triggered_by_details` nest (shared by both shapes).
fn render_details(row: &NotificationRow) -> Option<UserLiteBody> {
    row.tu_id.map(|tu_id| {
        let avatar = row.tu_avatar.as_deref().unwrap_or("");
        UserLiteBody {
            id: tu_id.to_string(),
            first_name: row.tu_first_name.clone().unwrap_or_default(),
            last_name: row.tu_last_name.clone().unwrap_or_default(),
            avatar: avatar.to_owned(),
            avatar_url: avatar_url(
                row.tu_avatar_asset_id,
                row.tu_avatar_asset_entity.as_deref(),
                avatar,
            ),
            is_bot: row.tu_is_bot.unwrap_or(false),
            display_name: row.tu_display_name.clone().unwrap_or_default(),
        }
    })
}

/// Render one row through the serializer shape. Datetimes render in
/// the request's zone (`TimezoneMixin.initial`); FKs render as UUID
/// strings with null FKs as `null`; `data` / `message` splice raw.
fn render_row(row: &NotificationRow, tz: &Tz) -> NotificationBody {
    NotificationBody {
        id: row.id.to_string(),
        triggered_by_details: render_details(row),
        is_inbox_issue: row.is_inbox_issue,
        is_intake_issue: row.is_intake_issue,
        is_mentioned_notification: row.is_mentioned_notification,
        created_at: render_datetime_in(&row.created_at, tz),
        updated_at: render_datetime_in(&row.updated_at, tz),
        deleted_at: row.deleted_at.as_ref().map(|dt| render_datetime_in(dt, tz)),
        data: row.data.clone(),
        entity_identifier: row.entity_identifier.map(|id| id.to_string()),
        entity_name: row.entity_name.clone(),
        title: row.title.clone(),
        message: row.message.clone(),
        message_html: row.message_html.clone(),
        message_stripped: row.message_stripped.clone(),
        sender: row.sender.clone(),
        read_at: row.read_at.as_ref().map(|dt| render_datetime_in(dt, tz)),
        snoozed_till: row
            .snoozed_till
            .as_ref()
            .map(|dt| render_datetime_in(dt, tz)),
        archived_at: row
            .archived_at
            .as_ref()
            .map(|dt| render_datetime_in(dt, tz)),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        workspace: row.workspace_id.to_string(),
        project: row.project_id.map(|id| id.to_string()),
        triggered_by: row.triggered_by_id.map(|id| id.to_string()),
        receiver: row.receiver_id.to_string(),
    }
}

/// Render one row for the single-instance actions (`retrieve`,
/// `partial_update`): the [`DetailBody`] 22-key shape.
fn render_detail(row: &NotificationRow, tz: &Tz) -> DetailBody {
    DetailBody {
        id: row.id.to_string(),
        triggered_by_details: render_details(row),
        created_at: render_datetime_in(&row.created_at, tz),
        updated_at: render_datetime_in(&row.updated_at, tz),
        deleted_at: row.deleted_at.as_ref().map(|dt| render_datetime_in(dt, tz)),
        data: row.data.clone(),
        entity_identifier: row.entity_identifier.map(|id| id.to_string()),
        entity_name: row.entity_name.clone(),
        title: row.title.clone(),
        message: row.message.clone(),
        message_html: row.message_html.clone(),
        message_stripped: row.message_stripped.clone(),
        sender: row.sender.clone(),
        read_at: row.read_at.as_ref().map(|dt| render_datetime_in(dt, tz)),
        snoozed_till: row
            .snoozed_till
            .as_ref()
            .map(|dt| render_datetime_in(dt, tz)),
        archived_at: row
            .archived_at
            .as_ref()
            .map(|dt| render_datetime_in(dt, tz)),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        workspace: row.workspace_id.to_string(),
        project: row.project_id.map(|id| id.to_string()),
        triggered_by: row.triggered_by_id.map(|id| id.to_string()),
        receiver: row.receiver_id.to_string(),
    }
}

// ---------------------------------------------------------------------------
// list (base.py:48-149)
// ---------------------------------------------------------------------------

/// `list`: session auth, the workspace gate (`:47`), then the
/// filtered queryset — serialized whole, or through the
/// `BasePaginator.paginate` envelope when *both* `per_page` and
/// `cursor` are present and non-empty (`:140`).
async fn list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let role = match workspace_role(pool, &actor.id, &slug).await {
        Ok(role) => role,
        Err(denial) => return denial.into_response(),
    };
    let row = gate_for("GET", "workspaces/<slug>/users/notifications/").expect("list gate row");
    if let Err(response) = enforce(decide_gate(
        &row.gate,
        &tenant_context(&slug),
        &workspace_facts(&slug, role),
    )) {
        return response;
    }

    // Query params (Python evaluation order: snoozed, archived, read,
    // mentioned, type — the KeyError 400s fire before later filters).
    let snoozed_param = query_last(&query, "snoozed").unwrap_or_else(|| "false".to_owned());
    let archived_param = query_last(&query, "archived").unwrap_or_else(|| "false".to_owned());
    let read_param = query_last(&query, "read");
    let type_param = query_last(&query, "type").unwrap_or_else(|| "all".to_owned());
    let mentioned_present = query_truthy(&query, "mentioned");

    let now = Utc::now();
    let snoozed = match snoozed_clause(&snoozed_param, "$3") {
        Ok(clause) => clause,
        Err(denial) => return denial.into_response(),
    };
    let archived = match archived_clause(&archived_param) {
        Ok(clause) => clause,
        Err(denial) => return denial.into_response(),
    };
    let mut fragments = vec![
        base_where("$1", "$2"),
        "n.\"entity_name\" = 'issue'".to_owned(),
        snoozed,
        archived.to_owned(),
        mentioned_clause(mentioned_present).to_owned(),
    ];
    if let Some(clause) = read_clause(read_param.as_deref()) {
        fragments.push(clause.to_owned());
    }

    // `type` (comma-split, OR arms). The `created` guard evaluates
    // here (`.exists()` hits the database); a hit short-circuits to
    // `.none()` — answered without further queries.
    let mut saw_created = false;
    for token in type_param.split(',') {
        if token == "created" {
            saw_created = true;
        }
    }
    let created_member = if saw_created {
        match created_member_guard(pool, &slug, &actor.id).await {
            Ok(hit) => hit,
            Err(denial) => return denial.into_response(),
        }
    } else {
        false
    };
    match list_type_filter(&type_param, "$1", "$2", created_member) {
        TypeFilter::Where(arm) => fragments.push(arm),
        TypeFilter::Empty => {
            return respond_empty(&query);
        }
        TypeFilter::None => {}
    }
    let where_sql = fragments.join(" AND ");

    if query_truthy(&query, "per_page") && query_truthy(&query, "cursor") {
        return paginate_list(
            pool,
            &where_sql,
            &slug,
            &actor.id,
            &now,
            &query,
            &actor.timezone,
        )
        .await;
    }

    // Full-serializer branch (`:148-149`): the whole filtered
    // queryset in `(snoozed_till, -created_at)` order.
    let sql = format!(
        "{} WHERE {} ORDER BY n.\"snoozed_till\" ASC, n.\"created_at\" DESC",
        row_select_sql("$1", "$2"),
        where_sql
    );
    let rows = match sqlx::query(&sql)
        .bind(&slug)
        .bind(actor.id)
        .bind(now)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut bodies = Vec::with_capacity(rows.len());
    for row in &rows {
        match NotificationRow::get(row) {
            Ok(parsed) => bodies.push(render_row(&parsed, &actor.timezone)),
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    json_ok(serde_json::to_string(&bodies).expect("serializable list"))
}

/// Answer `.none()` (QUIRK-created-none): `[]` on the full branch, or
/// the zero-count paginator envelope when pagination was requested.
fn respond_empty(query: &QueryMap) -> Response {
    if !(query_truthy(query, "per_page") && query_truthy(query, "cursor")) {
        return json_ok("[]".to_owned());
    }
    let per_page = match paginator::parse_per_page(
        query_last(query, "per_page").as_deref(),
        MAX_PER_PAGE,
        MAX_PER_PAGE,
    ) {
        Ok(value) => value,
        Err(error) => return page_denial(error).into_response(),
    };
    let limit = paginator::clamp_limit(per_page, paginator::MAX_LIMIT);
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = match Cursor::from_string(&cursor_raw) {
        Ok(cursor) => cursor,
        Err(error) => return page_denial(error).into_response(),
    };
    let total_pages = match paginator::max_hits(0, limit) {
        Ok(pages) => pages,
        Err(error) => return page_denial(error).into_response(),
    };
    let page = PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count: 0,
        next_cursor: paginator::next_cursor(limit, cursor.offset, false).to_string(),
        prev_cursor: paginator::prev_cursor(limit, cursor.offset).to_string(),
        next_page_results: false,
        prev_page_results: false,
        count: 0,
        total_pages,
        total_results: 0,
        extra_stats: None,
        results: Vec::<NotificationBody>::new(),
    };
    json_ok(serde_json::to_string(&page).expect("serializable envelope"))
}

/// Pagination branch (`:140-146`): `paginate(order_by=...,
/// queryset=..., on_results=serializer)`. The `order_by` param
/// (default `"-created_at"`) *replaces* the base ordering
/// (QUIRK-paginate-reorders).
#[allow(clippy::too_many_arguments)]
async fn paginate_list(
    pool: &PgPool,
    where_sql: &str,
    slug: &str,
    user_id: &Uuid,
    now: &DateTime<Utc>,
    query: &QueryMap,
    tz: &Tz,
) -> Response {
    let per_page = match paginator::parse_per_page(
        query_last(query, "per_page").as_deref(),
        MAX_PER_PAGE,
        MAX_PER_PAGE,
    ) {
        Ok(value) => value,
        Err(error) => return page_denial(error).into_response(),
    };
    let limit = paginator::clamp_limit(per_page, paginator::MAX_LIMIT);
    let cursor_raw = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = match Cursor::from_string(&cursor_raw) {
        Ok(cursor) => cursor,
        Err(error) => return page_denial(error).into_response(),
    };
    let window =
        match paginator::offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None) {
            Ok(window) => window,
            Err(error) => return page_denial(error).into_response(),
        };
    let order_input = query_last(query, "order_by").unwrap_or_else(|| "-created_at".to_owned());
    let (column, descending) = match order_column(&order_input) {
        Ok(order) => order,
        Err(denial) => return denial.into_response(),
    };
    let direction = if descending { "DESC" } else { "ASC" };
    // `queryset.order_by(F(key).dir(nulls_last=True), "-created_at")`.
    let order_sql = format!("n.\"{column}\" {direction} NULLS LAST, n.\"created_at\" DESC");

    let count_sql = format!(
        "SELECT COUNT(*) AS \"count\" FROM \"notifications\" n \
         INNER JOIN \"workspaces\" w ON (n.\"workspace_id\" = w.\"id\") \
         WHERE {where_sql}"
    );
    let total: i64 = match sqlx::query(&count_sql)
        .bind(slug)
        .bind(user_id)
        .bind(now)
        .fetch_one(pool)
        .await
    {
        Ok(row) => row.try_get("count").unwrap_or(0),
        Err(_) => return Denial::ServerError.into_response(),
    };
    // `[offset, stop)`: one extra row detects the next page.
    let fetch = (window.stop - window.offset).max(0);
    let rows_sql = format!(
        "{} WHERE {} ORDER BY {} LIMIT {} OFFSET {}",
        row_select_sql("$1", "$2"),
        where_sql,
        order_sql,
        fetch,
        window.offset
    );
    let rows = match sqlx::query(&rows_sql)
        .bind(slug)
        .bind(user_id)
        .bind(now)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let has_more = rows.len() as i64 > limit;
    // `results[:limit]` on the fetched window (a negative `limit`
    // is Django's lazy-queryset `ValueError` → the 500 envelope).
    if limit < 0 {
        return page_denial(PageError::NegativeSlice).into_response();
    }
    let kept = &rows[..(limit as usize).min(rows.len())];
    let mut bodies = Vec::with_capacity(kept.len());
    for row in kept {
        match NotificationRow::get(row) {
            Ok(parsed) => bodies.push(render_row(&parsed, tz)),
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    let total_pages = match paginator::max_hits(total, limit) {
        Ok(pages) => pages,
        Err(error) => return page_denial(error).into_response(),
    };
    let page = PageResponse {
        grouped_by: None,
        sub_grouped_by: None,
        total_count: total,
        next_cursor: paginator::next_cursor(limit, window.page, has_more).to_string(),
        prev_cursor: paginator::prev_cursor(limit, window.page).to_string(),
        next_page_results: has_more,
        prev_page_results: window.page > 0,
        count: bodies.len(),
        total_pages,
        total_results: total,
        extra_stats: None,
        results: bodies,
    };
    json_ok(serde_json::to_string(&page).expect("serializable envelope"))
}

// ---------------------------------------------------------------------------
// Detail lookup (get_queryset scoping for retrieve/destroy/partial_update)
// ---------------------------------------------------------------------------

/// Scoped single-row lookup: `workspace__slug` + `pk` + `receiver`
/// (`partial_update`'s `.get()`, `base.py:152`) — also what the
/// inherited `get_object` resolves through `get_queryset`. A miss is
/// `None`; the caller picks the 404 shape (they differ per action).
async fn fetch_one(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    pk: &Uuid,
) -> Result<Option<NotificationRow>, Denial> {
    let sql = format!(
        "{} WHERE {} AND n.\"id\" = $4",
        row_select_sql("$1", "$2"),
        base_where("$1", "$2")
    );
    let row = sqlx::query(&sql)
        .bind(slug)
        .bind(user_id)
        .bind(Utc::now())
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        Some(row) => NotificationRow::get(&row)
            .map(Some)
            .map_err(|_| Denial::ServerError),
        None => Ok(None),
    }
}

/// Parse a UUID path segment. Django's `<uuid:pk>` converter leaves
/// the route unmatched for garbage, so the request must fall through
/// to Django (its `unread/` + `mark-all-read/` literals and its own
/// 404 live there) — hence the proxy, not a Rust 404. The parse runs
/// *before* auth: Django answers its routing 404 without
/// authenticating.
fn parse_pk(raw: &str) -> Option<Uuid> {
    raw.parse::<Uuid>().ok()
}

// ---------------------------------------------------------------------------
// retrieve (inherited mixin) / destroy (inherited mixin)
// ---------------------------------------------------------------------------

/// `retrieve` (the DRF-default `ModelViewSet` retrieve — no override
/// in `NotificationViewSet`): the receiver-scoped `get_object` plus
/// the full serializer, 200. A miss is `Http404("No Notification
/// matches the given query.")`. Undecorated: any authenticated user
/// passes; foreign rows 404 ([`Gate::QuerysetScoped`]).
async fn retrieve(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Some(pk) = parse_pk(&pk_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    match fetch_one(pool, &slug, &actor.id, &pk).await {
        Ok(Some(row)) => json_ok(
            serde_json::to_string(&render_detail(&row, &actor.timezone)).expect("serializable row"),
        ),
        Ok(None) => Denial::NotificationNotFound.into_response(),
        Err(denial) => denial.into_response(),
    }
}

/// `destroy` (the DRF-default `ModelViewSet` destroy): the scoped
/// lookup (404 like retrieve), then `SoftDeleteModel.delete()` — the
/// row stays with `deleted_at` set (`mixins.py:72-79`, which also
/// stamps `updated_at`/`updated_by` through `save()` and fires the
/// related-objects sweep, enqueued best-effort below) — 204 with an
/// empty body. Undecorated like retrieve.
async fn destroy(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Some(pk) = parse_pk(&pk_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    match fetch_one(pool, &slug, &actor.id, &pk).await {
        Ok(Some(_)) => {}
        Ok(None) => return Denial::NotificationNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    }
    let now = Utc::now();
    if let Err(response) = soft_delete(pool, &pk, &actor.id, &now).await {
        return response;
    }
    enqueue_soft_delete(pool, &pk).await;
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204 response")
}

/// `SoftDeleteModel.delete()` (`db/mixins.py:72-79`): stamp
/// `deleted_at` (+ `updated_at`/`updated_by` through `save()`).
async fn soft_delete(
    pool: &PgPool,
    pk: &Uuid,
    user_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), Response> {
    sqlx::query(
        r#"UPDATE notifications SET deleted_at = $2, updated_at = $2, updated_by_id = $3
           WHERE id = $1"#,
    )
    .bind(pk)
    .bind(now)
    .bind(user_id)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| Denial::ServerError.into_response())
}

/// `soft_delete_related_objects.delay("db", "notification", pk, None)`
/// (`db/mixins.py:77-79`): best-effort post-commit enqueue like the
/// D-32 intake handlers — without it the response still stands.
async fn enqueue_soft_delete(pool: &PgPool, pk: &Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("notification".to_owned()),
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

// ---------------------------------------------------------------------------
// partial_update (base.py:151-161)
// ---------------------------------------------------------------------------

/// Parse the request body the way DRF does for JSON PATCHes: empty →
/// `{}`; malformed → `ParseError` 400; non-object JSON → the
/// attribute errors the view code hits (500 envelope).
#[allow(clippy::result_large_err)]
fn parse_body(raw: &[u8]) -> Result<Value, Response> {
    if raw.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(value) if value.is_object() => Ok(value),
        Ok(_) => Err(Denial::ServerError.into_response()),
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}")).into_response()),
    }
}

/// Validate `snoozed_till` like DRF's `DateTimeField` (default
/// `iso-8601` input formats, `null=True` on the model so JSON null
/// clears the column): null → `None`; RFC 3339 / naive
/// `YYYY-MM-DD[T ]hh:mm:ss[.f]` (naive read as UTC — `TIME_ZONE` is
/// UTC) → the instant; anything else → the exact 400 field error
/// (verified live).
fn parse_snoozed_till(value: Option<&Value>) -> Result<Option<DateTime<Utc>>, Value> {
    let invalid = || {
        Value::Object(
            [(
                "snoozed_till".to_owned(),
                Value::Array(vec![Value::String(INVALID_SNOOZED_MESSAGE.to_owned())]),
            )]
            .into_iter()
            .collect(),
        )
    };
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Value::String(raw) = value else {
        return Err(invalid());
    };
    if let Ok(aware) = DateTime::parse_from_rfc3339(raw) {
        return Ok(Some(aware.with_timezone(&Utc)));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return Ok(Some(naive.and_utc()));
        }
    }
    Err(invalid())
}

/// `partial_update`: the workspace gate (`:151`), then the scoped
/// `.get()` (a miss is the `ObjectDoesNotExist` 404 — *not* the
/// `get_object` message), then `{"snoozed_till":
/// request.data.get("snoozed_till", None)}` through the serializer
/// (QUIRK-partial-only-snoozed: every other body key is dropped),
/// `save()` (stamps `updated_at`/`updated_by`), and the full
/// serializer, 200.
async fn partial_update(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Some(pk) = parse_pk(&pk_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let role = match workspace_role(pool, &actor.id, &slug).await {
        Ok(role) => role,
        Err(denial) => return denial.into_response(),
    };
    let row = gate_for("PATCH", "workspaces/<slug>/users/notifications/<uuid>/")
        .expect("partial_update gate row");
    if let Err(response) = enforce(decide_gate(
        &row.gate,
        &tenant_context(&slug),
        &workspace_facts(&slug, role),
    )) {
        return response;
    }
    // `get_object` ordering: the lookup runs before the body parses —
    // a miss 404s even with a malformed payload.
    match fetch_one(pool, &slug, &actor.id, &pk).await {
        Ok(Some(_)) => {}
        Ok(None) => return Denial::ObjectNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    }
    let (_parts, body) = req.into_parts();
    let raw = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(raw) => raw,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let data = match parse_body(&raw) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // Only `snoozed_till` is honoured; everything else is dropped.
    let snoozed = match parse_snoozed_till(data.get("snoozed_till")) {
        Ok(snoozed) => snoozed,
        Err(errors) => return Denial::BadJson(errors).into_response(),
    };
    let now = Utc::now();
    if let Err(response) = save_snoozed(pool, &pk, &snoozed, &actor.id, &now).await {
        return response;
    }
    match fetch_one(pool, &slug, &actor.id, &pk).await {
        Ok(Some(row)) => json_ok(
            serde_json::to_string(&render_detail(&row, &actor.timezone)).expect("serializable row"),
        ),
        Ok(None) => Denial::ServerError.into_response(),
        Err(denial) => denial.into_response(),
    }
}

/// `serializer.save()`: write `snoozed_till` (+ `updated_at` /
/// `updated_by` through `save()`).
async fn save_snoozed(
    pool: &PgPool,
    pk: &Uuid,
    snoozed: &Option<DateTime<Utc>>,
    user_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), Response> {
    sqlx::query(
        r#"UPDATE notifications SET snoozed_till = $2, updated_at = $3, updated_by_id = $4
           WHERE id = $1"#,
    )
    .bind(pk)
    .bind(snoozed)
    .bind(now)
    .bind(user_id)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| Denial::ServerError.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four owned method+path rows: list + partial_update carry
    /// the workspace gate; retrieve + destroy are queryset-scoped
    /// (auth-only). Pins the [`gate`] wiring this module enforces.
    #[test]
    fn owned_routes_carry_their_gate_rows() {
        let list =
            gate_for("GET", "workspaces/<slug>/users/notifications/").expect("list gate row");
        assert!(matches!(list.gate, Gate::Workspace { .. }));
        let partial = gate_for("PATCH", "workspaces/<slug>/users/notifications/<uuid>/")
            .expect("partial_update gate row");
        assert!(matches!(partial.gate, Gate::Workspace { .. }));
        let retrieve = gate_for("GET", "workspaces/<slug>/users/notifications/<uuid>/")
            .expect("retrieve gate row");
        assert_eq!(retrieve.gate, Gate::QuerysetScoped);
        let destroy = gate_for("DELETE", "workspaces/<slug>/users/notifications/<uuid>/")
            .expect("destroy gate row");
        assert_eq!(destroy.gate, Gate::QuerysetScoped);
    }

    /// Facts mirror the gate matrix: ADMIN/MEMBER/GUEST pass the
    /// workspace gate, outsiders deny, anonymous never reaches it.
    #[test]
    fn workspace_facts_match_gate_matrix() {
        let scope = tenant_context("acme");
        for role in [Some(ROLE_ADMIN), Some(ROLE_MEMBER), Some(ROLE_GUEST)] {
            let outcome = decide_gate(
                &Gate::Workspace {
                    roles: &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
                },
                &scope,
                &workspace_facts("acme", role),
            );
            assert_eq!(outcome, GateOutcome::Allow);
        }
        assert_eq!(
            decide_gate(
                &Gate::Workspace {
                    roles: &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
                },
                &scope,
                &workspace_facts("acme", None),
            ),
            GateOutcome::Deny
        );
    }

    /// Denial bodies match the live Django probes byte for byte.
    #[test]
    fn denial_bodies_match_live_django() {
        assert_eq!(
            Denial::NotificationNotFound.status_and_body().1,
            r#"{"detail":"No Notification matches the given query."}"#
        );
        assert_eq!(
            Denial::ObjectNotFound.status_and_body().1,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            Denial::BadError("The required key does not exist.".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"The required key does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::BadDetail("Invalid cursor parameter.".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"detail":"Invalid cursor parameter."}"#.to_owned()
            )
        );
    }

    /// `snoozed_till` validation: null clears, RFC 3339 and naive
    /// wall times parse (naive as UTC), everything else is the exact
    /// DRF field error.
    #[test]
    fn snoozed_till_validation_mirrors_drf() {
        assert_eq!(parse_snoozed_till(None).expect("absent clears"), None);
        assert_eq!(
            parse_snoozed_till(Some(&Value::Null)).expect("null clears"),
            None
        );
        let parsed = parse_snoozed_till(Some(&Value::String("2027-05-05T00:00:00Z".to_owned())))
            .expect("rfc3339 parses");
        assert_eq!(
            parsed.map(|dt| dt.to_rfc3339()),
            Some("2027-05-05T00:00:00+00:00".to_owned())
        );
        let naive = parse_snoozed_till(Some(&Value::String("2027-05-05T00:00:00".to_owned())))
            .expect("naive parses as UTC");
        assert_eq!(
            naive.map(|dt| dt.to_rfc3339()),
            Some("2027-05-05T00:00:00+00:00".to_owned())
        );
        for bad in [
            Value::String("not-a-date".to_owned()),
            Value::String(String::new()),
            Value::Number(12345.into()),
            Value::Bool(true),
        ] {
            let errors = parse_snoozed_till(Some(&bad)).expect_err("invalid rejects");
            assert_eq!(
                errors,
                serde_json::json!({"snoozed_till": [INVALID_SNOOZED_MESSAGE]})
            );
        }
    }

    /// Filter branches: snoozed/archived KeyError on unknown values,
    /// read silent-ignore, mentioned presence rule.
    #[test]
    fn filter_branches_port_the_quirks() {
        assert!(snoozed_clause("true", "$3")
            .expect("true")
            .contains("IS NOT NULL"));
        assert!(snoozed_clause("false", "$3")
            .expect("false")
            .contains("IS NULL"));
        assert!(matches!(
            snoozed_clause("bogus", "$3"),
            Err(Denial::BadError(_))
        ));
        assert!(matches!(archived_clause("bogus"), Err(Denial::BadError(_))));
        assert_eq!(read_clause(None), None);
        assert_eq!(read_clause(Some("yes")), None);
        assert!(mentioned_clause(true).starts_with("n.\"sender\""));
        assert!(mentioned_clause(false).starts_with("NOT"));
    }

    /// `type=created` + sub-15 membership short-circuits to `.none()`.
    #[test]
    fn created_member_short_circuits() {
        assert!(matches!(
            list_type_filter("created", "$1", "$2", true),
            TypeFilter::Empty
        ));
        assert!(matches!(
            list_type_filter("all", "$1", "$2", false),
            TypeFilter::None
        ));
        let TypeFilter::Where(where_sql) =
            list_type_filter("subscribed,assigned", "$1", "$2", false)
        else {
            panic!("subscribed+assigned filters");
        };
        assert!(where_sql.contains(" OR "));
    }

    /// Single-instance responses omit the annotation keys (the
    /// `get_queryset` instances are unannotated, so DRF skips the
    /// read-only fields): 22 keys, verified live.
    #[test]
    fn detail_key_order_matches_live_django() {
        let body = serde_json::to_string(&DetailBody {
            id: "id".to_owned(),
            triggered_by_details: None,
            created_at: "t".to_owned(),
            updated_at: "t".to_owned(),
            deleted_at: None,
            data: None,
            entity_identifier: None,
            entity_name: "issue".to_owned(),
            title: "t".to_owned(),
            message: None,
            message_html: "<p></p>".to_owned(),
            message_stripped: None,
            sender: "s".to_owned(),
            read_at: None,
            snoozed_till: None,
            archived_at: None,
            created_by: None,
            updated_by: None,
            workspace: "w".to_owned(),
            project: None,
            triggered_by: None,
            receiver: "r".to_owned(),
        })
        .expect("body serializes");
        assert!(!body.contains("is_inbox_issue"));
        assert!(!body.contains("is_intake_issue"));
        assert!(!body.contains("is_mentioned_notification"));
        let mut last = 0;
        for key in [
            "id",
            "triggered_by_details",
            "created_at",
            "updated_at",
            "deleted_at",
            "data",
            "entity_identifier",
            "entity_name",
            "title",
            "message",
            "message_html",
            "message_stripped",
            "sender",
            "read_at",
            "snoozed_till",
            "archived_at",
            "created_by",
            "updated_by",
            "workspace",
            "project",
            "triggered_by",
            "receiver",
        ] {
            let needle = format!("\"{key}\":");
            let at = body[last..]
                .find(needle.as_str())
                .unwrap_or_else(|| panic!("key {key} missing or out of order"));
            last += at + needle.len();
        }
    }

    /// Rendered item keys follow the live wire order (`[pk] +
    /// declared + body`), not the services kernel order.
    #[test]
    fn item_key_order_matches_live_django() {
        let body = serde_json::to_string(&NotificationBody {
            id: "id".to_owned(),
            triggered_by_details: None,
            is_inbox_issue: false,
            is_intake_issue: false,
            is_mentioned_notification: false,
            created_at: "t".to_owned(),
            updated_at: "t".to_owned(),
            deleted_at: None,
            data: None,
            entity_identifier: None,
            entity_name: "issue".to_owned(),
            title: "t".to_owned(),
            message: None,
            message_html: "<p></p>".to_owned(),
            message_stripped: None,
            sender: "s".to_owned(),
            read_at: None,
            snoozed_till: None,
            archived_at: None,
            created_by: None,
            updated_by: None,
            workspace: "w".to_owned(),
            project: None,
            triggered_by: None,
            receiver: "r".to_owned(),
        })
        .expect("body serializes");
        let expected = [
            "id",
            "triggered_by_details",
            "is_inbox_issue",
            "is_intake_issue",
            "is_mentioned_notification",
            "created_at",
            "updated_at",
            "deleted_at",
            "data",
            "entity_identifier",
            "entity_name",
            "title",
            "message",
            "message_html",
            "message_stripped",
            "sender",
            "read_at",
            "snoozed_till",
            "archived_at",
            "created_by",
            "updated_by",
            "workspace",
            "project",
            "triggered_by",
            "receiver",
        ];
        let mut last = 0;
        for key in expected {
            let needle = format!("\"{key}\":");
            let at = body[last..]
                .find(needle.as_str())
                .unwrap_or_else(|| panic!("key {key} missing or out of order"));
            last += at + needle.len();
        }
    }
}
