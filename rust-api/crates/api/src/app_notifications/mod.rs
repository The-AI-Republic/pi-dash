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
//! All 7 routes are owned here: the viewset-core triplet (`GET` the
//! collection, `GET|PATCH|DELETE` the detail, PIDASHCONV-301), the
//! read/archive transitions + unread counts (PIDASHCONV-302), and
//! mark-all-read + the session-only preferences (PIDASHCONV-303, this
//! issue). Every other method on those paths proxies to Django —
//! route registration is the cutover granularity, no flag needed.
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
//! * QUIRK-mark-body-truthiness (`base.py:235-236`): `snoozed` /
//!   `archived` come from the JSON *body* (default `False`) but branch
//!   on Python truthiness — a `"false"` string is truthy while `0` /
//!   `null` / absent are falsy. Ported via [`data_truthy`].
//! * QUIRK-mark-type-spelling (`base.py:258-281`): the `type` values are
//!   the exact strings `watching` / `assigned` / `created` (default
//!   `"all"` = no filter) — not the list's comma-split `subscribed` /
//!   `assigned` / `created` — and the `watching` arm is the plain
//!   subscriber list with no created/assigned exclusion. The
//!   `role__lt=15` → `.none()` guard is shared with the list.
//! * QUIRK-preference-no-get-or-create (`base.py:296-308`): get/patch
//!   are a bare `.get(user=…)` (a miss is the `ObjectDoesNotExist`
//!   404); patch is partial, unknown keys are silently ignored (DRF
//!   iterates writable fields only — the FX-NOTIF-05 note saying they
//!   400 is wrong, verified against DRF 3.16 `to_internal_value`), and
//!   `save()` stamps `updated_by` through `BaseModel.save` + crum.
//! * NOTE-single-stamp: Python sets `read_at = timezone.now()` per row
//!   in a loop, then `bulk_update(["read_at"], batch_size=100)`
//!   (`updated_at` untouched, signals skipped); this port issues one
//!   `UPDATE` with a single `now()`. Touched rows share one stamp
//!   instead of microseconds-apart ones — unobservable through the API
//!   (the response body is constant).
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

/// Register the D-34 routes (all 7, `app/urls/notification.py:16-52`):
/// the viewset-core triplet (routes 1-3, `:17-31`, PIDASHCONV-301),
/// the state-transition and unread routes (routes 4-5, `:32-41`,
/// PIDASHCONV-302), plus mark-all-read (route 6, `:42-46`) and the
/// session-only preferences (route 7, `:47-51`, this issue,
/// PIDASHCONV-303). Merges keep both sides.
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
        .route(
            "/api/workspaces/{slug}/users/notifications/{pk}/read/",
            owned(
                axum::routing::post(mark_read).delete(mark_unread),
                &["GET", "PUT", "PATCH", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/users/notifications/{pk}/archive/",
            owned(
                axum::routing::post(archive).delete(unarchive),
                &["GET", "PUT", "PATCH", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/users/notifications/unread/",
            owned(
                axum::routing::get(unread_get),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/users/notifications/mark-all-read/",
            owned(
                axum::routing::post(mark_all_read_create),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/notification-preferences/",
            owned(
                axum::routing::get(preference_get).patch(preference_patch),
                &["POST", "PUT", "DELETE", "OPTIONS"],
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
#[allow(clippy::result_large_err)]
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
    parse_datetime_field(value, "snoozed_till")
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

/// `mark_read` (`base.py:163-169`): the workspace gate, then the
/// scoped bare `.get()` (a miss is the `ObjectDoesNotExist` 404 like
/// `partial_update`), then `read_at = timezone.now()` + `save()`
/// (which stamps `updated_at`/`updated_by`), and the full serializer
/// over the unannotated instance — the [`DetailBody`] 22-key shape —
/// 200.
async fn mark_read(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    state_transition(
        &state,
        extension,
        req,
        slug,
        pk_raw,
        Transition {
            gate_method: "POST",
            gate_path: "workspaces/<slug>/users/notifications/<uuid>/read/",
            column: StampColumn::Read,
            value: Some(Utc::now()),
        },
    )
    .await
}

/// `mark_unread` (`base.py:171-177`): like [`mark_read`] with
/// `read_at = None`.
async fn mark_unread(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    state_transition(
        &state,
        extension,
        req,
        slug,
        pk_raw,
        Transition {
            gate_method: "DELETE",
            gate_path: "workspaces/<slug>/users/notifications/<uuid>/read/",
            column: StampColumn::Read,
            value: None,
        },
    )
    .await
}

/// `archive` (`base.py:179-185`): like [`mark_read`] with
/// `archived_at = timezone.now()`.
async fn archive(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    state_transition(
        &state,
        extension,
        req,
        slug,
        pk_raw,
        Transition {
            gate_method: "POST",
            gate_path: "workspaces/<slug>/users/notifications/<uuid>/archive/",
            column: StampColumn::Archived,
            value: Some(Utc::now()),
        },
    )
    .await
}

/// `unarchive` (`base.py:187-193`): like [`mark_read`] with
/// `archived_at = None`.
async fn unarchive(
    State(state): State<AppState>,
    Path((slug, pk_raw)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    state_transition(
        &state,
        extension,
        req,
        slug,
        pk_raw,
        Transition {
            gate_method: "DELETE",
            gate_path: "workspaces/<slug>/users/notifications/<uuid>/archive/",
            column: StampColumn::Archived,
            value: None,
        },
    )
    .await
}

/// Which single timestamp column a state transition stamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StampColumn {
    Read,
    Archived,
}

/// The per-action half of a state transition: its gate row, the
/// column it stamps, and the value it writes (`Some(now)` for the
/// `POST` setters, `None` for the `DELETE` clearers).
#[derive(Debug, Clone, Copy)]
struct Transition {
    gate_method: &'static str,
    gate_path: &'static str,
    column: StampColumn,
    value: Option<DateTime<Utc>>,
}

/// Shared body of the four single-row state transitions
/// (`mark_read`/`mark_unread`/`archive`/`unarchive`,
/// `base.py:163-193`): session auth, the decorated workspace gate for
/// the transition's own method+path row, the receiver+workspace scoped
/// lookup (miss → the `ObjectDoesNotExist` 404 — these are bare
/// `.get()` calls, not `get_object`), the timestamp write through
/// `.save()`, and the re-fetched serializer, 200.
async fn state_transition(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
    slug: String,
    pk_raw: String,
    transition: Transition,
) -> Response {
    let Some(pk) = parse_pk(&pk_raw) else {
        return crate::edge::proxy(State(state.clone()), req).await;
    };
    let actor = match actor(state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let role = match workspace_role(pool, &actor.id, &slug).await {
        Ok(role) => role,
        Err(denial) => return denial.into_response(),
    };
    let row =
        gate_for(transition.gate_method, transition.gate_path).expect("state-transition gate row");
    if let Err(response) = enforce(decide_gate(
        &row.gate,
        &tenant_context(&slug),
        &workspace_facts(&slug, role),
    )) {
        return response;
    }
    match fetch_one(pool, &slug, &actor.id, &pk).await {
        Ok(Some(_)) => {}
        Ok(None) => return Denial::ObjectNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    }
    let now = Utc::now();
    if let Err(response) = save_stamp(
        pool,
        &pk,
        transition.column,
        &transition.value,
        &actor.id,
        &now,
    )
    .await
    {
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

/// `notification.save()` for the transitions: write the one timestamp
/// column (+ `updated_at`/`updated_by` through `save()`).
#[allow(clippy::result_large_err)]
async fn save_stamp(
    pool: &PgPool,
    pk: &Uuid,
    column: StampColumn,
    value: &Option<DateTime<Utc>>,
    user_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), Response> {
    let sql = match column {
        StampColumn::Read => {
            r#"UPDATE notifications SET read_at = $2, updated_at = $3, updated_by_id = $4
           WHERE id = $1"#
        }
        StampColumn::Archived => {
            r#"UPDATE notifications SET archived_at = $2, updated_at = $3, updated_by_id = $4
           WHERE id = $1"#
        }
    };
    sqlx::query(sql)
        .bind(pk)
        .bind(value)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|_| Denial::ServerError.into_response())
}

// ---------------------------------------------------------------------------
// UnreadNotificationEndpoint.get (base.py:196-229)
// ---------------------------------------------------------------------------

/// The exact 2-key `UnreadNotificationEndpoint.get` body
/// (`base.py:223-229`): `total_unread_notifications_count` first,
/// `mention_unread_notifications_count` second. Struct order is the
/// byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct UnreadBody {
    total_unread_notifications_count: i64,
    mention_unread_notifications_count: i64,
}

/// `UnreadNotificationEndpoint.get`: session auth, the workspace gate
/// (`:199`), then the two counts over the shared base
/// (receiver + unread + unarchived + unsnoozed, soft-delete-scoped).
/// The split is only `sender ILIKE '%mentioned%'` — `exclude` for the
/// total (`:210`), `filter` for the mentions (`:220`); there is no
/// `entity_name` guard here (unlike `list`), ported as-is. 200.
async fn unread_get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
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
    let row =
        gate_for("GET", "workspaces/<slug>/users/notifications/unread/").expect("unread gate row");
    if let Err(response) = enforce(decide_gate(
        &row.gate,
        &tenant_context(&slug),
        &workspace_facts(&slug, role),
    )) {
        return response;
    }
    let base = r#"FROM "notifications" n
         INNER JOIN "workspaces" w ON (n."workspace_id" = w."id")
         WHERE w."slug" = $1
         AND n."receiver_id" = $2
         AND n."read_at" IS NULL
         AND n."archived_at" IS NULL
         AND n."snoozed_till" IS NULL
         AND n."deleted_at" IS NULL"#;
    let total: i64 = match sqlx::query_scalar(&format!(
        r#"SELECT COUNT(*) {base} AND NOT (n."sender" ILIKE '%mentioned%')"#
    ))
    .bind(&slug)
    .bind(actor.id)
    .fetch_one(pool)
    .await
    {
        Ok(total) => total,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mentions: i64 = match sqlx::query_scalar(&format!(
        r#"SELECT COUNT(*) {base} AND n."sender" ILIKE '%mentioned%'"#
    ))
    .bind(&slug)
    .bind(actor.id)
    .fetch_one(pool)
    .await
    {
        Ok(mentions) => mentions,
        Err(_) => return Denial::ServerError.into_response(),
    };
    json_ok(
        serde_json::to_string(&UnreadBody {
            total_unread_notifications_count: total,
            mention_unread_notifications_count: mentions,
        })
        .expect("serializable counts"),
    )
}

/// `serializer.save()`: write `snoozed_till` (+ `updated_at` /
/// `updated_by` through `save()`).
#[allow(clippy::result_large_err)]
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

// ---------------------------------------------------------------------------
// MarkAllReadNotificationViewSet.create (base.py:232-288)
// ---------------------------------------------------------------------------

/// The exact constant 200 body (`base.py:288`): DRF's compact renderer
/// (`COMPACT_JSON` is the default and the project does not override it)
/// emits `{"message":"Successful"}` however many rows were touched —
/// including zero (an empty `bulk_update([])` is a no-op).
const MARK_ALL_READ_BODY: &str = r#"{"message":"Successful"}"#;

/// Python truthiness of one `request.data` value (`base.py:235-237`):
/// absent / `null` / `false` / `0` / `""` / `[]` / `{}` are falsy and
/// everything else — including the string `"false"` — is truthy
/// (QUIRK-mark-body-truthiness).
fn data_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}

/// `watching` type arm (mark-all-read, `base.py:258-262`): the plain
/// subscriber issue list — unlike the list's `subscribed` arm there is
/// no created/assigned exclusion (QUIRK-mark-type-spelling).
fn watching_arm(slug_param: &str, user_param: &str) -> String {
    format!(
        "n.\"entity_identifier\" IN (SELECT s.\"issue_id\" FROM \"issue_subscribers\" s \
         INNER JOIN \"workspaces\" ws ON (s.\"workspace_id\" = ws.\"id\") \
         WHERE (s.\"deleted_at\" IS NULL AND s.\"subscriber_id\" = {user_param} AND ws.\"slug\" = {slug_param}))"
    )
}

/// `MarkAllReadNotificationViewSet.create`: session auth, the workspace
/// gate (`:233`), then one `UPDATE` setting `read_at` over the filtered
/// slice (receiver + unread + soft-delete-scoped, the snoozed/archived
/// branches, and the exact-match `type` arm). A `type=created` caller
/// with a sub-15 active membership updates nothing but still answers
/// 200 (the shared [`created_member_guard`], `:273-276`). The constant
/// body answers 200.
async fn mark_all_read_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
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
    let row = gate_for(
        "POST",
        "workspaces/<slug>/users/notifications/mark-all-read/",
    )
    .expect("mark-all-read gate row");
    if let Err(response) = enforce(decide_gate(
        &row.gate,
        &tenant_context(&slug),
        &workspace_facts(&slug, role),
    )) {
        return response;
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
    let snoozed = data_truthy(data.get("snoozed"));
    let archived = data_truthy(data.get("archived"));
    let type_param = match data.get("type") {
        Some(Value::String(text)) => text.clone(),
        _ => "all".to_owned(),
    };
    if type_param == "created" {
        match created_member_guard(pool, &slug, &actor.id).await {
            Ok(true) => return json_ok(MARK_ALL_READ_BODY.to_owned()),
            Ok(false) => {}
            Err(denial) => return denial.into_response(),
        }
    }
    let snoozed_sql = match snoozed_clause(if snoozed { "true" } else { "false" }, "$3") {
        Ok(clause) => clause,
        Err(denial) => return denial.into_response(),
    };
    let archived_sql = match archived_clause(if archived { "true" } else { "false" }) {
        Ok(clause) => clause,
        Err(denial) => return denial.into_response(),
    };
    // `$1` slug, `$2` user, `$3` now — the same placeholders the
    // reused arm builders expect.
    let mut sql = format!(
        "UPDATE \"notifications\" n SET \"read_at\" = $3 FROM \"workspaces\" w \
         WHERE (n.\"workspace_id\" = w.\"id\") AND w.\"slug\" = $1 AND n.\"receiver_id\" = $2 \
         AND n.\"read_at\" IS NULL AND n.\"deleted_at\" IS NULL \
         AND {snoozed_sql} AND {archived_sql}"
    );
    match type_param.as_str() {
        "watching" => sql.push_str(&format!(" AND {}", watching_arm("$1", "$2"))),
        "assigned" => sql.push_str(&format!(" AND {}", assigned_arm("$1", "$2"))),
        "created" => sql.push_str(&format!(" AND {}", created_arm("$1", "$2"))),
        _ => {}
    }
    let now = Utc::now();
    if sqlx::query(&sql)
        .bind(&slug)
        .bind(actor.id)
        .bind(now)
        .execute(pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    json_ok(MARK_ALL_READ_BODY.to_owned())
}

// ---------------------------------------------------------------------------
// UserNotificationPreferenceEndpoint (base.py:291-308)
// ---------------------------------------------------------------------------

/// One `user_notification_preferences` row: the audit columns plus the
/// preference owner and the five flags (`db/models/notification.py:81-108`).
struct PreferenceRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    user_id: Uuid,
    workspace_id: Option<Uuid>,
    project_id: Option<Uuid>,
    property_change: bool,
    state_change: bool,
    comment: bool,
    mention: bool,
    issue_completed: bool,
}

impl PreferenceRow {
    fn get(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            deleted_at: row.try_get("deleted_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            user_id: row.try_get("user_id")?,
            workspace_id: row.try_get("workspace_id")?,
            project_id: row.try_get("project_id")?,
            property_change: row.try_get("property_change")?,
            state_change: row.try_get("state_change")?,
            comment: row.try_get("comment")?,
            mention: row.try_get("mention")?,
            issue_completed: row.try_get("issue_completed")?,
        })
    }
}

/// `SELECT` list for preference rows, in [`PreferenceRow`] order.
const PREFERENCE_SELECT: &str = r#""id", "created_at", "updated_at", "deleted_at", "created_by_id", "updated_by_id", "user_id", "workspace_id", "project_id", "property_change", "state_change", "comment", "mention", "issue_completed""#;

/// Scoped preference lookup: `UserNotificationPreference.objects.get(
/// user=request.user)` (`base.py:297,303`) — the default manager, so
/// soft-deleted rows are invisible. Zero rows is `None` (the caller
/// answers the `ObjectDoesNotExist` 404); two or more rows is a
/// `MultipleObjectsReturned` 500, like Django.
async fn fetch_preference(pool: &PgPool, user_id: &Uuid) -> Result<Option<PreferenceRow>, Denial> {
    let sql = format!(
        "SELECT {PREFERENCE_SELECT} FROM \"user_notification_preferences\" \
         WHERE \"user_id\" = $1 AND \"deleted_at\" IS NULL"
    );
    let rows = sqlx::query(&sql)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match rows.len() {
        0 => Ok(None),
        1 => PreferenceRow::get(&rows[0])
            .map(Some)
            .map_err(|_| Denial::ServerError),
        _ => Err(Denial::ServerError),
    }
}

/// Re-read after a patch write by primary key: patch may have moved
/// the row to another user, so the owner lookup no longer applies —
/// Python re-renders the same instance (`base.py:306-307`).
async fn fetch_preference_by_id(pool: &PgPool, id: &Uuid) -> Result<Option<PreferenceRow>, Denial> {
    let sql = format!(
        "SELECT {PREFERENCE_SELECT} FROM \"user_notification_preferences\" WHERE \"id\" = $1"
    );
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        Some(row) => PreferenceRow::get(&row)
            .map(Some)
            .map_err(|_| Denial::ServerError),
        None => Ok(None),
    }
}

/// `UserNotificationPreferenceSerializer` output (`fields="__all__"`,
/// `app/serializers/notification.py:25-28`) in the live key order: pk +
/// declared (`BaseSerializer.id`), then the non-relational model fields,
/// then the forward relations (DRF `get_default_field_names`) — the five
/// flags render before the FKs, exactly like `NotificationBody`. (The
/// FX-NOTIF-05 `output_keys` array lists `_meta` order; it is the key
/// *set*, not the wire order.) Struct order is the byte order.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct PreferenceBody {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    property_change: bool,
    state_change: bool,
    comment: bool,
    mention: bool,
    issue_completed: bool,
    created_by: Option<String>,
    updated_by: Option<String>,
    user: String,
    workspace: Option<String>,
    project: Option<String>,
}

/// Render one preference row: datetimes in the request's zone
/// (`TimezoneMixin.initial`), FKs as UUID strings with nulls as
/// `null`, flags as booleans.
fn render_preference(row: &PreferenceRow, tz: &Tz) -> PreferenceBody {
    PreferenceBody {
        id: row.id.to_string(),
        created_at: render_datetime_in(&row.created_at, tz),
        updated_at: render_datetime_in(&row.updated_at, tz),
        deleted_at: row.deleted_at.as_ref().map(|dt| render_datetime_in(dt, tz)),
        property_change: row.property_change,
        state_change: row.state_change,
        comment: row.comment,
        mention: row.mention,
        issue_completed: row.issue_completed,
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        user: row.user_id.to_string(),
        workspace: row.workspace_id.map(|id| id.to_string()),
        project: row.project_id.map(|id| id.to_string()),
    }
}

/// `preference_get` (`base.py:296-299`): no decorator, so session auth
/// is the only check ([`Gate::Authenticated`]) — any logged-in user
/// reaches the handler, with no workspace lookup at all.
async fn preference_get(
    State(state): State<AppState>,
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
    match fetch_preference(pool, &actor.id).await {
        Ok(Some(row)) => json_ok(
            serde_json::to_string(&render_preference(&row, &actor.timezone))
                .expect("serializable preference"),
        ),
        Ok(None) => Denial::ObjectNotFound.into_response(),
        Err(denial) => denial.into_response(),
    }
}

/// DRF `BooleanField` invalid-input message (`fields.py:659-662`).
const INVALID_BOOLEAN_MESSAGE: &str = "Must be a valid boolean.";
/// DRF `Field` null-input message (`fields.py:293`).
const NULL_FIELD_MESSAGE: &str = "This field may not be null.";

/// DRF `BooleanField.to_internal_value` (`fields.py:699-711`): the
/// case-insensitive true/false sets (note `1`/`1.0` count as true,
/// `0`/`0.0` as false); `null`/`""` only pass with `allow_null`
/// (these columns are `null=False`, so the caller rejects them first).
/// Everything else is invalid.
fn parse_preference_bool(value: &Value) -> Result<bool, ()> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_f64() == Some(1.0) {
                Ok(true)
            } else if number.as_i64() == Some(0) || number.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err(())
            }
        }
        Value::String(text) => match text.to_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(()),
        },
        _ => Err(()),
    }
}

/// One `{"field": ["message"]}` entry, the serializer-errors shape
/// (`base.py:308`).
fn field_error(message: &str) -> Value {
    Value::Array(vec![Value::String(message.to_owned())])
}

/// Validate one preference flag field: absent stays absent (partial,
/// `base.py:304`); `null` is the null-input error; anything outside
/// the DRF truth tables is the invalid-boolean error.
fn check_preference_flag(
    errors: &mut serde_json::Map<String, Value>,
    data: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<bool> {
    let value = data.get(key)?;
    if value.is_null() {
        errors.insert(key.to_owned(), field_error(NULL_FIELD_MESSAGE));
        return None;
    }
    match parse_preference_bool(value) {
        Ok(flag) => Some(flag),
        Err(()) => {
            errors.insert(key.to_owned(), field_error(INVALID_BOOLEAN_MESSAGE));
            None
        }
    }
}

/// The JSON type name DRF reports for a non-pk value
/// (`relations.py:263`, `type(data).__name__`).
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
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

/// How a preference PATCH compile ends: field errors answer 400,
/// an existence-probe failure answers 500.
enum CompileError {
    Invalid(Value),
    Server,
}

/// Validate one preference FK field (`PrimaryKeyRelatedField`,
/// `relations.py:256-263`): absent stays absent (partial); `null`
/// clears when the column allows it, else the null-input error; a
/// well-formed UUID that names no row is the does-not-exist error; a
/// malformed UUID string is Django's UUID-input error; anything else
/// is the incorrect-type error. Returns `Ok(None)` for absent (or
/// invalid — the caller checks `errors`), `Ok(Some(id-or-null))` to
/// write, and `Err(CompileError::Server)` only when the existence
/// probe itself fails.
async fn check_preference_fk(
    pool: &PgPool,
    errors: &mut serde_json::Map<String, Value>,
    data: &serde_json::Map<String, Value>,
    key: &str,
    table: &str,
    allow_null: bool,
    soft_scoped: bool,
) -> Result<Option<Option<Uuid>>, CompileError> {
    let Some(value) = data.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        if allow_null {
            return Ok(Some(None));
        }
        errors.insert(key.to_owned(), field_error(NULL_FIELD_MESSAGE));
        return Ok(None);
    }
    let Value::String(text) = value else {
        errors.insert(
            key.to_owned(),
            field_error(&format!(
                "Incorrect type. Expected pk value, received {}.",
                json_type_name(value)
            )),
        );
        return Ok(None);
    };
    let Ok(id) = text.parse::<Uuid>() else {
        errors.insert(
            key.to_owned(),
            field_error(&format!("\u{201c}{text}\u{201d} is not a valid UUID.")),
        );
        return Ok(None);
    };
    let mut sql = format!("SELECT 1 AS \"one\" FROM \"{table}\" WHERE \"id\" = $1");
    if soft_scoped {
        sql.push_str(" AND \"deleted_at\" IS NULL");
    }
    let found: Option<(i32,)> = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| CompileError::Server)?;
    if found.is_none() {
        errors.insert(
            key.to_owned(),
            field_error(&format!("Invalid pk \"{text}\" - object does not exist.")),
        );
        return Ok(None);
    }
    Ok(Some(Some(id)))
}

/// Validated preference PATCH writes: each `Some` replaces its column;
/// `None` leaves it. Nullable columns use the inner `Option` (`None`
/// clears the column). There is deliberately no `updated_by` slot: a
/// body-supplied value is validated (a malformed one still 400s) but
/// never written — `BaseModel.save` unconditionally overwrites it with
/// the crum user on update, so the stamp always wins.
#[derive(Debug, Default)]
struct PreferencePatch {
    deleted_at: Option<Option<DateTime<Utc>>>,
    created_by: Option<Option<Uuid>>,
    user: Option<Uuid>,
    workspace: Option<Option<Uuid>>,
    project: Option<Option<Uuid>>,
    property_change: Option<bool>,
    state_change: Option<bool>,
    comment: Option<bool>,
    mention: Option<bool>,
    issue_completed: Option<bool>,
}

/// Compile a preference PATCH body (`base.py:302-305`): partial, so
/// absent keys are skipped; unknown and read-only keys (`id`,
/// `created_at`, `updated_at`) are silently ignored — DRF iterates
/// writable fields only (`serializers.py:481`). Errors accumulate in
/// serializer field order and answer 400 as
/// `{"field": ["message", …]}` (`base.py:308`).
async fn compile_preference_patch(
    pool: &PgPool,
    data: &serde_json::Map<String, Value>,
) -> Result<PreferencePatch, CompileError> {
    let mut errors = serde_json::Map::new();
    let mut patch = PreferencePatch::default();
    if let Some(value) = data.get("deleted_at") {
        match parse_datetime_field(Some(value), "deleted_at") {
            Ok(stamp) => patch.deleted_at = Some(stamp),
            Err(invalid) => {
                if let Value::Object(fields) = invalid {
                    errors.extend(fields);
                }
            }
        }
    }
    // Each flag helper returns `Some` exactly when its key is present
    // and valid, so plain assignment is exact (absent and invalid both
    // leave the patch slot untouched; invalid additionally records).
    patch.property_change = check_preference_flag(&mut errors, data, "property_change");
    patch.state_change = check_preference_flag(&mut errors, data, "state_change");
    patch.comment = check_preference_flag(&mut errors, data, "comment");
    patch.mention = check_preference_flag(&mut errors, data, "mention");
    patch.issue_completed = check_preference_flag(&mut errors, data, "issue_completed");
    if let Some(id) =
        check_preference_fk(pool, &mut errors, data, "created_by", "users", true, false).await?
    {
        patch.created_by = Some(id);
    }
    // `updated_by` is validated but never stored: `BaseModel.save`
    // overwrites it with the crum user on every update
    // (`db/models/base.py:40-42`), so the stamp wins and writing both
    // would be a duplicate-`SET` Postgres error.
    check_preference_fk(pool, &mut errors, data, "updated_by", "users", true, false).await?;
    if let Some(Some(id)) =
        check_preference_fk(pool, &mut errors, data, "user", "users", false, false).await?
    {
        patch.user = Some(id);
    }
    if let Some(id) = check_preference_fk(
        pool,
        &mut errors,
        data,
        "workspace",
        "workspaces",
        true,
        true,
    )
    .await?
    {
        patch.workspace = Some(id);
    }
    if let Some(id) =
        check_preference_fk(pool, &mut errors, data, "project", "projects", true, true).await?
    {
        patch.project = Some(id);
    }
    if errors.is_empty() {
        Ok(patch)
    } else {
        Err(CompileError::Invalid(Value::Object(errors)))
    }
}

/// One validated PATCH column write.
enum PreferenceValue {
    Stamp(Option<DateTime<Utc>>),
    Id(Option<Uuid>),
    Flag(bool),
}

/// `serializer.save()` (`base.py:306`): write the validated columns
/// plus `updated_at`/`updated_by` (`BaseModel.save` stamps the updater
/// through crum on every update — `db/models/base.py:23-44`).
#[allow(clippy::result_large_err)]
async fn save_preference(
    pool: &PgPool,
    id: &Uuid,
    patch: &PreferencePatch,
    user_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), Response> {
    let mut columns: Vec<(&str, PreferenceValue)> = Vec::new();
    if let Some(stamp) = patch.deleted_at {
        columns.push(("deleted_at", PreferenceValue::Stamp(stamp)));
    }
    if let Some(owner) = patch.created_by {
        columns.push(("created_by_id", PreferenceValue::Id(owner)));
    }
    if let Some(owner) = patch.user {
        columns.push(("user_id", PreferenceValue::Id(Some(owner))));
    }
    if let Some(workspace) = patch.workspace {
        columns.push(("workspace_id", PreferenceValue::Id(workspace)));
    }
    if let Some(project) = patch.project {
        columns.push(("project_id", PreferenceValue::Id(project)));
    }
    for (key, flag) in [
        ("property_change", patch.property_change),
        ("state_change", patch.state_change),
        ("comment", patch.comment),
        ("mention", patch.mention),
        ("issue_completed", patch.issue_completed),
    ] {
        if let Some(flag) = flag {
            columns.push((key, PreferenceValue::Flag(flag)));
        }
    }
    let mut sql = String::from(
        "UPDATE \"user_notification_preferences\" SET \"updated_at\" = $2, \"updated_by_id\" = $3",
    );
    for (index, (column, _)) in columns.iter().enumerate() {
        sql.push_str(&format!(", \"{column}\" = ${}", index + 4));
    }
    sql.push_str(" WHERE \"id\" = $1");
    let mut query = sqlx::query(&sql).bind(id).bind(now).bind(user_id);
    for (_, value) in columns {
        query = match value {
            PreferenceValue::Stamp(stamp) => query.bind(stamp),
            PreferenceValue::Id(owner) => query.bind(owner),
            PreferenceValue::Flag(flag) => query.bind(flag),
        };
    }
    query
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|_| Denial::ServerError.into_response())
}

/// `preference_patch` (`base.py:302-308`): the owner lookup runs first
/// (a miss 404s even with a malformed payload), then the partial
/// serializer compile (400 on invalid), then `save()` and the
/// re-rendered row, 200. Like get, session auth is the only gate.
async fn preference_patch(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let current = match fetch_preference(pool, &actor.id).await {
        Ok(Some(row)) => row,
        Ok(None) => return Denial::ObjectNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    let (_parts, body) = req.into_parts();
    let raw = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(raw) => raw,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let data = match parse_body(&raw) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let Value::Object(fields) = data else {
        return Denial::ServerError.into_response();
    };
    let patch = match compile_preference_patch(pool, &fields).await {
        Ok(patch) => patch,
        Err(CompileError::Invalid(errors)) => return Denial::BadJson(errors).into_response(),
        Err(CompileError::Server) => return Denial::ServerError.into_response(),
    };
    let now = Utc::now();
    if let Err(response) = save_preference(pool, &current.id, &patch, &actor.id, &now).await {
        return response;
    }
    match fetch_preference_by_id(pool, &current.id).await {
        Ok(Some(row)) => json_ok(
            serde_json::to_string(&render_preference(&row, &actor.timezone))
                .expect("serializable preference"),
        ),
        Ok(None) => Denial::ServerError.into_response(),
        Err(denial) => denial.into_response(),
    }
}

/// Validate an optional datetime input like DRF's `DateTimeField`
/// (default `iso-8601` input formats, JSON null clears the column when
/// the model allows it): null → `None`; RFC 3339 / naive
/// `YYYY-MM-DD[T ]hh:mm:ss[.f]` (naive read as UTC — `TIME_ZONE` is
/// UTC) → the instant; the `strptime(value, 'iso-8601')` fallthrough
/// literal → naive 1900-01-01 as UTC; anything else → the exact 400
/// field error keyed by `field`.
fn parse_datetime_field(
    value: Option<&Value>,
    field: &str,
) -> Result<Option<DateTime<Utc>>, Value> {
    let invalid = || {
        Value::Object(
            [(
                field.to_owned(),
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
    // DRF's `strptime(value, 'iso-8601')` fallthrough (`to_internal_value`
    // runs it when `parse_datetime` returns `None` — PIDASHCONV-773):
    // the literal matches case-insensitively and yields naive
    // 1900-01-01, read as UTC like every naive input here. Exact match:
    // padding fails on both sides (probed); ASCII-only (765
    // unicode-gap family).
    if raw.eq_ignore_ascii_case("iso-8601") {
        let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("1900-01-01 valid");
        return Ok(Some(naive.and_utc()));
    }
    Err(invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The twelve owned method+path rows: list + partial_update + the
    /// four read/archive transitions + unread + mark-all-read carry
    /// the workspace gate; retrieve + destroy are queryset-scoped
    /// (auth-only); the two preference rows are session-only
    /// (`Gate::Authenticated`). Pins the [`gate`] wiring this module
    /// enforces.
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
        for (method, path) in [
            ("POST", "workspaces/<slug>/users/notifications/<uuid>/read/"),
            (
                "DELETE",
                "workspaces/<slug>/users/notifications/<uuid>/read/",
            ),
            (
                "POST",
                "workspaces/<slug>/users/notifications/<uuid>/archive/",
            ),
            (
                "DELETE",
                "workspaces/<slug>/users/notifications/<uuid>/archive/",
            ),
            ("GET", "workspaces/<slug>/users/notifications/unread/"),
            (
                "POST",
                "workspaces/<slug>/users/notifications/mark-all-read/",
            ),
        ] {
            let row = gate_for(method, path).expect("transition/unread gate row");
            assert!(
                matches!(row.gate, Gate::Workspace { .. }),
                "{method} {path}"
            );
        }
        for (method, path) in [
            ("GET", "users/me/notification-preferences/"),
            ("PATCH", "users/me/notification-preferences/"),
        ] {
            let row = gate_for(method, path).expect("preference gate row");
            assert_eq!(row.gate, Gate::Authenticated, "{method} {path}");
        }
    }

    /// The unread body carries exactly the two live keys in the live
    /// order (`base.py:223-229`).
    #[test]
    fn unread_body_matches_live_key_order() {
        let body = UnreadBody {
            total_unread_notifications_count: 1,
            mention_unread_notifications_count: 2,
        };
        assert_eq!(
            serde_json::to_string(&body).expect("serializable counts"),
            r#"{"total_unread_notifications_count":1,"mention_unread_notifications_count":2}"#,
        );
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

    #[test]
    fn datetime_field_iso8601_literal_fallback() {
        // PIDASHCONV-773: DRF `to_internal_value` falls through to
        // `strptime(value, 'iso-8601')` when `parse_datetime` returns None;
        // the literal matches case-insensitively and yields naive
        // 1900-01-01, read as UTC (probed live both backends).
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            let value = Value::String(text.to_owned());
            let parsed = parse_datetime_field(Some(&value), "deleted_at")
                .expect(text)
                .expect("some");
            assert_eq!(parsed.to_rfc3339(), "1900-01-01T00:00:00+00:00", "{text:?}");
        }
        // Near-misses stay invalid (exact match, both sides probed).
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            let value = Value::String(text.to_owned());
            assert!(
                parse_datetime_field(Some(&value), "deleted_at").is_err(),
                "{text:?}"
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

    /// mark-all-read answers the exact constant body (`base.py:288`).
    #[test]
    fn mark_all_read_body_is_constant() {
        assert_eq!(MARK_ALL_READ_BODY, r#"{"message":"Successful"}"#);
    }

    /// Body-param truthiness follows Python, not JSON: the string
    /// `"false"` is truthy, `0` / `null` / absent are falsy
    /// (`base.py:235-236`).
    #[test]
    fn body_truthiness_mirrors_python() {
        assert!(!data_truthy(None));
        assert!(!data_truthy(Some(&Value::Null)));
        assert!(!data_truthy(Some(&Value::Bool(false))));
        assert!(data_truthy(Some(&Value::Bool(true))));
        assert!(!data_truthy(Some(&serde_json::json!(0))));
        assert!(!data_truthy(Some(&serde_json::json!(0.0))));
        assert!(data_truthy(Some(&serde_json::json!(1))));
        assert!(!data_truthy(Some(&Value::String(String::new()))));
        assert!(data_truthy(Some(&Value::String("false".to_owned()))));
        assert!(!data_truthy(Some(&Value::Array(vec![]))));
        assert!(data_truthy(Some(&serde_json::json!([false]))));
        assert!(!data_truthy(Some(&Value::Object(Default::default()))));
    }

    /// The `watching` arm is the plain subscriber list (no
    /// created/assigned exclusion, unlike the list's `subscribed` arm)
    /// over the caller's placeholders (`base.py:258-262`).
    #[test]
    fn watching_arm_is_plain_subscribers() {
        let arm = watching_arm("$1", "$2");
        assert!(arm.contains("FROM \"issue_subscribers\""));
        assert!(arm.contains("s.\"subscriber_id\" = $2"));
        assert!(arm.contains("ws.\"slug\" = $1"));
        assert!(!arm.contains("created_by"));
        assert!(!arm.contains("assignee"));
    }

    /// Preference booleans follow DRF's truth tables, including the
    /// numeric spellings (`fields.py:665-686`).
    #[test]
    fn preference_bools_mirror_drf_tables() {
        for truthy in [
            Value::Bool(true),
            serde_json::json!(1),
            serde_json::json!(1.0),
            Value::String("True".to_owned()),
            Value::String("ON".to_owned()),
            Value::String("y".to_owned()),
        ] {
            assert_eq!(parse_preference_bool(&truthy), Ok(true));
        }
        for falsy in [
            Value::Bool(false),
            serde_json::json!(0),
            serde_json::json!(0.0),
            Value::String("False".to_owned()),
            Value::String("OFF".to_owned()),
            Value::String("n".to_owned()),
        ] {
            assert_eq!(parse_preference_bool(&falsy), Ok(false));
        }
        for bad in [
            Value::Null,
            Value::String(String::new()),
            Value::String("maybe".to_owned()),
            serde_json::json!(2),
            serde_json::json!([]),
        ] {
            assert_eq!(parse_preference_bool(&bad), Err(()));
        }
    }

    /// Flag validation: absent skips, `null` is the null-input error,
    /// anything outside the DRF tables is the invalid-boolean error
    /// (the serializer-errors shape, `base.py:308`).
    #[test]
    fn preference_flag_errors_match_drf() {
        let data = serde_json::json!({"mention": "maybe", "comment": null});
        let fields = data.as_object().expect("object body");
        let mut errors = serde_json::Map::new();
        assert_eq!(
            check_preference_flag(&mut errors, fields, "property_change"),
            None
        );
        assert!(errors.is_empty());
        assert_eq!(check_preference_flag(&mut errors, fields, "mention"), None);
        assert_eq!(check_preference_flag(&mut errors, fields, "comment"), None);
        assert_eq!(
            errors,
            serde_json::json!({
                "mention": ["Must be a valid boolean."],
                "comment": ["This field may not be null."],
            })
            .as_object()
            .expect("object errors")
            .clone()
        );
    }

    /// The preference body carries the 14 live keys in the live order:
    /// pk + declared, then the non-relational fields, then the forward
    /// relations (DRF `get_default_field_names`) — flags before FKs.
    #[test]
    fn preference_key_order_matches_live_django() {
        let body = serde_json::to_string(&PreferenceBody {
            id: "id".to_owned(),
            created_at: "t".to_owned(),
            updated_at: "t".to_owned(),
            deleted_at: None,
            property_change: true,
            state_change: true,
            comment: true,
            mention: true,
            issue_completed: true,
            created_by: None,
            updated_by: None,
            user: "u".to_owned(),
            workspace: None,
            project: None,
        })
        .expect("body serializes");
        let mut last = 0;
        for key in [
            "id",
            "created_at",
            "updated_at",
            "deleted_at",
            "property_change",
            "state_change",
            "comment",
            "mention",
            "issue_completed",
            "created_by",
            "updated_by",
            "user",
            "workspace",
            "project",
        ] {
            let needle = format!("\"{key}\":");
            let at = body[last..]
                .find(needle.as_str())
                .unwrap_or_else(|| panic!("key {key} missing or out of order"));
            last += at + needle.len();
        }
    }
}
