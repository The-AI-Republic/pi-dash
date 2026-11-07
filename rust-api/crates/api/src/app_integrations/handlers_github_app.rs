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
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::app_integrations::queries_github as queries;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{
    GITHUB_DISABLED_BODY, INSTALL_ADMIN_REQUIRED_BODY, REFRESH_ADMIN_REQUIRED_BODY,
    WEBHOOK_BAD_SIGNATURE_BODY,
};
use super::hmac::verify_webhook_signature;
pub use pidash_compat::response::SERVER_ERROR_BODY;
use pidash_compat::response::{json_response, redirect_found_html};

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

/// `HttpResponseRedirect` (`github.py:140-147`): 302 with a `Location`
/// header and an empty body.
fn redirect_response(location: String) -> Response {
    let Some(location) = crate::response_headers::header_value(&location) else {
        return Denial::ServerError.into_response();
    };
    redirect_found_html(location)
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
/// Python `isoformat()` with `+00:00` (Django never renders `Z`):
/// exactly six fraction digits when microseconds are nonzero, none
/// when zero. chrono `AutoSi` trims trailing zeros (`.120`) and
/// prints nanos (9 digits), so this shares
/// [`crate::assistant::common::py_iso`] (PIDASHCONV-1464).
fn isoformat(value: &DateTime<Utc>) -> String {
    crate::assistant::common::py_iso(value)
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = pidash_db::app_integrations::fetch_workspace_id_by_slug(&db, slug)
        .await
        .map_err(|_| Denial::ServerError.into_response())?;
    match row {
        Some(id) => Ok(WorkspaceRef {
            id,
            slug: slug.to_owned(),
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
    use pidash_db::entity::workspaces;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<(Uuid, String)> = workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .column(workspaces::Column::Slug)
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Id.eq(id))
        .limit(1)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        Some((id, slug)) => Ok(WorkspaceRef { id, slug }),
        None => Err(Denial::ServerError),
    }
}

/// `_get_workspace_integration(workspace)` (`github.py:102-106`): the
/// github `Integration` row, then the workspace's row for it.
async fn workspace_integration(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Option<queries::WorkspaceIntegrationRow>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let integration_id = queries::fetch_integration_id_by_provider(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    match integration_id {
        None => Ok(None),
        Some(integration_id) => {
            queries::fetch_workspace_integration(&db, workspace_id, integration_id)
                .await
                .map_err(|_| Denial::ServerError)
        }
    }
}

/// The GitHub App installation state as stored on
/// `git_provider_accounts` (PDASHOSS01-356): typed columns for
/// `verified_at`, `suspended_at`, `last_checked_at` and
/// `last_check_error`; payload fields (`account_type`,
/// `repository_selection`, `repository_count`, `permissions`,
/// `events`, `installed_at`) in `metadata`. `installation_id` is
/// `external_account_id` parsed back to `i64` (`str(installation_id)`
/// is the stored form, matching the webhook lookup key).
#[derive(Debug, Clone, PartialEq)]
struct AppInstallation {
    id: Uuid,
    workspace_integration_id: Option<Uuid>,
    installation_id: i64,
    account_login: String,
    account_type: String,
    repository_selection: String,
    repository_count: i32,
    permissions: serde_json::Value,
    events: serde_json::Value,
    installed_at: Option<DateTime<Utc>>,
    suspended_at: Option<DateTime<Utc>>,
    verified_at: Option<DateTime<Utc>>,
    last_checked_at: Option<DateTime<Utc>>,
    last_check_error: String,
    metadata: serde_json::Value,
}

impl From<pidash_db::entity::git_provider_accounts::Model> for AppInstallation {
    fn from(row: pidash_db::entity::git_provider_accounts::Model) -> Self {
        use serde_json::Value;
        let at = |at: sea_orm::prelude::DateTimeWithTimeZone| at.with_timezone(&chrono::Utc);
        let metadata = row.metadata.clone();
        let text = |key: &str| {
            metadata
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        Self {
            id: row.id,
            workspace_integration_id: row.workspace_integration_id,
            // `github_app` rows always store `str(installation_id)`
            // here (every write path builds it from the numeric id).
            installation_id: row.external_account_id.parse().unwrap_or_default(),
            account_login: row.external_account_login,
            account_type: {
                let value = text("account_type");
                if value.is_empty() {
                    "Unknown".to_owned()
                } else {
                    value
                }
            },
            repository_selection: {
                let value = text("repository_selection");
                if value.is_empty() {
                    "selected".to_owned()
                } else {
                    value
                }
            },
            repository_count: metadata
                .get("repository_count")
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32,
            permissions: metadata
                .get("permissions")
                .cloned()
                .unwrap_or(serde_json::json!({})),
            events: metadata
                .get("events")
                .cloned()
                .unwrap_or(serde_json::json!([])),
            installed_at: metadata
                .get("installed_at")
                .and_then(Value::as_str)
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.with_timezone(&chrono::Utc)),
            suspended_at: row.suspended_at.map(at),
            verified_at: row.verified_at.map(at),
            last_checked_at: row.last_checked_at.map(at),
            last_check_error: row.last_check_error,
            metadata,
        }
    }
}

impl AppInstallation {
    /// The metadata value a refresh writes back: the stored metadata
    /// with the live `repository_count` merged in. (Non-object
    /// metadata cannot gain a key; the upsert paths always write an
    /// object.)
    fn metadata_with_repository_count(&self, repository_count: i32) -> serde_json::Value {
        let mut metadata = self.metadata.clone();
        if let Some(object) = metadata.as_object_mut() {
            object.insert(
                "repository_count".to_owned(),
                serde_json::Value::Number(repository_count.into()),
            );
        }
        metadata
    }
}

/// The workspace's GitHub App account through its
/// `workspace_integration` (`github.py:636-638`). Ordered by
/// `updated_at` descending so a reinstall (which leaves the previous
/// installation id's row behind) resolves to the newest row.
async fn app_installation_for(
    pool: &PgPool,
    workspace_integration_id: Uuid,
) -> Result<Option<AppInstallation>, Denial> {
    use pidash_db::entity::git_provider_accounts;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = git_provider_accounts::Entity::find()
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::Provider.eq("github"))
        .filter(git_provider_accounts::Column::HostUrl.eq("https://github.com"))
        .filter(git_provider_accounts::Column::AuthType.eq("github_app"))
        .filter(
            git_provider_accounts::Column::WorkspaceIntegrationId
                .eq(Some(workspace_integration_id)),
        )
        .order_by_desc(git_provider_accounts::Column::UpdatedAt)
        .limit(1)
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(AppInstallation::from))
}

/// `_lazy_cleanup_install_sessions` (`github.py:170-188`): mark
/// `started` rows past their expiry as `expired` (strict `<`, NOT the
/// callback's `<=` — see the module docs), then hard-delete terminal
/// rows untouched for 7 days. Errors are logged and swallowed, never
/// raised — the callers always continue.
async fn lazy_cleanup_install_sessions(pool: &PgPool) {
    use pidash_db::entity::git_provider_install_sessions;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let stamped = chrono::Utc::now();
    let expired = git_provider_install_sessions::Entity::update_many()
        .col_expr(
            git_provider_install_sessions::Column::Status,
            sea_orm::sea_query::Expr::value("expired"),
        )
        .col_expr(
            git_provider_install_sessions::Column::Error,
            sea_orm::sea_query::Expr::value("Install session expired"),
        )
        .filter(git_provider_install_sessions::Column::DeletedAt.is_null())
        .filter(git_provider_install_sessions::Column::Status.eq("started"))
        .filter(git_provider_install_sessions::Column::ExpiresAt.lt(stamped))
        .exec(&db)
        .await;
    let week_ago = stamped - chrono::Duration::days(7);
    let deleted = git_provider_install_sessions::Entity::delete_many()
        .filter(git_provider_install_sessions::Column::Status.is_in([
            "completed",
            "expired",
            "failed",
        ]))
        .filter(git_provider_install_sessions::Column::UpdatedAt.lt(week_ago))
        .exec(&db)
        .await;
    match (expired, deleted) {
        (Ok(expired), Ok(deleted)) => {
            if expired.rows_affected > 0 || deleted.rows_affected > 0 {
                tracing::info!(
                    "GitHub App install session cleanup: expired={}, deleted={}",
                    expired.rows_affected,
                    deleted.rows_affected,
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
fn serialize_app_installation(installation: Option<&AppInstallation>) -> serde_json::Value {
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
    let memberships =
        match queries::fetch_admin_memberships(&pidash_db::pool::sea_orm_connection(pool), user.id)
            .await
        {
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
            "slug": membership.workspace__slug,
            "name": membership.workspace__name,
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
    if !match queries::fetch_is_workspace_admin(
        &pidash_db::pool::sea_orm_connection(pool),
        user.id,
        workspace.id,
    )
    .await
    {
        Ok(admin) => admin,
        Err(_) => return Denial::ServerError.into_response(),
    } {
        return json_response(
            StatusCode::FORBIDDEN,
            INSTALL_ADMIN_REQUIRED_BODY.to_owned(),
        );
    }
    let session_state = install_state();
    // `GithubAppInstallSession.objects.create(...)` (`github.py:681`):
    // `BaseModel.save` stamps `created_by` with the request user and
    // leaves `updated_by` NULL on create (`db/models/base.py:36-39`).
    use pidash_db::entity::git_provider_install_sessions;
    use sea_orm::{ActiveModelTrait, Set};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let stamped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    let inserted = git_provider_install_sessions::ActiveModel {
        id: Set(Uuid::new_v4()),
        created_at: Set(stamped),
        updated_at: Set(stamped),
        created_by_id: Set(Some(user.id)),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        state: Set(session_state.clone()),
        provider: Set("github".to_owned()),
        workspace_id: Set(workspace.id),
        actor_id: Set(user.id),
        external_account_id: Set(None),
        account_login: Set(String::new()),
        status: Set("started".to_owned()),
        expires_at: Set(stamped + chrono::Duration::minutes(15)),
        completed_at: Set(None),
        error: Set(String::new()),
    }
    .insert(&db)
    .await;
    let row = match inserted {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let returned_state = row.state;
    let expires_at = row.expires_at.with_timezone(&chrono::Utc);
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
    db: &impl sea_orm::ConnectionTrait,
    installation: &AppInstallation,
    config: &GithubAppConfig,
    raise_on_error: bool,
    extra_update_fields: &[&str],
) -> Result<AppInstallation, RefreshOutcome> {
    let mut refreshed = installation.clone();
    match list_installation_repository_count(config, installation.installation_id).await {
        Ok(repository_count) => {
            // `timezone.now()` (`github.py:197`) is microsecond-exact;
            // truncate so the stamped row — serialized below into the
            // 200 body — matches the stored value (PIDASHCONV-1464).
            let now = crate::runner_enroll::now_micros();
            refreshed.repository_count = repository_count;
            refreshed.verified_at = Some(now);
            refreshed.last_checked_at = Some(now);
            refreshed.last_check_error = String::new();
        }
        Err(error) => {
            // `except Exception` (`github.py:216-223`): stamp the
            // failure, log, save, and raise only when asked.
            refreshed.last_checked_at = Some(crate::runner_enroll::now_micros());
            refreshed.last_check_error = truncate_error(&error.to_string());
            save_installation_refresh(db, &refreshed, extra_update_fields)
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
    save_installation_refresh(db, &refreshed, extra_update_fields)
        .await
        .map_err(|_| RefreshOutcome::StoreFailed)?;
    Ok(refreshed)
}

/// The `save(update_fields=[repository_count, verified_at,
/// last_checked_at, last_check_error, updated_at, ...extra])`
/// (`github.py:198-206,220,225`).
async fn save_installation_refresh(
    db: &impl sea_orm::ConnectionTrait,
    installation: &AppInstallation,
    extra_update_fields: &[&str],
) -> Result<(), GithubTransportError> {
    use pidash_db::entity::git_provider_accounts;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    // `update_fields` always carries the five refresh columns; the only
    // `extra_update_fields` value in the codebase is `"suspended_at"`
    // (the unsuspend path, `github.py:942`), which the in-memory row
    // already carries — so every refresh writes the same columns.
    // `repository_count` lives in `metadata` on the generic table.
    let _ = extra_update_fields;
    let bumped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    let at = |value: Option<DateTime<Utc>>| {
        value.map(|at| at.with_timezone(&chrono::FixedOffset::east_opt(0).expect("zero offset")))
    };
    git_provider_accounts::Entity::update_many()
        .col_expr(
            git_provider_accounts::Column::VerifiedAt,
            sea_orm::sea_query::Expr::value(at(installation.verified_at)),
        )
        .col_expr(
            git_provider_accounts::Column::LastCheckedAt,
            sea_orm::sea_query::Expr::value(at(installation.last_checked_at)),
        )
        .col_expr(
            git_provider_accounts::Column::LastCheckError,
            sea_orm::sea_query::Expr::value(installation.last_check_error.clone()),
        )
        .col_expr(
            git_provider_accounts::Column::SuspendedAt,
            sea_orm::sea_query::Expr::value(at(installation.suspended_at)),
        )
        .col_expr(
            git_provider_accounts::Column::Metadata,
            sea_orm::sea_query::Expr::value(
                installation.metadata_with_repository_count(installation.repository_count),
            ),
        )
        .col_expr(
            git_provider_accounts::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(bumped),
        )
        .filter(git_provider_accounts::Column::Id.eq(installation.id))
        .exec(db)
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
    if !match queries::fetch_is_workspace_admin(
        &pidash_db::pool::sea_orm_connection(pool),
        user.id,
        workspace.id,
    )
    .await
    {
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
    let connection = pidash_db::orm_tx::TxConn::new(&mut connection);
    match refresh_app_installation(&connection, &installation, &config, true, &[]).await {
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
    db: &impl sea_orm::ConnectionTrait,
    workspace_id: Uuid,
    actor_id: Uuid,
) -> Result<queries::WorkspaceIntegrationRow, GithubTransportError> {
    use pidash_db::entity::{integrations, workspace_integrations};
    use sea_orm::{ActiveModelTrait, Set, Unchanged};
    // `_get_or_create_github_integration` (`github.py:90-99`).
    let integration_id: Uuid = match queries::fetch_integration_id_by_provider(db)
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?
    {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4();
            let stamped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
            integrations::ActiveModel {
                id: Set(id),
                created_at: Set(stamped),
                updated_at: Set(stamped),
                created_by_id: Unchanged(None),
                updated_by_id: Unchanged(None),
                deleted_at: Unchanged(None),
                title: Set("GitHub".to_owned()),
                provider: Set("github".to_owned()),
                network: Set(1_i32),
                description: Set(
                    serde_json::json!({"summary": "Mirror GitHub issues into Pi Dash projects."}),
                ),
                author: Set(String::new()),
                webhook_url: Set(String::new()),
                webhook_secret: Set(String::new()),
                redirect_url: Set(String::new()),
                metadata: Set(serde_json::json!({})),
                verified: Set(true),
                avatar_url: Unchanged(None),
            }
            .insert(db)
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?;
            id
        }
    };
    if let Some(existing) = queries::fetch_workspace_integration(db, workspace_id, integration_id)
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?
    {
        return Ok(existing);
    }
    // `APIToken.objects.create(...)` / `WorkspaceIntegration.objects.create(...)`
    // (`github.py:114-128`): `BaseModel.save` stamps `created_by` with the
    // request user and leaves `updated_by` NULL on create
    // (`db/models/base.py:36-39`).
    let token_id = Uuid::new_v4();
    // The shim row is never rendered (its id is the only thing the
    // integration reads back), so the bound `now()` for both stamps is
    // interchangeable with the two `now()` calls the inline text used.
    pidash_services::domain::tokens::insert_github_shim_token(
        db,
        &pidash_services::domain::tokens::NewGithubShimToken {
            id: token_id,
            label: format!("github-integration-{workspace_id}"),
            token: format!("github-shim-{}", Uuid::new_v4()),
            actor_id,
            workspace_id,
            now: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|error| GithubTransportError(error.to_string()))?;
    let wi_id = Uuid::new_v4();
    let stamped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    workspace_integrations::ActiveModel {
        id: Set(wi_id),
        created_at: Set(stamped),
        updated_at: Set(stamped),
        created_by_id: Set(Some(actor_id)),
        updated_by_id: Unchanged(None),
        deleted_at: Unchanged(None),
        workspace_id: Set(workspace_id),
        actor_id: Set(actor_id),
        integration_id: Set(integration_id),
        api_token_id: Set(token_id),
        metadata: Set(serde_json::json!({})),
        config: Set(serde_json::json!({})),
    }
    .insert(db)
    .await
    .map_err(|error| GithubTransportError(error.to_string()))?;
    Ok(queries::WorkspaceIntegrationRow {
        id: wi_id,
        workspace_id,
        integration_id,
        config: serde_json::json!({}),
    })
}

/// `GithubAppInstallation.objects.update_or_create(workspace_integration=wi,
/// defaults=...)` (`github.py:261-264`) folded with
/// `_upsert_git_account_for_app` (`github.py:314-352`): both wrote the
/// same facts, so the generic table takes a single account row. Match
/// is by `workspace_integration_id` (the OneToOne's unique), newest
/// first — a reinstall with a new installation id re-points the row
/// instead of orphaning it. Returns the account row id.
/// `BaseModel.save` stamps `created_by` with the request user and
/// leaves `updated_by` NULL on create (`db/models/base.py:36-39`); the
/// conflict branch rewrites the defaults columns only, never
/// `created_by_id` — and never `verified_at`/`last_checked_at`, which
/// the refresh stamps live.
#[allow(clippy::too_many_arguments)]
async fn upsert_account_row(
    db: &impl sea_orm::ConnectionTrait,
    workspace_integration_id: Uuid,
    actor_id: Uuid,
    installation_id: i64,
    account_login: &str,
    account_type: &str,
    repository_selection: &str,
    repository_count: i32,
    permissions: &serde_json::Value,
    events: &serde_json::Value,
    installed_at: Option<DateTime<Utc>>,
    suspended_at: Option<DateTime<Utc>>,
) -> Result<Uuid, GithubTransportError> {
    use pidash_db::entity::{git_provider_accounts, workspace_integrations};
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    };
    let at = |value: Option<DateTime<Utc>>| {
        value.map(|at| at.with_timezone(&chrono::FixedOffset::east_opt(0).expect("zero offset")))
    };
    let status = if suspended_at.is_some() {
        "degraded"
    } else {
        "connected"
    };
    let display_name = if account_login.is_empty() {
        format!("GitHub App {installation_id}")
    } else {
        account_login.to_owned()
    };
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
        "installation_id": installation_id,
    });
    // Metadata keys mirror the 0173 backfill so migrated and
    // newly-written rows read identically.
    let metadata = serde_json::json!({
        "permissions": permissions,
        "events": events,
        "repository_selection": repository_selection,
        "repository_count": repository_count,
        "account_type": account_type,
        "installed_at": installed_at.map(|at| isoformat(&at)),
        "suspended_at": suspended_at.map(|at| isoformat(&at)),
    });
    let existing: Option<Uuid> = git_provider_accounts::Entity::find()
        .select_only()
        .column(git_provider_accounts::Column::Id)
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::Provider.eq("github"))
        .filter(git_provider_accounts::Column::HostUrl.eq("https://github.com"))
        .filter(git_provider_accounts::Column::AuthType.eq("github_app"))
        .filter(
            git_provider_accounts::Column::WorkspaceIntegrationId
                .eq(Some(workspace_integration_id)),
        )
        .order_by_desc(git_provider_accounts::Column::UpdatedAt)
        .limit(1)
        .into_tuple()
        .one(db)
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    let stamped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    match existing {
        Some(account_id) => git_provider_accounts::ActiveModel {
            id: Set(account_id),
            external_account_id: Set(installation_id.to_string()),
            external_account_login: Set(account_login.to_owned()),
            display_name: Set(display_name),
            capabilities: Set(capabilities),
            credential_config: Set(credential_config),
            workspace_integration_id: Set(Some(workspace_integration_id)),
            status: Set(status.to_owned()),
            suspended_at: Set(at(suspended_at)),
            last_check_error: Set(String::new()),
            metadata: Set(metadata),
            updated_at: Set(stamped),
            ..Default::default()
        }
        .update(db)
        .await
        .map(|_| account_id)
        .map_err(|error| GithubTransportError(error.to_string())),
        None => {
            let workspace_id: Uuid = workspace_integrations::Entity::find()
                .select_only()
                .column(workspace_integrations::Column::WorkspaceId)
                .filter(workspace_integrations::Column::DeletedAt.is_null())
                .filter(workspace_integrations::Column::Id.eq(workspace_integration_id))
                .limit(1)
                .into_tuple()
                .one(db)
                .await
                .map_err(|error| GithubTransportError(error.to_string()))?
                .ok_or_else(|| GithubTransportError("workspace integration missing".to_owned()))?;
            let row = git_provider_accounts::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(stamped),
                updated_at: Set(stamped),
                created_by_id: Set(Some(actor_id)),
                updated_by_id: Set(None),
                deleted_at: Set(None),
                workspace_id: Set(workspace_id),
                provider: Set("github".to_owned()),
                host_url: Set("https://github.com".to_owned()),
                auth_type: Set("github_app".to_owned()),
                external_account_id: Set(installation_id.to_string()),
                external_account_login: Set(account_login.to_owned()),
                display_name: Set(display_name),
                capabilities: Set(capabilities),
                credential_config: Set(credential_config),
                workspace_integration_id: Set(Some(workspace_integration_id)),
                status: Set(status.to_owned()),
                verified_at: Set(None),
                suspended_at: Set(at(suspended_at)),
                last_checked_at: Set(None),
                last_check_error: Set(String::new()),
                metadata: Set(metadata),
            }
            .insert(db)
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?;
            Ok(row.id)
        }
    }
}

/// `_upsert_app_installation` (`github.py:229-267`): the transactional
/// workspace-integration + account upsert and verified refresh.
/// `require_verified=True` on the callback path. Everything runs inside
/// one transaction (`with transaction.atomic()`): a refresh failure
/// rolls the upsert back, exactly like Django — the callback then
/// redirects `github_verification_failed` with no account row left
/// behind.
async fn upsert_app_installation(
    pool: &PgPool,
    workspace_id: Uuid,
    actor_id: Uuid,
    installation_payload: &serde_json::Value,
    config: &GithubAppConfig,
) -> Result<AppInstallation, GithubTransportError> {
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
    use pidash_db::entity::{git_provider_accounts, workspaces};
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait};
    let wi = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut transaction);
        get_or_create_workspace_integration(&conn, workspace_id, actor_id).await?
    };
    // The cross-workspace guard (`github.py:256-260`): the same
    // installation id bound elsewhere raises `GithubAppAuthError`.
    // (Rows with a NULL `workspace_integration_id` never satisfy `<>`;
    // every write path here sets the WI.)
    let elsewhere: Option<String> = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut transaction);
        git_provider_accounts::Entity::find()
            .select_only()
            .column(workspaces::Column::Slug)
            .join(
                JoinType::InnerJoin,
                git_provider_accounts::Relation::Workspaces.def(),
            )
            .filter(git_provider_accounts::Column::DeletedAt.is_null())
            .filter(git_provider_accounts::Column::Provider.eq("github"))
            .filter(git_provider_accounts::Column::HostUrl.eq("https://github.com"))
            .filter(git_provider_accounts::Column::AuthType.eq("github_app"))
            .filter(
                git_provider_accounts::Column::ExternalAccountId.eq(installation_id.to_string()),
            )
            .filter(git_provider_accounts::Column::WorkspaceIntegrationId.ne(Some(wi.id)))
            .limit(1)
            .into_tuple()
            .one(&conn)
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?
    };
    if let Some(slug) = elsewhere {
        return Err(GithubTransportError(format!(
            "GitHub installation is already connected to workspace {slug}"
        )));
    }
    let installation = {
        let conn = pidash_db::orm_tx::TxConn::new(&mut transaction);
        let row_id = upsert_account_row(
            &conn,
            wi.id,
            actor_id,
            installation_id,
            &account_login,
            &account_type,
            &repository_selection,
            repository_count,
            &permissions,
            &events,
            installed_at,
            suspended_at,
        )
        .await?;
        let installation = app_installation_by_id(&conn, row_id)
            .await
            .map_err(|error| GithubTransportError(error.to_string()))?
            .ok_or_else(|| GithubTransportError("installation row missing".to_owned()))?;
        refresh_app_installation(&conn, &installation, config, true, &[])
            .await
            .map_err(|outcome| match outcome {
                RefreshOutcome::VerificationFailed(_) => {
                    GithubTransportError("GitHub App connection check failed".to_owned())
                }
                RefreshOutcome::StoreFailed => {
                    GithubTransportError("installation refresh save failed".to_owned())
                }
            })?
    };
    transaction
        .commit()
        .await
        .map_err(|error| GithubTransportError(error.to_string()))?;
    Ok(installation)
}

/// One `git_provider_accounts` row by id.
async fn app_installation_by_id(
    db: &impl sea_orm::ConnectionTrait,
    id: Uuid,
) -> Result<Option<AppInstallation>, sea_orm::DbErr> {
    use pidash_db::entity::git_provider_accounts;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    let row = git_provider_accounts::Entity::find()
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::Id.eq(id))
        .limit(1)
        .one(db)
        .await?;
    Ok(row.map(AppInstallation::from))
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
    use pidash_db::entity::git_provider_install_sessions;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let bumped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    git_provider_install_sessions::Entity::update_many()
        .col_expr(
            git_provider_install_sessions::Column::Status,
            sea_orm::sea_query::Expr::value("failed"),
        )
        .col_expr(
            git_provider_install_sessions::Column::Error,
            sea_orm::sea_query::Expr::value(error),
        )
        .col_expr(
            git_provider_install_sessions::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(bumped),
        )
        .filter(git_provider_install_sessions::Column::Id.eq(session_id))
        .exec(&db)
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
    let session = match queries::fetch_install_session_by_state(
        &pidash_db::pool::sea_orm_connection(pool),
        &session_state,
    )
    .await
    {
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
        use pidash_db::entity::git_provider_install_sessions;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let bumped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
        git_provider_install_sessions::Entity::update_many()
            .col_expr(
                git_provider_install_sessions::Column::Status,
                sea_orm::sea_query::Expr::value("expired"),
            )
            .col_expr(
                git_provider_install_sessions::Column::Error,
                sea_orm::sea_query::Expr::value("Install session expired"),
            )
            .col_expr(
                git_provider_install_sessions::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(bumped),
            )
            .filter(git_provider_install_sessions::Column::Id.eq(session.id))
            .exec(&db)
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
    if !match queries::fetch_is_workspace_admin(
        &pidash_db::pool::sea_orm_connection(pool),
        user.id,
        session.workspace_id,
    )
    .await
    {
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
    {
        use pidash_db::entity::git_provider_install_sessions;
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let db = pidash_db::pool::sea_orm_connection(pool);
        let stamped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
        git_provider_install_sessions::Entity::update_many()
            .col_expr(
                git_provider_install_sessions::Column::ExternalAccountId,
                sea_orm::sea_query::Expr::value(app_installation.installation_id.to_string()),
            )
            .col_expr(
                git_provider_install_sessions::Column::AccountLogin,
                sea_orm::sea_query::Expr::value(app_installation.account_login.clone()),
            )
            .col_expr(
                git_provider_install_sessions::Column::Status,
                sea_orm::sea_query::Expr::value("completed"),
            )
            .col_expr(
                git_provider_install_sessions::Column::CompletedAt,
                sea_orm::sea_query::Expr::value(stamped),
            )
            .col_expr(
                git_provider_install_sessions::Column::Error,
                sea_orm::sea_query::Expr::value(""),
            )
            .col_expr(
                git_provider_install_sessions::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(stamped),
            )
            .filter(git_provider_install_sessions::Column::Id.eq(session.id))
            .exec(&db)
            .await
            .ok();
    }
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
    fn store_error(error: sea_orm::DbErr) -> String {
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    #[derive(Debug, sea_orm::FromQueryResult)]
    #[allow(non_snake_case)]
    struct LinkRow {
        id: Uuid,
        pr_updated_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    }
    #[derive(Debug, sea_orm::FromQueryResult)]
    #[allow(non_snake_case)]
    struct ReviewRow {
        id: Uuid,
        remote_updated_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
        metadata: serde_json::Value,
    }
    use pidash_db::entity::{git_code_review_links, github_pull_request_links};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
    let links: Vec<LinkRow> = github_pull_request_links::Entity::find()
        .select_only()
        .column(github_pull_request_links::Column::Id)
        .column(github_pull_request_links::Column::PrUpdatedAt)
        .filter(github_pull_request_links::Column::DeletedAt.is_null())
        .filter(github_pull_request_links::Column::RepoOwner.eq(owner.clone()))
        .filter(github_pull_request_links::Column::RepoName.eq(name.clone()))
        .filter(github_pull_request_links::Column::PrNumber.eq(number as i32))
        .into_model::<LinkRow>()
        .all(&db)
        .await
        .map_err(store_error)?;
    let stamp = |value: Option<DateTime<Utc>>| {
        value.map(|at| at.with_timezone(&chrono::FixedOffset::east_opt(0).expect("zero offset")))
    };
    let stored_utc = |value: Option<sea_orm::prelude::DateTimeWithTimeZone>| {
        value.map(|at| at.with_timezone(&chrono::Utc))
    };
    let bumped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    for link in &links {
        matched += 1;
        let stored = stored_utc(link.pr_updated_at);
        // Stale / out-of-order delivery — matched but not applied
        // (`github.py:835-836`).
        if let (Some(incoming), Some(current)) = (snapshot.pr_updated_at, stored) {
            if incoming < current {
                continue;
            }
        }
        github_pull_request_links::Entity::update_many()
            .col_expr(
                github_pull_request_links::Column::Title,
                sea_orm::sea_query::Expr::value(snapshot.title.clone()),
            )
            .col_expr(
                github_pull_request_links::Column::State,
                sea_orm::sea_query::Expr::value(snapshot.state.clone()),
            )
            .col_expr(
                github_pull_request_links::Column::Merged,
                sea_orm::sea_query::Expr::value(snapshot.merged),
            )
            .col_expr(
                github_pull_request_links::Column::Draft,
                sea_orm::sea_query::Expr::value(snapshot.draft),
            )
            .col_expr(
                github_pull_request_links::Column::PrUpdatedAt,
                sea_orm::sea_query::Expr::value(stamp(snapshot.pr_updated_at)),
            )
            .col_expr(
                github_pull_request_links::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(bumped),
            )
            .filter(github_pull_request_links::Column::Id.eq(link.id))
            .exec(&db)
            .await
            .map_err(store_error)?;
    }
    let reviews: Vec<ReviewRow> = git_code_review_links::Entity::find()
        .select_only()
        .column(git_code_review_links::Column::Id)
        .column(git_code_review_links::Column::RemoteUpdatedAt)
        .column(git_code_review_links::Column::Metadata)
        .filter(git_code_review_links::Column::DeletedAt.is_null())
        .filter(git_code_review_links::Column::Provider.eq("github"))
        .filter(git_code_review_links::Column::HostUrl.eq("https://github.com"))
        .filter(git_code_review_links::Column::Namespace.eq(owner.clone()))
        .filter(git_code_review_links::Column::RepoName.eq(name.clone()))
        .filter(git_code_review_links::Column::ExternalIid.eq(number.to_string()))
        .into_model::<ReviewRow>()
        .all(&db)
        .await
        .map_err(store_error)?;
    for review in &reviews {
        matched += 1;
        let stored = stored_utc(review.remote_updated_at);
        if let (Some(incoming), Some(current)) = (snapshot.pr_updated_at, stored) {
            if incoming < current {
                continue;
            }
        }
        let mut metadata = review.metadata.as_object().cloned().unwrap_or_default();
        metadata.insert("remote".to_owned(), pull_value.clone());
        let review_state = if snapshot.merged {
            "merged".to_owned()
        } else if snapshot.state.is_empty() {
            "open".to_owned()
        } else {
            snapshot.state.clone()
        };
        git_code_review_links::Entity::update_many()
            .col_expr(
                git_code_review_links::Column::Title,
                sea_orm::sea_query::Expr::value(snapshot.title.clone()),
            )
            .col_expr(
                git_code_review_links::Column::State,
                sea_orm::sea_query::Expr::value(review_state),
            )
            .col_expr(
                git_code_review_links::Column::Merged,
                sea_orm::sea_query::Expr::value(snapshot.merged),
            )
            .col_expr(
                git_code_review_links::Column::Draft,
                sea_orm::sea_query::Expr::value(snapshot.draft),
            )
            .col_expr(
                git_code_review_links::Column::RemoteUpdatedAt,
                sea_orm::sea_query::Expr::value(stamp(snapshot.pr_updated_at)),
            )
            .col_expr(
                git_code_review_links::Column::Metadata,
                sea_orm::sea_query::Expr::value(serde_json::Value::Object(metadata)),
            )
            .col_expr(
                git_code_review_links::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(bumped),
            )
            .filter(git_code_review_links::Column::Id.eq(review.id))
            .exec(&db)
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
    use pidash_db::entity::github_webhook_deliveries;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, Set};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let stamped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
    let inserted =
        github_webhook_deliveries::Entity::insert(github_webhook_deliveries::ActiveModel {
            id: Set(Uuid::new_v4()),
            created_at: Set(stamped),
            updated_at: Set(stamped),
            created_by_id: Set(None),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            delivery_id: Set(delivery_uuid),
            event: Set(event.clone()),
            action: Set(action.clone()),
            installation_id: Set(installation_id),
            payload: Set(payload.clone()),
            status: Set("received".to_owned()),
            received_at: Set(stamped),
            processed_at: Set(None),
            error: Set(String::new()),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(github_webhook_deliveries::Column::DeliveryId)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(&db)
        .await;
    let created = match inserted {
        Ok(rows) => rows == 1,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if !created {
        // Retries and redeliveries reuse the GUID: fast-return only when
        // a prior attempt reached terminal success (`processed` /
        // `skipped`); `received` (interrupted) and `failed` reprocess so
        // the snapshot still converges (`github.py:904-917`).
        let prior: Option<String> = match github_webhook_deliveries::Entity::find()
            .select_only()
            .column(github_webhook_deliveries::Column::Status)
            .filter(github_webhook_deliveries::Column::DeliveryId.eq(delivery_uuid))
            .limit(1)
            .into_tuple()
            .one(&db)
            .await
        {
            Ok(row) => row,
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
    {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let stamped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
        github_webhook_deliveries::Entity::update_many()
            .col_expr(
                github_webhook_deliveries::Column::Status,
                sea_orm::sea_query::Expr::value(status.clone()),
            )
            .col_expr(
                github_webhook_deliveries::Column::Error,
                sea_orm::sea_query::Expr::value(error),
            )
            .col_expr(
                github_webhook_deliveries::Column::ProcessedAt,
                sea_orm::sea_query::Expr::value(stamped),
            )
            .col_expr(
                github_webhook_deliveries::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(stamped),
            )
            .filter(github_webhook_deliveries::Column::DeliveryId.eq(delivery_uuid))
            .exec(&db)
            .await
            .ok();
    }
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
            {
                use pidash_db::entity::git_provider_accounts;
                use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
                let db = pidash_db::pool::sea_orm_connection(pool);
                let bumped: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
                git_provider_accounts::Entity::update_many()
                    .col_expr(
                        git_provider_accounts::Column::SuspendedAt,
                        sea_orm::sea_query::Expr::value(installation.suspended_at.map(|at| {
                            at.with_timezone(&chrono::FixedOffset::east_opt(0).expect("zero"))
                        })),
                    )
                    .col_expr(
                        git_provider_accounts::Column::LastCheckError,
                        sea_orm::sea_query::Expr::value(installation.last_check_error.clone()),
                    )
                    .col_expr(
                        git_provider_accounts::Column::UpdatedAt,
                        sea_orm::sea_query::Expr::value(bumped),
                    )
                    .filter(git_provider_accounts::Column::Id.eq(installation.id))
                    .exec(&db)
                    .await
                    .map_err(|error| error.to_string())?;
            }
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
            let connection = pidash_db::orm_tx::TxConn::new(&mut connection);
            refresh_app_installation(
                &connection,
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
            let connection = pidash_db::orm_tx::TxConn::new(&mut connection);
            refresh_app_installation(&connection, &installation, &config, false, &[])
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

/// One `git_provider_accounts` row by `installation_id`
/// (`github.py:930-931`). The (`provider`, `host_url`,
/// `external_account_id`) index keeps webhook lookups indexed.
async fn installation_by_installation_id(
    pool: &PgPool,
    installation_id: i64,
) -> Result<Option<AppInstallation>, sea_orm::DbErr> {
    use pidash_db::entity::git_provider_accounts;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = git_provider_accounts::Entity::find()
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::Provider.eq("github"))
        .filter(git_provider_accounts::Column::HostUrl.eq("https://github.com"))
        .filter(git_provider_accounts::Column::AuthType.eq("github_app"))
        .filter(git_provider_accounts::Column::ExternalAccountId.eq(installation_id.to_string()))
        .order_by_desc(git_provider_accounts::Column::UpdatedAt)
        .limit(1)
        .one(&db)
        .await?;
    Ok(row.map(AppInstallation::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_installation() -> AppInstallation {
        AppInstallation {
            id: Uuid::nil(),
            workspace_integration_id: None,
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
            metadata: serde_json::json!({}),
        }
    }

    /// Live-kernel DB pool (PIDASHCONV-1083 convention): unset
    /// DATABASE_URL (plain `cargo test`) skips these tests.
    async fn kernel_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-kernel test: DATABASE_URL is not set");
                None
            }
        }
    }

    /// Seed an active user + workspace (column lists mirror
    /// `_harness.db.Database.make_user/make_workspace`). Tag-derived
    /// names keep reruns collision-free.
    async fn seed_kernel_world(pool: &sqlx::PgPool, tag: &str) -> (Uuid, Uuid) {
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let now = Utc::now();
        sqlx::query(
            r#"INSERT INTO "users" ("password", "id", "username", "email", "first_name", "last_name",
                "avatar", "date_joined", "created_at", "updated_at", "last_location",
                "created_location", "is_superuser", "is_managed", "is_password_expired",
                "is_active", "is_staff", "is_email_verified", "is_password_autoset",
                "token", "user_timezone", "last_login_ip", "last_logout_ip",
                "last_login_medium", "last_login_uagent", "is_bot", "display_name",
                "is_email_valid", "is_password_reset_required")
               VALUES ('pbkdf2_sha256$600000$kernelseed$kernelhash', $1, $2, $3, 'Kernel', 'User',
                '', $4, $4, $4, '', '', FALSE, FALSE, FALSE, TRUE, FALSE, TRUE, FALSE,
                '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
                'UTC', '', '', 'email', '', FALSE, 'Kernel User', TRUE, FALSE)"#,
        )
        .bind(user_id)
        .bind(format!("k1465a-{tag}"))
        .bind(format!("k1465a-{tag}@example.test"))
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            r#"INSERT INTO "workspaces" ("created_at", "updated_at", "id", "name", "slug",
                "owner_id", "timezone", "background_color")
               VALUES ($1, $1, $2, $3, $4, $5, 'UTC', '#ffffff')"#,
        )
        .bind(now)
        .bind(workspace_id)
        .bind(format!("kernel ws {tag}"))
        .bind(format!("k1465a-{tag}"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed workspace");
        (user_id, workspace_id)
    }

    async fn cleanup_kernel_world(
        pool: &sqlx::PgPool,
        user_id: Uuid,
        workspace_id: Uuid,
        integration_id: Option<Uuid>,
    ) {
        sqlx::query(r#"DELETE FROM "git_provider_accounts" WHERE "workspace_id" = $1"#)
            .bind(workspace_id)
            .execute(pool)
            .await
            .expect("cleanup accounts");
        sqlx::query(r#"DELETE FROM "git_provider_install_sessions" WHERE "workspace_id" = $1"#)
            .bind(workspace_id)
            .execute(pool)
            .await
            .expect("cleanup install sessions");
        sqlx::query(r#"DELETE FROM "workspace_integrations" WHERE "workspace_id" = $1"#)
            .bind(workspace_id)
            .execute(pool)
            .await
            .expect("cleanup workspace integrations");
        sqlx::query(r#"DELETE FROM "api_tokens" WHERE "workspace_id" = $1"#)
            .bind(workspace_id)
            .execute(pool)
            .await
            .expect("cleanup tokens");
        if let Some(integration_id) = integration_id {
            sqlx::query(r#"DELETE FROM "integrations" WHERE "id" = $1"#)
                .bind(integration_id)
                .execute(pool)
                .await
                .expect("cleanup integration");
        }
        sqlx::query(r#"DELETE FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .execute(pool)
            .await
            .expect("cleanup workspace");
        sqlx::query(r#"DELETE FROM "users" WHERE "id" = $1"#)
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup user");
    }

    /// PIDASHCONV-1465: the callback SQL kernels must stamp `created_by`
    /// with the request actor and leave `updated_by` NULL on create,
    /// exactly like `BaseModel.save` (`db/models/base.py:36-39`). The
    /// HTTP callback is sealed in tests (the exchange chain posts to
    /// hardcoded https://api.github.com URLs on both backends), so this
    /// drives `get_or_create_workspace_integration` and
    /// `upsert_account_row` directly with fabricated inputs — the
    /// oauth_github kernel convention (PIDASHCONV-1083).
    #[tokio::test]
    async fn callback_kernels_stamp_created_by() {
        let Some(pool) = kernel_pool().await else {
            return;
        };
        let tag = Uuid::new_v4().simple().to_string();
        let tag = tag[..8].to_owned();
        let (user_id, workspace_id) = seed_kernel_world(&pool, &tag).await;
        // The `github` integrations row is shared process-wide: only
        // delete it in cleanup when this run created it.
        let integration_preexisted: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "integrations" WHERE "provider" = 'github')"#,
        )
        .fetch_one(&pool)
        .await
        .expect("integration probe");
        let mut connection = pool.acquire().await.expect("connection");
        let wi = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut connection);
            get_or_create_workspace_integration(&conn, workspace_id, user_id)
                .await
                .expect("workspace integration")
        };
        let token_stamps: (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            r#"SELECT "created_by_id", "updated_by_id" FROM "api_tokens" WHERE "workspace_id" = $1"#,
        )
        .bind(workspace_id)
        .fetch_one(&mut *connection)
        .await
        .expect("token row");
        assert_eq!(token_stamps.0, Some(user_id), "api_tokens created_by");
        assert_eq!(token_stamps.1, None, "api_tokens updated_by");
        let wi_stamps: (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            r#"SELECT "created_by_id", "updated_by_id" FROM "workspace_integrations" WHERE "id" = $1"#,
        )
        .bind(wi.id)
        .fetch_one(&mut *connection)
        .await
        .expect("workspace integration row");
        assert_eq!(
            wi_stamps.0,
            Some(user_id),
            "workspace_integrations created_by"
        );
        assert_eq!(wi_stamps.1, None, "workspace_integrations updated_by");
        // `installation_id` is globally unique: random per run.
        let installation_id = 1_000_000_000 + (Uuid::new_v4().as_u128() % 8_000_000_000) as i64;
        let row_id = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut connection);
            upsert_account_row(
                &conn,
                wi.id,
                user_id,
                installation_id,
                "kernel-octocat",
                "Organization",
                "selected",
                3,
                &serde_json::json!({}),
                &serde_json::json!([]),
                None,
                None,
            )
            .await
            .expect("account row")
        };
        let account_stamps: (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            r#"SELECT "created_by_id", "updated_by_id" FROM "git_provider_accounts" WHERE "id" = $1"#,
        )
        .bind(row_id)
        .fetch_one(&mut *connection)
        .await
        .expect("account row");
        assert_eq!(
            account_stamps.0,
            Some(user_id),
            "git_provider_accounts created_by"
        );
        assert_eq!(account_stamps.1, None, "git_provider_accounts updated_by");
        let installation = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut connection);
            app_installation_by_id(&conn, row_id)
                .await
                .expect("fetch installation")
                .expect("installation present")
        };
        assert_eq!(
            installation.installation_id, installation_id,
            "external id round-trips"
        );
        assert_eq!(installation.account_type, "Organization");
        assert_eq!(installation.repository_count, 3);
        // The conflict branch must rewrite the defaults without
        // touching `created_by_id`: re-upsert the same workspace
        // integration with a different actor and installation id. (The
        // proposed row conflicts, so the unknown actor never reaches
        // the table; `updated_by` on this branch is the known
        // out-of-scope divergence and is deliberately not asserted.)
        let other_actor = Uuid::new_v4();
        let installation_id_2 = 1_000_000_000 + (Uuid::new_v4().as_u128() % 8_000_000_000) as i64;
        let row_id_2 = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut connection);
            upsert_account_row(
                &conn,
                wi.id,
                other_actor,
                installation_id_2,
                "kernel-octocat-2",
                "Organization",
                "selected",
                5,
                &serde_json::json!({}),
                &serde_json::json!([]),
                None,
                None,
            )
            .await
            .expect("account re-upsert")
        };
        assert_eq!(row_id_2, row_id, "conflict branch returns same row");
        let restamps: (Option<Uuid>, String) = sqlx::query_as(
            r#"SELECT "created_by_id", "external_account_id" FROM "git_provider_accounts" WHERE "id" = $1"#,
        )
        .bind(row_id)
        .fetch_one(&mut *connection)
        .await
        .expect("account row after conflict");
        assert_eq!(restamps.0, Some(user_id), "conflict keeps created_by");
        assert_eq!(
            restamps.1,
            installation_id_2.to_string(),
            "conflict rewrites defaults"
        );
        drop(connection);
        cleanup_kernel_world(
            &pool,
            user_id,
            workspace_id,
            (!integration_preexisted).then_some(wi.integration_id),
        )
        .await;
    }

    #[test]
    fn isoformat_matches_python_isoformat() {
        // Whole seconds render no fraction; trailing-zero micros keep
        // six digits; nanosecond in-memory stamps (the refresh path)
        // truncate to microseconds (PIDASHCONV-1464).
        let whole = chrono::DateTime::parse_from_rfc3339("2026-10-22T10:00:05+00:00")
            .expect("parses")
            .with_timezone(&Utc);
        assert_eq!(isoformat(&whole), "2026-10-22T10:00:05+00:00");
        let trailing = chrono::DateTime::parse_from_rfc3339("2026-10-22T10:00:05.12+00:00")
            .expect("parses")
            .with_timezone(&Utc);
        assert_eq!(isoformat(&trailing), "2026-10-22T10:00:05.120000+00:00");
        let nanos = chrono::DateTime::parse_from_rfc3339("2026-10-22T10:00:05.123456789+00:00")
            .expect("parses")
            .with_timezone(&Utc);
        assert_eq!(isoformat(&nanos), "2026-10-22T10:00:05.123456+00:00");
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
