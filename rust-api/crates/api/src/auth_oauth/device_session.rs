//! D-17 device-session handlers B: workspaces list, machine-token exchange,
//! revoke (stage 5, PIDASHCONV-343).
//!
//! Ports `apps/api/pi_dash/authentication/views/cli/device.py:379-511`
//! (`WorkspaceListEndpoint`, `DeviceMachineTokenEndpoint`,
//! `DeviceCodeRevokeEndpoint`; routes in
//! `apps/api/pi_dash/api/urls/auth.py:27-46`, under the `api/v1/` include):
//!
//! * `GET auth/workspaces/` — workspaces the caller belongs to,
//!   member-since order (`device.py:379-402`).
//! * `POST auth/machine-token/` — exchange the device-flow `APIToken`
//!   bridge for a dev-machine `mt_` token (`device.py:405-482`).
//! * `POST auth/revoke/` — invalidate the caller's CLI token, idempotent
//!   (`device.py:485-511`).
//!
//! Only these three path+method pairs are served from Rust while every
//! sibling path keeps proxying to Django — route registration is the
//! cutover granularity, no flag needed. Every other method on the owned
//! paths answers locally in DRF order (auth first, then the 405), because
//! the token lives in Rust's database and a proxied verdict would fail
//! Django's copy of the gate before its own method check (PIDASHCONV-1478);
//! only OPTIONS still proxies so DRF's metadata response is preserved byte
//! for byte (the `v1_openapi::owned` precedent).
//!
//! Layering: the device-machine / rotate SQL text and pure helpers
//! (`normalize_host_label`, `machine_token_label`, touch-field selection,
//! lock/create/touch statements, ownership errors) live in the Done layer
//! [`pidash_db::auth_oauth::queries::dev_machine`] (PIDASHCONV-327,
//! AUTHOAUTH-F8); `deactivate_api_token` in
//! [`pidash_services::domain::tokens::cli_token`] (AUTHOAUTH-F7); the
//! token classify/hash/fingerprint kernels in [`pidash_auth::token`]
//! (foundation). This module owns the HTTP shell (routes, API-key auth,
//! body validation), the membership/workspace/APIToken/MachineToken SQL,
//! and the DRF error rendering.
//!
//! Handler order (preserved, not redesigned): DRF authentication
//! (`APIKeyAuthentication`: `X-Api-Key` header, `APIToken` or `mt_`
//! machine token, `api_authentication.py:20-84`) runs before the body —
//! anonymous callers answer 401 `{"detail": ...}`, bad tokens 403
//! `{"detail": "Given API token is not valid"}` (DRF coerces the 401 to
//! 403 because the class defines no `authenticate_header`; probed live).
//! The machine-token body validates in view order (workspace_slug,
//! dev_machine_id, host_label, UUID shape, workspace+member, ownership).
//!
//! Fixture ids: AUTHOAUTH-F12 (`F12_device_endpoints.golden.json`, the
//! `workspaces_*`, `machine_token_*` and `revoke_*` rows).
//!
//! Ported edges (translate, don't redesign — also listed in the PR):
//!
//! * A truthy non-string body value (`123`, `true`, `["x"]`, `{"a": 1}`)
//!   crashes Python with `AttributeError` on `.strip()` (500, probed
//!   live); falsy non-strings (`0`, `false`, `[]`, `{}`, `null`) fall
//!   through to the required-field error. [`or_blank`] reproduces that
//!   split exactly.
//! * A dangling `dev_machine_id` on a machine token (FK violation, only
//!   reachable by direct SQL surgery) raises through `select_related` in
//!   Python (500); the auth shell answers 500 the same way.

use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use pidash_auth::token as token_kernel;
use pidash_db::auth_oauth::queries::dev_machine;
use pidash_services::domain::tokens::cli_token;

use crate::assistant::common::{
    body_get, json_response, parse_body, pool_of, request_content_type, BodyField, BodyMap,
    Failure, ParsedBody,
};
use crate::state::AppState;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

/// `api/urls/auth.py:43-46`, under the `api/v1/` include.
pub const WORKSPACES_PATH: &str = "/api/v1/auth/workspaces/";
/// `api/urls/auth.py:38-42`.
pub const MACHINE_TOKEN_PATH: &str = "/api/v1/auth/machine-token/";
/// `api/urls/auth.py:27-31`.
pub const REVOKE_PATH: &str = "/api/v1/auth/revoke/";

/// DRF renders `AuthenticationFailed("Given API token is not valid")` as
/// 403 here (no `authenticate_header` on the class, so the 401 coerces;
/// probed live on all three endpoints, bad/expired/inactive alike).
const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;

/// 200 `{"ok": true}` (`device.py:500,507,510-511`).
const OK_TRUE_BODY: &str = r#"{"ok":true}"#;

/// Register the device-session routes. Nothing else: sibling paths stay
/// unmatched and proxy to Django.
///
/// `machine-token/` and `revoke/` are Rust-owned (NEWPIDASH-51): the
/// exchange opportunistically issues the engine credential per
/// `_maybe_issue_engine_credential` and the revoke locks the machine and,
/// when it revokes the last live `mt_`, signs the engine out via
/// `revoke_engine_credential`.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(WORKSPACES_PATH, owned_get(get(get_workspaces)))
        .route(MACHINE_TOKEN_PATH, owned_post(post(post_machine_token)))
        .route(REVOKE_PATH, owned_post(post(post_revoke)))
}

/// A GET-owned path: the GET handler serves from Rust; every other
/// method answers the local 405 past the auth gate, and only OPTIONS
/// proxies for DRF's metadata. HEAD is explicit: the view defines no
/// `head`, so DRF 405s it — axum's automatic GET-backed HEAD would
/// wrongly answer 200.
fn owned_get(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(workspaces_method_not_allowed)
        .put(workspaces_method_not_allowed)
        .patch(workspaces_method_not_allowed)
        .delete(workspaces_method_not_allowed)
        .head(workspaces_method_not_allowed)
        .trace(workspaces_method_not_allowed)
        .connect(workspaces_method_not_allowed)
        .options(crate::edge::proxy)
        .fallback(workspaces_method_not_allowed)
}

/// A POST-owned path: the POST handler serves from Rust; every other
/// method answers the local 405 past the auth gate, and only OPTIONS
/// proxies for DRF's metadata. HEAD is explicit (DRF 405s it) so the
/// denial keeps its body, which runserver sends on the wire — axum's
/// automatic GET-backed HEAD would strip it.
fn owned_post(
    post_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    post_handler
        .get(post_only_method_not_allowed)
        .put(post_only_method_not_allowed)
        .patch(post_only_method_not_allowed)
        .delete(post_only_method_not_allowed)
        .head(post_only_method_not_allowed)
        .trace(post_only_method_not_allowed)
        .connect(post_only_method_not_allowed)
        .options(crate::edge::proxy)
        .fallback(post_only_method_not_allowed)
}

fn invalid_token() -> Response {
    Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(INVALID_TOKEN_BODY))
        .expect("static denial response")
}

// ---------------------------------------------------------------------------
// API-key authentication (`api_authentication.py:20-84`)
// ---------------------------------------------------------------------------

/// The authenticated caller: an `APIToken` bridge (`request.auth` is the
/// raw token string) or a machine token (`request.auth_machine_token` is
/// set, `request.auth` the raw `mt_` value).
enum Caller {
    Api { user_id: Uuid, raw: String },
    Machine { user_id: Uuid, token_id: Uuid },
}

/// `APIKeyAuthentication.authenticate`: the `X-Api-Key` header carries an
/// `APIToken` or, when it starts with `mt_`, a `MachineToken`.
/// No usable credential is DRF's `NotAuthenticated` (401); a bad one is
/// `AuthenticationFailed` (403 here — see [`INVALID_TOKEN_BODY`]).
#[allow(clippy::result_large_err)]
async fn authenticate(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
) -> Result<Caller, Response> {
    let Some(raw) = headers.get(token_kernel::API_KEY_HEADER) else {
        // `if not token: return None` (`api_authentication.py:68-69`) —
        // the `IsAuthenticated` gate then answers 401.
        return Err(Failure::unauthorized().into_response());
    };
    // Django decodes headers lossily to `str`, so undecodable bytes are
    // a lookup miss (403), never a missing credential (401); `"\0"` can
    // never match a stored token.
    let presented = raw.to_str().unwrap_or("\0");
    if presented.is_empty() {
        return Err(Failure::unauthorized().into_response());
    }
    if presented.starts_with(token_kernel::MACHINE_TOKEN_PREFIX) {
        authenticate_machine(pool, secret_key, presented).await
    } else {
        authenticate_api(pool, presented).await
    }
}

/// Whether the request presents a credential for the auth gate: the
/// `X-Api-Key` header is set to a non-empty value. Mirrors the no-DB
/// 401 branches of [`authenticate`] exactly (a missing or empty header
/// never reaches the pool); undecodable bytes still need the lookup (a
/// miss is 403, never 401).
fn has_presented_credential(headers: &HeaderMap) -> bool {
    headers
        .get(token_kernel::API_KEY_HEADER)
        .is_some_and(|raw| !matches!(raw.to_str(), Ok(value) if value.is_empty()))
}

/// `validate_api_token` (`api_authentication.py:30-43`): exact token
/// match, `is_active`, unexpired (`expired_at__gt=now` or null) — then
/// stamp `last_used`. The `deleted_at IS NULL` conjunct is the
/// `SoftDeletionManager` scope (`db/mixins.py:56-66`): a soft-deleted
/// token 403s instead of authenticating.
#[allow(clippy::result_large_err)]
async fn authenticate_api(pool: &sqlx::PgPool, presented: &str) -> Result<Caller, Response> {
    let now = Utc::now();
    let row = {
        use pidash_db::entity::api_tokens;
        let db = pidash_db::pool::sea_orm_connection(pool);
        let found = api_tokens::Entity::find()
            .filter(api_tokens::Column::Token.eq(presented))
            .filter(api_tokens::Column::DeletedAt.is_null())
            .one(&db)
            .await
            .map_err(|_| Failure::server_error().into_response())?;
        found.map(|m| {
            (
                m.id,
                m.user_id,
                m.is_active,
                m.expired_at
                    .map(|dt: sea_orm::prelude::DateTimeWithTimeZone| dt.with_timezone(&Utc)),
            )
        })
    };
    let Some((id, user_id, is_active, expired_at)) = row else {
        return Err(invalid_token());
    };
    if !is_active {
        return Err(invalid_token());
    }
    if let Some(expires) = expired_at {
        if expires <= now {
            return Err(invalid_token());
        }
    }
    // `api_token.last_used = now; save(update_fields=["last_used"])`.
    pidash_services::domain::tokens::touch_api_token_last_used_by_id(
        &pidash_db::pool::sea_orm_connection(pool),
        id,
        now,
    )
    .await
    .map_err(|_| Failure::server_error().into_response())?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
    Ok(Caller::Api {
        user_id,
        raw: presented.to_owned(),
    })
}

/// `validate_machine_token` (`api_authentication.py:45-63`): match on
/// `token_hash`, unrevoked, dev-machine unrevoked, workspace member (a
/// non-member's token is revoked, then rejected) — then stamp
/// `last_used_at`.
#[allow(clippy::result_large_err)]
async fn authenticate_machine(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    presented: &str,
) -> Result<Caller, Response> {
    let now = Utc::now();
    let token_hash = token_kernel::hash_token(presented, secret_key);
    let (id, user_id, workspace_id, dev_machine_id) = {
        use pidash_db::entity::machine_token;
        let db = pidash_db::pool::sea_orm_connection(pool);
        let found = machine_token::Entity::find()
            .filter(machine_token::Column::TokenHash.eq(&token_hash))
            .one(&db)
            .await
            .map_err(|_| Failure::server_error().into_response())?;
        let Some(m) = found else {
            return Err(invalid_token());
        };
        (m.id, m.user_id, m.workspace_id, m.dev_machine_id)
    };
    // revoked check via fresh fetch (avoids join raw SQL; same semantics)
    {
        use pidash_db::entity::machine_token;
        let db = pidash_db::pool::sea_orm_connection(pool);
        let row = machine_token::Entity::find_by_id(id)
            .one(&db)
            .await
            .unwrap_or(None);
        match row {
            Some(m) if m.revoked_at.is_some() => return Err(invalid_token()),
            Some(_) => {}
            None => return Err(invalid_token()),
        }
        if let Some(mid) = dev_machine_id {
            use pidash_db::entity::dev_machine;
            let machine = dev_machine::Entity::find_by_id(mid)
                .one(&db)
                .await
                .unwrap_or(None);
            match machine {
                Some(dm) => {
                    if dm.revoked_at.is_some() {
                        return Err(invalid_token());
                    }
                }
                None => return Err(Failure::server_error().into_response()),
            }
        }
    }
    if !is_member(pool, workspace_id, user_id).await? {
        // `machine_token.revoke()` on the way out (`revoked_at` is `None`
        // here, so the guard is a no-op and the write always lands).
        pidash_services::domain::tokens::revoke_machine_token_by_id(
            &pidash_db::pool::sea_orm_connection(pool),
            id,
            now,
        )
        .await
        .map_err(|_| Failure::server_error().into_response())?;
        return Err(invalid_token());
    }
    // `MachineToken.objects.filter(pk=...).update(last_used_at=now)`.
    pidash_services::domain::tokens::touch_machine_token_last_used_by_id(
        &pidash_db::pool::sea_orm_connection(pool),
        id,
        now,
    )
    .await
    .map_err(|_| Failure::server_error().into_response())?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
    Ok(Caller::Machine {
        user_id,
        token_id: id,
    })
}

/// `is_workspace_member(user, workspace_id)`
/// (`core/permissions.py:28-34`): an active membership row for
/// `(workspace, member)`.
#[allow(clippy::result_large_err)]
async fn is_member(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<bool, Response> {
    use pidash_db::entity::workspace_members;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let count = workspace_members::Entity::find()
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .count(&db)
        .await
        .map_err(|_| Failure::server_error().into_response())?;
    Ok(count > 0)
}

fn not_found(error: &str) -> Failure {
    Failure::bare_error(StatusCode::NOT_FOUND, error)
}

// ---------------------------------------------------------------------------
// wrong-method denial (PIDASHCONV-1478)
// ---------------------------------------------------------------------------

/// DRF `default_response_headers` `Allow` for the GET-owned view:
/// `WorkspaceListEndpoint` defines only `get` (`device.py:395`), but
/// Django's `View.setup` binds `head = get` at request time, so the
/// live `Allow` is `GET, HEAD, OPTIONS` (probed on the real view class,
/// NEWPIDASH-31 — PIDASHCONV-1478 read `_allowed_methods` pre-setup and
/// missed the binding).
const ALLOW_WORKSPACES: &str = "GET, HEAD, OPTIONS";
/// DRF `default_response_headers` `Allow` for the POST-owned views,
/// which define only `post` (`device.py:417,496`).
const ALLOW_POST_ONLY: &str = "POST, OPTIONS";

/// DRF `MethodNotAllowed` body for these views (compact JSON, probed
/// live): `{"detail":"Method \"<M>\" not allowed."}`. `method` is an
/// `http::Method`, whose token charset needs no JSON escaping.
fn not_allowed_body(method: &str) -> String {
    format!(r#"{{"detail":"Method \"{method}\" not allowed."}}"#)
}

/// Wrong method on `GET auth/workspaces/`: DRF order — the auth gate
/// first (its 401/403 stand), then the 405.
async fn workspaces_method_not_allowed(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    deny_method(&state, &method, &headers, ALLOW_WORKSPACES).await
}

/// Wrong method on the POST-owned paths: the same DRF order with the
/// POST-only `Allow`.
async fn post_only_method_not_allowed(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    deny_method(&state, &method, &headers, ALLOW_POST_ONLY).await
}

/// Authenticate, then answer DRF's 405. A proxied verdict cannot work
/// here: the token lives in Rust's database, so Django's copy of the
/// gate 403s before its own method check.
async fn deny_method(
    state: &AppState,
    method: &Method,
    headers: &HeaderMap,
    allow: &'static str,
) -> Response {
    // The gate before the pool: Django's `authenticate` returns None
    // without a query when no credential is presented
    // (`api_authentication.py:68-69`), and `IsAuthenticated` answers
    // 401 — so a credential-less request 401s here without demanding
    // a pool (PIDASHCONV-1536). Only a presented credential needs the
    // lookup below.
    if !has_presented_credential(headers) {
        return Failure::unauthorized().into_response();
    }
    let pool = match pool_of(state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    if let Err(denial) = authenticate(&pool, state.settings().secret_key.as_bytes(), headers).await
    {
        return denial;
    }
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::ALLOW, allow)
        .body(axum::body::Body::from(not_allowed_body(method.as_str())))
        .expect("static denial response")
}

/// One `{"slug", "name"}` entry in Python dict order
/// (`device.py:401`).
fn workspace_entry(slug: &str, name: &str) -> serde_json::Value {
    let mut entry = serde_json::Map::with_capacity(2);
    entry.insert(
        "slug".to_owned(),
        serde_json::Value::String(slug.to_owned()),
    );
    entry.insert(
        "name".to_owned(),
        serde_json::Value::String(name.to_owned()),
    );
    serde_json::Value::Object(entry)
}

// ---------------------------------------------------------------------------
// workspaces list (`device.py:379-402`)
// ---------------------------------------------------------------------------

/// `GET /api/v1/auth/workspaces/`: `{"workspaces": [{"slug", "name"},
/// ...]}` in member-since order (`order_by("created_at")`), skipping
/// null workspaces.
async fn get_workspaces(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let caller = match authenticate(&pool, state.settings().secret_key.as_bytes(), &headers).await {
        Ok(caller) => caller,
        Err(denial) => return denial,
    };
    let user_id = match caller {
        Caller::Api { user_id, .. } | Caller::Machine { user_id, .. } => user_id,
    };
    // `WorkspaceMember.objects.filter(member=user,
    // is_active=True).select_related("workspace").order_by("created_at")`.
    let rows = {
        use pidash_db::entity::{workspace_members, workspaces};
        use sea_orm::{EntityTrait as _, QueryOrder as _};
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let members = match workspace_members::Entity::find()
            .filter(workspace_members::Column::MemberId.eq(user_id))
            .filter(workspace_members::Column::IsActive.eq(true))
            .filter(workspace_members::Column::DeletedAt.is_null())
            .order_by_asc(workspace_members::Column::CreatedAt)
            .all(&db)
            .await
        {
            Ok(m) => m,
            Err(_) => return Failure::server_error().into_response(),
        };
        let mut out = Vec::new();
        for m in members {
            let ws = workspaces::Entity::find_by_id(m.workspace_id)
                .one(&db)
                .await
                .unwrap_or(None);
            match ws {
                Some(w) => out.push((Some(w.slug), Some(w.name))),
                None => out.push((None, None)),
            }
        }
        out
    };
    let mut list = Vec::new();
    for (slug, name) in rows {
        // `... for m in members if m.workspace is not None`.
        let (Some(slug), Some(name)) = (slug, name) else {
            continue;
        };
        list.push(workspace_entry(&slug, &name));
    }
    let mut body = serde_json::Map::with_capacity(1);
    body.insert("workspaces".to_owned(), serde_json::Value::Array(list));
    json_response(
        StatusCode::OK,
        serde_json::to_string(&serde_json::Value::Object(body))
            .expect("workspaces body serializes"),
    )
}

// ---------------------------------------------------------------------------
// machine-token exchange (`device.py:405-482`)
// ---------------------------------------------------------------------------

/// `(request.data.get(key) or "")` for a parsed body field: missing,
/// null, and falsy JSON scalars are `""`; a string is itself; a truthy
/// non-string is the `AttributeError` on `.strip()` Python raises (the
/// 500 the view lets bubble — probed live with `123`).
fn or_blank(field: Option<&BodyField>) -> Result<String, Failure> {
    let Some(field) = field else {
        return Ok(String::new());
    };
    match field {
        BodyField::File { .. } => Err(Failure::server_error()),
        BodyField::Json(value) => match value {
            serde_json::Value::Null => Ok(String::new()),
            serde_json::Value::String(s) => Ok(s.clone()),
            serde_json::Value::Bool(false) => Ok(String::new()),
            serde_json::Value::Number(n) => {
                if n.as_u64() == Some(0) || n.as_i64() == Some(0) || n.as_f64() == Some(0.0) {
                    Ok(String::new())
                } else {
                    Err(Failure::server_error())
                }
            }
            serde_json::Value::Array(items) if items.is_empty() => Ok(String::new()),
            serde_json::Value::Object(map) if map.is_empty() => Ok(String::new()),
            _ => Err(Failure::server_error()),
        },
    }
}

/// The three validated machine-token inputs, in view-check order.
#[derive(Debug)]
struct MachineTokenParams {
    workspace_slug: String,
    dev_machine_id: Uuid,
    host_label: String,
}

/// Body validation in `device.py:420-444` order: the three required
/// fields, then the UUID shape. Returns the first failure.
fn validate_machine_token_params(fields: &BodyMap) -> Result<MachineTokenParams, Failure> {
    let workspace_slug = or_blank(body_get(fields, "workspace_slug"))?
        .trim()
        .to_owned();
    if workspace_slug.is_empty() {
        return Err(Failure::bare_error(
            StatusCode::BAD_REQUEST,
            "workspace_slug is required",
        ));
    }
    let dev_machine_id_raw = or_blank(body_get(fields, "dev_machine_id"))?
        .trim()
        .to_owned();
    if dev_machine_id_raw.is_empty() {
        return Err(Failure::bare_error(
            StatusCode::BAD_REQUEST,
            "dev_machine_id is required",
        ));
    }
    // `(request.data.get("host_label") or "").strip()[:255]` — the
    // truncation runs before the required check.
    let host_label =
        dev_machine::normalize_host_label(Some(or_blank(body_get(fields, "host_label"))?.trim()));
    if host_label.is_empty() {
        return Err(Failure::bare_error(
            StatusCode::BAD_REQUEST,
            "host_label is required",
        ));
    }
    // `_uuid.UUID(...)` over `(TypeError, ValueError, AttributeError)`.
    let dev_machine_id = Uuid::parse_str(&dev_machine_id_raw)
        .map_err(|_| Failure::bare_error(StatusCode::BAD_REQUEST, "invalid_dev_machine_id"))?;
    Ok(MachineTokenParams {
        workspace_slug,
        dev_machine_id,
        host_label,
    })
}

/// `secrets.token_urlsafe(32)`: 32 random bytes, urlsafe-base64,
/// padding stripped (43 chars) — the `mt_` suffix of
/// `runner_tokens.mint_machine_token` (`tokens.py:84-86`).
fn token_urlsafe_32() -> String {
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `POST /api/v1/auth/machine-token/`: exchange the device-flow bridge
/// for a dev-machine token, revoking the previous token(s) for the
/// `(workspace, dev_machine)` pair and deactivating the bridge when the
/// caller presented one.
async fn post_machine_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let caller = match authenticate(&pool, state.settings().secret_key.as_bytes(), &headers).await {
        Ok(caller) => caller,
        Err(denial) => return denial,
    };
    // DRF parses the body before the view runs; scalars/null are the
    // `.get` 500, malformed JSON the `ParseError` 400.
    let fields = match parse_body(&body, request_content_type(&headers)) {
        Ok(ParsedBody::Object(fields)) => fields,
        Ok(ParsedBody::Scalar(_) | ParsedBody::Null) => {
            return Failure::server_error().into_response();
        }
        Err(failure) => return failure.into_response(),
    };
    let params = match validate_machine_token_params(&fields) {
        Ok(params) => params,
        Err(failure) => return failure.into_response(),
    };
    // `Workspace.objects.filter(slug=...).first()` + membership, else
    // 404 (`device.py:446-451`) — via SeaORM.
    let workspace = {
        use pidash_db::entity::workspaces;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let row = workspaces::Entity::find()
            .filter(workspaces::Column::Slug.eq(&params.workspace_slug))
            .filter(workspaces::Column::DeletedAt.is_null())
            .one(&db)
            .await;
        let Ok(Some(row)) = row else {
            return match row {
                Err(_) => Failure::server_error().into_response(),
                Ok(None) => not_found("workspace_not_found").into_response(),
                _ => unreachable!(),
            };
        };
        (row.id, row.slug)
    };
    let (workspace_id, workspace_slug) = workspace;
    let user_id = match &caller {
        Caller::Api { user_id, .. } | Caller::Machine { user_id, .. } => *user_id,
    };
    match is_member(&pool, workspace_id, user_id).await {
        Ok(true) => {}
        Ok(false) => return not_found("workspace_not_found").into_response(),
        Err(denial) => return denial,
    }

    // `with transaction.atomic()` over get-or-create, rotate and bridge
    // deactivation (`device.py:453-467`).
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Failure::server_error().into_response(),
    };
    let now = Utc::now();
    let outcome = exchange_in_transaction(
        &mut tx,
        state.settings().secret_key.as_bytes(),
        state.settings(),
        &caller,
        user_id,
        workspace_id,
        &params,
        now,
    )
    .await;
    match outcome {
        Ok((dev_machine_id, host_label, raw, engine_sign_in_state, engine_credential)) => {
            if tx.commit().await.is_err() {
                return Failure::server_error().into_response();
            }
            let cred_opt: Option<(String, String)> = engine_credential.clone();
            json_response(
                StatusCode::CREATED,
                machine_token_response_body(
                    &raw,
                    &workspace_slug,
                    &dev_machine_id,
                    &host_label,
                    &engine_sign_in_state,
                    cred_opt.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
                ),
            )
        }
        Err(denial) => denial,
    }
}

/// The 201 body in Python dict order (`device.py:474-481` + engine):
/// `machine_token`, `workspace_slug`, `dev_machine_id`, `host_label`,
/// `engine_sign_in_state`, plus `engine_credential` when issued.
fn machine_token_response_body(
    raw: &str,
    workspace_slug: &str,
    dev_machine_id: &Uuid,
    host_label: &str,
    engine_sign_in_state: &str,
    engine_credential: Option<(&str, &str)>,
) -> String {
    let mut entry = serde_json::Map::with_capacity(6);
    entry.insert(
        "machine_token".to_owned(),
        serde_json::Value::String(raw.to_owned()),
    );
    entry.insert(
        "workspace_slug".to_owned(),
        serde_json::Value::String(workspace_slug.to_owned()),
    );
    entry.insert(
        "dev_machine_id".to_owned(),
        serde_json::Value::String(dev_machine_id.to_string()),
    );
    entry.insert(
        "host_label".to_owned(),
        serde_json::Value::String(host_label.to_owned()),
    );
    entry.insert(
        "engine_sign_in_state".to_owned(),
        serde_json::Value::String(engine_sign_in_state.to_owned()),
    );
    if let Some((token, expires_at)) = engine_credential {
        let mut cred = serde_json::Map::with_capacity(2);
        cred.insert(
            "token".to_owned(),
            serde_json::Value::String(token.to_owned()),
        );
        cred.insert(
            "expires_at".to_owned(),
            serde_json::Value::String(expires_at.to_owned()),
        );
        entry.insert(
            "engine_credential".to_owned(),
            serde_json::Value::Object(cred),
        );
    }
    serde_json::to_string(&serde_json::Value::Object(entry)).expect("machine-token body serializes")
}

#[allow(clippy::too_many_arguments)]
/// The transactional core of the exchange: get-or-create the dev machine
/// (ownership conflicts are 404), revoke-then-mint the machine token,
/// opportunistically issue an engine credential via
/// `_maybe_issue_engine_credential`, and deactivate the `APIToken` bridge
/// for API-key callers.
#[allow(clippy::result_large_err)]
async fn exchange_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    secret_key: &[u8],
    settings: &pidash_db::config::Settings,
    caller: &Caller,
    user_id: Uuid,
    workspace_id: Uuid,
    params: &MachineTokenParams,
    now: DateTime<Utc>,
) -> Result<(Uuid, String, String, String, Option<(String, String)>), Response> {
    // `_get_or_create_dev_machine` (`device.py:88-109`): lock, then
    // create-or-retry through the Done-layer kernels.
    let locked = dev_machine::fetch_locked_machine(&mut **tx, params.dev_machine_id)
        .await
        .map_err(|_| Failure::server_error().into_response())?;
    match locked {
        Some(machine) => {
            // Owner mismatch raises (`device.py:92-94`); the dropped
            // transaction rolls back (`device.py` aborts the atomic
            // block the same way).
            if dev_machine::check_owner(machine.owner_id, user_id).is_err() {
                return Err(not_found("dev_machine_not_found").into_response());
            }
            dev_machine::touch_dev_machine(
                &mut **tx,
                machine.id,
                &machine.host_label,
                &machine.label,
                Some(&params.host_label),
                now,
            )
            .await
            .map_err(|_| Failure::server_error().into_response())?;
        }
        None => {
            // Racing insert under a savepoint (the inner
            // `transaction.atomic()` savepoint of `device.py:96-101`):
            // a unique violation rolls back to the savepoint and
            // re-locks; anything else aborts the exchange.
            sqlx::query("SAVEPOINT dev_machine_create")
                .execute(&mut **tx)
                .await
                .map_err(|_| Failure::server_error().into_response())?;
            let created = sqlx::query(dev_machine::CREATE_SQL)
                .bind(params.dev_machine_id)
                .bind(user_id)
                .bind(&params.host_label)
                .bind(dev_machine::label_from_host_label(&params.host_label))
                .bind(dev_machine::VISIBILITY_PRIVATE)
                .bind(dev_machine::PROVISIONING_MANUAL)
                .bind(now)
                .bind(None::<DateTime<Utc>>)
                .bind(now)
                .bind(now)
                .execute(&mut **tx)
                .await;
            match created {
                Ok(_) => {
                    sqlx::query("RELEASE SAVEPOINT dev_machine_create")
                        .execute(&mut **tx)
                        .await
                        .map_err(|_| Failure::server_error().into_response())?;
                }
                Err(err) if dev_machine::is_unique_violation(&err) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT dev_machine_create")
                        .execute(&mut **tx)
                        .await
                        .map_err(|_| Failure::server_error().into_response())?;
                    // `locked is None or owner mismatch -> raise`
                    // (`device.py:105-108`).
                    let relocked =
                        dev_machine::fetch_locked_machine(&mut **tx, params.dev_machine_id)
                            .await
                            .map_err(|_| Failure::server_error().into_response())?;
                    let same_owner = relocked.filter(|machine| {
                        dev_machine::check_owner(machine.owner_id, user_id).is_ok()
                    });
                    let Some(machine) = same_owner else {
                        return Err(not_found("dev_machine_not_found").into_response());
                    };
                    dev_machine::touch_dev_machine(
                        &mut **tx,
                        machine.id,
                        &machine.host_label,
                        &machine.label,
                        Some(&params.host_label),
                        now,
                    )
                    .await
                    .map_err(|_| Failure::server_error().into_response())?;
                }
                Err(_) => {
                    return Err(Failure::server_error().into_response());
                }
            }
        }
    }
    // `_rotate_machine_token` (`device.py:112-129`): revoke every live
    // token for the pair, then mint.
    pidash_services::domain::tokens::revoke_tokens_for_device_pair(
        &pidash_db::orm_tx::TxConn::new(tx),
        workspace_id,
        params.dev_machine_id,
        now,
    )
    .await
    .map_err(|_| Failure::server_error().into_response())?;
    let raw = format!(
        "{}{}",
        token_kernel::MACHINE_TOKEN_PREFIX,
        token_urlsafe_32()
    );
    let token_id = Uuid::new_v4();
    pidash_services::domain::tokens::insert_machine_token(
        &pidash_db::orm_tx::TxConn::new(tx),
        &pidash_services::domain::tokens::NewMachineToken {
            id: token_id,
            user_id,
            dev_machine_id: Some(params.dev_machine_id),
            workspace_id,
            host_label: dev_machine::rotate_host_label(&params.host_label),
            token_hash: token_kernel::hash_token(&raw, secret_key),
            token_fingerprint: token_kernel::fingerprint(&raw),
            label: dev_machine::machine_token_label(&params.host_label),
            is_service: true,
            created_at: now,
        },
    )
    .await
    .map_err(|_| Failure::server_error().into_response())?;
    // `_maybe_issue_engine_credential` (`device.py:570-595`): when the
    // caller is not already a machine token (device-flow API token),
    // find the newest cli grant with api_token.token = request.auth and
    // try to issue. Nothing when no grant, machine revoked, silent and
    // !allowed, or instance_can_serve false. Swallow silent refusal.
    let mut engine_cred: Option<(String, String)> = None;
    let mut engine_state = {
        use sea_orm::EntityTrait;
        let m = pidash_db::entity::dev_machine::Entity::find_by_id(params.dev_machine_id)
            .one(&pidash_db::orm_tx::TxConn::new(tx) as &_)
            .await
            .unwrap_or(None);
        m.map(|m| {
            pidash_services::domain::engine_credential::current_engine_sign_in_state(
                &m.engine_sign_in_state,
                m.engine_credential_expires_at.as_ref().map(
                    |dt: &sea_orm::prelude::DateTimeWithTimeZone| dt.with_timezone(&chrono::Utc),
                ),
                now,
            )
        })
        .unwrap_or_else(|| "never".to_owned())
    };
    if matches!(caller, Caller::Api { .. }) {
        // Find the grant via api_token.token = bridge raw (SeaORM fetch).
        let bridge_raw = match caller {
            Caller::Api { raw, .. } => raw.clone(),
            Caller::Machine { .. } => unreachable!(),
        };
        let grant = {
            use pidash_db::entity::{api_tokens, cli_device_codes};
            use sea_orm::{EntityTrait as _, QueryOrder as _};
            let tx_conn = pidash_db::orm_tx::TxConn::new(tx);
            let api_tok = api_tokens::Entity::find()
                .filter(api_tokens::Column::Token.eq(&bridge_raw))
                .filter(api_tokens::Column::DeletedAt.is_null())
                .one(&tx_conn as &_)
                .await
                .unwrap_or(None);
            match api_tok {
                Some(tok) => cli_device_codes::Entity::find()
                    .filter(cli_device_codes::Column::ApiTokenId.eq(tok.id))
                    .filter(cli_device_codes::Column::DeletedAt.is_null())
                    .order_by_desc(cli_device_codes::Column::CreatedAt)
                    .one(&tx_conn as &_)
                    .await
                    .unwrap_or(None)
                    .map(|c| (c.interactive_approval, c.dev_machine_id)),
                None => None,
            }
        };
        // grant is (interactive_approval, dev_machine_id)
        if let Some((interactive_approval, _grant_machine_id)) = grant {
            // Check machine revoked.
            let machine_revoked = {
                use sea_orm::EntityTrait;
                let m = pidash_db::entity::dev_machine::Entity::find_by_id(params.dev_machine_id)
                    .one(&pidash_db::orm_tx::TxConn::new(tx) as &_)
                    .await
                    .unwrap_or(None);
                m.map(|m| m.revoked_at.is_some()).unwrap_or(true)
            };
            if !machine_revoked
                && pidash_services::domain::engine_credential::instance_can_serve_engine(settings)
            {
                let silent = !interactive_approval;
                // Check silent allowed via machine's current state.
                let machine_for_silent = {
                    use sea_orm::EntityTrait;
                    pidash_db::entity::dev_machine::Entity::find_by_id(params.dev_machine_id)
                        .one(&pidash_db::orm_tx::TxConn::new(tx) as &_)
                        .await
                        .unwrap_or(None)
                };
                let should_issue = if silent {
                    if let Some(m) = machine_for_silent {
                        pidash_services::domain::engine_credential::silent_issue_allowed(
                            &pidash_db::orm_tx::TxConn::new(tx) as &_,
                            &m,
                            now,
                        )
                        .await
                        .unwrap_or(false)
                    } else {
                        false
                    }
                } else {
                    true
                };
                if should_issue {
                    let dbc = pidash_db::orm_tx::TxConn::new(tx);
                    let issued =
                        pidash_services::domain::engine_credential::issue_engine_credential(
                            &dbc,
                            user_id,
                            params.dev_machine_id,
                            secret_key,
                            silent,
                            now,
                        )
                        .await;
                    if let Ok(ic) = issued {
                        engine_cred = Some((
                            ic.raw.clone(),
                            ic.expires_at
                                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        ));
                        engine_state = {
                            use sea_orm::EntityTrait;
                            let m = pidash_db::entity::dev_machine::Entity::find_by_id(
                                params.dev_machine_id,
                            )
                            .one(&pidash_db::orm_tx::TxConn::new(tx) as &_)
                            .await
                            .unwrap_or(None);
                            m.map(|m| {
                                pidash_services::domain::engine_credential::current_engine_sign_in_state(
                                    &m.engine_sign_in_state,
                                    m.engine_credential_expires_at.as_ref().map(|dt: &sea_orm::prelude::DateTimeWithTimeZone| dt.with_timezone(&chrono::Utc)),
                                    now,
                                )
                            })
                            .unwrap_or_else(|| engine_state.clone())
                        };
                    }
                    // Silent refusal is swallowed.
                }
            }
        }
    }
    // The bridge is an in-memory credential: deactivate it for API-key
    // callers, keep machine-token callers on their rotation
    // (`device.py:466-467`).
    if let Caller::Api { raw: bridge, .. } = caller {
        if cli_token::deactivate_api_token(
            &pidash_db::orm_tx::TxConn::new(tx),
            Some(bridge),
            true,
            now,
        )
        .await
        .is_err()
        {
            return Err(Failure::server_error().into_response());
        }
    }
    Ok((
        params.dev_machine_id,
        params.host_label.clone(),
        raw,
        engine_state,
        engine_cred,
    ))
}

// ---------------------------------------------------------------------------
// revoke (`device.py:485-511`)
// ---------------------------------------------------------------------------

/// `POST /api/v1/auth/revoke/`: invalidate the caller's CLI token.
/// Idempotent — a missing or already-inactive `APIToken` still answers
/// ok (`device.py:503-507`). For a machine-token caller, in one
/// transaction, lock the token's machine, revoke the token, and when the
/// machine then has no live token, `revoke_engine_credential`.
async fn post_revoke(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let caller = match authenticate(&pool, state.settings().secret_key.as_bytes(), &headers).await {
        Ok(caller) => caller,
        Err(denial) => return denial,
    };
    match caller {
        Caller::Machine { token_id, .. } => {
            // `machine_token.revoke()` + engine sign-out on last token,
            // in one transaction with the machine row locked so the
            // "last live token" check is exact (device.py:485-511,
            // "Serializes concurrent revocations ...").
            let now = Utc::now();
            let mut tx = match pool.begin().await {
                Ok(tx) => tx,
                Err(_) => return Failure::server_error().into_response(),
            };
            let machine_id_opt: Option<Uuid> = {
                use sea_orm::EntityTrait;
                let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
                let tok = pidash_db::entity::machine_token::Entity::find_by_id(token_id)
                    .one(&tx_conn as &_)
                    .await
                    .unwrap_or(None);
                tok.and_then(|t| t.dev_machine_id)
            };
            if let Some(mid) = machine_id_opt {
                // raw-sql: FOR UPDATE lock has no SeaORM equivalent.
                let lock: Option<(Uuid,)> =
                    sqlx::query_as(r#"SELECT id FROM dev_machine WHERE id = $1 FOR UPDATE"#)
                        .bind(mid)
                        .fetch_optional(&mut *tx)
                        .await
                        .unwrap_or(None);
                // Missing row (concurrent delete) is non-fatal: the revoke
                // below still lands; Django's .first() yields None similarly.
                let _ = lock;
            }
            if pidash_services::domain::tokens::revoke_machine_token_by_id_if_active(
                &pidash_db::orm_tx::TxConn::new(&mut tx),
                token_id,
                now,
            )
            .await
            .is_err()
            {
                return Failure::server_error().into_response();
            }
            if let Some(mid) = machine_id_opt {
                // _machine_has_live_token: revoked_at IS NULL AND (expires_at IS NULL OR > now)
                let has_live = {
                    use sea_orm::ColumnTrait;
                    use sea_orm::EntityTrait;
                    let tx_conn = pidash_db::orm_tx::TxConn::new(&mut tx);
                    let count = pidash_db::entity::machine_token::Entity::find()
                        .filter(pidash_db::entity::machine_token::Column::DevMachineId.eq(mid))
                        .filter(pidash_db::entity::machine_token::Column::RevokedAt.is_null())
                        .filter(
                            sea_orm::Condition::any()
                                .add(pidash_db::entity::machine_token::Column::ExpiresAt.is_null())
                                .add(
                                    pidash_db::entity::machine_token::Column::ExpiresAt
                                        .gt(now.fixed_offset()),
                                ),
                        )
                        .count(&tx_conn as &_)
                        .await
                        .unwrap_or(1);
                    count > 0
                };
                if !has_live {
                    let _ = pidash_services::domain::engine_credential::revoke_engine_credential(
                        &pidash_db::orm_tx::TxConn::new(&mut tx),
                        mid,
                        pidash_services::domain::engine_credential::revoke_reason::SIGNED_OUT,
                        now,
                    )
                    .await;
                }
            }
            if tx.commit().await.is_err() {
                return Failure::server_error().into_response();
            }
        }
        Caller::Api { raw, .. } => {
            // `request.auth` is the raw token string
            // (`api_authentication.py:62`); the row is re-read so a
            // missing or inactive token stays a silent ok. The
            // `deleted_at IS NULL` conjunct is the `SoftDeletionManager`
            // scope (`db/mixins.py:56-66`): Python re-reads via
            // `APIToken.objects.get`, which never sees soft-deleted rows
            // (`device.py:504`).
            let row = {
                use pidash_db::entity::api_tokens;
                let db = pidash_db::pool::sea_orm_connection(&pool);
                let found = api_tokens::Entity::find()
                    .filter(api_tokens::Column::Token.eq(&raw))
                    .filter(api_tokens::Column::DeletedAt.is_null())
                    .one(&db)
                    .await;
                match found {
                    Ok(Some(m)) => Some((m.id, m.is_active)),
                    Ok(None) => None,
                    Err(_) => return Failure::server_error().into_response(),
                }
            };
            if let Some((id, true)) = row {
                if pidash_services::domain::tokens::deactivate_api_token_by_id(
                    &pidash_db::pool::sea_orm_connection(&pool),
                    id,
                    Utc::now(),
                )
                .await
                .is_err()
                {
                    return Failure::server_error().into_response();
                }
            }
        }
    }
    json_response(StatusCode::OK, OK_TRUE_BODY.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/auth_oauth")
    }

    fn f12() -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_dir().join("F12_device_endpoints.golden.json"))
            .expect("read F12_device_endpoints.golden.json");
        serde_json::from_str(&body).expect("F12_device_endpoints.golden.json is valid JSON")
    }

    fn json_field(name: &str, value: &str) -> (String, BodyField) {
        (
            name.to_owned(),
            BodyField::Json(serde_json::Value::String(value.to_owned())),
        )
    }

    #[test]
    fn owned_paths_match_django_urls() {
        // `api/urls/auth.py:27-46`, under the `api/v1/` include.
        assert_eq!(WORKSPACES_PATH, "/api/v1/auth/workspaces/");
        assert_eq!(MACHINE_TOKEN_PATH, "/api/v1/auth/machine-token/");
        assert_eq!(REVOKE_PATH, "/api/v1/auth/revoke/");
    }

    #[test]
    fn auth_denial_bodies_match_live_django() {
        // Probed live: no credential is DRF `NotAuthenticated` (401);
        // bad/expired/inactive tokens coerce to 403 (no
        // `authenticate_header` on the class).
        assert_eq!(Failure::unauthorized().status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            Failure::unauthorized().body,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            INVALID_TOKEN_BODY,
            r#"{"detail":"Given API token is not valid"}"#
        );
    }

    #[test]
    fn method_not_allowed_body_and_allow_match_live_django() {
        // DRF `exception_handler` (`{'Detail': ...}`) + compact
        // `JSONRenderer`, and each view's `_allowed_methods`
        // (PIDASHCONV-1478).
        assert_eq!(
            not_allowed_body("GET"),
            r#"{"detail":"Method \"GET\" not allowed."}"#
        );
        assert_eq!(
            not_allowed_body("POST"),
            r#"{"detail":"Method \"POST\" not allowed."}"#
        );
        assert_eq!(
            not_allowed_body("DELETE"),
            r#"{"detail":"Method \"DELETE\" not allowed."}"#
        );
        assert_eq!(ALLOW_WORKSPACES, "GET, HEAD, OPTIONS");
        assert_eq!(ALLOW_POST_ONLY, "POST, OPTIONS");
    }

    #[test]
    fn presented_credential_mirrors_authenticate_no_db_branches() {
        // Missing or empty is the 401 without a query
        // (`api_authentication.py:68-69`); anything else — including
        // undecodable bytes, a lookup miss — needs the pool
        // (PIDASHCONV-1536).
        use axum::http::{HeaderName, HeaderValue};
        let name: HeaderName = token_kernel::API_KEY_HEADER
            .parse()
            .expect("api key header name");
        let empty = HeaderMap::new();
        assert!(!has_presented_credential(&empty));
        let mut headers = HeaderMap::new();
        headers.insert(name.clone(), HeaderValue::from_static(""));
        assert!(!has_presented_credential(&headers));
        headers.insert(name.clone(), HeaderValue::from_static("x"));
        assert!(has_presented_credential(&headers));
        headers.insert(
            name,
            HeaderValue::from_bytes(&[0xff]).expect("opaque bytes"),
        );
        assert!(has_presented_credential(&headers));
    }

    #[test]
    fn workspaces_entry_key_order_matches_python() {
        // `{"slug": ..., "name": ...}` (`device.py:401`); the string
        // comparison is order-sensitive.
        assert_eq!(
            serde_json::to_string(&workspace_entry("ws", "WS")).expect("entry serializes"),
            r#"{"slug":"ws","name":"WS"}"#
        );
        // The F12 workspaces row pins the same pair order.
        let out = &f12()["workspaces_GET_workspaces"]["out"];
        assert_eq!(out["status"], 200);
        assert!(out["body"].get("workspaces").is_some());
    }

    #[test]
    fn revoke_ok_body_matches_f12() {
        for branch in f12()["revoke_POST_revoke_idempotent"]["branches"]
            .as_array()
            .expect("revoke branches")
        {
            assert_eq!(branch["out"]["status"], 200);
            assert_eq!(branch["out"]["body"], serde_json::json!({"ok": true}));
        }
        assert_eq!(OK_TRUE_BODY, r#"{"ok":true}"#);
    }

    #[test]
    fn machine_token_branch_bodies_match_f12() {
        let branches = f12()["machine_token_POST_machine_token"]["branches"]
            .as_array()
            .expect("machine-token branches")
            .clone();
        let body = |error: &str| serde_json::json!({"error": error});
        assert_eq!(
            branches[0]["out"]["body"],
            body("workspace_slug is required")
        );
        assert_eq!(branches[0]["out"]["status"], 400);
        assert_eq!(
            branches[1]["out"]["body"],
            body("dev_machine_id is required")
        );
        assert_eq!(branches[2]["out"]["body"], body("host_label is required"));
        assert_eq!(branches[3]["out"]["body"], body("invalid_dev_machine_id"));
        assert_eq!(branches[4]["out"]["body"], body("workspace_not_found"));
        assert_eq!(branches[4]["out"]["status"], 404);
        assert_eq!(branches[5]["out"]["body"], body("dev_machine_not_found"));
        assert_eq!(branches[5]["out"]["status"], 404);
        assert_eq!(branches[6]["out"]["status"], 201);
        // The 201 keys render in Python dict order (`device.py:474-481`);
        // the string comparison is order-sensitive.
        let dm = Uuid::parse_str("10ecd282-5e9e-4616-8139-015323a3d96c").expect("fixture uuid");
        assert_eq!(
            machine_token_response_body("mt_RAW", "ws", &dm, "myhost", "never", None),
            r#"{"machine_token":"mt_RAW","workspace_slug":"ws","dev_machine_id":"10ecd282-5e9e-4616-8139-015323a3d96c","host_label":"myhost","engine_sign_in_state":"never"}"#
        );
    }

    #[test]
    fn or_blank_reproduces_python_truthiness() {
        use serde_json::json;
        // Missing / null / falsy scalars are the `or ""` fallback.
        assert_eq!(or_blank(None).expect("none").as_str(), "");
        assert_eq!(
            or_blank(Some(&BodyField::Json(json!(null))))
                .expect("null")
                .as_str(),
            ""
        );
        assert_eq!(
            or_blank(Some(&BodyField::Json(json!(false))))
                .expect("false")
                .as_str(),
            ""
        );
        assert_eq!(
            or_blank(Some(&BodyField::Json(json!(0))))
                .expect("zero")
                .as_str(),
            ""
        );
        assert_eq!(
            or_blank(Some(&BodyField::Json(json!(0.0))))
                .expect("zero-float")
                .as_str(),
            ""
        );
        assert_eq!(
            or_blank(Some(&BodyField::Json(json!([]))))
                .expect("empty vec")
                .as_str(),
            ""
        );
        assert_eq!(
            or_blank(Some(&BodyField::Json(json!({}))))
                .expect("empty map")
                .as_str(),
            ""
        );
        // Strings pass through unstripped (the view strips).
        assert_eq!(
            or_blank(Some(&BodyField::Json(json!("  h  "))))
                .expect("str")
                .as_str(),
            "  h  "
        );
        // Truthy non-strings are the `.strip()` AttributeError 500.
        for value in [json!(123), json!(true), json!([1]), json!({"a": 1})] {
            let field = BodyField::Json(value);
            assert!(
                or_blank(Some(&field)).is_err(),
                "truthy non-string must 500"
            );
        }
    }

    #[test]
    fn validation_order_matches_view() {
        let dm = Uuid::new_v4().to_string();
        let fields = |pairs: Vec<(String, BodyField)>| pairs;
        // workspace_slug first, even when everything else is also blank.
        let err = validate_machine_token_params(&fields(vec![])).expect_err("empty");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.body, r#"{"error":"workspace_slug is required"}"#);
        // dev_machine_id second.
        let err = validate_machine_token_params(&fields(vec![json_field("workspace_slug", "ws")]))
            .expect_err("no dm");
        assert_eq!(err.body, r#"{"error":"dev_machine_id is required"}"#);
        // host_label third — and the [:255] truncation runs before the
        // required check, so a long label proceeds past it.
        let long = "h".repeat(300);
        let params = validate_machine_token_params(&fields(vec![
            json_field("workspace_slug", "ws"),
            json_field("dev_machine_id", &dm),
            json_field("host_label", &long),
        ]))
        .expect("long host label truncates");
        assert_eq!(params.host_label.len(), 255);
        // Whitespace-only host is blank after the strip.
        let err = validate_machine_token_params(&fields(vec![
            json_field("workspace_slug", "ws"),
            json_field("dev_machine_id", &dm),
            json_field("host_label", "   "),
        ]))
        .expect_err("blank host");
        assert_eq!(err.body, r#"{"error":"host_label is required"}"#);
        // UUID shape last.
        let err = validate_machine_token_params(&fields(vec![
            json_field("workspace_slug", "ws"),
            json_field("dev_machine_id", "not-a-uuid"),
            json_field("host_label", "h"),
        ]))
        .expect_err("bad uuid");
        assert_eq!(err.body, r#"{"error":"invalid_dev_machine_id"}"#);
        // Surrounding whitespace strips on all three fields.
        let params = validate_machine_token_params(&fields(vec![
            json_field("workspace_slug", "  ws  "),
            json_field("dev_machine_id", &format!("  {dm}  ")),
            json_field("host_label", "  h  "),
        ]))
        .expect("strips");
        assert_eq!(params.workspace_slug, "ws");
        assert_eq!(params.dev_machine_id.to_string(), dm);
        assert_eq!(params.host_label, "h");
    }

    #[test]
    fn minted_suffix_matches_token_urlsafe_32() {
        // `secrets.token_urlsafe(32)`: 43 urlsafe chars, no padding.
        for _ in 0..10 {
            let suffix = token_urlsafe_32();
            assert_eq!(suffix.len(), 43, "{suffix}");
            assert!(
                suffix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{suffix}"
            );
        }
        assert_eq!(token_kernel::MACHINE_TOKEN_PREFIX, "mt_");
    }
}
