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
//! Only these three path+method pairs are registered, so the edge serves
//! exactly this family from Rust while every sibling path keeps proxying
//! to Django — route registration is the cutover granularity, no flag
//! needed. Every other method on the owned paths proxies too, so Django
//! answers its own 405-after-auth and metadata OPTIONS byte for byte
//! (the loop-handlers precedent).
//!
//! Layering: the device-machine / rotate SQL text and pure helpers
//! (`normalize_host_label`, `machine_token_label`, touch-field selection,
//! lock/create/touch statements, ownership errors) live in the Done layer
//! [`pidash_db::auth_oauth::queries::dev_machine`] (PIDASHCONV-327,
//! AUTHOAUTH-F8); `deactivate_api_token` in
//! [`pidash_db::auth_oauth::queries::cli_token`] (AUTHOAUTH-F7); the
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
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use pidash_auth::token as token_kernel;
use pidash_db::auth_oauth::queries::{cli_token, dev_machine};

use crate::assistant::common::{
    body_get, json_response, parse_body, pool_of, request_content_type, BodyField, BodyMap,
    Failure, ParsedBody,
};
use crate::state::AppState;

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

/// Register the three device-session routes. Nothing else: sibling paths
/// stay unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(WORKSPACES_PATH, owned_get(get(get_workspaces)))
        .route(MACHINE_TOKEN_PATH, owned_post(post(post_machine_token)))
        .route(REVOKE_PATH, owned_post(post(post_revoke)))
}

/// A GET-owned path: the GET handler serves from Rust, everything else
/// falls through to Django.
fn owned_get(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// A POST-owned path: the POST handler serves from Rust, everything
/// else falls through to Django.
fn owned_post(
    post_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    post_handler
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
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
        // `if not token: return None` (`api_authentication.py:70-72`) —
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

/// One `api_tokens` row for the lookup: `(id, user_id, is_active,
/// expired_at)`.
type ApiTokenLookupRow = (Uuid, Uuid, bool, Option<DateTime<Utc>>);

/// `validate_api_token` (`api_authentication.py:30-43`): exact token
/// match, `is_active`, unexpired (`expired_at__gt=now` or null) — then
/// stamp `last_used`.
#[allow(clippy::result_large_err)]
async fn authenticate_api(pool: &sqlx::PgPool, presented: &str) -> Result<Caller, Response> {
    let now = Utc::now();
    let row: Option<ApiTokenLookupRow> = sqlx::query_as(
        r#"SELECT id, user_id, is_active, expired_at FROM api_tokens WHERE token = $1"#,
    )
    .bind(presented)
    .fetch_optional(pool)
    .await
    .map_err(|_| Failure::server_error().into_response())?;
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
    sqlx::query(r#"UPDATE api_tokens SET last_used = $1 WHERE id = $2"#)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Failure::server_error().into_response())?;
    Ok(Caller::Api {
        user_id,
        raw: presented.to_owned(),
    })
}

/// One `machine_token` row for the lookup, with the linked dev-machine
/// revocation flag: `(id, user_id, workspace_id, revoked_at,
/// dev_machine_id, dev_machine_revoked, dev_machine_found)`.
type MachineTokenLookupRow = (
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
#[allow(clippy::result_large_err)]
async fn authenticate_machine(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    presented: &str,
) -> Result<Caller, Response> {
    let now = Utc::now();
    let token_hash = token_kernel::hash_token(presented, secret_key);
    let row: Option<MachineTokenLookupRow> = sqlx::query_as(
        r#"SELECT mt.id, mt.user_id, mt.workspace_id, mt.revoked_at,
                  mt.dev_machine_id, dm.revoked_at, dm.id
           FROM machine_token mt
           LEFT JOIN dev_machine dm ON dm.id = mt.dev_machine_id
           WHERE mt.token_hash = $1"#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| Failure::server_error().into_response())?;
    let Some((id, user_id, workspace_id, revoked_at, dev_machine_id, dm_revoked, dm_found)) = row
    else {
        return Err(invalid_token());
    };
    if revoked_at.is_some() {
        return Err(invalid_token());
    }
    // `machine_token.dev_machine_id is not None and
    // machine_token.dev_machine.revoked_at is not None`. A set id with no
    // row is the `select_related` 500 (unreachable without FK surgery).
    if let Some(machine_id) = dev_machine_id {
        if dm_found != Some(machine_id) {
            return Err(Failure::server_error().into_response());
        }
        if dm_revoked.is_some() {
            return Err(invalid_token());
        }
    }
    if !is_member(pool, workspace_id, user_id).await? {
        // `machine_token.revoke()` on the way out (`revoked_at` is `None`
        // here, so the guard is a no-op and the write always lands).
        sqlx::query(r#"UPDATE machine_token SET revoked_at = $1 WHERE id = $2"#)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|_| Failure::server_error().into_response())?;
        return Err(invalid_token());
    }
    // `MachineToken.objects.filter(pk=...).update(last_used_at=now)`.
    sqlx::query(r#"UPDATE machine_token SET last_used_at = $1 WHERE id = $2"#)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Failure::server_error().into_response())?;
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
    let exists: Option<bool> = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM workspace_members
           WHERE workspace_id = $1 AND member_id = $2 AND is_active)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Failure::server_error().into_response())?;
    Ok(exists.unwrap_or(false))
}

fn not_found(error: &str) -> Failure {
    Failure::bare_error(StatusCode::NOT_FOUND, error)
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
    let rows: Vec<(Option<String>, Option<String>)> = match sqlx::query_as(
        r#"SELECT w.slug, w.name FROM workspace_members m
           LEFT JOIN workspaces w ON w.id = m.workspace_id
           WHERE m.member_id = $1 AND m.is_active
           ORDER BY m.created_at"#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Failure::server_error().into_response(),
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
    // 404 (`device.py:446-451`).
    let workspace: Option<(Uuid, String)> =
        match sqlx::query_as(r#"SELECT id, slug FROM workspaces WHERE slug = $1"#)
            .bind(&params.workspace_slug)
            .fetch_optional(&pool)
            .await
        {
            Ok(row) => row,
            Err(_) => return Failure::server_error().into_response(),
        };
    let Some((workspace_id, workspace_slug)) = workspace else {
        return not_found("workspace_not_found").into_response();
    };
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
        &caller,
        user_id,
        workspace_id,
        &params,
        now,
    )
    .await;
    match outcome {
        Ok((dev_machine_id, host_label, raw)) => {
            if tx.commit().await.is_err() {
                return Failure::server_error().into_response();
            }
            json_response(
                StatusCode::CREATED,
                machine_token_response_body(&raw, &workspace_slug, &dev_machine_id, &host_label),
            )
        }
        Err(denial) => denial,
    }
}

/// The 201 body in Python dict order (`device.py:474-481`):
/// `machine_token`, `workspace_slug`, `dev_machine_id`, `host_label`.
fn machine_token_response_body(
    raw: &str,
    workspace_slug: &str,
    dev_machine_id: &Uuid,
    host_label: &str,
) -> String {
    let mut entry = serde_json::Map::with_capacity(4);
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
    serde_json::to_string(&serde_json::Value::Object(entry)).expect("machine-token body serializes")
}

/// The transactional core of the exchange: get-or-create the dev machine
/// (ownership conflicts are 404), revoke-then-mint the machine token,
/// and deactivate the `APIToken` bridge for API-key callers.
#[allow(clippy::result_large_err)]
async fn exchange_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    secret_key: &[u8],
    caller: &Caller,
    user_id: Uuid,
    workspace_id: Uuid,
    params: &MachineTokenParams,
    now: DateTime<Utc>,
) -> Result<(Uuid, String, String), Response> {
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
    sqlx::query(dev_machine::ROTATE_REVOKE_SQL)
        .bind(now)
        .bind(workspace_id)
        .bind(params.dev_machine_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| Failure::server_error().into_response())?;
    let raw = format!(
        "{}{}",
        token_kernel::MACHINE_TOKEN_PREFIX,
        token_urlsafe_32()
    );
    let token_id = Uuid::new_v4();
    sqlx::query(dev_machine::ROTATE_CREATE_SQL)
        .bind(token_id)
        .bind(user_id)
        .bind(params.dev_machine_id)
        .bind(workspace_id)
        .bind(dev_machine::rotate_host_label(&params.host_label))
        .bind(token_kernel::hash_token(&raw, secret_key))
        .bind(token_kernel::fingerprint(&raw))
        .bind(dev_machine::machine_token_label(&params.host_label))
        .bind(true)
        .bind(now)
        .bind(None::<DateTime<Utc>>)
        .bind(None::<DateTime<Utc>>)
        .execute(&mut **tx)
        .await
        .map_err(|_| Failure::server_error().into_response())?;
    // The bridge is an in-memory credential: deactivate it for API-key
    // callers, keep machine-token callers on their rotation
    // (`device.py:466-467`).
    if let Caller::Api { raw: bridge, .. } = caller {
        if cli_token::deactivate_api_token(&mut **tx, Some(bridge), true, now)
            .await
            .is_err()
        {
            return Err(Failure::server_error().into_response());
        }
    }
    Ok((params.dev_machine_id, params.host_label.clone(), raw))
}

// ---------------------------------------------------------------------------
// revoke (`device.py:485-511`)
// ---------------------------------------------------------------------------

/// `POST /api/v1/auth/revoke/`: invalidate the caller's CLI token.
/// Idempotent — a missing or already-inactive `APIToken` still answers
/// ok (`device.py:503-507`).
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
            // `machine_token.revoke()`: no-op when already revoked, else
            // stamp `revoked_at` (`runner/models.py:865-869`).
            if sqlx::query(
                r#"UPDATE machine_token SET revoked_at = $1 WHERE id = $2 AND revoked_at IS NULL"#,
            )
            .bind(Utc::now())
            .bind(token_id)
            .execute(&pool)
            .await
            .is_err()
            {
                return Failure::server_error().into_response();
            }
        }
        Caller::Api { raw, .. } => {
            // `request.auth` is the raw token string
            // (`api_authentication.py:62`); the row is re-read so a
            // missing or inactive token stays a silent ok.
            let row: Option<(Uuid, bool)> =
                match sqlx::query_as(r#"SELECT id, is_active FROM api_tokens WHERE token = $1"#)
                    .bind(&raw)
                    .fetch_optional(&pool)
                    .await
                {
                    Ok(row) => row,
                    Err(_) => return Failure::server_error().into_response(),
                };
            if let Some((id, true)) = row {
                if sqlx::query(
                    r#"UPDATE api_tokens SET is_active = false, updated_at = $1 WHERE id = $2"#,
                )
                .bind(Utc::now())
                .bind(id)
                .execute(&pool)
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
            machine_token_response_body("mt_RAW", "ws", &dm, "myhost"),
            r#"{"machine_token":"mt_RAW","workspace_slug":"ws","dev_machine_id":"10ecd282-5e9e-4616-8139-015323a3d96c","host_label":"myhost"}"#
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
