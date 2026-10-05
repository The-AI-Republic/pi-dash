#![forbid(unsafe_code)]

//! Sticky handlers (D-21, stage 5, PIDASHCONV-423).
//!
//! Ports the five `StickyViewSet` actions from
//! `apps/api/pi_dash/api/views/sticky.py:48-113`:
//!
//! - `create` (`:48-54`): workspace lookup by slug, `StickySerializer`
//!   validation, save with workspace + owner, 201.
//! - `list` (`:66-77`): `query` param -> `description_stripped__icontains`,
//!   `-created_at` order, `paginate(..., default_per_page=20)`.
//! - `retrieve` (`:85-87`): `get_object` + `StickySerializer`, 200.
//! - `partial_update` (`:96-102`): partial serializer save,
//!   200 / 400 errors verbatim.
//! - `destroy` (`:110-113`): soft `delete()` + related sweep task, 204.
//!
//! Routes (`apps/api/pi_dash/api/urls/sticky.py:1-13`, `DefaultRouter`
//! `stickies` under `workspaces/<slug>/`, basename `workspace-stickies`):
//! `POST`/`GET /api/v1/workspaces/<slug>/stickies/` and
//! `GET`/`PATCH`/`DELETE /api/v1/workspaces/<slug>/stickies/<pk>/`.
//! Every other method on those paths (notably `PUT`, whose DRF-default
//! `update` this issue does not port) proxies to Django via [`routes`].
//! Mount wiring into [`crate::overlay`] / [`crate::routes`] belongs to the
//! intake handler issue (PIDASHCONV-426); this module only builds its own
//! router.
//!
//! Every action carries `WorkspaceUserPermission`
//! (`views/sticky.py:28`; gate in [`super::permissions`]); rows are
//! additionally owner-scoped by `get_queryset`
//! (`sticky.py:30-37`: `workspace__slug` + `owner_id`, `distinct()`).
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-h-sticky.json` (`fx-h-sticky`);
//! consumed `fx-ser-sticky` (validation), `fx-q-sticky` (SQL),
//! `fx-model-sticky` (columns + save rules), `fx-perm` (gates).
//! Layer map (foundation crates are read-only):
//!
//! - validation kernel: `pidash_types::v1_assets::sticky`
//!   (`validate_sticky_descriptions`, `strip_ignored_sticky_keys`);
//! - list SQL: `pidash_db::v1_assets::sticky_queries`
//!   (`sticky_list_sql`, `icontains_pattern`);
//! - columns/defaults/save rules: `pidash_db::v1_assets::model::sticky`
//!   (`COLUMNS`, `stripped_description`, `sort_order_on_create`);
//! - gates/denial bodies: [`super::permissions`]
//!   (`decide_sticky_gate`, `CLASS_DENIAL_BODY`);
//! - paginator envelope math: [`crate::paginator`]; envelope text:
//!   `pidash_services::app_issues::envelope` (the same 12-key
//!   `BasePaginator.paginate` shape every list path renders).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * Destroy SOFT-deletes: `sticky.py:112` calls instance `.delete()`,
//!   which resolves to `SoftDeleteModel.delete(soft=True)`
//!   (`db/mixins.py:72-82`) — the row is TOMBSTONED (plus an
//!   `updated_at`/`updated_by` bump from the inner `save()`), not
//!   removed, and `soft_delete_related_objects.delay("db", "sticky", pk,
//!   using=None)` fires. NOTE: `fx-h-sticky` prose claims a hard delete
//!   ("row is REMOVED"); that transcription misses the instance-method
//!   override (it reasons about the manager only), so the source wins
//!   here and the fixture text stands corrected.
//! * Create looks up the workspace through the plain default manager
//!   (`Workspace.objects.get(slug=slug)` — `deleted_at IS NULL`, no
//!   `is_active` notion on workspaces). Ported as-is.
//! * Concurrent creates read the same `MAX(sort_order)` and collide (no
//!   locking in `models/sticky.py:47-52`). Ported as-is.
//! * `description_binary` is read-only-dead (a `BinaryField` maps to a
//!   read-only field, so validated data never carries it); input under
//!   that key is silently dropped and a stored value 500s on read,
//!   exactly like Django (see [`render_sticky_row`]).
//! * Read-only writes (`workspace`, `owner`, plus DRF auto-fields) are
//!   silently dropped, not 400 (live DRF `to_internal_value` iterates
//!   `_writable_fields`; the fixture's bare-string bodies are the
//!   pre-live transcription — the types kernel documents the divergence).
//! * `validate()` error envelopes are list-wrapped by DRF's
//!   `as_serializer_error`
//!   (`{"error": ["html content is not valid"]}`), via the types kernel.
//! * The audit columns are writable (`editable`, not in
//!   `read_only_fields`): `created_by` input persists on update (create
//!   overwrites it from the request user), `updated_by` input validates
//!   and is then overwritten, `deleted_at` input persists (a create can
//!   be born deleted; an update can tombstone), and
//!   `description_stripped` input validates and is then recomputed.
//! * Badly-formed detail pks are 404, not 400: DRF's `get_object_or_404`
//!   converts the UUID `ValidationError` to `Http404`.
//! * `CharField` inputs coerce (`str()` of ints/floats), strip, and blank
//!   to `""`; over-long-with-NUL input reports both validator messages.
//!
//! # Known micro-gaps (absurd inputs, no contract coverage)
//!
//! * Float `str()` coercion preserves the source literal
//!   (`arbitrary_precision`), where CPython renders a float repr
//!   (`1e3` → `"1000.0"`); same choice as the merged precedents.
//! * Non-ASCII digits parse in CPython (`float()`, datetime regex) but
//!   not here.
//! * A >u64 sort_order literal overflows to `inf` (Django 400s
//!   `Integer value too large to convert to float`).
//! * Lone-surrogate escapes 400 at JSON parse here (Django 400s
//!   `Surrogate characters are not allowed.`); same documented
//!   limitation as the merged precedents.
//!
//! # Out of scope
//!
//! JSON bodies only (the contract suite sends `json=` throughout):
//! a non-empty non-JSON body answers 415; form/multipart field decoding is
//! left to the Django proxy. Throttle headers (`ApiKeyRateThrottle` /
//! `ServiceTokenRateThrottle` from `fx-perm`) are edge-owned: no merged
//! handler enforces them in-process, so this module matches that precedent.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Datelike, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;
use uuid::Uuid;

use pidash_auth::permissions::workspace::WorkspaceFacts;
use pidash_auth::scope::TenantScope;
use pidash_auth::token as token_kernel;
use pidash_db::v1_assets::{model::sticky as sticky_model, sticky_queries};
use pidash_types::v1_assets::sticky as sticky_types;
use pidash_types::WorkspaceId;

use super::permissions as v1_perm;
use crate::assistant::common::py_repr;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Collection path (`urls/sticky.py:9-13` router `stickies` under
/// `workspaces/<slug>/`).
pub const STICKIES_PATH: &str = "/api/v1/workspaces/{slug}/stickies/";
/// Detail path (router detail routes).
pub const STICKY_PATH: &str = "/api/v1/workspaces/{slug}/stickies/{pk}/";

/// Sticky routes: the five ported actions serve from Rust; every other
/// method on those paths (notably `PUT`, whose DRF-default `update` is not
/// one of the five units, and `OPTIONS` metadata) proxies to Django so its
/// bytes stay Django's. Mounting into the overlay belongs to PIDASHCONV-426.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            STICKIES_PATH,
            axum::routing::post(sticky_create)
                .get(sticky_list)
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
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Exact response bodies (byte parity with DRF)
// ---------------------------------------------------------------------------

/// DRF `NotAuthenticated` (anonymous on a guarded endpoint).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `AuthenticationFailed("Given API token is not valid")`, coerced to 403
/// (the class defines no `authenticate_header`; probed live).
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `BaseViewSet.handle_exception`'s `ObjectDoesNotExist` branch
/// (`views/base.py`): the workspace-miss body.
pub const WORKSPACE_MISSING_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// DRF's default `Http404` body: the `get_object` miss body (scoped pk miss
/// — unknown pk or another owner's row — and the detail-after-delete probe).
pub const NOT_FOUND_DETAIL_BODY: &str = r#"{"detail":"Not found."}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, anonymous (no usable credential).
    Unauthorized,
    /// 403, bad/expired/inactive token.
    InvalidToken,
    /// 403, `WorkspaceUserPermission` denial (DRF-default body).
    Forbidden,
    /// 404, workspace lookup miss (`ObjectDoesNotExist` branch).
    WorkspaceMissing,
    /// 404, `get_object` miss (DRF `Http404` body).
    NotFoundDetail,
    /// 400, `{"detail": ...}` (`ParseError`: per_page / cursor).
    BadDetail(String),
    /// 400, `{"error": ...}` (`handle_exception` `KeyError` branch:
    /// unknown stored time zone).
    BadError(String),
    /// 400, serializer `errors` dict.
    FieldErrors(Value),
    /// 415, unhandled content type with a non-empty body.
    UnsupportedMediaType(String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::InvalidToken => (StatusCode::FORBIDDEN, INVALID_TOKEN_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, v1_perm::CLASS_DENIAL_BODY.to_owned()),
            Denial::WorkspaceMissing => (StatusCode::NOT_FOUND, WORKSPACE_MISSING_BODY.to_owned()),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, NOT_FOUND_DETAIL_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::FieldErrors(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("error body serializes"),
            ),
            Denial::UnsupportedMediaType(raw) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!(
                    "{{\"detail\":{}}}",
                    json_string(&format!("Unsupported media type \"{raw}\" in request."))
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
        json_response(status, body)
    }
}

pub(crate) type HandlerResult = Result<Response, Denial>;

pub(crate) fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serializes")
}

/// Map a paginator-kernel error to its HTTP fate: input errors are
/// `ParseError` 400s (`{"detail": ...}`); arithmetic errors are the 500s
/// Python's `ZeroDivisionError` / lazy-queryset `ValueError` become.
fn page_denial(error: crate::paginator::PageError) -> Denial {
    use crate::paginator::PageError;
    match error {
        PageError::InvalidPerPage | PageError::PerPageTooLarge(_) | PageError::InvalidCursor => {
            Denial::BadDetail(error.detail())
        }
        _ => Denial::ServerError,
    }
}

// ---------------------------------------------------------------------------
// Authentication (`api/middleware/api_authentication.py:20-84`)
// ---------------------------------------------------------------------------

/// The authenticated caller. Mirrors the device-session precedent
/// (`auth_oauth::device_session`): the `X-Api-Key` header carries an
/// `APIToken` or, when it starts with `mt_`, a `MachineToken`. No usable
/// credential is 401; a bad one is 403 (see [`INVALID_TOKEN_BODY`]).
async fn authenticate(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
) -> Result<Uuid, Denial> {
    let Some(raw) = headers.get(token_kernel::API_KEY_HEADER) else {
        return Err(Denial::Unauthorized);
    };
    // Django decodes headers lossily to `str`: undecodable bytes are a
    // lookup miss (403), never a missing credential (401).
    let presented = raw.to_str().unwrap_or("\0");
    if presented.is_empty() {
        return Err(Denial::Unauthorized);
    }
    if presented.starts_with(token_kernel::MACHINE_TOKEN_PREFIX) {
        authenticate_machine(pool, secret_key, presented).await
    } else {
        authenticate_api(pool, presented).await
    }
}

/// `validate_api_token` (`api_authentication.py:30-43`): exact token match,
/// `is_active`, unexpired — then stamp `last_used`. The `deleted_at IS
/// NULL` conjunct is the `SoftDeletionManager` scope
/// (`db/mixins.py:56-66`): a soft-deleted token 403s.
async fn authenticate_api(pool: &sqlx::PgPool, presented: &str) -> Result<Uuid, Denial> {
    let now = Utc::now();
    let row: Option<(Uuid, Uuid, bool, Option<DateTime<Utc>>)> = sqlx::query_as(
        r#"SELECT id, user_id, is_active, expired_at FROM api_tokens WHERE token = $1 AND deleted_at IS NULL"#,
    )
    .bind(presented)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, user_id, is_active, expired_at)) = row else {
        return Err(Denial::InvalidToken);
    };
    if !is_active {
        return Err(Denial::InvalidToken);
    }
    if let Some(expires) = expired_at {
        if expires <= now {
            return Err(Denial::InvalidToken);
        }
    }
    sqlx::query(r#"UPDATE api_tokens SET last_used = $1 WHERE id = $2"#)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(user_id)
}

/// `is_workspace_member` for machine tokens
/// (`core/permissions.py:28-34`): active row, any role, through the
/// default manager (soft-deleted rows do not count — the
/// `handlers_members` precedent carries the same conjunct).
/// Binds: `$1` workspace id, `$2` user id.
const MACHINE_MEMBER_SQL: &str = r#"SELECT EXISTS(SELECT 1 FROM workspace_members
           WHERE workspace_id = $1 AND member_id = $2 AND is_active AND deleted_at IS NULL)"#;

/// One `machine_token` + `dev_machine` lookup row: token id, user id,
/// workspace id, token revocation, dev-machine id, dev-machine
/// revocation, dev-machine id again (the join-hit proof).
type MachineTokenRow = (
    Uuid,
    Uuid,
    Uuid,
    Option<DateTime<Utc>>,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    Option<Uuid>,
);

/// `validate_machine_token` (`api_authentication.py:45-63`): match on
/// `token_hash`, unrevoked, dev-machine unrevoked, workspace member (a
/// non-member's token is revoked, then rejected) — then stamp
/// `last_used_at`.
async fn authenticate_machine(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    presented: &str,
) -> Result<Uuid, Denial> {
    let now = Utc::now();
    let token_hash = token_kernel::hash_token(presented, secret_key);
    let row: Option<MachineTokenRow> = sqlx::query_as(
        r#"SELECT mt.id, mt.user_id, mt.workspace_id, mt.revoked_at,
                  mt.dev_machine_id, dm.revoked_at, dm.id
           FROM machine_token mt
           LEFT JOIN dev_machine dm ON dm.id = mt.dev_machine_id
           WHERE mt.token_hash = $1"#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, user_id, workspace_id, revoked_at, dev_machine_id, dm_revoked, dm_found)) = row
    else {
        return Err(Denial::InvalidToken);
    };
    if revoked_at.is_some() {
        return Err(Denial::InvalidToken);
    }
    if let Some(machine_id) = dev_machine_id {
        if dm_found != Some(machine_id) {
            return Err(Denial::ServerError);
        }
        if dm_revoked.is_some() {
            return Err(Denial::InvalidToken);
        }
    }
    let member: Option<bool> = sqlx::query_scalar(MACHINE_MEMBER_SQL)
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if !member.unwrap_or(false) {
        sqlx::query(r#"UPDATE machine_token SET revoked_at = $1 WHERE id = $2"#)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        return Err(Denial::InvalidToken);
    }
    sqlx::query(r#"UPDATE machine_token SET last_used_at = $1 WHERE id = $2"#)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(user_id)
}

// ---------------------------------------------------------------------------
// Permission gate (`WorkspaceUserPermission`, `sticky.py:28`)
// ---------------------------------------------------------------------------

/// Enforce `WorkspaceUserPermission` for one `(user, slug)`: anonymous was
/// already 401'd, so any denial here is the DRF-default 403. The membership
/// row is fetched with the exact `(member, workspace__slug, is_active)`
/// filters Python checks (see [`v1_perm::decide_sticky_gate`]).
async fn require_workspace_user(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &Uuid,
) -> Result<(), Denial> {
    let role: Option<i16> = sqlx::query_scalar(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let facts = WorkspaceFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        has_admin_or_member_role: matches!(role, Some(20) | Some(15)),
        has_admin_role: role == Some(20),
        is_member: role.is_some(),
        is_admin_unfiltered: role == Some(20),
    };
    let scope = TenantScope::new(WorkspaceId::from(slug.to_owned()));
    if v1_perm::decide_sticky_gate(&scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// The request's render zone name (`TimezoneMixin`: the acting user's
/// `user_timezone`), loaded but NOT parsed — parsing happens in
/// [`activate_timezone`].
async fn load_timezone_name(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<Option<String>, Denial> {
    let zone: Option<String> =
        sqlx::query_scalar(r#"SELECT user_timezone FROM users WHERE id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(zone)
}

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after permissions). A missing user row or a NULL zone is the 500
/// Python's `DoesNotExist`/`TypeError` path becomes (NULL already 500s
/// at decode); an unknown zone name 400s: `zoneinfo.ZoneInfo` raises
/// `ZoneInfoNotFoundError`, which subclasses `KeyError`
/// (`api/views/base.py:160-164`). An EMPTY zone 500s: `ZoneInfo('')`
/// raises `ValueError` (not `KeyError`), which falls through to the
/// generic 500 (`api/views/base.py:166-171`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Err(Denial::ServerError),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone
            .parse::<Tz>()
            .map_err(|_| Denial::BadError("The required key does not exist.".to_owned())),
    }
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Row read + `StickySerializer` render
// ---------------------------------------------------------------------------

/// One `stickies` row in [`sticky_model::COLUMNS`] order.
#[derive(Debug, Clone)]
struct StickyRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    name: Option<String>,
    description: Value,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    logo_props: Value,
    color: Option<String>,
    background_color: Option<String>,
    workspace_id: Uuid,
    owner_id: Uuid,
    sort_order: f64,
}

impl StickyRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, Denial> {
        Ok(Self {
            id: row.try_get("id").map_err(|_| Denial::ServerError)?,
            created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
            updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
            created_by_id: row
                .try_get("created_by_id")
                .map_err(|_| Denial::ServerError)?,
            updated_by_id: row
                .try_get("updated_by_id")
                .map_err(|_| Denial::ServerError)?,
            deleted_at: row.try_get("deleted_at").map_err(|_| Denial::ServerError)?,
            name: row.try_get("name").map_err(|_| Denial::ServerError)?,
            description: row
                .try_get("description")
                .map_err(|_| Denial::ServerError)?,
            description_html: row
                .try_get("description_html")
                .map_err(|_| Denial::ServerError)?,
            description_stripped: row
                .try_get("description_stripped")
                .map_err(|_| Denial::ServerError)?,
            description_binary: row
                .try_get("description_binary")
                .map_err(|_| Denial::ServerError)?,
            logo_props: row.try_get("logo_props").map_err(|_| Denial::ServerError)?,
            color: row.try_get("color").map_err(|_| Denial::ServerError)?,
            background_color: row
                .try_get("background_color")
                .map_err(|_| Denial::ServerError)?,
            workspace_id: row
                .try_get("workspace_id")
                .map_err(|_| Denial::ServerError)?,
            owner_id: row.try_get("owner_id").map_err(|_| Denial::ServerError)?,
            sort_order: row.try_get("sort_order").map_err(|_| Denial::ServerError)?,
        })
    }
}

fn opt_uuid(value: &Option<Uuid>) -> Value {
    value.map_or(Value::Null, |id| Value::String(id.to_string()))
}

fn opt_string(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::String)
}

fn render_datetime(dt: &DateTime<Utc>, timezone: &Tz) -> String {
    crate::serializer::render_datetime_in(dt, timezone)
}

/// `StickySerializer` read shape (`serializers/sticky.py:13-17`,
/// `fields = "__all__"`): the 17 keys in DRF serializer field order —
/// the pk first, then the plain fields in `_meta` order, then the
/// forward relations in `_meta` order (`get_default_field_names`,
/// DRF 3.15.2 `serializers.py`; pinned against the live field list, not
/// raw `_meta` order, which interleaves the audit FKs). `None` renders
/// `null`; datetimes render in the request's zone;
/// `description_binary` has no DRF JSON mapping — a set value fails
/// rendering in Django too, so it is the same 500 here (always null on
/// the wire: the key is read-only-dead on write).
fn render_sticky_row(row: &StickyRow, timezone: &Tz) -> Result<String, Denial> {
    if row.description_binary.is_some() {
        return Err(Denial::ServerError);
    }
    let sort_order = serde_json::Number::from_f64(row.sort_order).ok_or(Denial::ServerError)?;
    let mut map = Map::with_capacity(17);
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert(
        "created_at".to_owned(),
        Value::String(render_datetime(&row.created_at, timezone)),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(render_datetime(&row.updated_at, timezone)),
    );
    map.insert(
        "deleted_at".to_owned(),
        row.deleted_at.map_or(Value::Null, |dt| {
            Value::String(render_datetime(&dt, timezone))
        }),
    );
    map.insert("name".to_owned(), opt_string(&row.name));
    map.insert("description".to_owned(), row.description.clone());
    map.insert(
        "description_html".to_owned(),
        Value::String(row.description_html.clone()),
    );
    map.insert(
        "description_stripped".to_owned(),
        opt_string(&row.description_stripped),
    );
    map.insert("description_binary".to_owned(), Value::Null);
    map.insert("logo_props".to_owned(), row.logo_props.clone());
    map.insert("color".to_owned(), opt_string(&row.color));
    map.insert(
        "background_color".to_owned(),
        opt_string(&row.background_color),
    );
    map.insert("sort_order".to_owned(), Value::Number(sort_order));
    map.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    map.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    map.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    map.insert("owner".to_owned(), Value::String(row.owner_id.to_string()));
    Ok(serde_json::to_string(&Value::Object(map)).expect("sticky row serializes"))
}

// ---------------------------------------------------------------------------
// `StickySerializer` input validation (create + partial_update)
// ---------------------------------------------------------------------------

/// DRF `CharField` messages used by the sticky text fields.
const NOT_A_VALID_STRING: &str = "Not a valid string.";
const MAY_NOT_BE_NULL: &str = "This field may not be null.";
const MAX_255_MESSAGE: &str = "Ensure this field has no more than 255 characters.";
const NULL_CHARACTERS_MESSAGE: &str = "Null characters are not allowed.";
/// DRF `FloatField` messages for `sort_order`.
const A_VALID_NUMBER_IS_REQUIRED: &str = "A valid number is required.";
const STRING_VALUE_TOO_LARGE: &str = "String value too large.";
/// DRF `to_internal_value` on a non-dict body.
const INVALID_DATA_MESSAGE: &str = "Invalid data. Expected a dictionary, but got {what}.";

/// `type(data).__name__` over JSON values for the non-dict message
/// (`serializers.py:487`; same shape as `assistant::common`).
fn drf_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        // `arbitrary_precision`: only true floats `is_f64` — oversized
        // integer literals are still Python `int` (same shape as the
        // `v1_cycles_modules` precedent).
        Value::Number(number) => {
            if number.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python `str.strip()` for the `trim_whitespace` pass: Rust's
/// `char::is_whitespace` already covers `\x1c`-`\x1f` (Unicode
/// `White_Space`), but the range is spelled out so the parity is explicit
/// (same helper shape as the `handlers_members` precedent).
fn py_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// DRF `CharField.run_validation` + `run_validators` (`fields.py:749-767`,
/// DRF 3.16.1; same pipeline as `assistant::common::run_char_field`):
///
/// * `None` is `This field may not be null.` unless the model field is
///   nullable (`validate_empty_values`, before anything else).
/// * Empty or whitespace-only short-circuits to `""`: every sticky text
///   field allows blank (`blank=True`), so there is no `blank` failure
///   here and the validators below are skipped.
/// * Otherwise numbers stringify (`str(data)` — bools, lists and dicts
///   are `Not a valid string.`), the result is stripped
///   (`trim_whitespace`, always on), then `MaxLengthValidator` (on the
///   stripped value, counted in characters) and
///   `ProhibitNullCharactersValidator` run in validator order and their
///   failures are collected — both messages can appear together.
///   `ProhibitSurrogateCharactersValidator` cannot fire: neither JSON nor
///   Rust strings carry a lone surrogate (`serde_json` rejects the
///   escapes at parse time).
///
/// Returns the stripped string Django stores.
fn check_text(
    value: &Value,
    max_length: Option<usize>,
    nullable: bool,
) -> Result<Option<String>, Vec<String>> {
    if matches!(value, Value::Null) {
        if nullable {
            return Ok(None);
        }
        return Err(vec![MAY_NOT_BE_NULL.to_owned()]);
    }
    let coerced: String = match value {
        Value::String(text) => text.clone(),
        // `str()` of an int matches exactly. A float repr never contains
        // `@`-style sentinels and the exponent rendering only matters past
        // every length limit here, so `to_string()` (which preserves the
        // source literal under `arbitrary_precision`) is the merged
        // precedent's exact choice.
        Value::Number(number) => number.to_string(),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null => {
            return Err(vec![NOT_A_VALID_STRING.to_owned()]);
        }
    };
    if py_strip(&coerced).is_empty() {
        return Ok(Some(String::new()));
    }
    let stripped = py_strip(&coerced).to_owned();
    let mut failures = Vec::new();
    if let Some(max) = max_length {
        // DRF `max_length` counts characters (code points).
        if stripped.chars().count() > max {
            failures.push(MAX_255_MESSAGE.to_owned());
        }
    }
    if stripped.contains('\0') {
        failures.push(NULL_CHARACTERS_MESSAGE.to_owned());
    }
    if failures.is_empty() {
        Ok(Some(stripped))
    } else {
        Err(failures)
    }
}

fn check_float(value: &Value) -> Result<f64, String> {
    match value {
        // `validate_empty_values` precedes `to_internal_value`: an explicit
        // null is the `null` message, not the `invalid` one (`sort_order`
        // is `null=False`).
        Value::Null => Err(MAY_NOT_BE_NULL.to_owned()),
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| A_VALID_NUMBER_IS_REQUIRED.to_owned()),
        Value::String(text) => {
            // `MAX_STRING_LENGTH` guard runs before `float()` and counts
            // the raw string's characters.
            if text.chars().count() > 1000 {
                return Err(STRING_VALUE_TOO_LARGE.to_owned());
            }
            let trimmed = py_strip(text);
            // Python `float()` accepts underscores only between digits
            // (`float("1_0") == 10.0`; `float("_1")` raises).
            if trimmed.contains('_') {
                let chars: Vec<char> = trimmed.chars().collect();
                for (index, ch) in chars.iter().enumerate() {
                    if *ch == '_'
                        && (index == 0
                            || index + 1 == chars.len()
                            || !chars[index - 1].is_ascii_digit()
                            || !chars[index + 1].is_ascii_digit())
                    {
                        return Err(A_VALID_NUMBER_IS_REQUIRED.to_owned());
                    }
                }
            }
            // `nan`/`inf` spellings match `str::parse`'s case-insensitive
            // set. Non-ASCII digits parse in CPython but not here (noted
            // with the other micro-gaps in the module docs).
            trimmed
                .replace('_', "")
                .parse::<f64>()
                .map_err(|_| A_VALID_NUMBER_IS_REQUIRED.to_owned())
        }
        // `float(True) == 1.0` (`bool` is an `int` subclass).
        Value::Bool(true) => Ok(1.0),
        Value::Bool(false) => Ok(0.0),
        _ => Err(A_VALID_NUMBER_IS_REQUIRED.to_owned()),
    }
}

/// Validated sticky write: the columns an `INSERT`/`UPDATE` may set, with
/// `description_html` already sanitized (or the model default when absent
/// on create). Each `Option` is presence (`None` = key absent); the inner
/// value is the validated payload (`None` = JSON null on a nullable
/// field). `created_by` is honored on update only — create overwrites it
/// from the request user (`BaseModel.save` via crum) — but a present key
/// is still format- and existence-checked on both paths, exactly like
/// Django. `updated_by` and `description_stripped` are validated the same
/// way and then discarded (`save()` overwrites both unconditionally), so
/// they have no write slot.
#[derive(Debug, Clone, Default)]
struct StickyWrite {
    name: Option<Option<String>>,
    description: Option<Value>,
    description_html: Option<String>,
    logo_props: Option<Value>,
    color: Option<Option<String>>,
    background_color: Option<Option<String>>,
    sort_order: Option<f64>,
    created_by: Option<Option<Uuid>>,
    deleted_at: Option<Option<DateTime<Utc>>>,
}

/// A `created_by`/`updated_by` input whose UUID format parsed: the users
/// existence lookup still has to run. `display` is the original input
/// rendering for the `does_not_exist` message (`str(data)`: strings
/// verbatim, ints as digits — never the lowercased UUID text).
#[derive(Debug, Clone)]
struct PendingPk {
    field: &'static str,
    uuid: Uuid,
    display: String,
}

/// DRF `PrimaryKeyRelatedField` messages for the audit FKs
/// (`relations.py:240-242`, DRF 3.16.1).
const PK_INCORRECT_TYPE: &str = "Incorrect type. Expected pk value, received {what}.";

/// Django `UUIDField` message (`fields/__init__.py:2653`): curly quotes
/// around `str(value)`. It surfaces as a *field* error because DRF's
/// `Field.run_validation` converts Django `ValidationError`s
/// (`fields.py:561`).
fn invalid_uuid_message(rendered: &str) -> String {
    format!("\u{201c}{rendered}\u{201d} is not a valid UUID.")
}

/// Validate a `created_by`/`updated_by` input through
/// `PrimaryKeyRelatedField.to_internal_value` (`relations.py:253-265`):
/// JSON null passes (`allow_null`: both FKs are `null=True`); bools are
/// `incorrect_type` (raised before the queryset is touched); anything else
/// goes to `User.objects.get(pk=data)` whose `UUIDField` coercion decides:
/// ints in range and well-formed UUID strings (any of the four spellings
/// `Uuid::parse_str` accepts, matching `uuid.UUID`) reach the existence
/// lookup, every other value is the Django invalid-UUID field error
/// (floats, lists and dicts included — `uuid.UUID(hex=...)` raises
/// `AttributeError`, which `to_python` converts).
fn check_audit_pk(value: &Value) -> Result<Option<(Uuid, String)>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(_) => Err(PK_INCORRECT_TYPE.replace("{what}", drf_type_name(value))),
        Value::Number(number) => {
            if let Some(int) = number.as_i64().filter(|int| *int >= 0) {
                // Any non-negative `i64` fits in 128 bits, so the lookup
                // always runs (`uuid.UUID(int=...)` never raises in range).
                Ok(Some((Uuid::from_u128(int as u128), number.to_string())))
            } else if let Some(uint) = number.as_u64() {
                // `as_u64` fails for negatives and floats; every `u64`
                // still fits in 128 bits, so the lookup always runs.
                Ok(Some((
                    Uuid::from_u128(u128::from(uint)),
                    number.to_string(),
                )))
            } else if let Ok(big) = number.to_string().parse::<u128>() {
                // `arbitrary_precision` integers past `u64::MAX` keep
                // their exact digits: in-range values reach the lookup;
                // out-of-range ones (and every float spelling, which
                // never parses as `u128`) fail into the error below.
                Ok(Some((Uuid::from_u128(big), number.to_string())))
            } else {
                // Negative ints, out-of-128-bit-range ints, and floats
                // take the `hex` form and fail inside `to_python`.
                Err(invalid_uuid_message(&number.to_string()))
            }
        }
        Value::String(text) => match text.parse::<Uuid>() {
            Ok(uuid) => Ok(Some((uuid, text.clone()))),
            Err(_) => Err(invalid_uuid_message(text)),
        },
        Value::Array(_) | Value::Object(_) => Err(invalid_uuid_message(&py_repr(value))),
    }
}

/// DRF `DateTimeField` messages for `deleted_at` (`fields.py:193-198`).
const DATETIME_INVALID_MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
const DATETIME_OVERFLOW_MESSAGE: &str = "Datetime value out of range.";

fn make_aware_message(timezone: &Tz) -> String {
    format!("Invalid datetime for the timezone \"{timezone}\".")
}

/// Validate a `deleted_at` input through `DateTimeField.to_internal_value`
/// with the default `iso-8601` input format (`fields.py`, DRF 3.16.1; the
/// project overrides neither `DATETIME_INPUT_FORMATS` nor the field).
/// JSON null passes (`allow_null`: `null=True`); any non-string is the
/// `invalid` message (the `TypeError` is swallowed by the same
/// `suppress(ValueError, TypeError)` that wraps the whole parse block).
fn check_deleted_at(value: &Value, timezone: &Tz) -> Result<Option<DateTime<Utc>>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => parse_deleted_at(text, timezone).map(Some),
        _ => Err(DATETIME_INVALID_MESSAGE.to_owned()),
    }
}

/// Django 4.2 `parse_datetime` (`utils/dateparse.py:20-24`) plus DRF's
/// `enforce_timezone` with the request's current timezone (the acting
/// user's `user_timezone`, activated by `TimezoneMixin`; `USE_TZ` is on):
///
/// * Shape: `YYYY-M-D[T ]h:m`, optional `:ss`, optional `[.,]ffffff`
///   (1-12 digits, extra past 6 swallowed, kept digits left-justified to
///   6), optional whitespace, optional `Z`/`±HH`/`±HHMM`/`±HH:MM`
///   (offsets are *not* range-checked: `+99:99` is accepted), then end —
///   with Python `$` also matching before one trailing newline.
///   Anything else is the `invalid` message (a well-formed but impossible
///   date such as month 13 lands here too: the `ValueError` never
///   escapes the `suppress` block).
/// * Naive input is read in the request timezone; a wall time that does
///   not exist there (DST gap) is the `make_aware` message, an ambiguous
///   one takes the first occurrence (`fold=0`).
/// * Aware input converts to the request timezone for validation only —
///   storage keeps the instant; a conversion past year 9999/0001 is the
///   `overflow` message.
fn parse_deleted_at(text: &str, timezone: &Tz) -> Result<DateTime<Utc>, String> {
    let invalid = || DATETIME_INVALID_MESSAGE.to_owned();
    let text = text.strip_suffix('\n').unwrap_or(text);
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut pos = 0usize;
    // ASCII digits only (CPython also takes non-ASCII digits; noted with
    // the other micro-gaps in the module docs).
    fn digits(bytes: &[u8], pos: &mut usize, min: usize, max: usize) -> Option<u32> {
        let start = *pos;
        let mut value = 0u32;
        let mut count = 0usize;
        while *pos < bytes.len() && count < max && bytes[*pos].is_ascii_digit() {
            value = value * 10 + u32::from(bytes[*pos] - b'0');
            *pos += 1;
            count += 1;
        }
        if count >= min {
            Some(value)
        } else {
            *pos = start;
            None
        }
    }
    let year = digits(bytes, &mut pos, 4, 4).ok_or_else(invalid)?;
    if bytes.get(pos) != Some(&b'-') {
        return Err(invalid());
    }
    pos += 1;
    let month = digits(bytes, &mut pos, 1, 2).ok_or_else(invalid)?;
    if bytes.get(pos) != Some(&b'-') {
        return Err(invalid());
    }
    pos += 1;
    let day = digits(bytes, &mut pos, 1, 2).ok_or_else(invalid)?;
    if bytes.get(pos) != Some(&b'T') && bytes.get(pos) != Some(&b' ') {
        return Err(invalid());
    }
    pos += 1;
    let hour = digits(bytes, &mut pos, 1, 2).ok_or_else(invalid)?;
    if bytes.get(pos) != Some(&b':') {
        return Err(invalid());
    }
    pos += 1;
    let minute = digits(bytes, &mut pos, 1, 2).ok_or_else(invalid)?;
    let mut second = 0u32;
    let mut microsecond = 0u32;
    if bytes.get(pos) == Some(&b':') {
        pos += 1;
        second = digits(bytes, &mut pos, 1, 2).ok_or_else(invalid)?;
        if bytes.get(pos) == Some(&b'.') || bytes.get(pos) == Some(&b',') {
            pos += 1;
            let mut count = 0usize;
            let mut value = 0u32;
            while pos < len && count < 6 && bytes[pos].is_ascii_digit() {
                value = value * 10 + u32::from(bytes[pos] - b'0');
                pos += 1;
                count += 1;
            }
            if count == 0 {
                return Err(invalid());
            }
            for _ in count..6 {
                value *= 10;
            }
            microsecond = value;
            let mut extra = 0usize;
            while pos < len && extra < 6 && bytes[pos].is_ascii_digit() {
                pos += 1;
                extra += 1;
            }
        }
    }
    while pos < len {
        let ch = text[pos..].chars().next().ok_or_else(invalid)?;
        if ch.is_whitespace() {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    let mut offset: Option<i32> = None;
    if bytes.get(pos) == Some(&b'Z') {
        offset = Some(0);
        pos += 1;
    } else if bytes.get(pos) == Some(&b'+') || bytes.get(pos) == Some(&b'-') {
        let negative = bytes[pos] == b'-';
        let mut after_sign = pos + 1;
        if let Some(hours) = digits(bytes, &mut after_sign, 2, 2) {
            let mut end = after_sign;
            let mut minutes = 0u32;
            let mut probe = after_sign;
            if bytes.get(probe) == Some(&b':') {
                probe += 1;
            }
            if let Some(parsed) = digits(bytes, &mut probe, 2, 2) {
                minutes = parsed;
                end = probe;
            }
            // Else the `(:?\d{2})?` group matches empty and the zone is
            // hours-only (regex backtrack parity); a trailing `:0`-style
            // fragment then fails the end check below, exactly like
            // Django.
            let secs = (i32::try_from(hours * 60 + minutes).unwrap_or(i32::MAX)) * 60;
            offset = Some(if negative { -secs } else { secs });
            pos = end;
        }
        // Else the whole optional zone group fails and `pos` stays on the
        // sign, which the end check rejects.
    }
    if pos != len {
        return Err(invalid());
    }
    let date = NaiveDate::from_ymd_opt(year as i32, month, day).ok_or_else(invalid)?;
    let naive = date
        .and_hms_micro_opt(hour, minute, second, microsecond)
        .ok_or_else(invalid)?;
    match offset {
        Some(shift) => {
            let instant = naive
                .and_utc()
                .checked_sub_signed(chrono::Duration::seconds(i64::from(shift)))
                .ok_or_else(invalid)?;
            // `astimezone` overflow ⟺ converted wall year outside 1-9999.
            let wall = instant.with_timezone(timezone);
            if wall.year() > 9999 || wall.year() < 1 {
                return Err(DATETIME_OVERFLOW_MESSAGE.to_owned());
            }
            Ok(instant)
        }
        None => match timezone.from_local_datetime(&naive) {
            LocalResult::Single(aware) => Ok(aware.with_timezone(&Utc)),
            // Ambiguous wall time: first occurrence (`fold=0`).
            LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
            LocalResult::None => Err(make_aware_message(timezone)),
        },
    }
}

/// The pure field-validation pass: per-field results in DRF serializer
/// field order plus the users lookups still to run. Callers run
/// [`resolve_pk_lookups`], append those errors (both audit FKs sort
/// after every other writable key), and only then run the `validate()`
/// step — field errors short-circuit `validate()`, exactly like DRF's
/// `run_validation`.
struct FieldValidation {
    errors: Vec<(String, Vec<String>)>,
    write: StickyWrite,
    html: Option<String>,
    pending: Vec<PendingPk>,
}

/// Run the `StickySerializer` field path over an already-parsed JSON body:
/// silently drop read-only + unknown keys (live `to_internal_value`),
/// then per-field validate the survivors in DRF serializer field order
/// (`deleted_at`, `name`, `description`, `description_html`,
/// `description_stripped`, `logo_props`, `color`, `background_color`,
/// `sort_order`, `created_by`, `updated_by`).
fn validate_sticky_input(body: &Value, timezone: &Tz) -> FieldValidation {
    // Live projection: read-only keys (`id`, `created_at`, `updated_at`,
    // `description_binary`, `workspace`, `owner`) and unknown keys are
    // silently dropped — never errors. The audit columns (`created_by`,
    // `updated_by`, `deleted_at`) and `description_stripped` are
    // `editable`, so they survive here and validate below.
    let empty = Map::new();
    let input = body.as_object().unwrap_or(&empty);
    if !body.is_object() {
        // A JSON null never reaches `to_internal_value`: the serializer's
        // `errors` property rewrites the lone `null`-code failure to the
        // friendlier message (`serializers.py`, DRF 3.15.2).
        let message = if body.is_null() {
            "No data provided".to_owned()
        } else {
            INVALID_DATA_MESSAGE.replace("{what}", drf_type_name(body))
        };
        return FieldValidation {
            errors: vec![("non_field_errors".to_owned(), vec![message])],
            write: StickyWrite::default(),
            html: None,
            pending: Vec::new(),
        };
    }
    let writable = sticky_types::strip_ignored_sticky_keys(input);
    let mut errors: Vec<(String, Vec<String>)> = Vec::new();
    let mut write = StickyWrite::default();
    let mut pending = Vec::new();
    if let Some(value) = writable.get("deleted_at") {
        match check_deleted_at(value, timezone) {
            Ok(stamp) => write.deleted_at = Some(stamp),
            Err(message) => errors.push(("deleted_at".to_owned(), vec![message])),
        }
    }
    if let Some(value) = writable.get("name") {
        match check_text(value, None, true) {
            Ok(name) => write.name = Some(name),
            Err(messages) => errors.push(("name".to_owned(), messages)),
        }
    }
    // `JSONField(null=False)`: any JSON value passes through
    // (`to_internal_value` only re-serializes); an explicit null is the
    // `null` message (`allow_null` follows model `null`).
    if let Some(value) = writable.get("description") {
        if matches!(value, Value::Null) {
            errors.push(("description".to_owned(), vec![MAY_NOT_BE_NULL.to_owned()]));
        } else {
            write.description = Some(value.clone());
        }
    }
    let mut html: Option<String> = None;
    if let Some(value) = writable.get("description_html") {
        match check_text(value, None, false) {
            Ok(Some(text)) => {
                html = Some(text.clone());
                write.description_html = Some(text);
            }
            Ok(None) => unreachable!("description_html is not nullable"),
            Err(messages) => errors.push(("description_html".to_owned(), messages)),
        }
    }
    // Validated, then discarded: `Sticky.save` recomputes
    // `description_stripped` on every save, so only invalid input is
    // observable (as a 400).
    if let Some(value) = writable.get("description_stripped") {
        if let Err(messages) = check_text(value, None, true) {
            errors.push(("description_stripped".to_owned(), messages));
        }
    }
    if let Some(value) = writable.get("logo_props") {
        if matches!(value, Value::Null) {
            errors.push(("logo_props".to_owned(), vec![MAY_NOT_BE_NULL.to_owned()]));
        } else {
            write.logo_props = Some(value.clone());
        }
    }
    if let Some(value) = writable.get("color") {
        match check_text(value, Some(sticky_model::VARCHAR_MAX_LENGTH), true) {
            Ok(color) => write.color = Some(color),
            Err(messages) => errors.push(("color".to_owned(), messages)),
        }
    }
    if let Some(value) = writable.get("background_color") {
        match check_text(value, Some(sticky_model::VARCHAR_MAX_LENGTH), true) {
            Ok(color) => write.background_color = Some(color),
            Err(messages) => errors.push(("background_color".to_owned(), messages)),
        }
    }
    if let Some(value) = writable.get("sort_order") {
        match check_float(value) {
            Ok(order) => write.sort_order = Some(order),
            Err(message) => errors.push(("sort_order".to_owned(), vec![message])),
        }
    }
    // The audit FKs validate last: in DRF's serializer field order the
    // forward relations trail every plain field.
    for field in ["created_by", "updated_by"] {
        if let Some(value) = writable.get(field) {
            match check_audit_pk(value) {
                Ok(None) => {
                    if field == "created_by" {
                        write.created_by = Some(None);
                    }
                }
                Ok(Some((uuid, display))) => pending.push(PendingPk {
                    field,
                    uuid,
                    display,
                }),
                Err(message) => errors.push((field.to_owned(), vec![message])),
            }
        }
    }
    FieldValidation {
        errors,
        write,
        html,
        pending,
    }
}

/// Field errors in DRF serializer field order as the exact 400 body
/// (insertion-ordered: `preserve_order` is on for this crate).
fn errors_value(errors: Vec<(String, Vec<String>)>) -> Value {
    let mut map = Map::with_capacity(errors.len());
    for (field, messages) in errors {
        map.insert(
            field,
            Value::Array(messages.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(map)
}

/// The `validate()` step (`serializers/sticky.py:21-27`): HTML sanitize
/// over the truthy surviving input; the binary arm is unreachable
/// post-strip, so `None` is passed (the dead branch stays dead). Runs only
/// when field validation produced no errors.
fn apply_validate_step(
    mut write: StickyWrite,
    html: Option<String>,
) -> Result<(StickyWrite, Option<String>), Denial> {
    match sticky_types::validate_sticky_descriptions(html.as_deref(), None) {
        Ok(sanitized) => {
            if let Some(clean) = sanitized {
                write.description_html = Some(clean.clone());
                Ok((write, Some(clean)))
            } else {
                Ok((write, html))
            }
        }
        Err(error) => Err(Denial::FieldErrors(error.body())),
    }
}

/// Users existence lookup for format-valid `created_by`/`updated_by`
/// inputs (`User.objects.get(pk=...)`: the plain `UserManager`, no
/// soft-delete filter). A hit on `created_by` fills the write slot (the
/// caller decides whether the action honors it); `updated_by` hits are
/// discarded (`save()` overwrites). Misses become `does_not_exist`
/// errors, returned in `pending` order so the caller can append them
/// after the pure-phase errors.
async fn resolve_pk_lookups(
    pool: &sqlx::PgPool,
    write: &mut StickyWrite,
    pending: &[PendingPk],
) -> Result<Vec<(String, Vec<String>)>, Denial> {
    let mut errors = Vec::new();
    for item in pending {
        let found: Option<bool> = sqlx::query_scalar(USER_EXISTS_SQL)
            .bind(item.uuid)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        if found.unwrap_or(false) {
            if item.field == "created_by" {
                write.created_by = Some(Some(item.uuid));
            }
        } else {
            errors.push((
                item.field.to_owned(),
                vec![format!(
                    "Invalid pk \"{}\" - object does not exist.",
                    item.display
                )],
            ));
        }
    }
    Ok(errors)
}

// ---------------------------------------------------------------------------
// SQL (bind order documented per statement; reads honor the soft-delete
// manager, writes hit the table so partial unique indexes keep working)
// ---------------------------------------------------------------------------

/// Workspace lookup for create (`Workspace.objects.get(slug=slug)` through
/// the default manager: `deleted_at IS NULL`; miss is the 404 above).
/// Binds: `$1` workspace slug.
const WORKSPACE_ID_SQL: &str =
    r#"SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#;

/// `StickyViewSet.get_object` (`sticky.py:85,96,110`): the `get_queryset`
/// scope (`sticky.py:30-37`) plus the pk lookup. Miss (unknown pk or
/// another owner's row) is [`Denial::NotFoundDetail`].
/// Binds: `$1` workspace slug, `$2` owner id, `$3` sticky pk.
fn sticky_get_sql() -> String {
    let cols: Vec<String> = sticky_model::COLUMNS
        .iter()
        .map(|col| format!("\"stickies\".\"{col}\""))
        .collect();
    format!(
        "SELECT DISTINCT {} FROM \"stickies\" \
         INNER JOIN \"workspaces\" ON (\"stickies\".\"workspace_id\" = \"workspaces\".\"id\") \
         WHERE (\"stickies\".\"deleted_at\" IS NULL \
         AND \"workspaces\".\"slug\" = $1 AND \"stickies\".\"owner_id\" = $2 \
         AND \"stickies\".\"id\" = $3)",
        cols.join(", ")
    )
}

/// `MAX(sort_order)` per workspace for create (`models/sticky.py:51` via
/// the default manager: soft-deleted rows do not count).
/// Binds: `$1` workspace id.
const STICKY_MAX_SORT_SQL: &str =
    r#"SELECT MAX(sort_order) FROM stickies WHERE workspace_id = $1 AND deleted_at IS NULL"#;

/// Sticky insert (all 17 columns; application defaults supplied explicitly
/// — the live tables carry no `column_default`).
/// Binds: `$1..$17` in [`sticky_model::COLUMNS`] order.
const STICKY_INSERT_SQL: &str = r#"INSERT INTO stickies (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, name, description, description_html, description_stripped, description_binary, logo_props, color, background_color, workspace_id, owner_id, sort_order) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)"#;

/// Sticky update: every mutable column is written back (changed or not —
/// identical values are a no-op in effect). `sort_order` keeps its stored
/// value unless the input carried one (`save()` touches it only on
/// create); `updated_at`/`updated_by` always advance (`auto_now` /
/// `BaseModel.save`); `created_by` is rewritten only when the input
/// carried it (writable audit FK — `save()` leaves it alone on update).
/// Binds: `$1..$12` the twelve set columns below, `$13` the pk.
const STICKY_UPDATE_SQL: &str = r#"UPDATE stickies SET name = $1, description = $2, description_html = $3, description_stripped = $4, logo_props = $5, color = $6, background_color = $7, sort_order = $8, updated_at = $9, updated_by_id = $10, created_by_id = $11, deleted_at = $12 WHERE id = $13"#;

/// Soft delete (`sticky.py:112` instance `.delete()` — which resolves to
/// `SoftDeleteModel.delete(soft=True)`, `db/mixins.py:72-82`: the row is
/// TOMBSTONED, not removed). The `save()` inside the tombstone also bumps
/// `updated_at`/`updated_by_id` (`auto_now` / `BaseModel.save` via crum).
/// Binds: `$1` tombstone stamp, `$2` update stamp, `$3` acting user, `$4`
/// sticky pk.
const STICKY_SOFT_DELETE_SQL: &str =
    r#"UPDATE stickies SET deleted_at = $1, updated_at = $2, updated_by_id = $3 WHERE id = $4"#;

/// Post-write render fetch: Django renders the saved *instance* (even a
/// born-deleted or just-tombstoned row), so the create/update responses
/// re-read by pk *without* the soft-delete scope. A scoped re-read would
/// 404 a row `deleted_at` input just tombstoned, where Django answers
/// 201/200 with the row.
/// Binds: `$1` sticky pk.
fn sticky_get_by_pk_sql() -> String {
    let cols: Vec<String> = sticky_model::COLUMNS
        .iter()
        .map(|col| format!("\"stickies\".\"{col}\""))
        .collect();
    format!(
        "SELECT {} FROM \"stickies\" WHERE \"stickies\".\"id\" = $1",
        cols.join(", ")
    )
}

/// Users existence lookup for `created_by`/`updated_by` inputs
/// (`User.objects.get(pk=...)`: the plain `UserManager`, no soft-delete
/// filter — the `users` table has no `deleted_at`).
/// Binds: `$1` user id.
const USER_EXISTS_SQL: &str = r#"SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)"#;

/// Total-match count for the list envelope: the list query's `FROM`/`WHERE`
/// with `COUNT(DISTINCT id)` in place of the projection (equivalent to
/// Django's `queryset.count()` over the `DISTINCT` queryset: ids are unique,
/// so distinct-row and distinct-id counts agree).
fn sticky_count_sql(list_sql: &str) -> String {
    let from = list_sql
        .find(" FROM \"stickies\"")
        .expect("list sql carries the stickies from clause");
    let mut count = format!(
        "SELECT COUNT(DISTINCT \"stickies\".\"id\"){}",
        &list_sql[from..]
    );
    if let Some(order) = count.find(" ORDER BY ") {
        count.truncate(order);
    }
    count
}

// ---------------------------------------------------------------------------
// Shared request plumbing
// ---------------------------------------------------------------------------

/// Parse the request body (JSON only — the contract suite sends `json=`
/// throughout): an empty body is `{}` whatever the content type claims;
/// malformed JSON is DRF's `ParseError`; a non-empty non-JSON body is 415.
fn parse_json_body(body: &[u8], headers: &HeaderMap) -> Result<Value, Denial> {
    // DRF's `_load_stream` leaves the stream `None` at content-length
    // zero and `_parse` returns the empty mapping without touching a
    // parser (`request.py`, DRF 3.15.2; same shape as the merged
    // precedents) — so emptiness is checked before the media type.
    if body.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let media = content_type.split(';').next().unwrap_or("").trim();
    let is_json = media.eq_ignore_ascii_case("application/json");
    if !is_json {
        let raw = if content_type.is_empty() {
            "text/plain"
        } else {
            content_type
        };
        return Err(Denial::UnsupportedMediaType(raw.to_owned()));
    }
    serde_json::from_slice(body)
        .map_err(|error| Denial::BadDetail(format!("JSON parse error - {error}")))
}

/// Django `QueryDict.get`: the last value, or `None`.
fn query_last(params: &HashMap<String, Vec<String>>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|values| values.iter().last().cloned())
}

/// Badly-formed UUIDs in detail paths are `get_object` misses: DRF's
/// `get_object_or_404` (`generics.py:14-21`) catches the Django
/// `ValidationError` the UUID lookup raises and re-raises `Http404`, so a
/// bad pk is the 404 `{"detail": "Not found."}` — never the
/// `handle_exception` `ValidationError` branch (that branch only sees
/// `ValidationError`s raised outside `get_object`, which this view has
/// none of).
fn parse_uuid_or_invalid(raw: &str) -> Result<Uuid, Denial> {
    raw.parse::<Uuid>().map_err(|_| Denial::NotFoundDetail)
}

/// `get_object` for one `(slug, owner, pk)`: the scoped lookup above; any
/// miss is the DRF `Http404` 404.
async fn get_sticky(
    pool: &sqlx::PgPool,
    slug: &str,
    owner_id: &Uuid,
    pk: &Uuid,
) -> Result<StickyRow, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sticky_get_sql())
        .bind(slug)
        .bind(owner_id)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::NotFoundDetail)?;
    StickyRow::from_row(&row)
}

/// Post-write render fetch ([`sticky_get_by_pk_sql`]): unscoped by-pk, for
/// the create/update responses. A miss here is unreachable (no path
/// removes rows), so it is the 500 Python's `DoesNotExist`-outside-a-view
/// shape becomes.
async fn get_sticky_by_pk(pool: &sqlx::PgPool, pk: &Uuid) -> Result<StickyRow, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sticky_get_by_pk_sql())
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ServerError)?;
    StickyRow::from_row(&row)
}

/// Run the full serializer write path: pure field validation, the users
/// existence lookups for the audit FKs (errors appended: the audit FKs
/// are the last writable keys), then the `validate()` step. Returns the
/// write plus the final `description_html` for `description_stripped`
/// recomputation.
async fn validated_write(
    pool: &sqlx::PgPool,
    timezone: &Tz,
    parsed: &Value,
) -> Result<(StickyWrite, Option<String>), Denial> {
    let mut validation = validate_sticky_input(parsed, timezone);
    if !validation.pending.is_empty() {
        let mut pk_errors =
            resolve_pk_lookups(pool, &mut validation.write, &validation.pending).await?;
        validation.errors.append(&mut pk_errors);
    }
    if !validation.errors.is_empty() {
        return Err(Denial::FieldErrors(errors_value(validation.errors)));
    }
    apply_validate_step(validation.write, validation.html)
}

/// Request preamble shared by all five actions: pool, acting user (401/403),
/// `WorkspaceUserPermission` (403), render zone.
struct Context {
    pool: sqlx::PgPool,
    user_id: Uuid,
    timezone: Tz,
}

async fn context(state: &AppState, headers: &HeaderMap, slug: &str) -> Result<Context, Denial> {
    let pool = pool_of(state)?;
    let secret = state.settings().secret_key.as_bytes();
    let user_id = authenticate(&pool, secret, headers).await?;
    require_workspace_user(&pool, slug, &user_id).await?;
    // Gate passed: activate the stored zone now (`TimezoneMixin.initial`
    // runs after permissions; an unknown zone 400s and an empty zone
    // 500s, only for survivors).
    let zone_name = load_timezone_name(&pool, &user_id).await?;
    let timezone = activate_timezone(zone_name.as_deref())?;
    Ok(Context {
        pool,
        user_id,
        timezone,
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `create` (`sticky.py:48-54`): workspace lookup by slug, serializer
/// validation, save with workspace + owner, 201.
async fn sticky_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match create_inner(&state, &headers, &slug, &body).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn create_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    body: &[u8],
) -> HandlerResult {
    let ctx = context(state, headers, slug).await?;
    // Python order (`sticky.py:48-51`): the workspace lookup runs before
    // validation, so an unknown slug 404s even with an invalid body.
    let workspace_id: Option<Uuid> = sqlx::query_scalar(WORKSPACE_ID_SQL)
        .bind(slug)
        .fetch_optional(&ctx.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let workspace_id = workspace_id.ok_or(Denial::WorkspaceMissing)?;
    let parsed = parse_json_body(body, headers)?;
    let (write, html) = validated_write(&ctx.pool, &ctx.timezone, &parsed).await?;
    // `Sticky.save` (`models/sticky.py:38-53`): stripped text recomputed on
    // every save; `sort_order` is `MAX + 10000` whenever rows exist (an
    // input value is overwritten then), else the input or the 65535 default.
    let final_html = html.unwrap_or_else(|| "<p></p>".to_owned());
    let stripped = sticky_model::stripped_description(Some(&final_html));
    let existing_max: Option<f64> = sqlx::query_scalar(STICKY_MAX_SORT_SQL)
        .bind(workspace_id)
        .fetch_optional(&ctx.pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .flatten();
    let sort_order = match existing_max {
        Some(max) => max + sticky_model::SORT_ORDER_STEP,
        None => write.sort_order.unwrap_or(sticky_model::DEFAULT_SORT_ORDER),
    };
    let now = Utc::now();
    let id = Uuid::new_v4();
    sqlx::query(STICKY_INSERT_SQL)
        .bind(id)
        .bind(now)
        .bind(now)
        // `BaseModel.save` overwrites any `created_by` input from the
        // request user and leaves `updated_by` null on create.
        .bind(ctx.user_id)
        .bind(Option::<Uuid>::None)
        // A `deleted_at` input is honored: the row is born deleted (and
        // still answers 201 with the row, rendered unscoped below).
        .bind(write.deleted_at.unwrap_or(None))
        .bind(write.name.unwrap_or(None))
        .bind(write.description.unwrap_or_else(|| serde_json::json!({})))
        .bind(final_html)
        .bind(stripped)
        .bind(Option::<Vec<u8>>::None)
        .bind(write.logo_props.unwrap_or_else(|| serde_json::json!({})))
        .bind(write.color.unwrap_or(None))
        .bind(write.background_color.unwrap_or(None))
        .bind(workspace_id)
        .bind(ctx.user_id)
        .bind(sort_order)
        .execute(&ctx.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    // Re-read the stored row (never render the pre-insert values: the
    // database owns timestamp precision), unscoped: Django renders the
    // saved instance even when it is born deleted.
    let row = get_sticky_by_pk(&ctx.pool, &id).await?;
    Ok(json_response(
        StatusCode::CREATED,
        render_sticky_row(&row, &ctx.timezone)?,
    ))
}

/// `list` (`sticky.py:66-77`): owner-scoped queryset, `-created_at` order,
/// optional `description_stripped__icontains`, `paginate` with
/// `default_per_page=20` and the 12-key envelope.
async fn sticky_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, Vec<String>>>,
) -> Response {
    match list_inner(&state, &headers, &slug, &params).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    params: &HashMap<String, Vec<String>>,
) -> HandlerResult {
    use crate::paginator::{
        apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor, Cursor,
    };
    use pidash_services::app_issues::envelope;

    let ctx = context(state, headers, slug).await?;
    // `request.query_params.get("query", False)` + `if query:` — absent and
    // empty both yield the unfiltered list.
    let query = query_last(params, "query");
    let query_filter = query.as_deref().filter(|text| !text.is_empty());
    let per_page =
        crate::paginator::parse_per_page(query_last(params, "per_page").as_deref(), 20, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(params, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor =
        Cursor::from_string(&cursor_raw).map_err(|error| Denial::BadDetail(error.detail()))?;
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    let mut list_sql = sticky_queries::sticky_list_sql(query_filter);
    list_sql.push_str(&format!(
        " LIMIT {} OFFSET {}",
        window.stop - window.offset,
        window.offset
    ));
    let count_sql = sticky_count_sql(&sticky_queries::sticky_list_sql(query_filter));
    let pattern = query_filter.map(sticky_queries::icontains_pattern);
    let rows: Vec<sqlx::postgres::PgRow> = if let Some(pattern) = pattern.as_deref() {
        sqlx::query(&list_sql)
            .bind(slug)
            .bind(ctx.user_id)
            .bind(pattern)
            .fetch_all(&ctx.pool)
            .await
            .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query(&list_sql)
            .bind(slug)
            .bind(ctx.user_id)
            .fetch_all(&ctx.pool)
            .await
            .map_err(|_| Denial::ServerError)?
    };
    let has_more = rows.len() as i64 > limit;
    let sticky_rows: Vec<StickyRow> = rows
        .iter()
        .map(StickyRow::from_row)
        .collect::<Result<_, _>>()?;
    let page = apply_offset_window(&sticky_rows, limit).map_err(page_denial)?;
    // `COUNT` always yields exactly one non-null row.
    let total_count: i64 = if let Some(pattern) = pattern.as_deref() {
        sqlx::query_scalar(&count_sql)
            .bind(slug)
            .bind(ctx.user_id)
            .bind(pattern)
            .fetch_optional(&ctx.pool)
            .await
            .map_err(|_| Denial::ServerError)?
            .unwrap_or(0)
    } else {
        sqlx::query_scalar(&count_sql)
            .bind(slug)
            .bind(ctx.user_id)
            .fetch_optional(&ctx.pool)
            .await
            .map_err(|_| Denial::ServerError)?
            .unwrap_or(0)
    };
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let mut rendered = Vec::with_capacity(page.len());
    for row in &page {
        rendered.push(render_sticky_row(row, &ctx.timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        envelope(
            None,
            None,
            total_count,
            &next.to_string(),
            &prev.to_string(),
            next.has_results_or_false(),
            prev.has_results_or_false(),
            rendered.len(),
            max_hits(total_count, limit).map_err(page_denial)?,
            total_count,
            &format!("[{}]", rendered.join(",")),
        ),
    ))
}

/// `retrieve` (`sticky.py:85-87`): `get_object` + serializer, 200.
async fn sticky_retrieve(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    match detail_inner(&state, &headers, &slug, &pk, DetailAction::Retrieve, None).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailAction {
    Retrieve,
    PartialUpdate,
    Destroy,
}

/// `partial_update` (`sticky.py:96-102`): partial serializer save,
/// 200 / 400 errors verbatim.
async fn sticky_partial_update(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match detail_inner(
        &state,
        &headers,
        &slug,
        &pk,
        DetailAction::PartialUpdate,
        Some(&body),
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `destroy` (`sticky.py:110-113`): soft `delete()`, 204.
async fn sticky_destroy(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    match detail_inner(&state, &headers, &slug, &pk, DetailAction::Destroy, None).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn detail_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    raw_pk: &str,
    action: DetailAction,
    body: Option<&[u8]>,
) -> HandlerResult {
    let ctx = context(state, headers, slug).await?;
    let pk = parse_uuid_or_invalid(raw_pk)?;
    let row = get_sticky(&ctx.pool, slug, &ctx.user_id, &pk).await?;
    match action {
        DetailAction::Retrieve => Ok(json_response(
            StatusCode::OK,
            render_sticky_row(&row, &ctx.timezone)?,
        )),
        DetailAction::Destroy => {
            // `SoftDeleteModel.delete(soft=True)`: two stamps — `deleted_at`
            // first, then the `save()`-time `updated_at` (`auto_now`) —
            // plus `updated_by` from the request user, then the related
            // sweep task (best-effort; the 204 stands without it).
            let deleted_at = Utc::now();
            sqlx::query(STICKY_SOFT_DELETE_SQL)
                .bind(deleted_at)
                .bind(Utc::now())
                .bind(ctx.user_id)
                .bind(pk)
                .execute(&ctx.pool)
                .await
                .map_err(|_| Denial::ServerError)?;
            enqueue_soft_delete(&ctx.pool, &pk).await;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        DetailAction::PartialUpdate => {
            let parsed = parse_json_body(body.unwrap_or_default(), headers)?;
            let (write, html) = validated_write(&ctx.pool, &ctx.timezone, &parsed).await?;
            // `save()` recomputes `description_stripped` on EVERY save;
            // `sort_order` is untouched on update.
            let final_html = html.unwrap_or_else(|| row.description_html.clone());
            let stripped = sticky_model::stripped_description(Some(&final_html));
            sqlx::query(STICKY_UPDATE_SQL)
                .bind(write.name.unwrap_or_else(|| row.name.clone()))
                .bind(write.description.unwrap_or_else(|| row.description.clone()))
                .bind(
                    write
                        .description_html
                        .unwrap_or_else(|| row.description_html.clone()),
                )
                .bind(stripped)
                .bind(write.logo_props.unwrap_or_else(|| row.logo_props.clone()))
                .bind(write.color.unwrap_or_else(|| row.color.clone()))
                .bind(
                    write
                        .background_color
                        .unwrap_or_else(|| row.background_color.clone()),
                )
                .bind(write.sort_order.unwrap_or(row.sort_order))
                .bind(Utc::now())
                .bind(ctx.user_id)
                .bind(write.created_by.unwrap_or(row.created_by_id))
                .bind(write.deleted_at.unwrap_or(row.deleted_at))
                .bind(pk)
                .execute(&ctx.pool)
                .await
                .map_err(|_| Denial::ServerError)?;
            // Unscoped: Django renders the saved instance even when a
            // `deleted_at` input just tombstoned it.
            let updated = get_sticky_by_pk(&ctx.pool, &pk).await?;
            Ok(json_response(
                StatusCode::OK,
                render_sticky_row(&updated, &ctx.timezone)?,
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// The Celery message `SoftDeleteModel.delete` fires
/// (`db/mixins.py:78`): `soft_delete_related_objects.delay("db",
/// "sticky", pk, using=None)` — three positional args, `using` as a null
/// kwarg (the view calls `.delete()` with no `using`).
fn soft_delete_message(sticky_id: &Uuid) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("sticky".to_owned()),
            Value::String(sticky_id.to_string()),
        ],
        kwargs,
    )
}

/// Enqueue the related sweep after the tombstone lands (same best-effort
/// shape as the webhook precedent: without it the 204 still stands).
async fn enqueue_soft_delete(pool: &sqlx::PgPool, sticky_id: &Uuid) {
    let message = soft_delete_message(sticky_id);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_timezone_empty_zone_500s() {
        // `ZoneInfo('')` raises `ValueError` (not `KeyError`), so an
        // empty stored zone is the generic 500 while an unknown zone is
        // the `KeyError`-branch 400 (PIDASHCONV-747, live-probed). A
        // missing zone stays the 500 (unlike the sibling carriers'
        // UTC default — race-only, preserved as-is).
        assert!(matches!(activate_timezone(None), Err(Denial::ServerError)));
        assert!(matches!(
            activate_timezone(Some("")),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            activate_timezone(Some("Not/AZone")),
            Err(Denial::BadError(_))
        ));
        assert_eq!(activate_timezone(Some("UTC")).expect("utc"), chrono_tz::UTC);
    }

    fn json_map(pairs: &[(&str, Value)]) -> Value {
        let mut map = Map::with_capacity(pairs.len());
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        Value::Object(map)
    }

    fn utc() -> Tz {
        "UTC".parse().expect("tz")
    }

    /// Field errors of `body` as `(field, messages)` pairs in order.
    fn field_errors(body: &Value) -> Vec<(String, Vec<String>)> {
        validate_sticky_input(body, &utc()).errors
    }

    // -- routes (fx-h-sticky `routes`: DefaultRouter under workspaces/<slug>/) --

    #[test]
    fn collection_and_detail_paths_match_the_router() {
        assert_eq!(STICKIES_PATH, "/api/v1/workspaces/{slug}/stickies/");
        assert_eq!(STICKY_PATH, "/api/v1/workspaces/{slug}/stickies/{pk}/");
    }

    #[test]
    fn gate_for_sticky_is_workspace_user() {
        assert_eq!(
            v1_perm::gate_for(v1_perm::V1AssetsRoute::Sticky),
            v1_perm::V1AssetsGate::WorkspaceUser
        );
    }

    // -- denial bodies (byte parity with DRF) --

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            INVALID_TOKEN_BODY,
            r#"{"detail":"Given API token is not valid"}"#
        );
        assert_eq!(
            WORKSPACE_MISSING_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(NOT_FOUND_DETAIL_BODY, r#"{"detail":"Not found."}"#);
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
        // The class denial is the DRF-default body (fx-perm goldens).
        assert_eq!(
            v1_perm::CLASS_DENIAL_BODY,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
    }

    #[test]
    fn invalid_uuid_pk_is_a_get_object_404() {
        // DRF's `get_object_or_404` converts the UUID `ValidationError`
        // to `Http404` — never the `handle_exception` branch.
        let Err(Denial::NotFoundDetail) = parse_uuid_or_invalid("zzz") else {
            panic!("expected NotFoundDetail");
        };
        let (status, body) = Denial::NotFoundDetail.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, NOT_FOUND_DETAIL_BODY);
        assert!(parse_uuid_or_invalid("11111111-1111-1111-1111-111111111111").is_ok());
    }

    #[test]
    fn per_page_and_cursor_denials_carry_detail_bodies() {
        let (status, body) =
            Denial::BadDetail("Invalid per_page parameter.".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"detail":"Invalid per_page parameter."}"#);
        // page_denial maps kernel input errors to BadDetail, arithmetic to 500.
        let (status, _) = page_denial(crate::paginator::PageError::InvalidCursor).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = page_denial(crate::paginator::PageError::ZeroLimit).status_and_body();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, SERVER_ERROR_BODY);
    }

    // -- list pagination inputs (sticky.py:66-77, default_per_page=20) --

    #[test]
    fn per_page_defaults_to_20_with_a_1000_ceiling() {
        assert_eq!(
            crate::paginator::parse_per_page(None, 20, 1000).expect("default"),
            20
        );
        assert_eq!(
            crate::paginator::parse_per_page(Some("20"), 20, 1000).expect("exact"),
            20
        );
        assert_eq!(
            crate::paginator::parse_per_page(Some("1000"), 20, 1000).expect("max"),
            1000
        );
        assert!(crate::paginator::parse_per_page(Some("lots"), 20, 1000).is_err());
        assert!(crate::paginator::parse_per_page(Some("1001"), 20, 1000).is_err());
    }

    #[test]
    fn query_falsy_guard_matches_the_view() {
        // `request.query_params.get("query", False)` + `if query:` — absent
        // and empty both yield the unfiltered list (fx-q-sticky).
        let base = sticky_queries::sticky_list_sql(None);
        assert_eq!(sticky_queries::sticky_list_sql(Some("")), base);
        assert!(!base.contains("LIKE"));
        assert!(
            sticky_queries::sticky_list_sql(Some("pineapple")).contains("LIKE"),
            "truthy query filters"
        );
    }

    #[test]
    fn list_sql_carries_limit_offset_and_count_drops_order() {
        let mut list_sql = sticky_queries::sticky_list_sql(Some("x"));
        list_sql.push_str(&format!(" LIMIT {} OFFSET {}", 21, 0));
        assert!(list_sql.contains("SELECT DISTINCT "), "{list_sql}");
        assert!(
            list_sql.contains("ORDER BY \"stickies\".\"created_at\" DESC"),
            "{list_sql}"
        );
        assert!(list_sql.ends_with("LIMIT 21 OFFSET 0"), "{list_sql}");
        let count = sticky_count_sql(&list_sql);
        assert!(
            count.starts_with("SELECT COUNT(DISTINCT \"stickies\".\"id\") FROM \"stickies\""),
            "{count}"
        );
        assert!(!count.contains("ORDER BY"), "{count}");
        assert!(count.contains("LIKE"), "{count}");
        // Unfiltered count keeps the scope and drops nothing else.
        let plain = sticky_count_sql(&sticky_queries::sticky_list_sql(None));
        assert!(plain.contains("\"workspaces\".\"slug\" = ($1)"), "{plain}");
        assert!(!plain.contains("LIKE"), "{plain}");
    }

    // -- get SQL (get_queryset scope + pk) --

    #[test]
    fn get_sql_scopes_slug_owner_pk_and_projects_all_columns() {
        let sql = sticky_get_sql();
        assert!(sql.starts_with("SELECT DISTINCT "), "{sql}");
        for col in sticky_model::COLUMNS {
            assert!(
                sql.contains(&format!("\"stickies\".\"{col}\"")),
                "{sql}: {col}"
            );
        }
        assert!(sql.contains("\"workspaces\".\"slug\" = $1"), "{sql}");
        assert!(sql.contains("\"stickies\".\"owner_id\" = $2"), "{sql}");
        assert!(sql.contains("\"stickies\".\"id\" = $3"), "{sql}");
        assert!(sql.contains("\"stickies\".\"deleted_at\" IS NULL"), "{sql}");
    }

    #[test]
    fn write_statements_hit_the_table() {
        // Soft delete (`SoftDeleteModel.delete(soft=True)`): tombstone +
        // the inner `save()`'s `updated_at`/`updated_by` bump.
        assert_eq!(
            STICKY_SOFT_DELETE_SQL,
            "UPDATE stickies SET deleted_at = $1, updated_at = $2, updated_by_id = $3 WHERE id = $4"
        );
        assert!(
            STICKY_INSERT_SQL.starts_with("INSERT INTO stickies ("),
            "{STICKY_INSERT_SQL}"
        );
        assert!(
            STICKY_INSERT_SQL.contains("VALUES ($1"),
            "{STICKY_INSERT_SQL}"
        );
        assert!(
            STICKY_UPDATE_SQL.starts_with("UPDATE stickies SET "),
            "{STICKY_UPDATE_SQL}"
        );
        assert!(
            STICKY_UPDATE_SQL.ends_with("WHERE id = $13"),
            "{STICKY_UPDATE_SQL}"
        );
        // `created_by` (writable audit FK) and `deleted_at` ride along;
        // the read-only columns stay guarded (the `WHERE id = $13` tail
        // is the pk lookup, not a write).
        assert!(
            STICKY_UPDATE_SQL.contains("created_by_id = $11"),
            "{STICKY_UPDATE_SQL}"
        );
        assert!(
            STICKY_UPDATE_SQL.contains("deleted_at = $12"),
            "{STICKY_UPDATE_SQL}"
        );
        for guarded in ["workspace_id", "owner_id", "created_at = $"] {
            assert!(
                !STICKY_UPDATE_SQL.contains(guarded),
                "{STICKY_UPDATE_SQL}: {guarded}"
            );
        }
        assert!(
            WORKSPACE_ID_SQL.contains("deleted_at IS NULL"),
            "{WORKSPACE_ID_SQL}"
        );
        // Machine-token membership honors the soft-delete manager.
        assert!(
            MACHINE_MEMBER_SQL.contains("deleted_at IS NULL"),
            "{MACHINE_MEMBER_SQL}"
        );
        assert_eq!(
            USER_EXISTS_SQL,
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)"
        );
    }

    #[test]
    fn post_write_reread_is_unscoped_by_pk() {
        // Django renders the saved instance even when born deleted, so
        // the create/update responses must not apply the manager scope.
        let sql = sticky_get_by_pk_sql();
        assert!(!sql.contains("deleted_at IS NULL"), "{sql}");
        assert!(!sql.contains("DISTINCT"), "{sql}");
        assert!(sql.contains("WHERE \"stickies\".\"id\" = $1"), "{sql}");
        for col in sticky_model::COLUMNS {
            assert!(
                sql.contains(&format!("\"stickies\".\"{col}\"")),
                "{sql}: {col}"
            );
        }
    }

    #[test]
    fn soft_delete_message_matches_the_celery_call() {
        // `.delay("db", "sticky", pk, using=None)`: three positionals,
        // `using` as a null kwarg.
        let pk = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let message = soft_delete_message(&pk);
        assert_eq!(
            message.task,
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
        );
        assert_eq!(
            message.args,
            vec![
                Value::String("db".to_owned()),
                Value::String("sticky".to_owned()),
                Value::String(pk.to_string()),
            ]
        );
        assert_eq!(message.kwargs.get("using"), Some(&Value::Null));
        assert_eq!(message.kwargs.len(), 1);
    }

    // -- validation (fx-ser-sticky goldens through the handler path) --

    #[test]
    fn empty_object_validates_to_no_writes() {
        let validation = validate_sticky_input(&json_map(&[]), &utc());
        assert!(validation.errors.is_empty());
        assert!(validation.pending.is_empty());
        assert!(validation.write.name.is_none());
        assert!(validation.html.is_none());
    }

    #[test]
    fn read_only_and_unknown_keys_are_silently_dropped() {
        let body = json_map(&[
            ("name", serde_json::json!("note")),
            (
                "workspace",
                serde_json::json!("11111111-1111-1111-1111-111111111111"),
            ),
            (
                "owner",
                serde_json::json!("22222222-2222-2222-2222-222222222222"),
            ),
            (
                "id",
                serde_json::json!("33333333-3333-3333-3333-333333333333"),
            ),
            ("description_binary", serde_json::json!("not-base64!!!___")),
            ("nope", serde_json::json!(1)),
        ]);
        let validation = validate_sticky_input(&body, &utc());
        assert!(validation.errors.is_empty(), "{:?}", validation.errors);
        assert_eq!(validation.write.name, Some(Some("note".to_owned())));
        assert!(validation.write.description_html.is_none());
    }

    #[test]
    fn validate_step_sanitizes_but_field_pass_keeps_raw() {
        // Field validation keeps the stripped input; `validate()` (the
        // types kernel) does the sanitize — and only runs error-free.
        let validation = validate_sticky_input(
            &json_map(&[(
                "description_html",
                serde_json::json!("<p>hello</p><script>evil()</script>"),
            )]),
            &utc(),
        );
        assert!(validation.errors.is_empty());
        assert_eq!(
            validation.html,
            Some("<p>hello</p><script>evil()</script>".to_owned())
        );
        let (_, html) = apply_validate_step(validation.write, validation.html)
            .expect("unsafe html sanitizes, not rejects");
        assert_eq!(html, Some("<p>hello</p>".to_owned()));
        let safe = validate_sticky_input(
            &json_map(&[("description_html", serde_json::json!("<p>hello</p>"))]),
            &utc(),
        );
        let (_, html) = apply_validate_step(safe.write, safe.html).expect("safe");
        assert_eq!(html, Some("<p>hello</p>".to_owned()));
    }

    #[test]
    fn falsy_html_skips_the_branches() {
        let validation = validate_sticky_input(
            &json_map(&[("description_html", serde_json::json!(""))]),
            &utc(),
        );
        assert!(validation.errors.is_empty());
        assert_eq!(validation.html, Some(String::new()));
    }

    #[test]
    fn text_fields_coerce_strip_and_blank() {
        // `str()` of ints/floats, stripped; bools and containers reject.
        for (raw, want) in [
            (serde_json::json!(123), "123"),
            (serde_json::json!(12.5), "12.5"),
            (serde_json::json!("  padded  "), "padded"),
        ] {
            let validation = validate_sticky_input(&json_map(&[("name", raw)]), &utc());
            assert!(validation.errors.is_empty(), "{:?}", validation.errors);
            assert_eq!(validation.write.name, Some(Some(want.to_owned())));
        }
        for raw in [
            serde_json::json!(true),
            serde_json::json!([1]),
            serde_json::json!({"a": 1}),
        ] {
            let errors = field_errors(&json_map(&[("name", raw)]));
            assert_eq!(
                errors,
                vec![("name".to_owned(), vec!["Not a valid string.".to_owned()])]
            );
        }
        // Whitespace-only blanks to `""` (all text fields allow blank).
        let validation =
            validate_sticky_input(&json_map(&[("color", serde_json::json!("   "))]), &utc());
        assert!(validation.errors.is_empty());
        assert_eq!(validation.write.color, Some(Some(String::new())));
    }

    #[test]
    fn color_over_255_collects_both_validator_messages_in_serializer_order() {
        let body = json_map(&[
            ("sort_order", serde_json::json!("oops")),
            ("color", serde_json::json!("x".repeat(256))),
        ]);
        let errors = field_errors(&body);
        let keys: Vec<&str> = errors.iter().map(|(field, _)| field.as_str()).collect();
        // Serializer field order: color before sort_order.
        assert_eq!(keys, vec!["color", "sort_order"]);
        assert_eq!(
            errors[0].1,
            vec!["Ensure this field has no more than 255 characters.".to_owned()]
        );
        assert_eq!(errors[1].1, vec!["A valid number is required.".to_owned()]);
        // Over-long *and* NUL-bearing: both validator messages collected.
        let mut long = "y".repeat(256);
        long.push('\0');
        let errors = field_errors(&json_map(&[("color", serde_json::json!(long))]));
        assert_eq!(
            errors,
            vec![(
                "color".to_owned(),
                vec![
                    "Ensure this field has no more than 255 characters.".to_owned(),
                    "Null characters are not allowed.".to_owned(),
                ]
            )]
        );
        // The 400 body preserves that order (`preserve_order`).
        let Value::Object(rendered) = errors_value(errors) else {
            panic!("object body");
        };
        let keys: Vec<&str> = rendered.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["color"]);
    }

    #[test]
    fn audit_fields_sort_last_in_serializer_order() {
        // DRF serializer field order: the plain `deleted_at` leads, the
        // audit FKs trail every content field.
        let body = json_map(&[
            ("name", serde_json::json!([1])),
            ("created_by", serde_json::json!(true)),
            ("deleted_at", serde_json::json!("tomorrow")),
        ]);
        let errors = field_errors(&body);
        let keys: Vec<&str> = errors.iter().map(|(field, _)| field.as_str()).collect();
        assert_eq!(keys, vec!["deleted_at", "name", "created_by"]);
        assert_eq!(
            errors[2].1,
            vec!["Incorrect type. Expected pk value, received bool.".to_owned()]
        );
    }

    #[test]
    fn null_name_clears_and_null_html_rejects() {
        let validation = validate_sticky_input(&json_map(&[("name", Value::Null)]), &utc());
        assert!(validation.errors.is_empty());
        assert_eq!(validation.write.name, Some(None));
        let errors = field_errors(&json_map(&[("description_html", Value::Null)]));
        assert_eq!(
            errors,
            vec![(
                "description_html".to_owned(),
                vec!["This field may not be null.".to_owned()]
            )]
        );
    }

    #[test]
    fn null_json_and_sort_order_reject() {
        // `description`, `logo_props` and `sort_order` are `null=False`:
        // explicit nulls are the `null` message (validate_empty_values
        // precedes `to_internal_value`).
        for field in ["description", "logo_props", "sort_order"] {
            let errors = field_errors(&json_map(&[(field, Value::Null)]));
            assert_eq!(
                errors,
                vec![(
                    field.to_owned(),
                    vec!["This field may not be null.".to_owned()]
                )],
                "{field}"
            );
        }
        // Other JSON values pass through untouched.
        let validation = validate_sticky_input(
            &json_map(&[
                ("description", serde_json::json!({"blocks": []})),
                ("logo_props", serde_json::json!("plain-string")),
            ]),
            &utc(),
        );
        assert!(validation.errors.is_empty());
        assert_eq!(
            validation.write.description,
            Some(serde_json::json!({"blocks": []}))
        );
    }

    #[test]
    fn non_dict_body_is_a_non_field_error() {
        let over_u64: Value = serde_json::from_str("18446744073709551616").expect("bignum");
        for (raw, what) in [
            (serde_json::json!([1, 2]), "list"),
            (serde_json::json!(1.5), "float"),
            (serde_json::json!(7), "int"),
            (over_u64, "int"),
            (serde_json::json!(true), "bool"),
        ] {
            let errors = field_errors(&raw);
            assert_eq!(errors.len(), 1, "{raw}");
            assert_eq!(errors[0].0, "non_field_errors");
            assert_eq!(
                errors[0].1,
                vec![format!(
                    "Invalid data. Expected a dictionary, but got {what}."
                )]
            );
        }
        // A JSON null never reaches `to_internal_value`: the serializer
        // reports the friendlier message instead of the `NoneType` shape.
        let errors = field_errors(&Value::Null);
        assert_eq!(
            errors,
            vec![(
                "non_field_errors".to_owned(),
                vec!["No data provided".to_owned()]
            )]
        );
    }

    #[test]
    fn empty_body_is_an_empty_object_whatever_the_content_type() {
        // DRF returns the empty mapping for a zero-length body without
        // touching a parser — even under a JSON content type.
        let mut json_headers = HeaderMap::new();
        json_headers.insert(
            header::CONTENT_TYPE,
            "application/json".parse().expect("content type"),
        );
        for headers in [HeaderMap::new(), json_headers] {
            let parsed = parse_json_body(b"", &headers).expect("empty body");
            assert_eq!(parsed, Value::Object(Map::new()));
        }
        // A non-empty non-JSON body is 415 with the content type echoed.
        let mut text_headers = HeaderMap::new();
        text_headers.insert(
            header::CONTENT_TYPE,
            "text/plain".parse().expect("content type"),
        );
        let denial = parse_json_body(b"hello", &text_headers).expect_err("415");
        let (status, body) = denial.status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            body,
            r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#
        );
        // Malformed JSON is DRF's `ParseError`.
        let mut json_headers = HeaderMap::new();
        json_headers.insert(
            header::CONTENT_TYPE,
            "application/json".parse().expect("content type"),
        );
        let denial = parse_json_body(b"{oops", &json_headers).expect_err("parse error");
        let (status, body) = denial.status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.starts_with(r#"{"detail":"JSON parse error - "#),
            "{body}"
        );
    }

    #[test]
    fn sort_order_accepts_numbers_and_numeric_strings() {
        for (raw, want) in [
            (serde_json::json!(1), 1.0),
            (serde_json::json!(75535.0), 75535.0),
            (serde_json::json!("12.5"), 12.5),
            (serde_json::json!(" 1_0 "), 10.0),
            (serde_json::json!(true), 1.0),
        ] {
            let validation = validate_sticky_input(&json_map(&[("sort_order", raw)]), &utc());
            assert!(validation.errors.is_empty(), "{:?}", validation.errors);
            assert_eq!(validation.write.sort_order, Some(want));
        }
        // Misplaced underscores, over-long strings, and non-numerics.
        for raw in [
            serde_json::json!("_1"),
            serde_json::json!("1_"),
            serde_json::json!("1__2"),
            serde_json::json!([1]),
        ] {
            let errors = field_errors(&json_map(&[("sort_order", raw)]));
            assert_eq!(
                errors,
                vec![(
                    "sort_order".to_owned(),
                    vec!["A valid number is required.".to_owned()]
                )]
            );
        }
        let errors = field_errors(&json_map(&[(
            "sort_order",
            serde_json::json!("9".repeat(1001)),
        )]));
        assert_eq!(
            errors,
            vec![(
                "sort_order".to_owned(),
                vec!["String value too large.".to_owned()]
            )]
        );
    }

    // -- audit FK inputs (`created_by` / `updated_by`) --

    #[test]
    fn audit_pk_null_passes_and_formats_queue_lookups() {
        let uuid = "11111111-1111-1111-1111-111111111111";
        let validation = validate_sticky_input(
            &json_map(&[
                ("created_by", serde_json::json!(uuid)),
                ("updated_by", Value::Null),
            ]),
            &utc(),
        );
        assert!(validation.errors.is_empty(), "{:?}", validation.errors);
        assert_eq!(validation.pending.len(), 1);
        assert_eq!(validation.pending[0].field, "created_by");
        assert_eq!(validation.pending[0].uuid.to_string(), uuid);
        // The verbatim input renders the miss message (never lowercased).
        assert_eq!(validation.pending[0].display, uuid);
        let upper = "AAAAAAAA-1111-1111-1111-111111111111";
        let validation = validate_sticky_input(
            &json_map(&[("created_by", serde_json::json!(upper))]),
            &utc(),
        );
        assert!(validation.errors.is_empty());
        assert_eq!(validation.pending[0].display, upper);
        // Small ints take the `int` form and queue a lookup for the
        // derived UUID.
        let validation =
            validate_sticky_input(&json_map(&[("created_by", serde_json::json!(123))]), &utc());
        assert!(validation.errors.is_empty());
        assert_eq!(validation.pending[0].uuid, Uuid::from_u128(123));
        assert_eq!(validation.pending[0].display, "123");
        // The full `u64` range fits in 128 bits (`uuid.UUID(int=...)`
        // accepts everything below 2**128), not just `i64`.
        let validation = validate_sticky_input(
            &json_map(&[("created_by", serde_json::json!(u64::MAX))]),
            &utc(),
        );
        assert!(validation.errors.is_empty(), "{:?}", validation.errors);
        assert_eq!(
            validation.pending[0].uuid,
            Uuid::from_u128(u128::from(u64::MAX))
        );
        assert_eq!(validation.pending[0].display, u64::MAX.to_string());
        // Past `u64::MAX` the exact digits still reach the lookup;
        // past 2**128 they are out of range like negatives.
        let over_u64: Value = serde_json::from_str("18446744073709551616").expect("bignum");
        let validation = validate_sticky_input(&json_map(&[("created_by", over_u64)]), &utc());
        assert!(validation.errors.is_empty(), "{:?}", validation.errors);
        assert_eq!(validation.pending[0].uuid, Uuid::from_u128(1u128 << 64));
        assert_eq!(validation.pending[0].display, "18446744073709551616");
        let over_u128: Value =
            serde_json::from_str("340282366920938463463374607431768211456").expect("bignum");
        let errors = field_errors(&json_map(&[("created_by", over_u128)]));
        assert_eq!(
            errors,
            vec![(
                "created_by".to_owned(),
                vec![
                    "\u{201c}340282366920938463463374607431768211456\u{201d} is not a valid UUID."
                        .to_owned()
                ]
            )]
        );
    }

    #[test]
    fn audit_pk_format_failures_are_field_errors() {
        // Curly quotes wrap `str(value)` (Django `UUIDField` message via
        // DRF's Django-`ValidationError` conversion).
        for (raw, message) in [
            (
                serde_json::json!("zzz"),
                "\u{201c}zzz\u{201d} is not a valid UUID.",
            ),
            (
                serde_json::json!(-1),
                "\u{201c}-1\u{201d} is not a valid UUID.",
            ),
            (
                serde_json::json!(12.5),
                "\u{201c}12.5\u{201d} is not a valid UUID.",
            ),
            (
                serde_json::json!(["x"]),
                "\u{201c}['x']\u{201d} is not a valid UUID.",
            ),
        ] {
            let errors = field_errors(&json_map(&[("created_by", raw)]));
            assert_eq!(
                errors,
                vec![("created_by".to_owned(), vec![message.to_owned()])]
            );
        }
    }

    // -- `deleted_at` inputs --

    #[test]
    fn deleted_at_parses_iso_shapes_in_request_tz() {
        // Zulu.
        let validation = validate_sticky_input(
            &json_map(&[("deleted_at", serde_json::json!("2026-01-02T03:04:05Z"))]),
            &utc(),
        );
        assert!(validation.errors.is_empty(), "{:?}", validation.errors);
        assert_eq!(
            validation.write.deleted_at,
            Some(Some(
                DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
                    .expect("time")
                    .with_timezone(&Utc)
            ))
        );
        // Offset (hours-only, compact, colon).
        for raw in [
            "2026-01-02T05:04:05+02",
            "2026-01-02T05:04:05+0200",
            "2026-01-02T05:04:05+02:00",
        ] {
            let validation =
                validate_sticky_input(&json_map(&[("deleted_at", serde_json::json!(raw))]), &utc());
            assert!(
                validation.errors.is_empty(),
                "{raw}: {:?}",
                validation.errors
            );
            assert_eq!(
                validation.write.deleted_at,
                Some(Some(
                    DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
                        .expect("time")
                        .with_timezone(&Utc)
                )),
                "{raw}"
            );
        }
        // Naive reads in the request zone (Tokyo is +9, no DST).
        let tokyo: Tz = "Asia/Tokyo".parse().expect("tz");
        let validation = validate_sticky_input(
            &json_map(&[("deleted_at", serde_json::json!("2026-01-02T12:00:00"))]),
            &tokyo,
        );
        assert!(validation.errors.is_empty(), "{:?}", validation.errors);
        assert_eq!(
            validation.write.deleted_at,
            Some(Some(
                DateTime::parse_from_rfc3339("2026-01-02T03:00:00Z")
                    .expect("time")
                    .with_timezone(&Utc)
            ))
        );
        // Space separator, comma fraction truncated to 6 digits, null.
        let validation = validate_sticky_input(
            &json_map(&[(
                "deleted_at",
                serde_json::json!("2026-01-02 03:04:05,123456789"),
            )]),
            &utc(),
        );
        assert!(validation.errors.is_empty(), "{:?}", validation.errors);
        assert_eq!(
            validation.write.deleted_at,
            Some(Some(
                DateTime::parse_from_rfc3339("2026-01-02T03:04:05.123456Z")
                    .expect("time")
                    .with_timezone(&Utc)
            ))
        );
        let validation = validate_sticky_input(&json_map(&[("deleted_at", Value::Null)]), &utc());
        assert!(validation.errors.is_empty());
        assert_eq!(validation.write.deleted_at, Some(None));
    }

    #[test]
    fn deleted_at_rejects_with_exact_messages() {
        // Malformed, impossible, and non-string inputs share the
        // `invalid` message (DRF's `suppress` swallows the `ValueError`
        // / `TypeError` alike).
        for raw in [
            serde_json::json!("tomorrow"),
            serde_json::json!("2026-13-01T00:00:00"),
            serde_json::json!("2026-01-01"),
            serde_json::json!(123),
            serde_json::json!(true),
        ] {
            let errors = field_errors(&json_map(&[("deleted_at", raw.clone())]));
            assert_eq!(
                errors,
                vec![(
                    "deleted_at".to_owned(),
                    vec![DATETIME_INVALID_MESSAGE.to_owned()]
                )],
                "{raw}"
            );
        }
        // DST gap in the request zone is the `make_aware` message
        // (America/New_York sprang forward on 2026-03-08 02:00).
        let eastern: Tz = "America/New_York".parse().expect("tz");
        let errors = validate_sticky_input(
            &json_map(&[("deleted_at", serde_json::json!("2026-03-08T02:30:00"))]),
            &eastern,
        )
        .errors;
        assert_eq!(
            errors,
            vec![(
                "deleted_at".to_owned(),
                vec!["Invalid datetime for the timezone \"America/New_York\".".to_owned()]
            )]
        );
        // Conversion past year 9999 is the `overflow` message.
        let apia: Tz = "Pacific/Apia".parse().expect("tz");
        let errors = validate_sticky_input(
            &json_map(&[("deleted_at", serde_json::json!("9999-12-31T23:30:00+00:00"))]),
            &apia,
        )
        .errors;
        assert_eq!(
            errors,
            vec![(
                "deleted_at".to_owned(),
                vec![DATETIME_OVERFLOW_MESSAGE.to_owned()]
            )]
        );
    }

    #[test]
    fn stripped_input_validates_then_discards() {
        // Valid input vanishes (`save()` recomputes); invalid input 400s.
        let validation = validate_sticky_input(
            &json_map(&[("description_stripped", serde_json::json!("hand-set"))]),
            &utc(),
        );
        assert!(validation.errors.is_empty());
        let errors = field_errors(&json_map(&[(
            "description_stripped",
            serde_json::json!([1]),
        )]));
        assert_eq!(
            errors,
            vec![(
                "description_stripped".to_owned(),
                vec!["Not a valid string.".to_owned()]
            )]
        );
    }

    // -- render (StickySerializer read shape) --

    fn sample_row() -> StickyRow {
        let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let workspace = Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        let owner = Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid");
        let moment = DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z")
            .expect("time")
            .with_timezone(&Utc);
        StickyRow {
            id,
            created_at: moment,
            updated_at: moment,
            created_by_id: Some(owner),
            updated_by_id: None,
            deleted_at: None,
            name: Some("note".to_owned()),
            description: serde_json::json!({}),
            description_html: "<p>hello world</p>".to_owned(),
            description_stripped: Some("hello world".to_owned()),
            description_binary: None,
            logo_props: serde_json::json!({}),
            color: Some("#ff0000".to_owned()),
            background_color: None,
            workspace_id: workspace,
            owner_id: owner,
            sort_order: 65535.0,
        }
    }

    #[test]
    fn render_carries_all_17_keys_in_serializer_order_with_zulu_datetimes() {
        let timezone: Tz = "UTC".parse().expect("tz");
        let rendered = render_sticky_row(&sample_row(), &timezone).expect("renders");
        let value: Value = serde_json::from_str(&rendered).expect("json");
        let Value::Object(map) = &value else {
            panic!("object");
        };
        assert_eq!(map.len(), 17, "{rendered}");
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "description",
                "description_html",
                "description_stripped",
                "description_binary",
                "logo_props",
                "color",
                "background_color",
                "sort_order",
                "created_by",
                "updated_by",
                "workspace",
                "owner",
            ]
        );
        assert_eq!(
            value["created_at"],
            serde_json::json!("2026-09-01T12:00:00Z")
        );
        assert_eq!(
            value["description_stripped"],
            serde_json::json!("hello world")
        );
        assert_eq!(value["description_binary"], Value::Null);
        assert_eq!(value["updated_by"], Value::Null);
        assert_eq!(
            value["workspace"],
            serde_json::json!("22222222-2222-2222-2222-222222222222")
        );
        assert_eq!(
            value["owner"],
            serde_json::json!("33333333-3333-3333-3333-333333333333")
        );
        // Float keeps its `.0` (serde_json preserves the f64 shape).
        assert!(rendered.contains("\"sort_order\":65535.0"), "{rendered}");
    }

    #[test]
    fn render_null_name_and_set_binary_is_a_500() {
        let timezone: Tz = "UTC".parse().expect("tz");
        let mut row = sample_row();
        row.name = None;
        let rendered = render_sticky_row(&row, &timezone).expect("null name renders");
        assert!(rendered.contains("\"name\":null"), "{rendered}");
        row.description_binary = Some(vec![1, 2, 3]);
        assert!(
            render_sticky_row(&row, &timezone).is_err(),
            "binary set 500s like Django"
        );
    }

    // -- save rules (fx-model-sticky goldens, handler-owned sequencing) --

    #[test]
    fn save_rules_match_the_model() {
        // Stripped text recomputed on every save.
        assert_eq!(sticky_model::stripped_description(Some("")), None);
        assert_eq!(
            sticky_model::stripped_description(Some("<p></p>")),
            Some(String::new())
        );
        // Sort order: existing max wins (input overwritten); empty workspace
        // keeps the input or the 65535 default.
        assert_eq!(
            sticky_model::sort_order_on_create(None),
            sticky_model::DEFAULT_SORT_ORDER
        );
        assert_eq!(sticky_model::sort_order_on_create(Some(65535.0)), 75535.0);
        assert_eq!(sticky_model::SORT_ORDER_STEP, 10000.0);
    }
}
