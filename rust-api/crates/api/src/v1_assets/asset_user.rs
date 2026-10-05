//! User-asset handlers (D-21, stage 5, PIDASHCONV-419).
//!
//! Ports `UserAssetEndpoint` (`apps/api/pi_dash/api/views/asset.py:48-243`)
//! with routes from `apps/api/pi_dash/api/urls/asset.py:14-23`:
//!
//! * `POST assets/user-assets/` (`asset.py:110-174`)
//! * `PATCH assets/user-assets/<uuid:asset_id>/` (`asset.py:203-220`)
//! * `DELETE assets/user-assets/<uuid:asset_id>/` (`asset.py:231-243`)
//! * helpers `asset_delete` (`asset.py:51-58`) + `entity_asset_delete`
//!   (`asset.py:60-73`)
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-h-asset-user.json`
//! (`fx-h-asset-user`; trace: `rust-api/fixtures/v1_assets/TRACE.md`).
//! The `#[cfg(test)]` suite replays every golden body below, so any drift
//! fails the build. Consumed layers (all merged, read-only here):
//! models (`pidash_db::v1_assets::model::file_asset`), queries
//! (`pidash_db::v1_assets::asset_queries`, incl. `user_scoped_get_sql`,
//! `soft_delete_*_sql`, `entity_user_*_sql` + `EntityClearTarget`), gates
//! (`super::permissions`), tasks
//! (`pidash_jobs::v1_assets::tasks::{metadata_job, storage_metadata_missing}`).
//!
//! This module also owns the shared core for [`super::asset_server`]:
//! the server trio is a line-for-line duplicate of the user trio except
//! `S3Storage(is_server=True)` (`asset.py:336` vs `:163`), so the server
//! handlers call [`handle_post`] with `is_server=true` and share
//! [`handle_patch`] / [`handle_delete`] verbatim (one implementation,
//! parameterized by the credential flag).
//!
//! Shape of the port (translate, don't redesign);
//!
//! * Route registration is the cutover granularity (Porting guide cutover
//!   row, `app_issues::routes` precedent): [`routes`] serves the three
//!   owned methods; every other method falls through to
//!   [`crate::edge::proxy`] so Django answers the 405s, OPTIONS metadata
//!   and resolver 404s exactly as before. Mount wiring (merging [`routes`]
//!   into the app router) belongs to PIDASHCONV-426 — this module must not
//!   touch `overlay.rs` / `routes.rs`.
//! * Authentication is `APIKeyAuthentication`
//!   (`api/middleware/api_authentication.py`): the `X-Api-Key` header only,
//!   via the [`pidash_auth::token`] kernel (exact match, `is_active`,
//!   `expired_at` null or strictly future, `last_used` bump; `mt_` prefix
//!   takes the machine-token path with its revoke-then-deny). Missing/empty
//!   header answers 401; any other failure answers 403 `Given API token is
//!   not valid` (the merged D-19 `v1_projects::handlers_members`
//!   precedent, pinned by contract `test_me_bad_token_403`). The class
//!   carries no extra permission beyond `IsAuthenticated`
//!   (`api/views/base.py:103`), so there is no workspace-membership gate
//!   on any unit — exactly like Python.
//! * Order per request mirrors DRF `initial()`: authN (401/403), then the
//!   body. Anonymous callers never reach a lookup.
//! * Bodies are JSON (`request.data` over the default parsers; the contract
//!   suites speak JSON). An empty or malformed JSON body is proxied to
//!   Django untouched (the D-19 `proxy_through` precedent) so the DRF
//!   `ParseError` bytes stay exact; a well-formed non-object body answers
//!   the generic 500, matching the `AttributeError` the view's `.get`
//!   raises on a list/scalar. PATCH parses after its lookup (DRF lazily
//!   parses `request.data`); DELETE never touches the body at all, so it
//!   parses nothing.
//! * Reads reuse `pidash_db::v1_assets::asset_queries`
//!   (`user_scoped_get_sql`, `soft_delete_*_sql`, `entity_user_*_sql`).
//!   Writes are single statements with the same column semantics as
//!   `FileAsset.objects.create` / `save(update_fields=[...])` / the
//!   full-row `user.save()`. `save(update_fields=[...])` writes ONLY
//!   those columns — verified in Django 4.2 `_save_table` source, which
//!   filters `non_pks` by `update_fields` and never adds `auto_now` —
//!   so (unlike the 421 generic PATCH, whose extra `updated_at = now()`
//!   is contract-invisible) these saves omit `updated_at`.
//! * S3 presigning mirrors `S3Storage` (`settings/storage.py`) offline
//!   (no network), following the merged D-21 `asset_generic` SigV4 shape:
//!   MinIO user mode signs against `{scheme}://{Host}` (scheme from
//!   `X-Forwarded-Proto`, default `http`); server mode signs the
//!   configured endpoint URL; otherwise the virtual-hosted AWS default.
//!   The POST policy lists conditions in `storage.py` order with CPython
//!   `json.dumps` separators.
//! * The PATCH metadata publish (`get_asset_object_metadata.delay`, the
//!   `asset_id=str(asset_id)` kwarg form at `asset.py:215`) enqueues
//!   through [`pidash_jobs::v1_assets::tasks::metadata_job`]. Best-effort:
//!   a failed enqueue warns and the 204 stands (the merged D-31
//!   `handlers_v2_user_workspace::publish_metadata` precedent; the proxy
//!   contract tests never run a worker).
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * BUG-none-key (`asset.py:117,150`): `name` has no required check, so
//!   a missing/null name mints the key `<hex>-None` (and stores
//!   `attributes.name` null). Ported: null name renders `None` in the key.
//! * BUG-size-500 (`asset.py:119`): `int()` runs before the guards, so a
//!   null/non-numeric `size` raises into `handle_exception`'s generic 500
//!   instead of a 400. Ported: convert errors answer 500
//!   `Something went wrong please try again later`.
//! * BUG-mime-message (`asset.py:141-147`): the invalid-type message claims
//!   `Only JPEG and PNG files are allowed.` while webp/jpg/gif also pass.
//!   Ported verbatim.
//! * BUG-auth-only (`asset.py:110-174`): no authZ beyond `IsAuthenticated`
//!   — any authenticated user mints uploads; cross-user isolation is only
//!   the `(id, user_id)` lookup on patch/delete (404 mapping). Ported.
//! * BUG-no-is-deleted (`asset.py:210,237`): the patch/delete `get()` and
//!   the `asset_delete` `filter().first()` carry no `is_deleted`
//!   predicate (only the `deleted_at IS NULL` manager scope). Ported via
//!   the query builders, which pin the exact predicates.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::{Map, Value};
use sha2::Sha256;
use sqlx::Row;
use uuid::Uuid;

use crate::state::AppState;
use pidash_auth::token as auth_token;
use pidash_db::v1_assets::asset_queries;

// ---------------------------------------------------------------------------
// Exact bodies (fx-h-asset-user `error_bodies`, byte-exact)
// ---------------------------------------------------------------------------

/// POST entity guard (`asset.py:126-130`, server twin `:298-303`).
pub const INVALID_ENTITY_BODY: &str = r#"{"error":"Invalid entity type.","status":false}"#;
/// POST MIME guard (`asset.py:140-147`, server twin `:313-320`).
pub const INVALID_TYPE_BODY: &str =
    r#"{"error":"Invalid file type. Only JPEG and PNG files are allowed.","status":false}"#;
/// `handle_exception` `ObjectDoesNotExist` (`base.py:154-158`): patch/delete
/// lookup misses (unknown id, other user's row, second delete).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception` `IntegrityError` (`base.py:142-146`).
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception` generic 500 (`base.py:166-170`; `log_exception` + 500).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// DRF `NotAuthenticated` (`IsAuthenticated` denial on every route here).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `APIKeyAuthentication` failure (invalid, revoked, expired, inactive).
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// Resolver 404 for a non-UUID `<uuid:asset_id>` segment (global
/// `handler404`, `pi_dash/urls.py:15`).
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;

/// POST entity guard allowlist (`asset.py:126`).
pub const USER_ENTITY_TYPES: &[&str] = &["USER_AVATAR", "USER_COVER"];
/// POST MIME allowlist (`asset.py:133-139`), source order.
pub const USER_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the user-asset routes. PIDASHCONV-426 merges this router
/// into the app router; on rebase keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/assets/user-assets/",
            post(user_post).fallback(crate::edge::proxy),
        )
        .route(
            "/api/v1/assets/user-assets/{asset_id}/",
            axum::routing::patch(user_patch)
                .delete(user_delete)
                .fallback(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Small responders (exact Django bytes)
// ---------------------------------------------------------------------------

fn raw(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static response")
}

fn json_body(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(&body).expect("serializable response"),
        ))
        .expect("json response")
}

/// `handle_exception` generic 500 (`base.py:166-170`).
fn server_error(error: impl std::fmt::Display) -> Response {
    tracing::warn!(%error, "user asset handler failed");
    raw(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
}

fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

fn pool(state: &AppState) -> Option<&sqlx::PgPool> {
    state.pools().map(|pools| pools.primary())
}

/// `handle_exception` write split (`base.py:142-146` vs `:166-170`):
/// Django raises `IntegrityError` for SQLSTATE class 23 (NOT NULL, FK,
/// check violations) → 400; every other column error → generic 500.
fn write_error(error: sqlx::Error) -> Response {
    let integrity = matches!(&error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")));
    if integrity {
        return raw(StatusCode::BAD_REQUEST, PAYLOAD_NOT_VALID_BODY);
    }
    server_error(error)
}

// ---------------------------------------------------------------------------
// Auth (`APIKeyAuthentication`, `api/middleware/api_authentication.py`)
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    Unauthorized,
    InvalidToken,
    InvalidEntity,
    InvalidType,
    NotFound,
    ServerError,
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::Unauthorized => raw(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY),
            Denial::InvalidToken => raw(StatusCode::FORBIDDEN, INVALID_TOKEN_BODY),
            Denial::InvalidEntity => raw(StatusCode::BAD_REQUEST, INVALID_ENTITY_BODY),
            Denial::InvalidType => raw(StatusCode::BAD_REQUEST, INVALID_TYPE_BODY),
            Denial::NotFound => raw(StatusCode::NOT_FOUND, NOT_FOUND_BODY),
            Denial::ServerError => raw(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY),
        }
    }
}

/// The authenticated actor: the API token's user id
/// (`validate_api_token` / `validate_machine_token`).
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub id: Uuid,
}

/// Authenticate one request from its `X-Api-Key` header.
///
/// * Missing/empty → 401 (every route here requires `IsAuthenticated`).
/// * `mt_` prefix → machine-token path; anything else → `api_tokens`
///   lookup (exact match, `is_active`, `expired_at` null or strictly
///   future; `last_used` bumped).
/// * Every other failure → 403 `Given API token is not valid`.
pub async fn authenticate(
    pool: &sqlx::PgPool,
    headers: &HeaderMap,
    secret_key: &[u8],
) -> Result<Actor, Denial> {
    let presented = headers
        .get(auth_token::API_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // `if not token: return None` (`api_authentication.py`).
    let kind = auth_token::classify_token(presented).ok_or(Denial::Unauthorized)?;
    let id = match kind {
        auth_token::TokenKind::Api => authenticate_api_token(pool, presented).await?,
        auth_token::TokenKind::Machine => {
            authenticate_machine_token(pool, presented, secret_key).await?
        }
    };
    Ok(Actor { id })
}

async fn authenticate_api_token(pool: &sqlx::PgPool, presented: &str) -> Result<Uuid, Denial> {
    // `deleted_at IS NULL` is the `SoftDeletionManager` scope
    // (`db/mixins.py:56-66`): `APIToken.objects` never sees soft-deleted
    // rows, so a soft-deleted token 403s instead of authenticating.
    let row: Option<(Uuid, bool, Option<DateTime<Utc>>)> =
        sqlx::query_as("SELECT user_id, is_active, expired_at FROM api_tokens WHERE token = $1 AND deleted_at IS NULL")
            .bind(presented)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((user_id, is_active, expired_at)) = row else {
        return Err(Denial::InvalidToken);
    };
    let now = Utc::now();
    let kernel_row = auth_token::ApiTokenRow {
        token: presented.to_owned(),
        is_active,
        expired_at_unix: expired_at.map(|dt| dt.timestamp()),
    };
    // Exact bytes again (constant-time) plus the `expired_at__gt=now`
    // predicate — strictly greater, null never expires.
    auth_token::validate_api_token(Some(&kernel_row), presented, now.timestamp())
        .map_err(|_| Denial::InvalidToken)?;
    // `api_token.last_used = now; save(update_fields=["last_used"])`.
    sqlx::query("UPDATE api_tokens SET last_used = now() WHERE token = $1")
        .bind(presented)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(user_id)
}

async fn authenticate_machine_token(
    pool: &sqlx::PgPool,
    presented: &str,
    secret_key: &[u8],
) -> Result<Uuid, Denial> {
    let token_hash = auth_token::hash_token(presented, secret_key);
    let row: Option<MachineTokenLookup> = sqlx::query_as(
        r#"SELECT mt.id, mt.user_id, mt.workspace_id, mt.revoked_at,
                  mt.dev_machine_id, dm.revoked_at AS dev_revoked_at
           FROM machine_token mt
           LEFT JOIN dev_machine dm ON dm.id = mt.dev_machine_id
           WHERE mt.token_hash = $1"#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::InvalidToken);
    };
    let kernel_row = auth_token::MachineTokenRow {
        token_hash: token_hash.clone(),
        revoked_at_unix: row.revoked_at.map(|dt| dt.timestamp()),
        dev_machine_revoked: row.dev_machine_id.is_some() && row.dev_revoked_at.is_some(),
    };
    auth_token::validate_machine_token_static(Some(&kernel_row), &token_hash)
        .map_err(|_| Denial::InvalidToken)?;
    // `is_workspace_member(user, workspace_id)`: a non-member is revoked
    // first, then denied — exactly like Python.
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspace_members
          WHERE workspace_id = $1 AND member_id = $2 AND is_active
          AND deleted_at IS NULL)",
    )
    .bind(row.workspace_id)
    .bind(row.user_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if !member {
        sqlx::query("UPDATE machine_token SET revoked_at = now() WHERE id = $1")
            .bind(row.id)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        return Err(Denial::InvalidToken);
    }
    sqlx::query("UPDATE machine_token SET last_used_at = now() WHERE id = $1")
        .bind(row.id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.user_id)
}

#[derive(Debug, sqlx::FromRow)]
struct MachineTokenLookup {
    id: Uuid,
    user_id: Uuid,
    workspace_id: Uuid,
    revoked_at: Option<DateTime<Utc>>,
    dev_machine_id: Option<Uuid>,
    dev_revoked_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Request-data parsing (JSON; empty/malformed proxy to Django)
// ---------------------------------------------------------------------------

/// Parse `request.data` for the JSON bodies the contract suites send.
///
/// A well-formed JSON object parses as-is. Anything else keeps Django's
/// exact bytes: an empty or malformed body is proxied to Django (DRF
/// `ParseError`), while a well-formed non-object body (list/scalar)
/// answers the generic 500 — the `AttributeError` the view's `.get`
/// raises on it (`asset.py` post/patch read `request.data.get(...)`
/// before anything else validates).
#[allow(clippy::result_large_err)]
fn parse_object(body: &Bytes) -> Result<Map<String, Value>, ProxyOr500> {
    if body.is_empty() {
        return Err(ProxyOr500::Proxy);
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(ProxyOr500::ServerError),
        Err(_) => Err(ProxyOr500::Proxy),
    }
}

enum ProxyOr500 {
    Proxy,
    ServerError,
}

/// Rebuild a request from its parts and proxy it to Django.
async fn proxy_through(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body))
        .expect("rebuild proxy request");
    *req.headers_mut() = headers;
    crate::edge::proxy(State(state), req).await
}

// ---------------------------------------------------------------------------
// Python-value kernels (`request.data.get(...)` semantics)
// ---------------------------------------------------------------------------

/// `str(value)` for the f-string renderings (`asset.py:150` asset key).
/// Scalars match CPython exactly (`True`/`False`/`None`, ints, float
/// shortest roundtrip); containers use CPython `repr` spelling (single
/// quotes) — `f"{...}"` renders containers via `str()`, which for
/// list/dict nests `repr()`.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!(
                    "{}: {}",
                    python_repr(&Value::String(k.clone())),
                    python_repr(v)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// CPython `repr` of one JSON scalar (single-quoted strings).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        _ => python_str(value),
    }
}

/// Outcome of the POST `size = int(request.data.get("size", LIMIT))`
/// conversion (`asset.py:119`): a parsed byte count, or the
/// `TypeError`/`ValueError` 500 the conversion raises for explicit
/// nulls and non-numeric input (BUG-size-500, ported as-is).
enum SizeOutcome {
    Size(i64),
    ConvertError,
}

/// `int()` over an already-parsed JSON value (`asset.py:119`).
/// Missing keys read the `FILE_SIZE_LIMIT` default; floats truncate
/// toward zero like CPython `int()`; numeric strings take an optional
/// sign plus ASCII digits (a `str` underscore or decimal point raises,
/// exactly like CPython).
fn python_int_size(value: Option<&Value>, default: i64) -> SizeOutcome {
    let Some(value) = value else {
        return SizeOutcome::Size(default);
    };
    match value {
        Value::Null => SizeOutcome::ConvertError,
        Value::Bool(flag) => SizeOutcome::Size(i64::from(*flag)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SizeOutcome::Size(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).map_or(SizeOutcome::ConvertError, SizeOutcome::Size)
            } else if let Some(f) = n.as_f64() {
                // CPython `int()` truncates; out-of-range floats raise.
                if f.is_finite() && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    #[allow(clippy::cast_possible_truncation)]
                    SizeOutcome::Size(f.trunc() as i64)
                } else {
                    SizeOutcome::ConvertError
                }
            } else {
                SizeOutcome::ConvertError
            }
        }
        Value::String(s) => {
            // CPython `int(str)`: surrounding whitespace, one sign, then
            // digits with single `_` separators; unbounded width (the
            // column clamps via `min()` later, so overflow saturates).
            let text = s.trim();
            let (negative, digits) = match text.strip_prefix('+') {
                Some(rest) => (false, rest),
                None => match text.strip_prefix('-') {
                    Some(rest) => (true, rest),
                    None => (false, text),
                },
            };
            let clean: String = digits.split('_').collect::<Vec<_>>().join("");
            let well_formed = !digits.is_empty()
                && !digits.starts_with('_')
                && !digits.ends_with('_')
                && !digits.contains("__")
                && !clean.is_empty()
                && clean.bytes().all(|b| b.is_ascii_digit());
            if !well_formed {
                return SizeOutcome::ConvertError;
            }
            match clean.parse::<i64>() {
                Ok(size) => SizeOutcome::Size(if negative { -size } else { size }),
                Err(_) if !negative => SizeOutcome::Size(i64::MAX),
                Err(_) => SizeOutcome::Size(i64::MIN),
            }
        }
        Value::Array(_) | Value::Object(_) => SizeOutcome::ConvertError,
    }
}

fn host_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Request scheme for MinIO-mode signing: `X-Forwarded-Proto` when the
/// proxy sets it, else `http` (Django's `request.scheme` default).
fn scheme_of(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_lowercase())
        .filter(|v| v == "http" || v == "https")
        .unwrap_or_else(|| "http".to_owned())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `file_assets` row with every column the user units read (the
/// row id is the lookup key the caller already holds).
struct AssetRow {
    entity_type: Option<String>,
    user_id: Option<Uuid>,
    storage_metadata: Option<Value>,
    attributes: Option<Value>,
}

fn asset_row(row: &sqlx::postgres::PgRow) -> Result<AssetRow, sqlx::Error> {
    Ok(AssetRow {
        entity_type: row.try_get("entity_type")?,
        user_id: row.try_get("user_id")?,
        storage_metadata: row.try_get("storage_metadata")?,
        attributes: row.try_get("attributes")?,
    })
}

/// `FileAsset.objects.get(id=asset_id, user_id=request.user.id)`
/// (`asset.py:210,237`, server twins `:366,394`) via
/// [`asset_queries::user_scoped_get_sql`]: manager scope plus both exact
/// predicates, deliberately **no** `is_deleted` filter (BUG-no-is-deleted).
/// Binds: `$1` asset id, `$2` acting-user id.
async fn user_scoped_get(
    pool: &sqlx::PgPool,
    asset_id: Uuid,
    user_id: Uuid,
) -> Result<Option<AssetRow>, sqlx::Error> {
    let row = sqlx::query(&asset_queries::user_scoped_get_sql())
        .bind(asset_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    row.map(|row| asset_row(&row)).transpose()
}

/// `FileAsset.asset_url` (`db/models/asset.py:80-87`) for the POST 200.
/// The entity guard admits only `USER_AVATAR` / `USER_COVER`, both of
/// which take the static branch — the only reachable rendering here.
fn user_asset_url(asset_id: &Uuid) -> Value {
    Value::String(format!("/api/assets/v2/static/{asset_id}/"))
}

// ---------------------------------------------------------------------------
// Metadata publisher
// ---------------------------------------------------------------------------

/// `get_asset_object_metadata.delay(asset_id=str(asset_id))`
/// (`asset.py:215`, server twin `:371`, kwarg form) through the
/// Postgres-backed queue. Best-effort: a failed enqueue warns and the
/// response stands (the merged D-31 precedent).
async fn publish_metadata(pool: &sqlx::PgPool, asset_id: &Uuid) {
    let job = pidash_jobs::v1_assets::tasks::metadata_job(&asset_id.to_string());
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata", "task enqueue failed; response stands");
    }
}

// ---------------------------------------------------------------------------
// Helpers (`asset.py:51-73`, server twins `:249-271` — one implementation)
// ---------------------------------------------------------------------------

/// `asset_delete` (`asset.py:51-58`): `filter(id).first()` — manager scope
/// only, no `is_deleted` filter (BUG-no-is-deleted) — then
/// `save(update_fields=["is_deleted", "deleted_at"])`, which writes only
/// those two columns. Unknown ids return silently (callers ignore the
/// return; the wire stays 204). The views never call this helper — it is
/// ported because the issue names it, and stays uncalled exactly like
/// Python.
pub async fn asset_delete(pool: &sqlx::PgPool, asset_id: Uuid) -> Result<(), sqlx::Error> {
    let row = sqlx::query(&asset_queries::soft_delete_select_sql())
        .bind(asset_id)
        .fetch_optional(pool)
        .await?;
    if row.is_none() {
        return Ok(());
    }
    sqlx::query(&asset_queries::soft_delete_update_sql())
        .bind(true)
        .bind(Utc::now())
        .bind(asset_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Outcome of [`entity_asset_delete`]: cleared, no-op, or the
/// `User.DoesNotExist` 404 (dangling `user_id`, or a null `user_id` —
/// `User.objects.get(id=None)` raises `DoesNotExist`, not `ValueError`).
enum EntityDeleteOutcome {
    Done,
    UserMissing,
}

/// `entity_asset_delete` (`asset.py:60-73`): `USER_AVATAR` clears
/// `avatar_asset_id`, `USER_COVER` clears `cover_image_asset_id`, anything
/// else returns without touching the profile. The clear is a full-row
/// `user.save()` — every non-pk column in `_meta` order via
/// [`asset_queries::entity_user_save_sql`] — after the `User.save()`
/// value shaping (`db/models/user.py:169-189`): email lower+strip (a null
/// email raises `AttributeError` → generic 500), token rotation iff
/// `token_updated_at` is set, the falsy-`display_name` local-part default
/// (whose `else` is dead — `len(split)` is always ≥ 1), and
/// `is_staff` forced for superusers.
async fn entity_asset_delete(
    pool: &sqlx::PgPool,
    entity_type: Option<&str>,
    user_id: Option<Uuid>,
) -> Result<EntityDeleteOutcome, sqlx::Error> {
    use asset_queries::{entity_clear_target, EntityClearTarget as Target};
    let target = entity_clear_target(entity_type);
    if target == Target::NoOp {
        return Ok(EntityDeleteOutcome::Done);
    }
    let Some(user_id) = user_id else {
        // `User.objects.get(id=None)` → `DoesNotExist` → 404, no writes.
        return Ok(EntityDeleteOutcome::UserMissing);
    };
    let row = sqlx::query(&asset_queries::entity_user_select_sql())
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(EntityDeleteOutcome::UserMissing);
    };
    let user = UserRow::decode(&row)?;
    let shaped = match user.shaped(target) {
        Some(shaped) => shaped,
        // Null email: `self.email.lower()` raises `AttributeError` →
        // `handle_exception` generic 500, before any write. There is no
        // `sqlx::Error` spelling for it, so the caller maps the marker.
        None => return Err(null_email_error()),
    };
    shaped.save(pool).await?;
    Ok(EntityDeleteOutcome::Done)
}

/// Marker error for the null-email `AttributeError` path (see
/// [`entity_asset_delete`]). sqlx has no ad-hoc error variant, so the
/// caller matches the message — it never reaches the wire.
fn null_email_error() -> sqlx::Error {
    sqlx::Error::Protocol("entity_asset_delete: user.email is None".to_owned())
}

fn is_null_email_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Protocol(message) if message.starts_with("entity_asset_delete:"))
}

/// One `users` row in [`asset_queries::USER_COLUMNS`] order, for the
/// full-row `user.save()` rebind. Nullable decodings follow
/// `db/models/user.py:56-133` (`null=True` only on `last_login`,
/// `mobile_number`, `email`, `cover_image`, `last_active`,
/// `last_login_time`, `last_logout_time`, `token_updated_at`, `bot_type`,
/// `masked_at` — everything else is `NOT NULL`).
struct UserRow {
    password: String,
    last_login: Option<DateTime<Utc>>,
    id: Uuid,
    username: String,
    mobile_number: Option<String>,
    email: Option<String>,
    display_name: String,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_asset_id: Option<Uuid>,
    cover_image: Option<String>,
    cover_image_asset_id: Option<Uuid>,
    date_joined: DateTime<Utc>,
    created_at: DateTime<Utc>,
    // `updated_at` is decoded nowhere: the full-row `save()` always
    // overwrites it with `now()` (`auto_now`).
    last_location: String,
    created_location: String,
    is_superuser: bool,
    is_managed: bool,
    is_password_expired: bool,
    is_active: bool,
    is_staff: bool,
    is_email_verified: bool,
    is_password_autoset: bool,
    is_password_reset_required: bool,
    token: String,
    last_active: Option<DateTime<Utc>>,
    last_login_time: Option<DateTime<Utc>>,
    last_logout_time: Option<DateTime<Utc>>,
    last_login_ip: String,
    last_logout_ip: String,
    last_login_medium: String,
    last_login_uagent: String,
    token_updated_at: Option<DateTime<Utc>>,
    is_bot: bool,
    bot_type: Option<String>,
    user_timezone: String,
    is_email_valid: bool,
    masked_at: Option<DateTime<Utc>>,
}

impl UserRow {
    fn decode(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            password: row.try_get("password")?,
            last_login: row.try_get("last_login")?,
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            mobile_number: row.try_get("mobile_number")?,
            email: row.try_get("email")?,
            display_name: row.try_get("display_name")?,
            first_name: row.try_get("first_name")?,
            last_name: row.try_get("last_name")?,
            avatar: row.try_get("avatar")?,
            avatar_asset_id: row.try_get("avatar_asset_id")?,
            cover_image: row.try_get("cover_image")?,
            cover_image_asset_id: row.try_get("cover_image_asset_id")?,
            date_joined: row.try_get("date_joined")?,
            created_at: row.try_get("created_at")?,
            last_location: row.try_get("last_location")?,
            created_location: row.try_get("created_location")?,
            is_superuser: row.try_get("is_superuser")?,
            is_managed: row.try_get("is_managed")?,
            is_password_expired: row.try_get("is_password_expired")?,
            is_active: row.try_get("is_active")?,
            is_staff: row.try_get("is_staff")?,
            is_email_verified: row.try_get("is_email_verified")?,
            is_password_autoset: row.try_get("is_password_autoset")?,
            is_password_reset_required: row.try_get("is_password_reset_required")?,
            token: row.try_get("token")?,
            last_active: row.try_get("last_active")?,
            last_login_time: row.try_get("last_login_time")?,
            last_logout_time: row.try_get("last_logout_time")?,
            last_login_ip: row.try_get("last_login_ip")?,
            last_logout_ip: row.try_get("last_logout_ip")?,
            last_login_medium: row.try_get("last_login_medium")?,
            last_login_uagent: row.try_get("last_login_uagent")?,
            token_updated_at: row.try_get("token_updated_at")?,
            is_bot: row.try_get("is_bot")?,
            bot_type: row.try_get("bot_type")?,
            user_timezone: row.try_get("user_timezone")?,
            is_email_valid: row.try_get("is_email_valid")?,
            masked_at: row.try_get("masked_at")?,
        })
    }

    /// Apply the FK clear plus the `User.save()` shaping, returning `None`
    /// when the email is null (the `AttributeError` → 500 path).
    fn shaped(mut self, target: asset_queries::EntityClearTarget) -> Option<ShapedUserRow> {
        use asset_queries::EntityClearTarget as Target;
        match target {
            Target::AvatarAssetId => self.avatar_asset_id = None,
            Target::CoverImageAssetId => self.cover_image_asset_id = None,
            Target::NoOp => {}
        }
        let email = self.email.as_deref()?.to_lowercase();
        let email = email.trim().to_owned();
        let now = Utc::now();
        let (token, token_updated_at) = match self.token_updated_at {
            Some(_) => (
                format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
                Some(now),
            ),
            None => (std::mem::take(&mut self.token), None),
        };
        let display_name = if self.display_name.is_empty() {
            // `email.split("@")[0] if len(...) else random` — `len` is
            // always ≥ 1, so the random branch is dead.
            email.split('@').next().unwrap_or_default().to_owned()
        } else {
            std::mem::take(&mut self.display_name)
        };
        let is_staff = self.is_staff || self.is_superuser;
        Some(ShapedUserRow {
            row: self,
            email,
            token,
            token_updated_at,
            display_name,
            is_staff,
            updated_at: now,
        })
    }
}

/// A [`UserRow`] with the clear + `User.save()` shaping applied.
struct ShapedUserRow {
    row: UserRow,
    email: String,
    token: String,
    token_updated_at: Option<DateTime<Utc>>,
    display_name: String,
    is_staff: bool,
    updated_at: DateTime<Utc>,
}

impl ShapedUserRow {
    /// The full-row `user.save()`: every non-pk column in `_meta` order
    /// (`$1`–`$39`), pk in `WHERE` (`$40`) — the
    /// [`asset_queries::entity_user_save_sql`] bind contract.
    async fn save(&self, pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
        let row = &self.row;
        sqlx::query(&asset_queries::entity_user_save_sql())
            .bind(&row.password)
            .bind(row.last_login)
            .bind(&row.username)
            .bind(&row.mobile_number)
            .bind(&self.email)
            .bind(&self.display_name)
            .bind(&row.first_name)
            .bind(&row.last_name)
            .bind(&row.avatar)
            .bind(row.avatar_asset_id)
            .bind(&row.cover_image)
            .bind(row.cover_image_asset_id)
            .bind(row.date_joined)
            .bind(row.created_at)
            .bind(self.updated_at)
            .bind(&row.last_location)
            .bind(&row.created_location)
            .bind(row.is_superuser)
            .bind(row.is_managed)
            .bind(row.is_password_expired)
            .bind(row.is_active)
            .bind(self.is_staff)
            .bind(row.is_email_verified)
            .bind(row.is_password_autoset)
            .bind(row.is_password_reset_required)
            .bind(&self.token)
            .bind(row.last_active)
            .bind(row.last_login_time)
            .bind(row.last_logout_time)
            .bind(&row.last_login_ip)
            .bind(&row.last_logout_ip)
            .bind(&row.last_login_medium)
            .bind(&row.last_login_uagent)
            .bind(self.token_updated_at)
            .bind(row.is_bot)
            .bind(&row.bot_type)
            .bind(&row.user_timezone)
            .bind(row.is_email_valid)
            .bind(row.masked_at)
            .bind(row.id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// POST `assets/user-assets/` (`asset.py:110-174`, server twin `:282-347`)
// ---------------------------------------------------------------------------

/// Mint a user asset row and answer the presigned upload POST.
async fn user_post(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_post(state, method, uri, headers, body, false).await
}

/// Shared POST core, parameterized by the credential flag: `is_server`
/// selects the `S3Storage(is_server=True)` endpoint branch
/// (`asset.py:336` vs `:163`) — the only line the server twin changes.
#[allow(clippy::too_many_lines)]
pub(crate) async fn handle_post(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    is_server: bool,
) -> Response {
    let Some(pool) = pool(&state) else {
        return server_error("no database pool");
    };
    let actor = match authenticate(pool, &headers, state.settings().secret_key.as_bytes()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let data = match parse_object(&body) {
        Ok(data) => data,
        Err(ProxyOr500::Proxy) => {
            return proxy_through(state, method, uri, headers, body).await;
        }
        Err(ProxyOr500::ServerError) => return server_error("request.data has no .get"),
    };
    // `size = int(request.data.get("size", FILE_SIZE_LIMIT))` runs BEFORE
    // the guards (BUG-size-500): null/non-numeric sizes 500.
    let size = match python_int_size(data.get("size"), state.settings().file_size_limit) {
        SizeOutcome::Size(size) => size,
        SizeOutcome::ConvertError => return server_error("int(size) raised"),
    };
    let size_limit = size.min(state.settings().file_size_limit);
    // `if not entity_type or entity_type not in [...]` → 400. Missing
    // reads the `False` default (falsy → 400); only the two exact
    // strings pass — every other JSON type 400s.
    let entity_type: &str = match data.get("entity_type") {
        Some(Value::String(entity)) if USER_ENTITY_TYPES.contains(&entity.as_str()) => entity,
        _ => return Denial::InvalidEntity.into_response(),
    };
    // `type = request.data.get("type", "image/jpeg")` — the default
    // applies only when the key is MISSING (explicit null → 400).
    let mime: &str = match data.get("type") {
        None => "image/jpeg",
        Some(Value::String(mime)) if USER_MIME_TYPES.contains(&mime.as_str()) => mime,
        _ => return Denial::InvalidType.into_response(),
    };
    // `asset_key = f"{uuid4hex}-{name}"` — `name` has no required check
    // (BUG-none-key): missing/null renders `None`.
    let name_value = data.get("name").cloned().unwrap_or(Value::Null);
    let asset_key = format!("{}-{}", Uuid::new_v4().simple(), python_str(&name_value));
    let attributes = serde_json::json!({
        "name": name_value,
        "type": mime,
        "size": size_limit,
    });
    let asset_id = Uuid::new_v4();
    let now = Utc::now();
    // `FileAsset.objects.create(...)` (`asset.py:153-160`): every column
    // in `_meta` order with the Django defaults inlined (`uuid4` pk,
    // `auto_now_add` stamps, `false` booleans, `'{}'` JSON dicts).
    let inserted = sqlx::query(
        r#"INSERT INTO "file_assets" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "attributes", "asset", "user_id", "workspace_id", "draft_issue_id", "project_id", "issue_id", "comment_id", "page_id", "entity_type", "entity_identifier", "is_deleted", "is_archived", "external_id", "external_source", "size", "is_uploaded", "storage_metadata") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, NULL, NULL, NULL, NULL, NULL, NULL, $8, NULL, false, false, NULL, NULL, $9, false, '{}')"#,
    )
    .bind(asset_id)
    .bind(now)
    .bind(now)
    .bind(actor.id)
    .bind(sqlx::types::Json(attributes))
    .bind(&asset_key)
    .bind(actor.id)
    .bind(entity_type)
    .bind(size_limit as f64)
    .execute(pool)
    .await;
    if let Err(error) = inserted {
        return write_error(error);
    }
    let Some(host) = host_of(&headers) else {
        return server_error("missing Host");
    };
    let storage = &state.settings().storage;
    let upload_data = presigned_post(
        storage,
        &scheme_of(&headers),
        &host,
        &asset_key,
        mime,
        size_limit,
        &now,
        is_server,
    );
    json_body(
        StatusCode::OK,
        serde_json::json!({
            "upload_data": upload_data,
            "asset_id": asset_id.to_string(),
            "asset_url": user_asset_url(&asset_id),
        }),
    )
}

// ---------------------------------------------------------------------------
// PATCH `assets/user-assets/<uuid:asset_id>/` (`asset.py:203-220`)
// ---------------------------------------------------------------------------

/// Mark the asset uploaded and fire metadata extraction.
async fn user_patch(
    State(state): State<AppState>,
    Path(asset_raw): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_patch(state, method, uri, headers, &asset_raw, body).await
}

/// Shared PATCH core (the server twin at `asset.py:359-376` is identical —
/// no credential flag involved).
pub(crate) async fn handle_patch(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    asset_raw: &str,
    body: Bytes,
) -> Response {
    let asset_id: Uuid = match asset_raw.parse() {
        Ok(id) => id,
        Err(_) => return raw(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY),
    };
    let Some(pool) = pool(&state) else {
        return server_error("no database pool");
    };
    let actor = match authenticate(pool, &headers, state.settings().secret_key.as_bytes()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // The lookup runs BEFORE `request.data` is touched (DRF parses
    // lazily): a missing asset 404s even with a malformed body.
    // `FileAsset.objects.get(id, user_id)` → 404 mapping.
    let asset = match user_scoped_get(pool, asset_id, actor.id).await {
        Ok(Some(asset)) => asset,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return server_error(error),
    };
    // `request.data` parses here — after the lookup, like Python.
    let data = match parse_object(&body) {
        Ok(data) => data,
        Err(ProxyOr500::Proxy) => {
            return proxy_through(state, method, uri, headers, body).await;
        }
        Err(ProxyOr500::ServerError) => return server_error("request.data has no .get"),
    };
    // `if not asset.storage_metadata: delay` — before the save, exactly
    // like Python (`asset.py:214-215`).
    let storage_metadata = asset.storage_metadata.unwrap_or(Value::Null);
    if pidash_jobs::v1_assets::tasks::storage_metadata_missing(&storage_metadata) {
        publish_metadata(pool, &asset_id).await;
    }
    // `asset.attributes = request.data.get("attributes", current)` —
    // any value replaces, including explicit null (which violates the
    // `NOT NULL` column → 400, exactly like Django's `IntegrityError`).
    let attributes = data
        .get("attributes")
        .cloned()
        .unwrap_or_else(|| asset.attributes.unwrap_or(Value::Null));
    let attributes_bind: Option<sqlx::types::Json<Value>> = match attributes {
        Value::Null => None,
        value => Some(sqlx::types::Json(value)),
    };
    // `asset.save(update_fields=["is_uploaded", "attributes"])` — only
    // those two columns, no `updated_at` bump (Django 4.2 `_save_table`
    // filters `non_pks` by `update_fields`).
    if let Err(error) = sqlx::query(
        r#"UPDATE "file_assets" SET "is_uploaded" = $1, "attributes" = $2 WHERE "id" = $3"#,
    )
    .bind(true)
    .bind(attributes_bind)
    .bind(asset_id)
    .execute(pool)
    .await
    {
        return write_error(error);
    }
    no_content()
}

// ---------------------------------------------------------------------------
// DELETE `assets/user-assets/<uuid:asset_id>/` (`asset.py:231-243`)
// ---------------------------------------------------------------------------

/// Soft-delete the asset after clearing its profile reference.
async fn user_delete(
    State(state): State<AppState>,
    Path(asset_raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    handle_delete(state, headers, &asset_raw).await
}

/// Shared DELETE core (the server twin at `asset.py:387-400` is identical —
/// no credential flag involved). The body is never parsed — Python never
/// touches `request.data` here.
pub(crate) async fn handle_delete(
    state: AppState,
    headers: HeaderMap,
    asset_raw: &str,
) -> Response {
    let asset_id: Uuid = match asset_raw.parse() {
        Ok(id) => id,
        Err(_) => return raw(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY),
    };
    let Some(pool) = pool(&state) else {
        return server_error("no database pool");
    };
    let actor = match authenticate(pool, &headers, state.settings().secret_key.as_bytes()).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // `FileAsset.objects.get(id, user_id)` → 404 mapping. The first
    // DELETE stamps `deleted_at`, hiding the row from the manager scope,
    // so a second DELETE 404s.
    let asset = match user_scoped_get(pool, asset_id, actor.id).await {
        Ok(Some(asset)) => asset,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(error) => return server_error(error),
    };
    // `entity_asset_delete` runs BEFORE the asset save (`asset.py:241-242`):
    // a user miss 404s and a null email 500s with no writes at all.
    match entity_asset_delete(pool, asset.entity_type.as_deref(), asset.user_id).await {
        Ok(EntityDeleteOutcome::Done) => {}
        Ok(EntityDeleteOutcome::UserMissing) => return Denial::NotFound.into_response(),
        Err(error) if is_null_email_error(&error) => {
            return server_error("user.email.lower() raised AttributeError");
        }
        Err(error) => {
            return write_error(error);
        }
    }
    // `asset.save(update_fields=["is_deleted", "deleted_at"])` — only
    // those two columns, no `updated_at` bump.
    if let Err(error) = sqlx::query(
        r#"UPDATE "file_assets" SET "is_deleted" = $1, "deleted_at" = $2 WHERE "id" = $3"#,
    )
    .bind(true)
    .bind(Utc::now())
    .bind(asset_id)
    .execute(pool)
    .await
    {
        return write_error(error);
    }
    no_content()
}

// ---------------------------------------------------------------------------
// SigV4 presigning (offline; mirrors `S3Storage` + botocore)
// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// SigV4 signing key: `kDate/kRegion/kService/kSigning`
/// (`storage.py` always signs `s3`).
fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    hmac_sha256(&k_service, b"aws4_request")
}

fn credential_scope(date: &str, region: &str) -> String {
    format!("{date}/{region}/s3/aws4_request")
}

/// Endpoint for signing, mirroring `S3Storage.__init__` with a request:
/// MinIO user mode signs `{scheme}://{Host}`; MinIO server mode
/// (`is_server=True`) signs the configured endpoint URL; an explicit
/// endpoint URL signs against it; otherwise the virtual-hosted AWS
/// default. Credentials never differ — only the endpoint does.
fn endpoint_parts(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    is_server: bool,
) -> (String, String) {
    if storage.use_minio && !is_server {
        (format!("{scheme}://{host}"), host.to_owned())
    } else if let Some(endpoint) = storage.endpoint_url.as_deref().filter(|e| !e.is_empty()) {
        let endpoint = endpoint.trim_end_matches('/');
        let signed_host = endpoint
            .rsplit("://")
            .next()
            .unwrap_or(endpoint)
            .split('/')
            .next()
            .unwrap_or(endpoint);
        (endpoint.to_owned(), signed_host.to_owned())
    } else {
        let region = storage.region.as_str();
        let base = if region.is_empty() {
            "s3.amazonaws.com".to_owned()
        } else {
            format!("s3.{region}.amazonaws.com")
        };
        (
            format!("https://{}.{}", storage.bucket_name, base),
            format!("{}.{}", storage.bucket_name, base),
        )
    }
}

/// `generate_presigned_post(object_name, file_type, file_size)`
/// (`storage.py`): `{"url","fields"}` with botocore's field order and
/// the `storage.py` condition order. Python's `ClientError` → `None`
/// branch is unreachable offline (signing is pure arithmetic), so the
/// 200 always carries the object.
#[allow(clippy::too_many_arguments)]
fn presigned_post(
    storage: &pidash_db::config::StorageSettings,
    scheme: &str,
    host: &str,
    object_name: &str,
    file_type: &str,
    file_size: i64,
    now: &DateTime<Utc>,
    is_server: bool,
) -> Value {
    let region = storage.region.as_str();
    let (endpoint, _) = endpoint_parts(storage, scheme, host, is_server);
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = credential_scope(&date, region);
    let expiration = (*now + chrono::Duration::seconds(storage.signed_url_expiration_secs))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let credential = format!("{}/{}", storage.access_key_id, scope);
    // Condition order mirrors `storage.py`: bucket, content-length
    // range, Content-Type, key — then the three signer conditions the
    // client-level `generate_presigned_post` appends
    // (`botocore/signers.py`). Serialized with CPython `json.dumps`
    // default separators so the policy bytes match botocore's.
    let conditions = format!(
        "[{{\"bucket\": {}}}, [\"content-length-range\", 1, {}], {{\"Content-Type\": {}}}, {{\"key\": {}}}, {{\"bucket\": {}}}, {{\"key\": {}}}, {{\"x-amz-algorithm\": \"AWS4-HMAC-SHA256\"}}, {{\"x-amz-credential\": {}}}, {{\"x-amz-date\": {}}}]",
        py_json_string(&storage.bucket_name),
        file_size,
        py_json_string(file_type),
        py_json_string(object_name),
        py_json_string(&storage.bucket_name),
        py_json_string(object_name),
        py_json_string(&credential),
        py_json_string(&amz_date),
    );
    let policy_json = format!(
        "{{\"expiration\": {}, \"conditions\": {conditions}}}",
        py_json_string(&expiration),
    );
    let policy_b64 = base64_encode(policy_json.as_bytes());
    let signature = hex(&hmac_sha256(
        &signing_key(&storage.secret_access_key, &date, region),
        policy_b64.as_bytes(),
    ));
    let path_style = (storage.use_minio && !is_server)
        || storage
            .endpoint_url
            .as_deref()
            .is_some_and(|e| !e.is_empty());
    let url = if path_style {
        format!("{endpoint}/{}", storage.bucket_name)
    } else {
        format!("{endpoint}/")
    };
    let mut fields = Map::with_capacity(7);
    fields.insert(
        "Content-Type".to_owned(),
        Value::String(file_type.to_owned()),
    );
    fields.insert("key".to_owned(), Value::String(object_name.to_owned()));
    fields.insert(
        "x-amz-algorithm".to_owned(),
        Value::String("AWS4-HMAC-SHA256".to_owned()),
    );
    fields.insert("x-amz-credential".to_owned(), Value::String(credential));
    fields.insert("x-amz-date".to_owned(), Value::String(amz_date));
    fields.insert("policy".to_owned(), Value::String(policy_b64));
    fields.insert("x-amz-signature".to_owned(), Value::String(signature));
    serde_json::json!({"url": url, "fields": fields})
}

/// CPython `json.dumps` string encoding (`ensure_ascii`): `"` and
/// `\` escaped, C0 controls short/`\u00XX`, everything else non-ASCII
/// as `\uXXXX` (surrogate pairs past the BMP).
fn py_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// Standard base64 with padding (policy documents).
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let mut n: u32 = 0;
        for (i, b) in chunk.iter().enumerate() {
            n |= (*b as u32) << (16 - 8 * i);
        }
        let pad = 3 - chunk.len();
        for i in 0..4 - pad {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
        }
        for _ in 0..pad {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::config::StorageSettings;
    use pidash_db::v1_assets::model::file_asset;

    fn test_storage() -> StorageSettings {
        StorageSettings {
            use_minio: true,
            access_key_id: "access-key".to_owned(),
            secret_access_key: "secret-key".to_owned(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: Some("https://minio.internal:9000".to_owned()),
            signed_url_expiration_secs: 3600,
        }
    }

    fn test_now() -> DateTime<Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("fixed test time")
    }

    // fx-h-asset-user `error_bodies`, byte for byte.
    #[test]
    fn error_bodies_match_fixture() {
        assert_eq!(
            INVALID_ENTITY_BODY,
            r#"{"error":"Invalid entity type.","status":false}"#
        );
        assert_eq!(
            INVALID_TYPE_BODY,
            r#"{"error":"Invalid file type. Only JPEG and PNG files are allowed.","status":false}"#
        );
        assert_eq!(
            NOT_FOUND_BODY,
            r#"{"error":"The requested resource does not exist."}"#
        );
        assert_eq!(
            PAYLOAD_NOT_VALID_BODY,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            INVALID_TOKEN_BODY,
            r#"{"detail":"Given API token is not valid"}"#
        );
        assert_eq!(PAGE_NOT_FOUND_BODY, r#"{"error":"Page not found."}"#);
    }

    // Response key order: DRF renders declaration order.
    #[test]
    fn response_key_order_matches_python_dicts() {
        let post = serde_json::json!({
            "upload_data": {},
            "asset_id": "id",
            "asset_url": "url",
        });
        assert_eq!(
            serde_json::to_string(&post).expect("json"),
            r#"{"upload_data":{},"asset_id":"id","asset_url":"url"}"#
        );
        // `status` renders JSON false for Python `False`.
        let entity: Value = serde_json::from_str(INVALID_ENTITY_BODY).expect("fixture body");
        assert_eq!(entity["status"], Value::Bool(false));
    }

    // The POST guards: exactly the two entity strings and the 5-mime
    // allowlist (`asset.py:126,133-139`) — `image/jpg` passes despite the
    // message claiming JPEG-and-PNG-only (BUG-mime-message).
    #[test]
    fn guard_allowlists_match_python() {
        assert_eq!(USER_ENTITY_TYPES, &["USER_AVATAR", "USER_COVER"]);
        assert_eq!(
            USER_MIME_TYPES,
            &[
                "image/jpeg",
                "image/png",
                "image/webp",
                "image/jpg",
                "image/gif"
            ]
        );
        for mime in ["application/pdf", "", "IMAGE/JPEG", "image/svg+xml"] {
            assert!(!USER_MIME_TYPES.contains(&mime), "{mime}");
        }
    }

    // `int(request.data.get("size", LIMIT))`: missing reads the default;
    // null and non-numeric raise (BUG-size-500 → ConvertError → 500).
    #[test]
    fn size_conversion_runs_before_guards() {
        let limit = 5_242_880_i64;
        assert!(matches!(
            python_int_size(None, limit),
            SizeOutcome::Size(5_242_880)
        ));
        assert!(matches!(
            python_int_size(Some(&Value::Null), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("abc")), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("12.5")), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!([1])), limit),
            SizeOutcome::ConvertError
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!("1024")), limit),
            SizeOutcome::Size(1024)
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!(12.9)), limit),
            SizeOutcome::Size(12)
        ));
        assert!(matches!(
            python_int_size(Some(&serde_json::json!(true)), limit),
            SizeOutcome::Size(1)
        ));
        // `size_limit = min(size, FILE_SIZE_LIMIT)`; only `min`, no floor.
        assert_eq!(1_000_i64.min(limit), 1_000);
        assert_eq!(99_999_999_i64.min(limit), limit);
        assert_eq!((-5_i64).min(limit), -5);
    }

    // BUG-none-key: a missing/null name renders `None` in the key
    // (`asset.py:150`); scalars match CPython `str()`.
    #[test]
    fn asset_key_renders_none_name() {
        assert_eq!(python_str(&Value::Null), "None");
        let key = format!("{}-{}", "hex", python_str(&Value::Null));
        assert_eq!(key, "hex-None");
        assert_eq!(python_str(&serde_json::json!("profile.jpg")), "profile.jpg");
        assert_eq!(python_str(&serde_json::json!(true)), "True");
        assert_eq!(python_str(&serde_json::json!(7)), "7");
    }

    // `FileAsset.asset_url` static branch (`db/models/asset.py:81-87`):
    // both guarded entity types render it.
    #[test]
    fn asset_url_is_static_branch() {
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").expect("uuid");
        assert_eq!(
            user_asset_url(&id),
            serde_json::json!("/api/assets/v2/static/123e4567-e89b-12d3-a456-426614174000/")
        );
    }

    // The metadata guard + wire shape come from the merged tasks layer.
    #[test]
    fn metadata_guard_and_wire_match_fixture() {
        use pidash_jobs::v1_assets::tasks::{metadata_job, storage_metadata_missing};
        assert!(storage_metadata_missing(&Value::Null));
        assert!(storage_metadata_missing(&serde_json::json!({})));
        assert!(storage_metadata_missing(&serde_json::json!("")));
        assert!(!storage_metadata_missing(&serde_json::json!({"a": 1})));
        let job = metadata_job("123e4567-e89b-12d3-a456-426614174000");
        assert_eq!(
            job.task,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        assert_eq!(
            job.kwargs,
            serde_json::json!({"asset_id": "123e4567-e89b-12d3-a456-426614174000"})
        );
    }

    // The user-scoped lookup carries the manager scope but no
    // `is_deleted` predicate (BUG-no-is-deleted); pin the bind contract.
    #[test]
    fn user_lookup_has_no_soft_delete_filter() {
        let sql = asset_queries::user_scoped_get_sql();
        assert!(sql.contains("\"file_assets\".\"id\" = ($1)"), "{sql}");
        assert!(sql.contains("\"file_assets\".\"user_id\" = ($2)"), "{sql}");
        assert!(
            sql.contains("\"file_assets\".\"deleted_at\" IS NULL"),
            "{sql}"
        );
        assert!(!sql.contains("NOT \"file_assets\".\"is_deleted\""), "{sql}");
        assert!(!sql.contains("\"is_deleted\" ="), "{sql}");
    }

    // `entity_clear_target` branches (`asset.py:60-73`): avatar clears
    // the avatar FK, cover clears the cover FK, anything else no-ops.
    #[test]
    fn entity_clear_branches_like_python() {
        use asset_queries::{entity_clear_target, EntityClearTarget as Target};
        assert_eq!(
            entity_clear_target(Some("USER_AVATAR")),
            Target::AvatarAssetId
        );
        assert_eq!(
            entity_clear_target(Some("USER_COVER")),
            Target::CoverImageAssetId
        );
        for other in [
            None,
            Some("ISSUE_ATTACHMENT"),
            Some(""),
            Some("USER_AVATAR "),
        ] {
            assert_eq!(entity_clear_target(other), Target::NoOp, "{other:?}");
        }
    }

    // The full-row user save binds all 39 non-pk columns plus the pk
    // (`$1`–`$40`); the soft-delete helper binds id + two fields.
    #[test]
    fn helper_builders_carry_their_binds() {
        let save = asset_queries::entity_user_save_sql();
        assert!(
            save.contains("$1") && save.contains("$39") && save.contains("$40"),
            "{save}"
        );
        assert_eq!(asset_queries::USER_COLUMNS.len(), 40);
        let select = asset_queries::soft_delete_select_sql();
        assert!(select.contains("$1"), "{select}");
        assert!(select.contains("LIMIT 1"), "{select}");
        let update = asset_queries::soft_delete_update_sql();
        assert!(
            update.contains("SET \"is_deleted\" = $1, \"deleted_at\" = $2"),
            "{update}"
        );
        assert!(update.contains("($3)"), "{update}");
    }

    #[test]
    fn model_defaults_match_create_row() {
        // The INSERT writes these Django defaults explicitly.
        let col = |name: &str| {
            file_asset::COLUMNS
                .iter()
                .position(|c| *c == name)
                .expect("col")
        };
        assert_eq!(file_asset::DEFAULTS[col("is_deleted")], Some("false"));
        assert_eq!(file_asset::DEFAULTS[col("is_archived")], Some("false"));
        assert_eq!(file_asset::DEFAULTS[col("is_uploaded")], Some("false"));
        assert_eq!(file_asset::DEFAULTS[col("size")], Some("0"));
        assert_eq!(file_asset::DEFAULTS[col("attributes")], Some("dict"));
        assert_eq!(file_asset::DEFAULTS[col("storage_metadata")], Some("dict"));
        for entity in USER_ENTITY_TYPES {
            assert!(file_asset::ENTITY_TYPES.contains(entity), "{entity}");
        }
    }

    // SigV4 POST smoke: deterministic per instant, botocore field order,
    // MinIO user mode signed against the request host.
    #[test]
    fn presigned_post_shape_is_stable() {
        let storage = test_storage();
        let now = test_now();
        let first = presigned_post(
            &storage,
            "http",
            "example.test",
            "hex-profile.jpg",
            "image/jpeg",
            100,
            &now,
            false,
        );
        let second = presigned_post(
            &storage,
            "http",
            "example.test",
            "hex-profile.jpg",
            "image/jpeg",
            100,
            &now,
            false,
        );
        assert_eq!(first, second);
        assert_eq!(
            first["url"],
            serde_json::json!("http://example.test/uploads")
        );
        let fields = first["fields"].as_object().expect("fields object");
        let order: Vec<&str> = fields.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            [
                "Content-Type",
                "key",
                "x-amz-algorithm",
                "x-amz-credential",
                "x-amz-date",
                "policy",
                "x-amz-signature"
            ]
        );
        assert_eq!(fields["Content-Type"], serde_json::json!("image/jpeg"));
        assert_eq!(fields["key"], serde_json::json!("hex-profile.jpg"));
    }

    // `is_server` selects the endpoint branch only: server mode signs the
    // configured endpoint URL (the contract `https://` shape) while user
    // mode signs the request host; envelopes stay identical.
    #[test]
    fn server_mode_signs_endpoint_url() {
        let storage = test_storage();
        let now = test_now();
        let (user_endpoint, _) = endpoint_parts(&storage, "http", "example.test", false);
        let (server_endpoint, _) = endpoint_parts(&storage, "http", "example.test", true);
        assert_eq!(user_endpoint, "http://example.test");
        assert_eq!(server_endpoint, "https://minio.internal:9000");
        let user_post = presigned_post(
            &storage,
            "http",
            "example.test",
            "hex-a.jpg",
            "image/jpeg",
            100,
            &now,
            false,
        );
        let server_post = presigned_post(
            &storage,
            "http",
            "example.test",
            "hex-a.jpg",
            "image/jpeg",
            100,
            &now,
            true,
        );
        assert_eq!(
            user_post["url"],
            serde_json::json!("http://example.test/uploads")
        );
        assert_eq!(
            server_post["url"],
            serde_json::json!("https://minio.internal:9000/uploads")
        );
        // Same envelope keys, same key echo — only the signing differs.
        assert_eq!(
            user_post["fields"]
                .as_object()
                .expect("fields")
                .keys()
                .collect::<Vec<_>>(),
            server_post["fields"]
                .as_object()
                .expect("fields")
                .keys()
                .collect::<Vec<_>>(),
        );
        assert_eq!(server_post["fields"]["key"], serde_json::json!("hex-a.jpg"));
    }

    #[test]
    fn base64_and_py_json_helpers() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(py_json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
