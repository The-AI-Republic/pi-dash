//! GitHub App install-flow handlers (D-33, stage 5, PIDASHCONV-443).
//!
//! Port of the five app endpoints in
//! `apps/api/pi_dash/app/views/integration/github.py` with routes from
//! `apps/api/pi_dash/app/urls/integration.py:31-56`:
//!
//! * `GithubAppStatusEndpoint` (`github.py:611-655`):
//!   `GET /api/users/me/integrations/github/app/`
//! * `GithubAppInstallStartEndpoint` (`github.py:658-695`):
//!   `POST /api/users/me/integrations/github/app/install/`
//! * `GithubAppRefreshEndpoint` (`github.py:698-727`):
//!   `POST /api/users/me/integrations/github/app/refresh/`
//! * `GithubAppCallbackEndpoint` (`github.py:730-808`, `AllowAny`):
//!   `GET /api/integrations/github/app/callback/`
//! * `GithubAppWebhookEndpoint` (`github.py:860-956`, `AllowAny` + HMAC +
//!   delivery dedupe): `POST /api/integrations/github/app/webhook/`
//!
//! The dependency layers are owned by sibling sub-issues and reused here,
//! never re-derived: permission gates + denial bodies
//! (`super::gates`, PIDASHCONV-436), the HMAC guard (`super::hmac`,
//! PIDASHCONV-436), read SQL + outbound-request shapes
//! (`pidash_db::app_integrations::queries_github`, PIDASHCONV-433), the
//! webhook serializers (services, PIDASHCONV-365), and the enqueue
//! boundary (PIDASHCONV-439 — the app flow enqueues nothing; the install
//! session lifecycle is synchronous DB writes).
//!
//! Fixtures: FX-GHA-01
//! (`rust-api/fixtures/app_integrations/fx-gha-01-app-flow.json`: callback
//! branches, install-start shape, delivery dedupe, event routing, HMAC
//! vectors, config rules) and FX-PERM-01 (the gate matrix; the two
//! `AllowAny` endpoints plus the manual admin checks).
//!
//! Outbound GitHub calls keep the fixture shapes (no live calls in tests):
//! the JWT claims, app/user headers, exchange body, and installation URLs
//! come from `queries_github`; transport is `reqwest` with
//! `DEFAULT_TIMEOUT_SECONDS`. Under test the dummy config makes every
//! outbound call fail, which is exactly the contract (`refresh` answers
//! 502 with the recorded error string; `callback` redirects with
//! `github_verification_failed`).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * QUIRK-callback-expiry (`github.py:765` vs `:173-176`): the callback
//!   treats `expires_at <= now` as expired while the lazy-cleanup sweep
//!   only marks `expires_at < now` — a session expiring at exactly `now`
//!   fails the callback but is not yet swept. Both operators are
//!   preserved as written (the sweep and the predicate both live here).
//! * QUIRK-broad-callback-except (`github.py:780-793`): ANY exception in
//!   the exchange/verify/fetch/upsert chain (network, auth, DB) answers
//!   `github_verification_failed` after logging. The handler catches the
//!   whole chain, not per-call errors.
//! * QUIRK-invalid-utf8-body (`github.py:887`): only `JSONDecodeError` is
//!   caught, so a non-UTF-8 webhook body escapes as a 500. Invalid UTF-8
//!   answers the 500 envelope here too.
//! * QUIRK-pr-state (`github_client.py:298`, fixture B6): the snapshot
//!   maps `state` to `closed` only on exact `"closed"` — `"merged"`
//!   renders as `open`. Ported as-is in [`pr_snapshot_from_payload`].
//! * QUIRK-terminal-passthrough (`github.py:763-764`): a callback replay
//!   for a non-`started` session redirects with `github_app=<status>`
//!   (`completed`/`expired`/`failed`), verified by the contract test's
//!   `failed` replay.
//! * QUIRK-title-truncation (`github_client.py:297`): snapshot titles cut
//!   at 500 code points (which can split a UTF-8 boundary in Python and
//!   raise); the cut here is char-boundary safe, which only differs on
//!   inputs that crash Django.
//! * NOTE-installation-token-cache: `revoke_installation_cache`
//!   (`github.py:939`) deletes a Django-cache row the Rust side never
//!   populates (no token cache exists in `serve`), so revocation is a
//!   documented no-op — there is nothing to evict.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::app_integrations::queries_github as queries;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{
    GITHUB_DISABLED_BODY, INSTALL_ADMIN_REQUIRED_BODY, REFRESH_ADMIN_REQUIRED_BODY,
    WEBHOOK_BAD_SIGNATURE_BODY,
};
use super::hmac::verify_webhook_signature;

/// The five owned paths. Every other method on each path proxies to
/// Django (DRF's 405-after-auth and metadata responses live there),
/// following the pilot pattern: registration is the cutover granularity.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/users/me/integrations/github/app/",
            owned(axum::routing::get(app_status), &["GET"]),
        )
        .route(
            "/api/users/me/integrations/github/app/install/",
            owned(axum::routing::post(install_start), &["POST"]),
        )
        .route(
            "/api/users/me/integrations/github/app/refresh/",
            owned(axum::routing::post(app_refresh), &["POST"]),
        )
        .route(
            "/api/integrations/github/app/callback/",
            owned(axum::routing::get(app_callback), &["GET"]),
        )
        .route(
            "/api/integrations/github/app/webhook/",
            owned(axum::routing::post(app_webhook), &["POST"]),
        )
}

/// An app-flow path: the owned methods serve from Rust while every other
/// method falls through to Django. `HEAD` proxies explicitly: axum would
/// auto-serve it from `get`, but Django defines no `head` and 405s after
/// auth. `OPTIONS` proxies so DRF metadata (401 anon / 200 authed) is
/// preserved.
fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Wire plumbing
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial on the session
/// endpoints (`views/base.py:189-194` runs before any handler logic).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;

/// DRF `get_object_or_404(Workspace, ...)` body.
pub const WORKSPACE_NOT_FOUND_BODY: &str = r#"{"detail":"No Workspace matches the given query."}"#;

/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    Unauthorized,
    ServerError,
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY),
            Denial::ServerError => (StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY),
        };
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

/// Compact-JSON response with an explicit status (`JSONRenderer` bytes:
/// no spaces, insertion order preserved via `preserve_order`).
fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// `HttpResponseRedirect` (`github.py:140-147`): 302 with a `Location`
/// header and an empty body.
fn redirect_response(location: String) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

/// `urlencode(params)` (`urllib.parse`, `quote_via=quote_plus`): joining
/// `key=value` pairs in call-site order. Slugs, states, and error codes
/// are URL-safe in practice; anything outside the unreserved set is
/// percent-encoded (space renders as `+`, exactly like `quote_plus`).
fn urlencode_query(pairs: &[(&str, &str)]) -> String {
    fn escape(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for byte in value.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    }
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", escape(key), escape(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `request.user` through Django-session auth on the three session
/// endpoints (`BaseAPIView`: session auth + `IsAuthenticated`,
/// `views/base.py:189-194`): anonymous answers the DRF
/// `NotAuthenticated` body before anything else runs.
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

/// The callback's optional actor (`AllowAny` skips DRF auth entirely,
/// `github.py:733`): a broken session reads as anonymous (the
/// `login_required` redirect), while a store failure is a 500.
async fn callback_actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Option<crate::license::Actor>, Denial> {
    let pool = pool_of(state)?;
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `install_session.expires_at.isoformat()` / DRF datetime rendering:
/// RFC 3339 with `+00:00` (Django never renders `Z`).
fn isoformat(value: &DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

/// `str(e)[:2000]` — code-point truncation, never splitting UTF-8.
fn truncate_error(message: &str) -> String {
    message.chars().take(2000).collect()
}

/// Whether a `payload.get(key) or {}` read fails the request
/// (`github.py:822-823,891-892`): a falsy value reads as `{}` (the
/// caller continues with no match); a truthy non-dict has no `.get`,
/// so Python raises and the request fails.
fn nested_shape_fails(value: &serde_json::Value) -> bool {
    json_truthy(Some(value)) && !value.is_object()
}

/// `(request.data.get("workspace_slug") or "").strip()`
/// (`github.py:670,704`): a missing/null/falsy value reads as `""` (the
/// `workspace_slug is required` 400); a truthy non-string, or a
/// non-object JSON body, raises `AttributeError` in Python (the 500
/// envelope). Unparseable bytes keep the historically answered 400
/// (DRF's `ParseError` detail is the documented deviation, as is
/// form-encoded input).
fn workspace_slug_from_body(body: &[u8]) -> Result<String, Denial> {
    let payload: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    let payload = match payload {
        None => return Ok(String::new()),
        Some(payload) => payload,
    };
    let object = match payload.as_object() {
        Some(object) => object,
        None => return Err(Denial::ServerError),
    };
    match object.get("workspace_slug") {
        None | Some(serde_json::Value::Null) => Ok(String::new()),
        Some(serde_json::Value::String(value)) => Ok(value.trim().to_owned()),
        Some(value) if !json_truthy(Some(value)) => Ok(String::new()),
        _ => Err(Denial::ServerError),
    }
}

// ---------------------------------------------------------------------------
// GitHub App config (`utils/github_app_auth.py:33-73`)
// ---------------------------------------------------------------------------

/// The six config keys (`get_github_app_config`): `app_id`/`app_slug`/
/// `client_id` from `instance_configurations` (db source), `private_key`/
/// `webhook_secret`/`client_secret` from the server environment (secret-env
/// source). Reads go through the registry + `PgConfigStore` exactly like
/// the license handlers, so per-key sources stay identical to Django.
#[derive(Debug)]
struct GithubAppConfig {
    app_id: String,
    app_slug: String,
    private_key: String,
    webhook_secret: String,
    client_id: String,
    client_secret: String,
}

const CONFIG_KEYS: &[&str] = &[
    "GITHUB_APP_ID",
    "GITHUB_APP_SLUG",
    "GITHUB_APP_PRIVATE_KEY",
    "GITHUB_APP_WEBHOOK_SECRET",
    "GITHUB_APP_CLIENT_ID",
    "GITHUB_APP_CLIENT_SECRET",
];

/// `get_github_app_config` (`github_app_auth.py:33-51`): strip every
/// value; the private key normalizes escaped newlines
/// (`_normalize_private_key`, `:54-60`). A store failure escapes to the
/// base 500, exactly like a DB error in Python.
async fn github_app_config(pool: &PgPool, secret_key: &str) -> Result<GithubAppConfig, Denial> {
    use pidash_db::config::{ConfigValue, PgConfigStore};
    let store = PgConfigStore::new(pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = pidash_services::license::encryption::Keyring::from_secret(secret_key);
    let values = pidash_db::config::accessor::get_many_in(registry, &store, &keyring, CONFIG_KEYS)
        .await
        .map_err(|_| Denial::ServerError)?;
    let string = |key: &str| match values.get(key) {
        Some(ConfigValue::Str(value)) => value.clone(),
        Some(ConfigValue::Int(value)) => value.to_string(),
        Some(ConfigValue::Float(value)) => value.to_string(),
        Some(ConfigValue::Bool(true)) => "True".to_owned(),
        Some(ConfigValue::Bool(false)) => "False".to_owned(),
        _ => String::new(),
    };
    Ok(GithubAppConfig {
        app_id: string("GITHUB_APP_ID").trim().to_owned(),
        app_slug: string("GITHUB_APP_SLUG").trim().to_owned(),
        private_key: queries::normalize_private_key(Some(string("GITHUB_APP_PRIVATE_KEY").trim())),
        webhook_secret: string("GITHUB_APP_WEBHOOK_SECRET").trim().to_owned(),
        client_id: string("GITHUB_APP_CLIENT_ID").trim().to_owned(),
        client_secret: string("GITHUB_APP_CLIENT_SECRET").trim().to_owned(),
    })
}

/// `require_github_app_config` (`github_app_auth.py:63-73`): the missing
/// key names in required order (`app_id`, `app_slug`, `private_key`,
/// then `client_id`/`client_secret` for `oauth`, then `webhook_secret`
/// for `webhook`), or the valid config.
fn require_config(
    config: GithubAppConfig,
    oauth: bool,
    webhook: bool,
) -> Result<GithubAppConfig, String> {
    let mut required = vec!["app_id", "app_slug", "private_key"];
    if oauth {
        required.extend(["client_id", "client_secret"]);
    }
    if webhook {
        required.push("webhook_secret");
    }
    let value = |key: &str| match key {
        "app_id" => config.app_id.as_str(),
        "app_slug" => config.app_slug.as_str(),
        "private_key" => config.private_key.as_str(),
        "client_id" => config.client_id.as_str(),
        "client_secret" => config.client_secret.as_str(),
        "webhook_secret" => config.webhook_secret.as_str(),
        _ => "",
    };
    let missing: Vec<&str> = required
        .into_iter()
        .filter(|key| value(key).is_empty())
        .collect();
    if missing.is_empty() {
        Ok(config)
    } else {
        Err(format!("GitHub App config missing: {}", missing.join(", ")))
    }
}

/// `configured` for the status endpoint (`github.py:620-627`): all six
/// keys non-empty (the full set, not the `require` subset).
fn status_configured(config: &GithubAppConfig) -> bool {
    !config.app_id.is_empty()
        && !config.app_slug.is_empty()
        && !config.private_key.is_empty()
        && !config.webhook_secret.is_empty()
        && !config.client_id.is_empty()
        && !config.client_secret.is_empty()
}

// ---------------------------------------------------------------------------
// Shared reads
// ---------------------------------------------------------------------------

/// One `workspaces` row identity for slug lookups.
struct WorkspaceRef {
    id: Uuid,
    slug: String,
}

/// `get_object_or_404(Workspace, slug=...)` with the default manager's
/// `deleted_at IS NULL` scope. A miss answers
/// `{"detail": "No Workspace matches the given query."}` (404).
#[allow(clippy::result_large_err)]
async fn workspace_by_slug(pool: &PgPool, slug: &str) -> Result<WorkspaceRef, Response> {
    let row = sqlx::query(
        r#"SELECT "id", "slug" FROM "workspaces" WHERE "deleted_at" IS NULL AND "slug" = $1 ORDER BY "id" ASC LIMIT 1"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    match row {
        Some(row) => Ok(WorkspaceRef {
            id: row
                .try_get("id")
                .map_err(|_| Denial::ServerError.into_response())?,
            slug: row
                .try_get("slug")
                .map_err(|_| Denial::ServerError.into_response())?,
        }),
        None => Err(json_response(
            StatusCode::NOT_FOUND,
            WORKSPACE_NOT_FOUND_BODY.to_owned(),
        )),
    }
}

/// `Workspace.objects.get(pk=...)` for the callback success redirect
/// (the session's `select_related("workspace")`, `github.py:752`).
async fn workspace_by_id(pool: &PgPool, id: Uuid) -> Result<WorkspaceRef, Denial> {
    let row = sqlx::query(
        r#"SELECT "id", "slug" FROM "workspaces" WHERE "deleted_at" IS NULL AND "id" = $1 LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row {
        Some(row) => Ok(WorkspaceRef {
            id: row.try_get("id").map_err(|_| Denial::ServerError)?,
            slug: row.try_get("slug").map_err(|_| Denial::ServerError)?,
        }),
        None => Err(Denial::ServerError),
    }
}

/// `_get_workspace_integration(workspace)` (`github.py:102-106`): the
/// github `Integration` row, then the workspace's row for it.
async fn workspace_integration(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Option<queries::WorkspaceIntegrationRow>, Denial> {
    let integration_id: Option<Uuid> = sqlx::query(&queries::integration_by_provider_sql())
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .map(|row| row.try_get("id"))
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    match integration_id {
        None => Ok(None),
        Some(integration_id) => {
            queries::fetch_workspace_integration(pool, workspace_id, integration_id)
                .await
                .map_err(|_| Denial::ServerError)
        }
    }
}

/// The workspace's `GithubAppInstallation` row through the OneToOne
/// `workspace_integration.github_app_installation` (`github.py:636-638`).
async fn app_installation_for(
    pool: &PgPool,
    workspace_integration_id: Uuid,
) -> Result<
    Option<pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation>,
    Denial,
> {
    let row = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_integration_id", "installation_id", "account_login", "account_type", "repository_selection", "repository_count", "permissions", "events", "installed_at", "suspended_at", "verified_at", "last_checked_at", "last_check_error" FROM "github_app_installations" WHERE "deleted_at" IS NULL AND "workspace_integration_id" = $1 LIMIT 1"#,
    )
    .bind(workspace_integration_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    app_installation_row(row).map_err(|_| Denial::ServerError)
}

/// `_lazy_cleanup_install_sessions` (`github.py:170-188`): mark
/// `started` rows past their expiry as `expired` (strict `<`, NOT the
/// callback's `<=` — see the module docs), then hard-delete terminal
/// rows untouched for 7 days. Errors are logged and swallowed, never
/// raised — the callers always continue.
async fn lazy_cleanup_install_sessions(pool: &PgPool) {
    let expired: Result<_, sqlx::Error> = sqlx::query(
        r#"UPDATE "github_app_install_sessions" SET "status" = 'expired', "error" = 'Install session expired' WHERE "deleted_at" IS NULL AND "status" = 'started' AND "expires_at" < now()"#,
    )
    .execute(pool)
    .await;
    let deleted: Result<_, sqlx::Error> = sqlx::query(
        r#"DELETE FROM "github_app_install_sessions" WHERE "status" IN ('completed', 'expired', 'failed') AND "updated_at" < now() - interval '7 days'"#,
    )
    .execute(pool)
    .await;
    match (expired, deleted) {
        (Ok(expired), Ok(deleted)) => {
            if expired.rows_affected() > 0 || deleted.rows_affected() > 0 {
                tracing::info!(
                    "GitHub App install session cleanup: expired={}, deleted={}",
                    expired.rows_affected(),
                    deleted.rows_affected(),
                );
            }
        }
        (Err(error), _) | (_, Err(error)) => {
            tracing::warn!("GitHub App install session cleanup failed: {error}");
        }
    }
}

/// `_serialize_app_installation` (`github.py:150-167`): the connected
/// installation shape, or `{"connected": false}`. Key order is the
/// serializer order; datetimes render `isoformat()` or `null`.
fn serialize_app_installation(
    installation: Option<
        &pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation,
    >,
) -> serde_json::Value {
    use serde_json::{Map, Value};
    let Some(row) = installation else {
        let mut disconnected = Map::with_capacity(1);
        disconnected.insert("connected".to_owned(), Value::Bool(false));
        return Value::Object(disconnected);
    };
    let optional = |value: &Option<DateTime<Utc>>| match value {
        Some(value) => Value::String(isoformat(value)),
        None => Value::Null,
    };
    let mut body = Map::with_capacity(13);
    body.insert("connected".to_owned(), Value::Bool(true));
    body.insert(
        "installation_id".to_owned(),
        Value::Number(row.installation_id.into()),
    );
    body.insert(
        "account_login".to_owned(),
        Value::String(row.account_login.clone()),
    );
    body.insert(
        "account_type".to_owned(),
        Value::String(row.account_type.clone()),
    );
    body.insert(
        "repository_selection".to_owned(),
        Value::String(row.repository_selection.clone()),
    );
    body.insert(
        "repository_count".to_owned(),
        Value::Number(row.repository_count.into()),
    );
    body.insert("permissions".to_owned(), row.permissions.clone());
    body.insert("events".to_owned(), row.events.clone());
    body.insert("installed_at".to_owned(), optional(&row.installed_at));
    body.insert("suspended_at".to_owned(), optional(&row.suspended_at));
    body.insert("verified_at".to_owned(), optional(&row.verified_at));
    body.insert("last_checked_at".to_owned(), optional(&row.last_checked_at));
    body.insert(
        "last_check_error".to_owned(),
        Value::String(row.last_check_error.clone()),
    );
    Value::Object(body)
}

// ---------------------------------------------------------------------------
// GET /api/users/me/integrations/github/app/ (`github.py:611-655`)
// ---------------------------------------------------------------------------

/// Authenticated status read: the six-key `configured` bit, the app slug,
/// and every ADMIN-active workspace (ordered by workspace name) with its
/// installation shape.
async fn app_status(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let user = match actor(&state, extension).await {
        Ok(user) => user,
        Err(denial) => return denial.into_response(),
    };
    if !state.settings().github_sync_enabled {
        return json_response(StatusCode::NOT_FOUND, GITHUB_DISABLED_BODY.to_owned());
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    lazy_cleanup_install_sessions(pool).await;
    let config = match github_app_config(pool, state.settings().secret_key.as_str()).await {
        Ok(config) => config,
        Err(denial) => return denial.into_response(),
    };
    let memberships = match queries::fetch_admin_memberships(pool, user.id).await {
        Ok(memberships) => memberships,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut workspaces = Vec::with_capacity(memberships.len());
    for membership in &memberships {
        let installation = match workspace_integration(pool, membership.workspace_id).await {
            Ok(Some(wi)) => match app_installation_for(pool, wi.id).await {
                Ok(installation) => installation,
                Err(denial) => return denial.into_response(),
            },
            Ok(None) => None,
            Err(denial) => return denial.into_response(),
        };
        workspaces.push(serde_json::json!({
            "id": membership.workspace_id.to_string(),
            "slug": membership.workspace_slug,
            "name": membership.workspace_name,
            "github_app": serialize_app_installation(installation.as_ref()),
        }));
    }
    // `serde_json::json!` preserves key order (`preserve_order`); the
    // envelope order is `configured`, `app_slug`, `workspaces`
    // (`github.py:648-654`).
    let body = serde_json::json!({
        // `config.get("app_slug") or ""` (`github.py:651`).
        "configured": status_configured(&config),
        "app_slug": config.app_slug,
        "workspaces": workspaces,
    });
    json_response(
        StatusCode::OK,
        serde_json::to_string(&body).expect("serializable status body"),
    )
}

// ---------------------------------------------------------------------------
// POST /api/users/me/integrations/github/app/install/ (`github.py:658-695`)
// ---------------------------------------------------------------------------

/// `secrets.token_urlsafe(32)`: 32 random bytes, URL-safe base64, padding
/// stripped — 43 characters. Two v4 UUIDs supply the 32 bytes (both from
/// the OS CSPRNG, like `os.urandom`).
fn install_state() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(first.as_bytes());
    bytes[16..].copy_from_slice(second.as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Authenticated install start: slug validation, workspace lookup, the
/// manual admin check, session insert, and the 201 with the GitHub
/// install URL carrying the session state.
async fn install_start(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let user = match actor(&state, extension).await {
        Ok(user) => user,
        Err(denial) => return denial.into_response(),
    };
    if !state.settings().github_sync_enabled {
        return json_response(StatusCode::NOT_FOUND, GITHUB_DISABLED_BODY.to_owned());
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    lazy_cleanup_install_sessions(pool).await;
    let config = match github_app_config(pool, state.settings().secret_key.as_str()).await {
        Ok(config) => config,
        Err(denial) => return denial.into_response(),
    };
    // `require_github_app_config(oauth=True, webhook=True)`
    // (`github.py:665-668`): 409 with the missing names.
    let config = match require_config(config, true, true) {
        Ok(config) => config,
        Err(message) => {
            return json_response(
                StatusCode::CONFLICT,
                serde_json::to_string(&serde_json::json!({"error": message}))
                    .expect("serializable config error"),
            );
        }
    };
    let workspace_slug = match workspace_slug_from_body(&body) {
        Ok(slug) => slug,
        Err(denial) => return denial.into_response(),
    };
    if workspace_slug.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"workspace_slug is required"}"#.to_owned(),
        );
    }
    let workspace = match workspace_by_slug(pool, &workspace_slug).await {
        Ok(workspace) => workspace,
        Err(response) => return response,
    };
    if !match queries::fetch_is_workspace_admin(pool, user.id, workspace.id).await {
        Ok(admin) => admin,
        Err(_) => return Denial::ServerError.into_response(),
    } {
        return json_response(
            StatusCode::FORBIDDEN,
            INSTALL_ADMIN_REQUIRED_BODY.to_owned(),
        );
    }
    let session_state = install_state();
    let row = sqlx::query(
        r#"INSERT INTO "github_app_install_sessions" ("id", "created_at", "updated_at", "state", "workspace_id", "actor_id", "installation_id", "account_login", "status", "expires_at", "completed_at", "error") VALUES ($1, now(), now(), $2, $3, $4, NULL, '', 'started', now() + interval '15 minutes', NULL, '') RETURNING "state", "expires_at""#,
    )
    .bind(Uuid::new_v4())
    .bind(&session_state)
    .bind(workspace.id)
    .bind(user.id)
    .fetch_one(pool)
    .await;
    let row = match row {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let returned_state: String = match row.try_get("state") {
        Ok(value) => value,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let expires_at: DateTime<Utc> = match row.try_get("expires_at") {
        Ok(value) => value,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // `f"https://github.com/apps/{slug}/installations/new?{urlencode(...)}"`
    // (`github.py:687`): the state alphabet needs no encoding, so the
    // query renders as `state=<state>`.
    let install_url = format!(
        "https://github.com/apps/{}/installations/new?{}",
        config.app_slug,
        urlencode_query(&[("state", &returned_state)]),
    );
    let response_body = serde_json::json!({
        "state": returned_state,
        "expires_at": isoformat(&expires_at),
        "install_url": install_url,
    });
    json_response(
        StatusCode::CREATED,
        serde_json::to_string(&response_body).expect("serializable install body"),
    )
}

// ---------------------------------------------------------------------------
// Outbound GitHub transport (`utils/github_app_auth.py`, no live calls in
// tests). Shapes (URLs, headers, bodies) come from `queries_github`; only
// the `reqwest` execution lives here.
// ---------------------------------------------------------------------------

/// Any transport/auth failure in the app-flow outbound calls.
#[derive(Debug)]
struct GithubTransportError(String);

impl std::fmt::Display for GithubTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// `build_app_jwt` (`github_app_auth.py:76-85`): RS256 over the claims
/// shape (`iat = now - 60`, `exp = now + 540`, `iss = app_id`).
fn build_app_jwt(config: &GithubAppConfig) -> Result<String, GithubTransportError> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| GithubTransportError(error.to_string()))?
        .as_secs() as i64;
    let claims = queries::build_app_jwt_claims(now, &config.app_id);
    let key = EncodingKey::from_rsa_pem(config.private_key.as_bytes())
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".to_owned());
    #[derive(serde::Serialize)]
    struct Claims {
        iat: i64,
        exp: i64,
        iss: String,
    }
    encode(
        &header,
        &Claims {
            iat: claims.iat,
            exp: claims.exp,
            iss: claims.iss,
        },
        &key,
    )
    .map_err(|error| GithubTransportError(error.to_string()))
}

fn github_client() -> Result<reqwest::Client, GithubTransportError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            queries::DEFAULT_TIMEOUT_SECONDS,
        ))
        .build()
        .map_err(|error| GithubTransportError(error.to_string()))
}

/// `GithubClient.for_installation(id).list_installation_repositories()`
/// as used by `_refresh_app_installation` (`github.py:208-211`): mint an
/// installation token, then read the first repos page's `total_count`.
async fn list_installation_repository_count(
    config: &GithubAppConfig,
    installation_id: i64,
) -> Result<i32, GithubTransportError> {
    let jwt = build_app_jwt(config)?;
    let client = github_client()?;
    let token_response = client
        .post(format!(
            "{}/app/installations/{}/access_tokens",
            queries::GITHUB_API_BASE,
            installation_id
        ))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", queries::GITHUB_ACCEPT_HEADER)
        .header("X-GitHub-Api-Version", queries::GITHUB_API_VERSION_HEADER)
        .header("User-Agent", queries::GITHUB_USER_AGENT)
        .send()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let status = token_response.status();
    if !status.is_success() {
        return Err(GithubTransportError(format!(
            "GitHub installation token request failed: {status}"
        )));
    }
    let payload: serde_json::Value = token_response
        .bytes()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| GithubTransportError(error.to_string()))
        })?;
    let token = payload
        .get("token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if token.is_empty() {
        return Err(GithubTransportError(
            "GitHub did not return an installation token".to_owned(),
        ));
    }
    let repos_response = client
        .get(format!(
            "{}/installation/repositories?per_page=100&page=1",
            queries::GITHUB_API_BASE
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", queries::GITHUB_ACCEPT_HEADER)
        .header("X-GitHub-Api-Version", queries::GITHUB_API_VERSION_HEADER)
        .header("User-Agent", queries::GITHUB_USER_AGENT)
        .send()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let status = repos_response.status();
    if !status.is_success() {
        return Err(GithubTransportError(format!(
            "GitHub installation repositories request failed: {status}"
        )));
    }
    let payload: serde_json::Value = repos_response
        .bytes()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| GithubTransportError(error.to_string()))
        })?;
    Ok(payload
        .get("total_count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0) as i32)
}

/// `_refresh_app_installation` (`github.py:191-226`): verify upstream and
/// stamp `repository_count`/`verified_at`/`last_checked_at`, clearing the
/// error — or stamp the failure, save it, and with `raise_on_error`
/// report `VerificationFailed` carrying the stamped error string (the
/// refresh 502 body). A failing `save` escapes as `StoreFailed` (the
/// base 500, exactly like a DB error inside the Python `except` block).
#[derive(Debug)]
enum RefreshOutcome {
    VerificationFailed(String),
    StoreFailed,
}

impl From<GithubTransportError> for RefreshOutcome {
    fn from(_: GithubTransportError) -> Self {
        RefreshOutcome::StoreFailed
    }
}

async fn refresh_app_installation(
    connection: &mut sqlx::PgConnection,
    installation: &pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation,
    config: &GithubAppConfig,
    raise_on_error: bool,
    extra_update_fields: &[&str],
) -> Result<
    pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation,
    RefreshOutcome,
> {
    let mut refreshed = installation.clone();
    match list_installation_repository_count(config, installation.installation_id).await {
        Ok(repository_count) => {
            let now = Utc::now();
            refreshed.repository_count = repository_count;
            refreshed.verified_at = Some(now);
            refreshed.last_checked_at = Some(now);
            refreshed.last_check_error = String::new();
        }
        Err(error) => {
            // `except Exception` (`github.py:216-223`): stamp the
            // failure, log, save, and raise only when asked.
            refreshed.last_checked_at = Some(Utc::now());
            refreshed.last_check_error = truncate_error(&error.to_string());
            save_installation_refresh(&mut *connection, &refreshed, extra_update_fields)
                .await
                .map_err(|_| RefreshOutcome::StoreFailed)?;
            if raise_on_error {
                return Err(RefreshOutcome::VerificationFailed(
                    refreshed.last_check_error.clone(),
                ));
            }
            return Ok(refreshed);
        }
    }
    save_installation_refresh(&mut *connection, &refreshed, extra_update_fields)
        .await
        .map_err(|_| RefreshOutcome::StoreFailed)?;
    Ok(refreshed)
}

/// The `save(update_fields=[repository_count, verified_at,
/// last_checked_at, last_check_error, updated_at, ...extra])`
/// (`github.py:198-206,220,225`).
async fn save_installation_refresh(
    connection: &mut sqlx::PgConnection,
    installation: &pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation,
    extra_update_fields: &[&str],
) -> Result<(), GithubTransportError> {
    // `update_fields` always carries the five refresh columns; the only
    // `extra_update_fields` value in the codebase is `"suspended_at"`
    // (the unsuspend path, `github.py:942`), which the in-memory row
    // already carries — so every refresh writes the same six columns.
    let _ = extra_update_fields;
    sqlx::query(
        r#"UPDATE "github_app_installations" SET "repository_count" = $1, "verified_at" = $2, "last_checked_at" = $3, "last_check_error" = $4, "suspended_at" = $5, "updated_at" = now() WHERE "id" = $6"#,
    )
    .bind(installation.repository_count)
    .bind(installation.verified_at)
    .bind(installation.last_checked_at)
    .bind(&installation.last_check_error)
    .bind(installation.suspended_at)
    .bind(installation.id)
    .execute(&mut *connection)
    .await
    .map(|_| ())
    .map_err(|error| GithubTransportError(error.to_string()))
}

// ---------------------------------------------------------------------------
// POST /api/users/me/integrations/github/app/refresh/ (`github.py:698-727`)
// ---------------------------------------------------------------------------

/// Authenticated refresh: slug validation, the manual admin check, the
/// installed check, then the live verification whose failure answers 502
/// with the recorded error string.
async fn app_refresh(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let user = match actor(&state, extension).await {
        Ok(user) => user,
        Err(denial) => return denial.into_response(),
    };
    if !state.settings().github_sync_enabled {
        return json_response(StatusCode::NOT_FOUND, GITHUB_DISABLED_BODY.to_owned());
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let workspace_slug = match workspace_slug_from_body(&body) {
        Ok(slug) => slug,
        Err(denial) => return denial.into_response(),
    };
    if workspace_slug.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"workspace_slug is required"}"#.to_owned(),
        );
    }
    let workspace = match workspace_by_slug(pool, &workspace_slug).await {
        Ok(workspace) => workspace,
        Err(response) => return response,
    };
    if !match queries::fetch_is_workspace_admin(pool, user.id, workspace.id).await {
        Ok(admin) => admin,
        Err(_) => return Denial::ServerError.into_response(),
    } {
        return json_response(
            StatusCode::FORBIDDEN,
            REFRESH_ADMIN_REQUIRED_BODY.to_owned(),
        );
    }
    let installation = match workspace_integration(pool, workspace.id).await {
        Ok(Some(wi)) => match app_installation_for(pool, wi.id).await {
            Ok(installation) => installation,
            Err(denial) => return denial.into_response(),
        },
        Ok(None) => None,
        Err(denial) => return denial.into_response(),
    };
    let Some(installation) = installation else {
        return json_response(
            StatusCode::NOT_FOUND,
            r#"{"error":"GitHub App is not installed for this workspace"}"#.to_owned(),
        );
    };
    // The refresh path needs no stored secrets beyond the private key,
    // but the config read stays unconditional (a store failure is the
    // base 500, exactly like Django).
    let config = match github_app_config(pool, state.settings().secret_key.as_str()).await {
        Ok(config) => config,
        Err(denial) => return denial.into_response(),
    };
    // Autocommit here, exactly like Django (the refresh endpoint wraps
    // nothing in `transaction.atomic`): the verification stamps its row
    // and answers, with no surrounding transaction to roll back.
    let mut connection = match pool.acquire().await {
        Ok(connection) => connection,
        Err(_) => return Denial::ServerError.into_response(),
    };
    match refresh_app_installation(&mut connection, &installation, &config, true, &[]).await {
        Ok(refreshed) => {
            let body = serialize_app_installation(Some(&refreshed));
            json_response(
                StatusCode::OK,
                serde_json::to_string(&body).expect("serializable refresh body"),
            )
        }
        // `except GithubAppAuthError` (`github.py:722-726`): the in-memory
        // row already carries the stamped failure
        // (`app_installation.last_check_error or "Failed to verify..."`).
        Err(RefreshOutcome::VerificationFailed(stamped)) => {
            let message = if stamped.is_empty() {
                "Failed to verify GitHub App installation".to_owned()
            } else {
                stamped
            };
            json_response(
                StatusCode::BAD_GATEWAY,
                serde_json::to_string(&serde_json::json!({"error": message}))
                    .expect("serializable refresh error"),
            )
        }
        Err(RefreshOutcome::StoreFailed) => Denial::ServerError.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Callback outbound chain (`github.py:780-793`)
// ---------------------------------------------------------------------------

/// `exchange_user_code(code)` (`github_app_auth.py:112-131`): the OAuth
/// form POST, then the token (or the `error_description`/`error`/default
/// selection when the payload carries none).
async fn exchange_user_code(
    config: &GithubAppConfig,
    code: &str,
) -> Result<String, GithubTransportError> {
    let shape = queries::exchange_user_code_shape(&config.client_id, &config.client_secret, code);
    let client = github_client()?;
    let response = client
        .post(&shape.url)
        .header("Accept", shape.accept)
        .form(&[
            ("client_id", shape.client_id.as_str()),
            ("client_secret", shape.client_secret.as_str()),
            ("code", shape.code.as_str()),
        ])
        .send()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(GithubTransportError(format!(
            "GitHub OAuth exchange failed: {status}"
        )));
    }
    let payload: serde_json::Value = response
        .bytes()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| GithubTransportError(error.to_string()))
        })?;
    match payload
        .get("access_token")
        .and_then(serde_json::Value::as_str)
    {
        Some(token) if !token.is_empty() => Ok(token.to_owned()),
        _ => Err(GithubTransportError(queries::exchange_missing_token_error(
            payload
                .get("error_description")
                .and_then(serde_json::Value::as_str),
            payload.get("error").and_then(serde_json::Value::as_str),
        ))),
    }
}

/// `verify_user_can_access_installation` (`github_app_auth.py:151-155`):
/// page `GET user/installations` following `Link: rel="next"` until the
/// installation id appears (or the pages run out).
async fn verify_user_can_access_installation(
    user_token: &str,
    installation_id: i64,
) -> Result<Option<serde_json::Value>, GithubTransportError> {
    let client = github_client()?;
    let mut url = Some(queries::user_installations_first_page_url());
    while let Some(next) = url {
        let response = client
            .get(&next)
            .header("Authorization", format!("Bearer {user_token}"))
            .header("Accept", queries::GITHUB_ACCEPT_HEADER)
            .header("X-GitHub-Api-Version", queries::GITHUB_API_VERSION_HEADER)
            .header("User-Agent", queries::GITHUB_USER_AGENT)
            .send()
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(GithubTransportError(format!(
                "GitHub user installations request failed: {status}"
            )));
        }
        let link = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let payload: serde_json::Value = response
            .bytes()
            .await
            .map_err(|error| GithubTransportError(error.to_string()))
            .and_then(|bytes| {
                serde_json::from_slice(&bytes)
                    .map_err(|error| GithubTransportError(error.to_string()))
            })?;
        let installations = payload
            .get("installations")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for installation in &installations {
            if installation
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
                == installation_id
            {
                return Ok(Some(installation.clone()));
            }
        }
        url = queries::next_installations_page(&link);
    }
    Ok(None)
}

/// `get_installation(id)` (`github_app_auth.py:158-165`): the app-JWT
/// read of one installation.
async fn get_installation(
    config: &GithubAppConfig,
    installation_id: i64,
) -> Result<serde_json::Value, GithubTransportError> {
    let jwt = build_app_jwt(config)?;
    let client = github_client()?;
    let response = client
        .get(format!(
            "{}/app/installations/{}",
            queries::GITHUB_API_BASE,
            installation_id
        ))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", queries::GITHUB_ACCEPT_HEADER)
        .header("X-GitHub-Api-Version", queries::GITHUB_API_VERSION_HEADER)
        .header("User-Agent", queries::GITHUB_USER_AGENT)
        .send()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(GithubTransportError(format!(
            "GitHub installation request failed: {status}"
        )));
    }
    response
        .bytes()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| GithubTransportError(error.to_string()))
        })
}

/// `_get_or_create_workspace_integration` (`github.py:109-128`):
/// `_get_or_create_github_integration` (`github.py:90-99`) first, then
/// the workspace row (created with an inactive `api_tokens` FK shim
/// when missing).
async fn get_or_create_workspace_integration(
    connection: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    actor_id: Uuid,
) -> Result<queries::WorkspaceIntegrationRow, GithubTransportError> {
    // `_get_or_create_github_integration` (`github.py:90-99`).
    let integration_id: Uuid = match sqlx::query(&queries::integration_by_provider_sql())
        .fetch_optional(&mut *connection)
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?
        .map(|row| row.try_get::<Uuid, _>("id"))
        .transpose()
        .map_err(|error| GithubTransportError(error.to_string()))?
    {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO "integrations" ("id", "title", "provider", "network", "description", "author", "webhook_url", "webhook_secret", "redirect_url", "metadata", "verified", "created_at", "updated_at") VALUES ($1, 'GitHub', 'github', 1, '{"summary": "Mirror GitHub issues into Pi Dash projects."}', '', '', '', '', '{}', true, now(), now())"#,
            )
            .bind(id)
            .execute(&mut *connection)
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?;
            id
        }
    };
    if let Some(existing) =
        queries::fetch_workspace_integration(&mut *connection, workspace_id, integration_id)
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?
    {
        return Ok(existing);
    }
    let token_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO "api_tokens" ("id", "created_at", "updated_at", "label", "description", "is_active", "last_used", "token", "user_id", "user_type", "workspace_id", "expired_at", "is_service", "allowed_rate_limit") VALUES ($1, now(), now(), $2, 'GitHub integration FK shim — not for auth', false, NULL, $3, $4, 1, $5, NULL, false, '60/min')"#,
    )
    .bind(token_id)
    .bind(format!("github-integration-{workspace_id}"))
    .bind(format!("github-shim-{}", Uuid::new_v4()))
    .bind(actor_id)
    .bind(workspace_id)
    .execute(&mut *connection)
    .await
    .map_err(|error| GithubTransportError(error.to_string()))?;
    let wi_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO "workspace_integrations" ("id", "metadata", "config", "actor_id", "api_token_id", "integration_id", "workspace_id", "created_at", "updated_at") VALUES ($1, '{}', '{}', $2, $3, $4, $5, now(), now())"#,
    )
    .bind(wi_id)
    .bind(actor_id)
    .bind(token_id)
    .bind(integration_id)
    .bind(workspace_id)
    .execute(&mut *connection)
    .await
    .map_err(|error| GithubTransportError(error.to_string()))?;
    Ok(queries::WorkspaceIntegrationRow {
        id: wi_id,
        workspace_id,
        integration_id,
        config: serde_json::json!({}),
    })
}

/// `_upsert_git_account_for_app` (`github.py:314-352`): the
/// `git_provider_accounts` companion row for an installation
/// (`update_or_create` on workspace/provider/host/auth/external id).
async fn upsert_git_account_for_app(
    connection: &mut sqlx::PgConnection,
    installation: &pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation,
) -> Result<(), GithubTransportError> {
    let status = if installation.suspended_at.is_some() {
        "degraded"
    } else {
        "connected"
    };
    let display_name = if installation.account_login.is_empty() {
        format!("GitHub App {}", installation.installation_id)
    } else {
        installation.account_login.clone()
    };
    let existing: Option<Uuid> = sqlx::query(
        r#"SELECT "id" FROM "git_provider_accounts" WHERE "deleted_at" IS NULL AND "workspace_id" = $1 AND "provider" = 'github' AND "host_url" = 'https://github.com' AND "auth_type" = 'github_app' AND "external_account_id" = $2 LIMIT 1"#,
    )
    .bind(
        sqlx::query(
            r#"SELECT "workspace_id" FROM "workspace_integrations" WHERE "deleted_at" IS NULL AND "id" = $1 LIMIT 1"#,
        )
        .bind(installation.workspace_integration_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?
        .map(|row| row.try_get::<Uuid, _>("workspace_id"))
        .transpose()
        .map_err(|error| GithubTransportError(error.to_string()))?
        .ok_or_else(|| GithubTransportError("workspace integration missing".to_owned()))?,
    )
    .bind(installation.installation_id.to_string())
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| GithubTransportError(error.to_string()))?
    .map(|row| row.try_get("id"))
    .transpose()
    .map_err(|error| GithubTransportError(error.to_string()))?;
    let capabilities = serde_json::json!({
        "read_repositories": true,
        "read_issues": true,
        "write_comments": false,
        "manage_webhooks": true,
        "clone": false,
    });
    let credential_config = serde_json::json!({
        "auth_type": "github_app",
        "host_url": "https://github.com",
        "installation_id": installation.installation_id,
    });
    let metadata = serde_json::json!({
        "permissions": installation.permissions,
        "events": installation.events,
        "repository_selection": installation.repository_selection,
        "repository_count": installation.repository_count,
    });
    match existing {
        Some(account_id) => {
            sqlx::query(
                r#"UPDATE "git_provider_accounts" SET "external_account_login" = $1, "display_name" = $2, "capabilities" = $3, "credential_config" = $4, "workspace_integration_id" = $5, "status" = $6, "verified_at" = $7, "last_check_error" = $8, "metadata" = $9, "updated_at" = now() WHERE "id" = $10"#,
            )
            .bind(&installation.account_login)
            .bind(&display_name)
            .bind(&capabilities)
            .bind(&credential_config)
            .bind(installation.workspace_integration_id)
            .bind(status)
            .bind(installation.verified_at)
            .bind(&installation.last_check_error)
            .bind(&metadata)
            .bind(account_id)
        }
        None => {
            let workspace_id: Uuid = sqlx::query(
                r#"SELECT "workspace_id" FROM "workspace_integrations" WHERE "deleted_at" IS NULL AND "id" = $1 LIMIT 1"#,
            )
            .bind(installation.workspace_integration_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?
            .map(|row| row.try_get("workspace_id"))
            .transpose()
            .map_err(|error| GithubTransportError(error.to_string()))?
            .ok_or_else(|| GithubTransportError("workspace integration missing".to_owned()))?;
            sqlx::query(
                r#"INSERT INTO "git_provider_accounts" ("id", "created_at", "updated_at", "workspace_id", "provider", "host_url", "auth_type", "external_account_id", "external_account_login", "display_name", "capabilities", "credential_config", "workspace_integration_id", "status", "verified_at", "last_check_error", "metadata") VALUES ($1, now(), now(), $2, 'github', 'https://github.com', 'github_app', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"#,
            )
            .bind(Uuid::new_v4())
            .bind(workspace_id)
            .bind(installation.installation_id.to_string())
            .bind(&installation.account_login)
            .bind(&display_name)
            .bind(&capabilities)
            .bind(&credential_config)
            .bind(installation.workspace_integration_id)
            .bind(status)
            .bind(installation.verified_at)
            .bind(&installation.last_check_error)
            .bind(&metadata)
        }
    }
    .execute(&mut *connection)
    .await
    .map(|_| ())
    .map_err(|error| GithubTransportError(error.to_string()))
}

/// `_upsert_app_installation` (`github.py:229-267`): the transactional
/// workspace-integration + installation upsert, verified refresh, and
/// git-account companion. `require_verified=True` on the callback path.
/// Everything runs inside one transaction (`with transaction.atomic()`):
/// a refresh or account failure rolls the upsert back, exactly like
/// Django — the callback then redirects `github_verification_failed`
/// with no installation row left behind.
async fn upsert_app_installation(
    pool: &PgPool,
    workspace_id: Uuid,
    actor_id: Uuid,
    installation_payload: &serde_json::Value,
    config: &GithubAppConfig,
) -> Result<
    pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation,
    GithubTransportError,
> {
    let account = installation_payload
        .get("account")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let installation_id = installation_payload
        .get("id")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    if installation_id == 0 {
        return Err(GithubTransportError(
            "GitHub installation response did not include an id".to_owned(),
        ));
    }
    let account_login = account
        .get("login")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    // `AccountType.UNKNOWN` is `"Unknown"` (`models/.../github.py:124`).
    let account_type = account
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Unknown")
        .to_owned();
    let repository_selection = installation_payload
        .get("repository_selection")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("selected")
        .to_owned();
    let repository_count = installation_payload
        .get("repository_count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0) as i32;
    let permissions = installation_payload
        .get("permissions")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let events = installation_payload
        .get("events")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let installed_at = installation_payload
        .get("created_at")
        .and_then(serde_json::Value::as_str);
    let suspended_at = installation_payload
        .get("suspended_at")
        .and_then(serde_json::Value::as_str);
    // `parse_github_datetime` raises on malformed input there
    // (`_parse` → `ValueError` → the broad `except`); here the error
    // joins the same failure bucket.
    let installed_at = queries::parse_github_datetime(installed_at)
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let suspended_at = queries::parse_github_datetime(suspended_at)
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let wi = get_or_create_workspace_integration(&mut transaction, workspace_id, actor_id).await?;
    // The cross-workspace guard (`github.py:256-260`): the same
    // installation id bound elsewhere raises `GithubAppAuthError`.
    let elsewhere: Option<String> = sqlx::query(
        r#"SELECT "w"."slug" FROM "github_app_installations" AS "gai" INNER JOIN "workspace_integrations" AS "wi" ON "gai"."workspace_integration_id" = "wi"."id" INNER JOIN "workspaces" AS "w" ON "wi"."workspace_id" = "w"."id" WHERE "gai"."deleted_at" IS NULL AND "gai"."installation_id" = $1 AND "gai"."workspace_integration_id" != $2 LIMIT 1"#,
    )
    .bind(installation_id)
    .bind(wi.id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| GithubTransportError(error.to_string()))?
    .map(|row| row.try_get("slug"))
    .transpose()
    .map_err(|error| GithubTransportError(error.to_string()))?;
    if let Some(slug) = elsewhere {
        return Err(GithubTransportError(format!(
            "GitHub installation is already connected to workspace {slug}"
        )));
    }
    // `update_or_create(workspace_integration=wi, defaults=...)`
    // (`github.py:261-264`): exactly one row per workspace integration
    // (the OneToOne carries the column-level unique).
    let row_id: Uuid = sqlx::query(
        r#"INSERT INTO "github_app_installations" ("id", "created_at", "updated_at", "workspace_integration_id", "installation_id", "account_login", "account_type", "repository_selection", "repository_count", "permissions", "events", "installed_at", "suspended_at", "last_check_error") VALUES ($1, now(), now(), $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, '') ON CONFLICT ("workspace_integration_id") DO UPDATE SET "installation_id" = EXCLUDED."installation_id", "account_login" = EXCLUDED."account_login", "account_type" = EXCLUDED."account_type", "repository_selection" = EXCLUDED."repository_selection", "repository_count" = EXCLUDED."repository_count", "permissions" = EXCLUDED."permissions", "events" = EXCLUDED."events", "installed_at" = EXCLUDED."installed_at", "suspended_at" = EXCLUDED."suspended_at", "last_check_error" = '', "updated_at" = now() RETURNING "id""#,
    )
    .bind(Uuid::new_v4())
    .bind(wi.id)
    .bind(installation_id)
    .bind(&account_login)
    .bind(&account_type)
    .bind(&repository_selection)
    .bind(repository_count)
    .bind(&permissions)
    .bind(&events)
    .bind(installed_at)
    .bind(suspended_at)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| GithubTransportError(error.to_string()))
    .and_then(|row| {
        row.try_get("id")
            .map_err(|error| GithubTransportError(error.to_string()))
    })?;
    let installation = app_installation_by_id(&mut transaction, row_id)
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?
        .ok_or_else(|| GithubTransportError("installation row missing".to_owned()))?;
    let installation = refresh_app_installation(&mut transaction, &installation, config, true, &[])
        .await
        .map_err(|outcome| match outcome {
            RefreshOutcome::VerificationFailed(_) => {
                GithubTransportError("GitHub App connection check failed".to_owned())
            }
            RefreshOutcome::StoreFailed => {
                GithubTransportError("installation refresh save failed".to_owned())
            }
        })?;
    upsert_git_account_for_app(&mut transaction, &installation).await?;
    transaction
        .commit()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    Ok(installation)
}

/// One `github_app_installations` row by id.
async fn app_installation_by_id(
    connection: &mut sqlx::PgConnection,
    id: Uuid,
) -> Result<
    Option<pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation>,
    sqlx::Error,
> {
    let row = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_integration_id", "installation_id", "account_login", "account_type", "repository_selection", "repository_count", "permissions", "events", "installed_at", "suspended_at", "verified_at", "last_checked_at", "last_check_error" FROM "github_app_installations" WHERE "deleted_at" IS NULL AND "id" = $1 LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    app_installation_row(row)
}

// ---------------------------------------------------------------------------
// GET /api/integrations/github/app/callback/ (`github.py:730-808`)
// ---------------------------------------------------------------------------

/// The redirect base (`_redirect_to_profile_integrations`,
/// `github.py:140-147`): `WEB_URL` or `APP_BASE_URL` or `""`, trailing
/// slashes stripped, then `/settings/profile/integrations/?<params>`.
fn profile_integrations_url(
    settings: &pidash_db::config::Settings,
    params: &[(&str, &str)],
) -> String {
    let base = settings
        .urls
        .web_url
        .as_deref()
        .or(settings.urls.app_base_url.as_deref())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_owned();
    let path = "/settings/profile/integrations/";
    let query = urlencode_query(params);
    let mut url = format!("{base}{path}");
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    url
}

/// The callback's `fail(error)` (`github.py:757-761`): stamp the session
/// `FAILED` and redirect with `github_app=error&error=<code>`.
async fn callback_fail(
    pool: &PgPool,
    session_id: Uuid,
    error: &str,
    settings: &pidash_db::config::Settings,
) -> Response {
    sqlx::query(
        r#"UPDATE "github_app_install_sessions" SET "status" = 'failed', "error" = $1, "updated_at" = now() WHERE "id" = $2"#,
    )
    .bind(error)
    .bind(session_id)
    .execute(pool)
    .await
    .ok();
    redirect_response(profile_integrations_url(
        settings,
        &[("github_app", "error"), ("error", error)],
    ))
}

/// The browser callback (`AllowAny`): session lookup, expiry/actor/admin
/// guards, the exchange chain, and the success redirect. Every failure
/// mode redirects — the endpoint never answers JSON.
async fn app_callback(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    // The disabled flag fires first here (a 302, never the JSON 404 —
    // `github.py:736-737`).
    if !state.settings().github_sync_enabled {
        return redirect_response(profile_integrations_url(
            state.settings(),
            &[("github_app", "disabled")],
        ));
    }
    // Anonymous browsers redirect with `login_required` instead of a
    // bare 403 (`github.py:741-742`); this check precedes the session
    // lookup.
    let user = match callback_actor(&state, extension).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return redirect_response(profile_integrations_url(
                state.settings(),
                &[("github_app", "error"), ("error", "login_required")],
            ));
        }
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    lazy_cleanup_install_sessions(pool).await;
    // `request.GET.get(...) or ""` (`github.py:745-747`): Django's
    // `QueryDict.get` reads the last value; missing reads as `""`.
    let session_state = query.get("state").cloned().unwrap_or_default();
    let code = query.get("code").cloned().unwrap_or_default();
    let installation_id_raw = query.get("installation_id").cloned().unwrap_or_default();
    if session_state.is_empty() {
        return redirect_response(profile_integrations_url(
            state.settings(),
            &[("github_app", "error"), ("error", "missing_state")],
        ));
    }
    let session = match queries::fetch_install_session_by_state(pool, &session_state).await {
        Ok(session) => session,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(session) = session else {
        return redirect_response(profile_integrations_url(
            state.settings(),
            &[("github_app", "error"), ("error", "unknown_state")],
        ));
    };
    // Terminal replay: redirect with the stored status, no DB write
    // (`github.py:763-764`).
    if session.status != "started" {
        return redirect_response(profile_integrations_url(
            state.settings(),
            &[("github_app", session.status.as_str())],
        ));
    }
    // `expires_at <= now` (strictly `<=`, NOT the sweep's `<` —
    // `github.py:765-769`).
    if queries::install_session_callback_expired(session.expires_at, Utc::now()) {
        sqlx::query(
            r#"UPDATE "github_app_install_sessions" SET "status" = 'expired', "error" = 'Install session expired', "updated_at" = now() WHERE "id" = $1"#,
        )
        .bind(session.id)
        .execute(pool)
        .await
        .ok();
        return redirect_response(profile_integrations_url(
            state.settings(),
            &[("github_app", "expired")],
        ));
    }
    if session.actor_id != user.id {
        return callback_fail(pool, session.id, "actor_mismatch", state.settings()).await;
    }
    if !match queries::fetch_is_workspace_admin(pool, user.id, session.workspace_id).await {
        Ok(admin) => admin,
        Err(_) => return Denial::ServerError.into_response(),
    } {
        return callback_fail(
            pool,
            session.id,
            "workspace_admin_required",
            state.settings(),
        )
        .await;
    }
    if installation_id_raw.is_empty()
        || !installation_id_raw
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return callback_fail(
            pool,
            session.id,
            "missing_installation_id",
            state.settings(),
        )
        .await;
    }
    if code.is_empty() {
        return callback_fail(pool, session.id, "missing_oauth_code", state.settings()).await;
    }
    let installation_id: i64 = match installation_id_raw.parse() {
        Ok(value) => value,
        Err(_) => {
            return callback_fail(
                pool,
                session.id,
                "missing_installation_id",
                state.settings(),
            )
            .await;
        }
    };
    // The exchange chain (`github.py:780-793`): ANY failure (network,
    // auth, DB) logs and redirects `github_verification_failed`.
    let config = match github_app_config(pool, state.settings().secret_key.as_str()).await {
        Ok(config) => config,
        Err(_) => {
            return callback_fail(
                pool,
                session.id,
                "github_verification_failed",
                state.settings(),
            )
            .await;
        }
    };
    // The exchange chain (`github.py:780-793`): the
    // `installation_not_visible_to_user` branch redirects with its own
    // code; every other failure (network, auth, DB — the broad
    // `except Exception`) logs and redirects
    // `github_verification_failed`.
    enum CallbackFailure {
        FailCode(&'static str),
        Transport(GithubTransportError),
    }
    impl From<GithubTransportError> for CallbackFailure {
        fn from(error: GithubTransportError) -> Self {
            CallbackFailure::Transport(error)
        }
    }
    let verified = async {
        let user_token = exchange_user_code(&config, &code).await?;
        if verify_user_can_access_installation(&user_token, installation_id)
            .await?
            .is_none()
        {
            return Err(CallbackFailure::FailCode(
                "installation_not_visible_to_user",
            ));
        }
        let installation = get_installation(&config, installation_id).await?;
        upsert_app_installation(pool, session.workspace_id, user.id, &installation, &config)
            .await
            .map_err(CallbackFailure::from)
    }
    .await;
    let app_installation = match verified {
        Ok(installation) => installation,
        Err(CallbackFailure::FailCode(code)) => {
            return callback_fail(pool, session.id, code, state.settings()).await;
        }
        Err(CallbackFailure::Transport(error)) => {
            tracing::warn!("GitHub App callback verification failed: {error}");
            return callback_fail(
                pool,
                session.id,
                "github_verification_failed",
                state.settings(),
            )
            .await;
        }
    };
    sqlx::query(
        r#"UPDATE "github_app_install_sessions" SET "installation_id" = $1, "account_login" = $2, "status" = 'completed', "completed_at" = now(), "error" = '', "updated_at" = now() WHERE "id" = $3"#,
    )
    .bind(app_installation.installation_id)
    .bind(&app_installation.account_login)
    .bind(session.id)
    .execute(pool)
    .await
    .ok();
    let workspace = match workspace_by_id(pool, session.workspace_id).await {
        Ok(workspace) => workspace,
        Err(denial) => return denial.into_response(),
    };
    redirect_response(profile_integrations_url(
        state.settings(),
        &[
            ("github_app", "connected"),
            ("workspace_slug", workspace.slug.as_str()),
        ],
    ))
}

// ---------------------------------------------------------------------------
// Webhook delivery (`github.py:860-956`)
// ---------------------------------------------------------------------------

/// `pr_snapshot_from_payload` (`github_client.py:287-301`): the
/// display-only snapshot for one PR object. `state` maps to `closed`
/// only on exact `"closed"` (fixture B6 — `"merged"` renders `open`);
/// `merged` derives from `merged` or `merged_at`; titles cut at 500
/// code points; `pr_updated_at` parses `updated_at` or reads `None`.
struct PrSnapshot {
    title: String,
    state: String,
    merged: bool,
    draft: bool,
    pr_updated_at: Option<DateTime<Utc>>,
}

/// Python truthiness for JSON-native values (`bool(x)` in
/// `pr_snapshot_from_payload`, `github_client.py:295`): null/false are
/// false, empty string/0/empty containers are false, everything else is
/// true.
fn json_truthy(value: Option<&serde_json::Value>) -> bool {
    match value {
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => false,
        Some(serde_json::Value::Bool(true)) => true,
        Some(serde_json::Value::Number(number)) => {
            number.as_i64().is_some_and(|n| n != 0)
                || number.as_u64().is_some_and(|n| n != 0)
                || number.as_f64().is_some_and(|n| n != 0.0)
        }
        Some(serde_json::Value::String(text)) => !text.is_empty(),
        Some(serde_json::Value::Array(items)) => !items.is_empty(),
        Some(serde_json::Value::Object(fields)) => !fields.is_empty(),
    }
}

fn pr_snapshot_from_payload(pull_request: &serde_json::Value) -> PrSnapshot {
    let object = pull_request.as_object();
    let field = |key: &str| object.and_then(|fields| fields.get(key));
    let text = |key: &str| field(key).and_then(serde_json::Value::as_str).unwrap_or("");
    // `bool(merged or merged_at)` (`github_client.py:295`).
    let merged = json_truthy(field("merged")) || json_truthy(field("merged_at"));
    PrSnapshot {
        title: text("title").chars().take(500).collect(),
        state: if text("state") == "closed" {
            "closed".to_owned()
        } else {
            "open".to_owned()
        },
        merged,
        // `bool(pull_request.get("draft"))` (`github_client.py:300`).
        draft: json_truthy(field("draft")),
        pr_updated_at: {
            let raw = text("updated_at");
            if raw.is_empty() {
                None
            } else {
                queries::parse_github_datetime(Some(raw)).ok().flatten()
            }
        },
    }
}

/// `_refresh_pr_links` (`github.py:811-857`): refresh the display
/// snapshot of every link matching the `pull_request` payload. Returns
/// the number of matched links (0 when unattached — the caller maps
/// that to `skipped`). Matched-but-stale rows count as matched; only
/// the write is skipped, via the PR's `updated_at`.
async fn refresh_pr_links(pool: &PgPool, payload: &serde_json::Value) -> Result<i64, String> {
    let empty = serde_json::Map::new();
    // `(payload.get("pull_request") or {}).get(...)` (`github.py:822`):
    // falsy reads as `{}` (no match); a truthy non-dict has no `.get`
    // and fails the delivery (the webhook `except` → `failed`).
    let pull_request_value = payload
        .get("pull_request")
        .unwrap_or(&serde_json::Value::Null);
    if nested_shape_fails(pull_request_value) {
        return Err("github pull_request payload is not an object".to_owned());
    }
    // Same for `(payload.get("repository") or {})` (`github.py:823`).
    let repository_value = payload
        .get("repository")
        .unwrap_or(&serde_json::Value::Null);
    if nested_shape_fails(repository_value) {
        return Err("github repository payload is not an object".to_owned());
    }
    let pull_request = pull_request_value.as_object().unwrap_or(&empty);
    let repository = repository_value.as_object().unwrap_or(&empty);
    // A store failure fails the delivery (the webhook `except` branch,
    // `github.py:950-955`); only the message shape differs from Python's
    // `str(e)`, which the contract never byte-compares.
    fn store_error(error: sqlx::Error) -> String {
        error.to_string()
    }
    // `pr_number` for the `IntegerField`: Django coerces integral
    // floats (`int(1.0)`); anything else misses (0 matches nothing).
    let number = pull_request.get("number").and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|float| float.fract() == 0.0)
                .map(|float| float as i64)
        }),
        _ => None,
    });
    let owner = repository
        .get("owner")
        .and_then(serde_json::Value::as_object)
        .and_then(|owner| owner.get("login"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let name = repository
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let Some(number) = number else {
        return Ok(0);
    };
    if owner.is_empty() || name.is_empty() {
        return Ok(0);
    }
    let pull_value = payload.get("pull_request").cloned().unwrap_or_default();
    let snapshot = pr_snapshot_from_payload(&pull_value);
    let mut matched: i64 = 0;
    let links: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "pr_updated_at" FROM "github_pull_request_links" WHERE "deleted_at" IS NULL AND "repo_owner" = $1 AND "repo_name" = $2 AND "pr_number" = $3"#,
    )
    .bind(&owner)
    .bind(&name)
    .bind(number as i32)
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    for link in &links {
        matched += 1;
        let link_id: Uuid = link.try_get("id").map_err(store_error)?;
        let stored: Option<DateTime<Utc>> = link.try_get("pr_updated_at").map_err(store_error)?;
        // Stale / out-of-order delivery — matched but not applied
        // (`github.py:835-836`).
        if let (Some(incoming), Some(current)) = (snapshot.pr_updated_at, stored) {
            if incoming < current {
                continue;
            }
        }
        sqlx::query(
            r#"UPDATE "github_pull_request_links" SET "title" = $1, "state" = $2, "merged" = $3, "draft" = $4, "pr_updated_at" = $5, "updated_at" = now() WHERE "id" = $6"#,
        )
        .bind(&snapshot.title)
        .bind(&snapshot.state)
        .bind(snapshot.merged)
        .bind(snapshot.draft)
        .bind(snapshot.pr_updated_at)
        .bind(link_id)
        .execute(pool)
        .await
        .map_err(store_error)?;
    }
    let reviews: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "remote_updated_at", "metadata" FROM "git_code_review_links" WHERE "deleted_at" IS NULL AND "provider" = 'github' AND "host_url" = 'https://github.com' AND "namespace" = $1 AND "repo_name" = $2 AND "external_iid" = $3"#,
    )
    .bind(&owner)
    .bind(&name)
    .bind(number.to_string())
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    for review in &reviews {
        matched += 1;
        let review_id: Uuid = review.try_get("id").map_err(store_error)?;
        let stored: Option<DateTime<Utc>> =
            review.try_get("remote_updated_at").map_err(store_error)?;
        if let (Some(incoming), Some(current)) = (snapshot.pr_updated_at, stored) {
            if incoming < current {
                continue;
            }
        }
        let stored_metadata: serde_json::Value = review
            .try_get("metadata")
            .unwrap_or(serde_json::Value::Null);
        let mut metadata = stored_metadata.as_object().cloned().unwrap_or_default();
        metadata.insert("remote".to_owned(), pull_value.clone());
        let review_state = if snapshot.merged {
            "merged".to_owned()
        } else if snapshot.state.is_empty() {
            "open".to_owned()
        } else {
            snapshot.state.clone()
        };
        sqlx::query(
            r#"UPDATE "git_code_review_links" SET "title" = $1, "state" = $2, "merged" = $3, "draft" = $4, "remote_updated_at" = $5, "metadata" = $6, "updated_at" = now() WHERE "id" = $7"#,
        )
        .bind(&snapshot.title)
        .bind(&review_state)
        .bind(snapshot.merged)
        .bind(snapshot.draft)
        .bind(snapshot.pr_updated_at)
        .bind(serde_json::Value::Object(metadata))
        .bind(review_id)
        .execute(pool)
        .await
        .map_err(store_error)?;
    }
    Ok(matched)
}

/// Delivery statuses (`models/integration/github.py:164-168`).
mod delivery_status {
    pub const PROCESSED: &str = "processed";
    pub const FAILED: &str = "failed";
    pub const SKIPPED: &str = "skipped";
}

/// The inbound webhook (`AllowAny`, signature-is-auth,
/// `throttle_classes = []`, `github.py:860-956`): HMAC, delivery-header
/// checks, crash-safe `get_or_create` dedupe, event routing, and the
/// terminal save. Every path answers 202 with the delivery status (or
/// the exact 4xx/409 above it).
async fn app_webhook(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !state.settings().github_sync_enabled {
        return json_response(StatusCode::NOT_FOUND, GITHUB_DISABLED_BODY.to_owned());
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // The signature check reads the config first: a missing config
    // answers 409 before any signature verdict (`github.py:872-876`).
    let config = match github_app_config(pool, state.settings().secret_key.as_str()).await {
        Ok(config) => config,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let required = match require_config(config, false, true) {
        Ok(config) => config,
        Err(message) => {
            return json_response(
                StatusCode::CONFLICT,
                serde_json::to_string(&serde_json::json!({"error": message}))
                    .expect("serializable config error"),
            );
        }
    };
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok());
    if !verify_webhook_signature(&required.webhook_secret, &body, signature) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            WEBHOOK_BAD_SIGNATURE_BODY.to_owned(),
        );
    }
    let delivery_id_raw = headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if delivery_id_raw.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"Missing X-GitHub-Delivery"}"#.to_owned(),
        );
    }
    let delivery_uuid = match delivery_id_raw.parse::<Uuid>() {
        Ok(value) => value,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"Invalid X-GitHub-Delivery"}"#.to_owned(),
            );
        }
    };
    let event = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    // `(request.body.decode("utf-8") or "{}")` (`github.py:887`): only
    // `JSONDecodeError` is caught — undecodable bytes escape to the 500.
    let text = match std::str::from_utf8(&body) {
        Ok(text) => text,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let raw = if text.is_empty() { "{}" } else { text };
    let payload: serde_json::Value = match serde_json::from_str(raw) {
        Ok(payload) => payload,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"Invalid JSON"}"#.to_owned(),
            );
        }
    };
    // A non-object JSON body (`null`, `[...]`, `"s"`) has no `.get` in
    // Python (`github.py:891`) — the `AttributeError` escapes to the 500.
    if !payload.is_object() {
        return Denial::ServerError.into_response();
    }
    // `(payload.get("installation") or {}).get("id")`
    // (`github.py:891-892`): a falsy `installation` reads as `{}` (no
    // id); a truthy non-dict has no `.get` (the 500 envelope).
    // `installation.get("id")` for the `BigIntegerField`: Django's
    // `get_prep_value` coerces digit strings (`int("123")`); anything
    // else reads as `None`.
    let installation_value = payload
        .get("installation")
        .unwrap_or(&serde_json::Value::Null);
    if nested_shape_fails(installation_value) {
        return Denial::ServerError.into_response();
    }
    let installation_id = installation_value
        .as_object()
        .and_then(|installation| installation.get("id"))
        .and_then(|value| match value {
            serde_json::Value::Number(number) => number.as_i64(),
            serde_json::Value::String(text)
                if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                text.parse().ok()
            }
            _ => None,
        });
    // `payload.get("action") or ""` for the `CharField`: falsy values
    // (`None`, `""`, `0`, `False`, `[]`, `{}`) read as `""`; truthy
    // non-strings render with `str()` on save (`True` → `"True"`).
    let action = match payload.get("action") {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(text)) if text.is_empty() => String::new(),
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(value) if !json_truthy(Some(value)) => String::new(),
        Some(serde_json::Value::Bool(true)) => "True".to_owned(),
        Some(other) => other.to_string(),
    };
    // `get_or_create(delivery_id, defaults=...)` (`github.py:894-903`).
    let inserted = sqlx::query(
        r#"INSERT INTO "github_webhook_deliveries" ("id", "created_at", "updated_at", "delivery_id", "event", "action", "installation_id", "payload", "status", "received_at", "processed_at", "error") VALUES ($1, now(), now(), $2, $3, $4, $5, $6, 'received', now(), NULL, '') ON CONFLICT ("delivery_id") DO NOTHING"#,
    )
    .bind(Uuid::new_v4())
    .bind(delivery_uuid)
    .bind(&event)
    .bind(&action)
    .bind(installation_id)
    .bind(&payload)
    .execute(pool)
    .await;
    let created = match inserted {
        Ok(outcome) => outcome.rows_affected() == 1,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if !created {
        // Retries and redeliveries reuse the GUID: fast-return only when
        // a prior attempt reached terminal success (`processed` /
        // `skipped`); `received` (interrupted) and `failed` reprocess so
        // the snapshot still converges (`github.py:904-917`).
        let prior: Option<String> = match sqlx::query(
            r#"SELECT "status" FROM "github_webhook_deliveries" WHERE "delivery_id" = $1 LIMIT 1"#,
        )
        .bind(delivery_uuid)
        .fetch_optional(pool)
        .await
        {
            Ok(row) => row.and_then(|row| row.try_get("status").ok()),
            Err(_) => return Denial::ServerError.into_response(),
        };
        if matches!(
            prior.as_deref(),
            Some(delivery_status::PROCESSED) | Some(delivery_status::SKIPPED)
        ) {
            let status = prior.unwrap_or_else(|| delivery_status::SKIPPED.to_owned());
            return json_response(
                StatusCode::ACCEPTED,
                serde_json::to_string(&serde_json::json!({"status": status}))
                    .expect("serializable delivery status"),
            );
        }
    }
    // Route the delivery (`github.py:919-949`); any exception marks it
    // `failed` with the truncated error (`github.py:950-955`).
    let outcome: Result<String, String> = route_delivery(
        pool,
        state.settings().secret_key.as_str(),
        &event,
        &action,
        installation_id,
        &payload,
    )
    .await;
    let (status, error) = match outcome {
        Ok(status) => (status, String::new()),
        Err(error) => {
            tracing::warn!("GitHub App webhook delivery failed: {error}");
            (delivery_status::FAILED.to_owned(), truncate_error(&error))
        }
    };
    sqlx::query(
        r#"UPDATE "github_webhook_deliveries" SET "status" = $1, "error" = $2, "processed_at" = now(), "updated_at" = now() WHERE "delivery_id" = $3"#,
    )
    .bind(&status)
    .bind(&error)
    .bind(delivery_uuid)
    .execute(pool)
    .await
    .ok();
    json_response(
        StatusCode::ACCEPTED,
        serde_json::to_string(&serde_json::json!({"status": status}))
            .expect("serializable delivery status"),
    )
}

/// Event routing for one delivery (`github.py:920-947`). Returns the
/// terminal status, or the failure message for the `failed` branch.
async fn route_delivery(
    pool: &PgPool,
    secret_key: &str,
    event: &str,
    action: &str,
    installation_id: Option<i64>,
    payload: &serde_json::Value,
) -> Result<String, String> {
    if event == "ping" {
        return Ok(delivery_status::PROCESSED.to_owned());
    }
    if event == "pull_request" {
        let matched = refresh_pr_links(pool, payload).await?;
        return Ok(if matched > 0 {
            delivery_status::PROCESSED.to_owned()
        } else {
            delivery_status::SKIPPED.to_owned()
        });
    }
    if event == "installation" || event == "installation_repositories" {
        let installation = match installation_id {
            Some(id) => installation_by_installation_id(pool, id)
                .await
                .map_err(|error| error.to_string())?,
            None => None,
        };
        let Some(mut installation) = installation else {
            return Ok(delivery_status::SKIPPED.to_owned());
        };
        if event == "installation" && (action == "deleted" || action == "suspend") {
            installation.suspended_at = Some(Utc::now());
            installation.last_check_error =
                "GitHub App installation removed or suspended".to_owned();
            sqlx::query(
                r#"UPDATE "github_app_installations" SET "suspended_at" = $1, "last_check_error" = $2, "updated_at" = now() WHERE "id" = $3"#,
            )
            .bind(installation.suspended_at)
            .bind(&installation.last_check_error)
            .bind(installation.id)
            .execute(pool)
            .await
            .map_err(|error| error.to_string())?;
            // `revoke_installation_cache`: the Rust side populates no
            // installation-token cache, so there is nothing to evict
            // (see the module docs).
        } else if event == "installation" && action == "unsuspend" {
            installation.suspended_at = None;
            let config = github_app_config(pool, secret_key)
                .await
                .map_err(|_| "installation refresh config failed".to_owned())?;
            // Autocommit here, exactly like Django (the webhook handler
            // wraps nothing in `transaction.atomic`).
            let mut connection = pool.acquire().await.map_err(|error| error.to_string())?;
            refresh_app_installation(
                &mut connection,
                &installation,
                &config,
                false,
                &["suspended_at"],
            )
            .await
            .map_err(|outcome| match outcome {
                RefreshOutcome::VerificationFailed(error) => error,
                RefreshOutcome::StoreFailed => "installation refresh save failed".to_owned(),
            })?;
        } else if event == "installation_repositories" {
            let config = github_app_config(pool, secret_key)
                .await
                .map_err(|_| "installation refresh config failed".to_owned())?;
            // Autocommit here, exactly like Django (see above).
            let mut connection = pool.acquire().await.map_err(|error| error.to_string())?;
            refresh_app_installation(&mut connection, &installation, &config, false, &[])
                .await
                .map_err(|outcome| match outcome {
                    RefreshOutcome::VerificationFailed(error) => error,
                    RefreshOutcome::StoreFailed => "installation refresh save failed".to_owned(),
                })?;
        }
        return Ok(delivery_status::PROCESSED.to_owned());
    }
    Ok(delivery_status::SKIPPED.to_owned())
}

/// One `github_app_installations` row by `installation_id`
/// (`github.py:930-931`).
async fn installation_by_installation_id(
    pool: &PgPool,
    installation_id: i64,
) -> Result<
    Option<pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation>,
    sqlx::Error,
> {
    let row = sqlx::query(
        r#"SELECT "id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_integration_id", "installation_id", "account_login", "account_type", "repository_selection", "repository_count", "permissions", "events", "installed_at", "suspended_at", "verified_at", "last_checked_at", "last_check_error" FROM "github_app_installations" WHERE "deleted_at" IS NULL AND "installation_id" = $1 LIMIT 1"#,
    )
    .bind(installation_id)
    .fetch_optional(pool)
    .await?;
    app_installation_row(row)
}

/// Map a full `github_app_installations` row (shared by the id and
/// installation-id lookups).
fn app_installation_row(
    row: Option<sqlx::postgres::PgRow>,
) -> Result<
    Option<pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation>,
    sqlx::Error,
> {
    use pidash_db::integrations::github_models::github_app_installation as model;
    row.map(|row| {
        Ok(model::GithubAppInstallation {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            deleted_at: row.try_get("deleted_at")?,
            workspace_integration_id: row.try_get("workspace_integration_id")?,
            installation_id: row.try_get("installation_id")?,
            account_login: row.try_get("account_login")?,
            account_type: row.try_get("account_type")?,
            repository_selection: row.try_get("repository_selection")?,
            repository_count: row.try_get("repository_count")?,
            permissions: row.try_get("permissions")?,
            events: row.try_get("events")?,
            installed_at: row.try_get("installed_at")?,
            suspended_at: row.try_get("suspended_at")?,
            verified_at: row.try_get("verified_at")?,
            last_checked_at: row.try_get("last_checked_at")?,
            last_check_error: row.try_get("last_check_error")?,
        })
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    use pidash_db::integrations::github_models::github_app_installation::GithubAppInstallation;

    fn test_installation() -> GithubAppInstallation {
        GithubAppInstallation {
            id: Uuid::nil(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_integration_id: Uuid::nil(),
            installation_id: 12345678,
            account_login: "contract-octocat".to_owned(),
            account_type: "Organization".to_owned(),
            repository_selection: "selected".to_owned(),
            repository_count: 3,
            permissions: serde_json::json!({}),
            events: serde_json::json!([]),
            installed_at: None,
            suspended_at: None,
            verified_at: None,
            last_checked_at: None,
            last_check_error: String::new(),
        }
    }

    #[test]
    fn urlencode_keeps_order_and_encodes() {
        assert_eq!(
            urlencode_query(&[("github_app", "error"), ("error", "login_required")]),
            "github_app=error&error=login_required"
        );
        assert_eq!(urlencode_query(&[("state", "a b+c")]), "state=a+b%2Bc");
        assert_eq!(urlencode_query(&[]), "");
    }

    #[test]
    fn require_config_missing_orders_match_python() {
        // After the contract test wipes the db keys, the client/secret
        // env dummies stay: install requires the oauth set, the webhook
        // only the webhook secret (`github_app_auth.py:63-73`).
        let wiped = GithubAppConfig {
            app_id: String::new(),
            app_slug: String::new(),
            private_key: "dummy".to_owned(),
            webhook_secret: "dummy".to_owned(),
            client_id: String::new(),
            client_secret: "dummy".to_owned(),
        };
        assert_eq!(
            require_config(wiped, true, true).unwrap_err(),
            "GitHub App config missing: app_id, app_slug, client_id"
        );
        let wiped = GithubAppConfig {
            app_id: String::new(),
            app_slug: String::new(),
            private_key: "dummy".to_owned(),
            webhook_secret: "dummy".to_owned(),
            client_id: "dummy".to_owned(),
            client_secret: "dummy".to_owned(),
        };
        assert_eq!(
            require_config(wiped, false, true).unwrap_err(),
            "GitHub App config missing: app_id, app_slug"
        );
    }

    #[test]
    fn status_configured_needs_all_six_keys() {
        let full = GithubAppConfig {
            app_id: "a".to_owned(),
            app_slug: "b".to_owned(),
            private_key: "c".to_owned(),
            webhook_secret: "d".to_owned(),
            client_id: "e".to_owned(),
            client_secret: "f".to_owned(),
        };
        assert!(status_configured(&full));
        let mut partial = full;
        partial.webhook_secret.clear();
        assert!(!status_configured(&partial));
    }

    #[test]
    fn install_state_is_43_urlsafe_chars() {
        let first = install_state();
        let second = install_state();
        assert_eq!(first.len(), 43);
        assert!(first
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
        assert_ne!(first, second);
    }

    #[test]
    fn pr_snapshot_ports_state_bug_and_shapes() {
        // B6: only exact "closed" maps to closed; "merged" renders open.
        let merged = pr_snapshot_from_payload(&serde_json::json!({
            "title": "Fix it",
            "state": "merged",
            "merged": false,
            "merged_at": null,
            "draft": false,
            "updated_at": "2026-01-02T03:04:05Z",
        }));
        assert_eq!(merged.state, "open");
        assert!(!merged.merged);
        assert_eq!(
            merged.pr_updated_at,
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:05+00:00")
                    .expect("fixture time")
                    .with_timezone(&Utc)
            )
        );
        let closed = pr_snapshot_from_payload(&serde_json::json!({
            "title": "Done",
            "state": "closed",
            "merged": true,
            "draft": 1,
            "updated_at": null,
        }));
        assert_eq!(closed.state, "closed");
        assert!(closed.merged);
        assert!(closed.draft);
        assert_eq!(closed.pr_updated_at, None);
        // `merged_at` alone derives merged (`github_client.py:295`).
        let via_merged_at = pr_snapshot_from_payload(&serde_json::json!({
            "title": "x",
            "state": "open",
            "merged_at": "2026-01-02T03:04:05Z",
        }));
        assert!(via_merged_at.merged);
        // Titles cut at 500 code points.
        let long = "é".repeat(600);
        let snapshot =
            pr_snapshot_from_payload(&serde_json::json!({"title": long, "state": "open"}));
        assert_eq!(snapshot.title.chars().count(), 500);
    }

    #[test]
    fn serialize_installation_key_order_and_disconnected() {
        assert_eq!(
            serialize_app_installation(None),
            serde_json::json!({"connected": false})
        );
        let body = serialize_app_installation(Some(&test_installation()));
        let object = body.as_object().expect("installation object");
        let keys: Vec<&str> = object.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "connected",
                "installation_id",
                "account_login",
                "account_type",
                "repository_selection",
                "repository_count",
                "permissions",
                "events",
                "installed_at",
                "suspended_at",
                "verified_at",
                "last_checked_at",
                "last_check_error",
            ]
        );
        assert_eq!(body["account_login"], "contract-octocat");
        assert_eq!(body["installed_at"], serde_json::Value::Null);
    }

    #[test]
    fn redirect_url_strips_base_and_orders_params() {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.web_url = Some("https://dash.example.com/".to_owned());
        assert_eq!(
            profile_integrations_url(
                &settings,
                &[("github_app", "error"), ("error", "login_required")]
            ),
            "https://dash.example.com/settings/profile/integrations/?github_app=error&error=login_required"
        );
        settings.urls.web_url = None;
        settings.urls.app_base_url = None;
        assert_eq!(
            profile_integrations_url(&settings, &[("github_app", "connected")]),
            "/settings/profile/integrations/?github_app=connected"
        );
    }

    #[test]
    fn truncate_error_caps_at_2000_chars() {
        assert_eq!(truncate_error(&"e".repeat(3000)).chars().count(), 2000);
        assert_eq!(truncate_error("short"), "short");
    }

    #[test]
    fn workspace_slug_mirrors_python_falsy_and_attribute_errors() {
        // `(request.data.get("workspace_slug") or "").strip()`
        // (`github.py:670,704`): missing/null/falsy read as `""` (the
        // required-400); truthy non-strings and non-object bodies raise
        // `AttributeError` (the 500 envelope); unparseable bytes keep the
        // historical required-400.
        let ok = |body: &[u8]| workspace_slug_from_body(body).unwrap();
        let err = |body: &[u8]| workspace_slug_from_body(body).unwrap_err();
        assert_eq!(ok(b"{}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": null}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": \"\"}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": \"  \"}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": false}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": 0}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": []}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": {}}"), "");
        assert_eq!(ok(b"{\"workspace_slug\": \"  ws-1  \"}"), "ws-1");
        assert_eq!(ok(b"{not json"), "");
        assert!(matches!(
            err(b"{\"workspace_slug\": 1}"),
            Denial::ServerError
        ));
        assert!(matches!(
            err(b"{\"workspace_slug\": true}"),
            Denial::ServerError
        ));
        assert!(matches!(
            err(b"{\"workspace_slug\": [1]}"),
            Denial::ServerError
        ));
        assert!(matches!(
            err(b"{\"workspace_slug\": {\"a\": 1}}"),
            Denial::ServerError
        ));
        assert!(matches!(err(b"null"), Denial::ServerError));
        assert!(matches!(err(b"[1]"), Denial::ServerError));
        assert!(matches!(err(b"\"s\""), Denial::ServerError));
    }

    #[test]
    fn webhook_shape_guards_reject_truthy_non_objects() {
        // `payload.get("installation") or {}` / `pull_request` /
        // `repository` (`github.py:822-823,891`): falsy reads as `{}`,
        // truthy non-dicts fail the request.
        for raw in [
            serde_json::json!(null),
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!(""),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"id": 7}),
        ] {
            assert!(!nested_shape_fails(&raw), "{raw}");
        }
        for raw in [
            serde_json::json!(true),
            serde_json::json!(1),
            serde_json::json!("x"),
            serde_json::json!([1]),
        ] {
            assert!(nested_shape_fails(&raw), "{raw}");
        }
    }
}
