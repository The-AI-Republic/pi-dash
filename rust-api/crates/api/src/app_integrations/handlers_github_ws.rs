//! Workspace GitHub PAT handlers (D-33, stage 5, PIDASHCONV-446).
//!
//! Port of `GithubIntegrationConnectEndpoint` (POST `connect/`)
//! (`apps/api/pi_dash/app/views/integration/github.py:426-492`),
//! `GithubIntegrationDisconnectEndpoint` (POST `disconnect/`, `:494-537`),
//! `GithubIntegrationStatusEndpoint` (GET status, `:540-563`), and
//! `GithubIntegrationReposEndpoint` (GET `repos/`, `:565-606`).
//!
//! Routes (`app/urls/integration.py:59-79`):
//!
//! - `GET /api/workspaces/<slug>/integrations/github/`
//! - `POST /api/workspaces/<slug>/integrations/github/connect/`
//! - `POST /api/workspaces/<slug>/integrations/github/disconnect/`
//! - `GET /api/workspaces/<slug>/integrations/github/repos/?page=N`
//!
//! Fixture ids: FX-GHA-02
//! (`rust-api/fixtures/app_integrations/fx-gha-02-workspace.json`),
//! FX-PERM-01 (gate rows; decision via [`super::gates`], ported by
//! PIDASHCONV-436).
//!
//! Gate order (preserved, not redesigned): DRF session auth (401 anonymous)
//! runs before the `@allow_permission` decorator (403), which runs before
//! the handler body — so the `_feature_enabled` 404 fires only *after* a
//! passing gate. Connect reads the workspace row *after* the live GitHub
//! call (`github.py:448`); status/repos/disconnect read it first.
//!
//! The live GitHub calls mirror `pi_dash/utils/github_client.py`
//! (`GithubClient.get_authenticated_user`, `list_user_repos`): `Bearer`
//! auth, the `2022-11-28` API version header, 30s timeout, and the exact
//! error mapping per call site (connect maps 403/404 to 400
//! `{"error": "GitHub error: ..."}`, repos lets them fall through to the
//! 502). The `affiliation` filter required to surface org repos (`§6.1`)
//! is sent verbatim.
//!
//! Write semantics (translate, don't redesign):
//!
//! - Connect runs under `transaction.atomic()` (`github.py:450`): the
//!   `Integration(provider="github")` get-or-create, the `APIToken` FK
//!   shim + `WorkspaceIntegration` create on first connect, the
//!   `config`-only save, and the PAT `GitProviderAccount` upsert. The
//!   statements below run sequentially on the pool; each is atomic.
//! - `save(update_fields=["config"])` writes `config` only — `updated_at`
//!   is untouched (Django only writes the listed fields).
//! - `QuerySet.update()` never touches `auto_now` fields (verified against
//!   Django 4.2 `UpdateQuery.add_update_values`), so the disconnect
//!   updates name every column they change.
//!
//! Ported quirks (also listed in the PR):
//!
//! - BUG-config-plaintext-status (`github.py:553`): status checks the
//!   *stored* `config.token` truthiness without decrypting, so a row whose
//!   token fails decryption still reads connected until repos/disconnect
//!   proves otherwise. Kept.
//! - QUIRK-disconnect-no-row (`github.py:508-510`): disconnect with no
//!   `WorkspaceIntegration` row answers `200 {"connected": false}` — not
//!   an error. Kept.
//! - QUIRK-connect-workspace-after-verify (`github.py:440-448`): a bogus
//!   token on a nonexistent workspace answers 401, not 404. Kept.
//! - QUIRK-empty-config-token (`github.py:581-583`): repos decrypts the
//!   stored token and 409s when it decrypts empty — including a stored
//!   plaintext dummy that Fernet rejects (decrypt failure reads as `""`).
//!   Kept via the shared [`decrypt_token`] (same fail-closed-to-`""`
//!   semantics as `decrypt_data`).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

use super::gates::{decide_gate, tenant_context, Gate, GateOutcome, GITHUB_DISABLED_BODY};
use crate::license::{resolve_actor, Actor};
use crate::state::AppState;
use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_types::WorkspaceId;

/// `@allow_permission([ADMIN])` rows (connect, disconnect).
const ADMIN_ONLY: &[i32] = &[ROLE_ADMIN];
/// `@allow_permission([ADMIN, MEMBER])` rows (status, repos).
const ADMIN_MEMBER: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER];

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the four workspace-GitHub routes under `api/workspaces/`.
///
/// Nothing else: sibling D-33 paths (the GitHub App flow, project bind,
/// git providers, webhooks, external — sibling handler issues) stay
/// unmatched and proxy to Django. Unowned methods on these paths proxy
/// too (DRF authenticates before it checks the method, so answering 405
/// in Rust would break the contract); `HEAD` rides axum's `get` handling
/// like Django's `GET`-backed `HEAD`; `OPTIONS` proxies so DRF metadata
/// is preserved.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/integrations/github/",
            owned(axum::routing::get(status), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/integrations/github/connect/",
            owned(axum::routing::post(connect), &["POST"]),
        )
        .route(
            "/api/workspaces/{slug}/integrations/github/disconnect/",
            owned(axum::routing::post(disconnect), &["POST"]),
        )
        .route(
            "/api/workspaces/{slug}/integrations/github/repos/",
            owned(axum::routing::get(repos), &["GET"]),
        )
}

/// An owned path: the owned methods serve from Rust, every other method
/// falls through to Django (its 405-after-auth and metadata responses
/// live there).
fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body. Variants map one-to-one
/// onto the Python branches: decorator denial, `get_object_or_404`, the
/// disabled flag, and the view-inline error shapes (including the
/// per-call-site GitHub error mapping).
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded endpoint).
    Unauthorized,
    /// 403, `@allow_permission` allow-style body
    /// (`app/permissions/base.py:80-84`).
    Forbidden,
    /// 404, DRF `Http404` default body (`get_object_or_404`).
    NotFoundDetail,
    /// 400, `{"detail": ...}` (unparseable connect body: DRF `ParseError`,
    /// `JSON parse error - ...`).
    BadDetail(String),
    /// 404, `_disabled_response` (`github.py:85-87`).
    Disabled,
    /// 400, `{"error": ...}` (view-inline: blank PAT, GitHub 403/404 on
    /// connect, malformed `token` shape is a 500 instead — see below).
    BadError(String),
    /// 401, `{"error": ...}` (GitHub rejected the credential).
    UnauthorizedError(String),
    /// 409, `{"error": ...}` (stored credential decrypts empty).
    ConflictError(String),
    /// 502, `{"error": ...}` (GitHub call failed beyond the mapped
    /// statuses; `log_exception` in Python).
    BadGatewayError(String),
    /// 500, generic branch (`handle_exception` fallback; also the
    /// `AttributeError` a truthy non-string `token` raises on `.strip()`).
    ServerError,
}

/// Exact bytes of DRF's default `Http404` body.
pub const NOT_FOUND_DETAIL_BODY: &str = r#"{"detail":"Not found."}"#;

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                crate::license::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, NOT_FOUND_DETAIL_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::Disabled => (StatusCode::NOT_FOUND, GITHUB_DISABLED_BODY.to_owned()),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::UnauthorizedError(message) => (
                StatusCode::UNAUTHORIZED,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ConflictError(message) => (
                StatusCode::CONFLICT,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadGatewayError(message) => (
                StatusCode::BAD_GATEWAY,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::license::SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        // The 502 call sites log the upstream text themselves
        // (`log_exception`, `github.py:445,595`); only the 500 stays here.
        if matches!(self, Denial::ServerError) {
            tracing::warn!("github workspace handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn json_response(value: Value) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(&value).expect("json response"),
        ))
        .expect("json response")
}

// ---------------------------------------------------------------------------
// Shared preamble
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `BaseSessionAuthentication` + `IsAuthenticated` (`views/base.py:189-194`):
/// full session verification (backend, hash, active row); anonymous callers
/// never reach a gate.
async fn actor_of(
    state: &AppState,
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Actor, Denial> {
    resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

/// `@allow_permission(..., level="WORKSPACE")`: an active workspace
/// membership with a listed role (`app/permissions/base.py:44-51`). The
/// joined `workspaces` row carries no `deleted_at` filter — Django does
/// not filter joined tables — while the membership row uses the default
/// manager (`deleted_at IS NULL`) plus `is_active`.
async fn gate_workspace(
    pool: &sqlx::PgPool,
    slug: &str,
    actor_id: &uuid::Uuid,
    gate: Gate,
) -> Result<(), Denial> {
    let Gate::Workspace { roles: allowed } = gate else {
        return Err(Denial::ServerError);
    };
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(actor_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let role = row.map(|row| row.0 as i32);
    let facts = AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.map(|role| allowed.contains(&role)).unwrap_or(false),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(pidash_auth::permissions::ROLE_ADMIN),
    };
    match decide_gate(&gate, &tenant_context(slug), &facts) {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Deny => Err(Denial::Forbidden),
        GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

/// `get_object_or_404(Workspace, slug=slug)`: the live row's id (default
/// manager, so soft-deleted slugs 404 with the DRF detail body).
async fn workspace_id(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFoundDetail)
}

/// `_feature_enabled()` (`github.py:82-83`): `GITHUB_SYNC_ENABLED`,
/// default on. Fires after the gate (it lives in the handler body).
fn feature_enabled(state: &AppState) -> Result<(), Denial> {
    if state.settings().github_sync_enabled {
        Ok(())
    } else {
        Err(Denial::Disabled)
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// One `workspace_integrations` row for the status/repos/disconnect reads:
/// id plus the `config` JSON the handlers inspect
/// (`github.py:508,553,581`).
struct WorkspaceIntegrationRow {
    id: uuid::Uuid,
    config: Value,
}

/// `_get_workspace_integration` (`github.py:102-106`):
/// `Integration(provider="github")` then the workspace's row for it, both
/// through the default manager. `None` when either is missing.
async fn workspace_integration(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
) -> Result<Option<WorkspaceIntegrationRow>, Denial> {
    let row: Option<(uuid::Uuid, Value)> = sqlx::query_as(
        r#"SELECT wi.id, wi.config FROM workspace_integrations wi
           JOIN integrations i ON i.id = wi.integration_id
           WHERE wi.workspace_id = $1 AND i.provider = 'github'
           AND wi.deleted_at IS NULL AND i.deleted_at IS NULL
           ORDER BY wi.id ASC LIMIT 1"#,
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| WorkspaceIntegrationRow {
        id: row.0,
        config: row.1,
    }))
}

/// `(wi.config or {})` (`github.py:508,553,581`): falsy configs read as
/// `{}`; a truthy non-object has no `.get` (`AttributeError` → base 500).
fn config_or_empty(config: &Value) -> Result<Option<&serde_json::Map<String, Value>>, Denial> {
    match config {
        Value::Object(map) => Ok(Some(map)),
        Value::Null => Ok(None),
        Value::Bool(false) => Ok(None),
        Value::Number(n) if json_number_is_zero(n) => Ok(None),
        Value::String(s) if s.is_empty() => Ok(None),
        Value::Array(a) if a.is_empty() => Ok(None),
        _ => Err(Denial::ServerError),
    }
}

/// Python truthiness for JSON values (`or` chains, `github.py:434,553`).
fn value_is_falsy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Bool(flag)) => !flag,
        Some(Value::Number(n)) => json_number_is_zero(n),
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::Object(o)) => o.is_empty(),
    }
}

/// `x or ""`: falsy JSON reads `""`, truthy values pass through raw
/// (`github.py:272-277,434`).
fn or_empty(value: Option<&Value>) -> Value {
    if value_is_falsy(value) {
        Value::String(String::new())
    } else {
        value.cloned().unwrap_or(Value::Null)
    }
}

/// The status check (`github.py:552-562`): `wi is None or not
/// (wi.config or {}).get("token")` reads disconnected — the stored value's
/// truthiness, never decrypted (the ported BUG-config-plaintext-status).
/// Connected responses echo the raw stored values (`.get`, so a missing
/// key renders `null`, exactly like DRF).
fn status_body(config: &Value) -> Result<Value, Denial> {
    let map = config_or_empty(config)?;
    let token = map.and_then(|map| map.get("token"));
    if value_is_falsy(token) {
        return Ok(serde_json::json!({"connected": false}));
    }
    let map = map.expect("connected implies an object config");
    Ok(serde_json::json!({
        "connected": true,
        "github_user_login": map.get("github_user_login").cloned().unwrap_or(Value::Null),
        "verified_at": map.get("verified_at").cloned().unwrap_or(Value::Null),
    }))
}

/// `GET .../integrations/github/` (`github.py:540-563`): cheap, no GitHub
/// call. ADMIN or MEMBER at WORKSPACE level (`:547`).
async fn status(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor_of(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate_workspace(
        &pool,
        &slug,
        &actor.id,
        Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
    )
    .await
    {
        return denial.into_response();
    }
    if let Err(denial) = feature_enabled(&state) {
        return denial.into_response();
    }
    let workspace = match workspace_id(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let wi = match workspace_integration(&pool, &workspace).await {
        Ok(wi) => wi,
        Err(denial) => return denial.into_response(),
    };
    match wi {
        None => json_response(serde_json::json!({"connected": false})),
        Some(wi) => match status_body(&wi.config) {
            Ok(body) => json_response(body),
            Err(denial) => denial.into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// Disconnect
// ---------------------------------------------------------------------------

/// Soft-disconnect error stamp shared by the three updates
/// (`github.py:520,530,534`).
const DISCONNECT_ERROR: &str = "Workspace GitHub integration disconnected";

/// `POST .../integrations/github/disconnect/` (`github.py:494-537`):
/// ADMIN at WORKSPACE level (`:502`). Soft-disconnect: clears the
/// credential on `config`, flips dependent syncs/bindings off, revokes the
/// PAT accounts — rows stay (no cascade-delete trap, `§6.1`).
async fn disconnect(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor_of(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate_workspace(
        &pool,
        &slug,
        &actor.id,
        Gate::Workspace { roles: ADMIN_ONLY },
    )
    .await
    {
        return denial.into_response();
    }
    if let Err(denial) = feature_enabled(&state) {
        return denial.into_response();
    }
    let workspace = match workspace_id(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let wi = match workspace_integration(&pool, &workspace).await {
        Ok(wi) => wi,
        Err(denial) => return denial.into_response(),
    };
    // No row is not an error (`github.py:508-510`).
    let Some(wi) = wi else {
        return json_response(serde_json::json!({"connected": false}));
    };
    if let Err(denial) = apply_disconnect(&pool, &workspace, &wi).await {
        return denial.into_response();
    }
    json_response(serde_json::json!({"connected": false}))
}

/// The disconnect writes (`github.py:512-536`, under
/// `transaction.atomic()`):
///
/// - `wi.config.token = ""` + `disconnected_at = now`, config-only save;
/// - `GithubRepositorySync(workspace_integration=wi)` off with the stamp;
/// - `GitRepositoryBinding`s of the PAT accounts off with the stamp;
/// - PAT `GitProviderAccount`s to REVOKED with the stamp and an emptied
///   `credential_config`.
async fn apply_disconnect(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    wi: &WorkspaceIntegrationRow,
) -> Result<(), Denial> {
    // `config = wi.config or {}` (`github.py:513`): a truthy non-object
    // fails the item assignment below (`TypeError` → 500), like Python.
    let mut config = match config_or_empty(&wi.config)? {
        Some(map) => Value::Object(map.clone()),
        None => serde_json::json!({}),
    };
    config["token"] = Value::String(String::new());
    config["disconnected_at"] = Value::String(now_iso());
    sqlx::query(r#"UPDATE workspace_integrations SET config = $1 WHERE id = $2"#)
        .bind(&config)
        .bind(wi.id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    sqlx::query(
        r#"UPDATE github_repository_syncs
           SET is_sync_enabled = false, last_sync_error = $1
           WHERE workspace_integration_id = $2"#,
    )
    .bind(DISCONNECT_ERROR)
    .bind(wi.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // The PAT accounts of this integration (`github.py:522-527`): their
    // bindings go off first, then the accounts are revoked.
    disconnect_pat_accounts(pool, workspace_id, &wi.id).await
}

/// Flip the PAT bindings + accounts off. The account ids feed the binding
/// update (`provider_account__in=pat_accounts`, `github.py:528-531`)
/// before the accounts themselves are revoked (`:532-536`).
async fn disconnect_pat_accounts(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    wi_id: &uuid::Uuid,
) -> Result<(), Denial> {
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM git_provider_accounts
           WHERE workspace_id = $1 AND provider = 'github' AND auth_type = 'pat'
           AND workspace_integration_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .bind(wi_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if !ids.is_empty() {
        sqlx::query(
            r#"UPDATE git_repository_bindings
               SET is_sync_enabled = false, last_sync_error = $1
               WHERE provider_account_id = ANY($2)"#,
        )
        .bind(DISCONNECT_ERROR)
        .bind(&ids)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    let emptied = serde_json::json!({
        "auth_type": "pat",
        "host_url": "https://github.com",
        "token": "",
    });
    sqlx::query(
        r#"UPDATE git_provider_accounts
           SET status = 'revoked', last_check_error = $1, credential_config = $2
           WHERE workspace_id = $3 AND provider = 'github' AND auth_type = 'pat'
           AND workspace_integration_id = $4 AND deleted_at IS NULL"#,
    )
    .bind(DISCONNECT_ERROR)
    .bind(&emptied)
    .bind(workspace_id)
    .bind(wi_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Repos
// ---------------------------------------------------------------------------

/// `GET .../integrations/github/repos/?page=N` (`github.py:565-606`):
/// paginated browse of the repos visible to the connected PAT. ADMIN or
/// MEMBER at WORKSPACE level (`:572`).
async fn repos(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor_of(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate_workspace(
        &pool,
        &slug,
        &actor.id,
        Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
    )
    .await
    {
        return denial.into_response();
    }
    if let Err(denial) = feature_enabled(&state) {
        return denial.into_response();
    }
    let workspace = match workspace_id(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let wi = match workspace_integration(&pool, &workspace).await {
        Ok(wi) => wi,
        Err(denial) => return denial.into_response(),
    };
    // NOTE: this denial is a 404 (`github.py:579`), unlike every other
    // view-inline error on these paths.
    let Some(wi) = wi else {
        return repos_not_connected();
    };
    // `decrypt_data((wi.config or {}).get("token") or "")` (`github.py:581`):
    // only a non-empty string reaches Fernet — every other shape decrypts
    // as `""` (falsy short-circuit, or the caught `AttributeError`).
    let map = match config_or_empty(&wi.config) {
        Ok(map) => map,
        Err(denial) => return denial.into_response(),
    };
    let stored = match map.and_then(|map| map.get("token")) {
        Some(Value::String(s)) => s.as_str(),
        _ => "",
    };
    let token = decrypt_token(&state, stored);
    if token.is_empty() {
        return Denial::ConflictError("GitHub credential is missing".to_owned()).into_response();
    }
    // `page = max(1, int(query page or "1"))`; `ValueError` answers 1
    // (`github.py:585-588`).
    let page = parse_page(query.get("page").map(String::as_str).unwrap_or("1"));
    match list_user_repos(&token, page).await {
        Ok((repos, has_next)) => {
            let body = serde_json::json!({
                "repos": repos.iter().map(serialize_repo).collect::<Vec<_>>(),
                "page": page,
                "has_next_page": has_next,
            });
            json_response(body)
        }
        Err(GithubFailure::Auth) => {
            Denial::UnauthorizedError("GitHub token rejected".to_owned()).into_response()
        }
        Err(GithubFailure::Other(body)) => {
            // `log_exception(e)` (`github.py:595`).
            tracing::warn!("github repos: list failed: {body}");
            Denial::BadGatewayError("Failed to list repositories".to_owned()).into_response()
        }
        Err(GithubFailure::Permission(_) | GithubFailure::NotFound(_)) => {
            Denial::BadGatewayError("Failed to list repositories".to_owned()).into_response()
        }
    }
}

/// The repos 404: `{"error": "GitHub not connected"}` 404
/// (`github.py:578-579`). Rendered through a dedicated variant so the
/// status cannot drift to the view-inline 400.
fn repos_not_connected() -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            r#"{"error":"GitHub not connected"}"#,
        ))
        .expect("static repos 404")
}

/// `max(1, int(raw or "1"))` with `ValueError` answering 1
/// (`github.py:585-588`). Python's `int()` strips surrounding whitespace
/// and takes an optional sign; overflow (unbounded ints) also answers 1.
fn parse_page(raw: &str) -> i64 {
    let trimmed = raw.trim();
    let parsed: Option<i64> = trimmed.parse().ok();
    match parsed {
        Some(page) => page.max(1),
        None => 1,
    }
}

/// `_serialize_repo` (`github.py:270-279`): the six keys. `id` passes
/// through raw (`.get`, `null` when missing); the four strings render
/// `x or ""`; `owner` unwraps the nested login (or `""`); `private` goes
/// through Python truthiness. Shapes outside GitHub's wire format (a
/// truthy non-object `owner`) answer 500 in Python and degrade to `""`
/// here — unreachable over the wire, where `owner` is always an object.
fn serialize_repo(repo: &Value) -> Value {
    let owner_login = repo
        .get("owner")
        .and_then(|owner| owner.as_object())
        .and_then(|owner| owner.get("login"));
    serde_json::json!({
        "id": repo.get("id").cloned().unwrap_or(Value::Null),
        "owner": or_empty(owner_login),
        "name": or_empty(repo.get("name")),
        "full_name": or_empty(repo.get("full_name")),
        "default_branch": or_empty(repo.get("default_branch")),
        "private": py_truthy(repo.get("private")),
    })
}

/// Python truthiness for the `private` flag (`bool(...)`,
/// `github.py:278`): only real JSON booleans occur over the wire; the
/// remaining arms keep exotic payloads rendering like Python.
fn py_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                return i != 0;
            }
            if let Some(u) = n.as_u64() {
                return u != 0;
            }
            n.as_f64().map(|f| f != 0.0).unwrap_or(true)
        }
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// GitHub API client
// ---------------------------------------------------------------------------

/// GitHub REST base (`github_client.py:21`, `GITHUB_API_BASE`).
const GITHUB_API_BASE: &str = "https://api.github.com";

/// Failure of one GitHub call, mirroring `github_client.py:80-93`:
/// 401 is `GithubAuthError` (its body is dropped: `except GithubAuthError`
/// answers a fixed string with no log), 403 is `GithubPermissionError`,
/// 404 is `GithubNotFoundError`; every other status and every transport
/// error surfaces through `raise_for_status`/requests as a generic
/// `Exception` whose text is logged (`log_exception`).
#[derive(Debug)]
enum GithubFailure {
    Auth,
    Permission(String),
    NotFound(String),
    Other(String),
}

fn github_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("github client builds")
}

/// Request headers (`github_client.py:64-70`).
fn github_headers(token: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        reqwest::header::HeaderValue::from_static("2022-11-28"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static("pi-dash-github-sync"),
    );
    if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    headers
}

async fn github_get(url: &str, token: &str) -> Result<reqwest::Response, GithubFailure> {
    github_client()
        .get(url)
        .headers(github_headers(token))
        .send()
        .await
        .map_err(|err| GithubFailure::Other(err.to_string()))
}

fn classify(status: reqwest::StatusCode, body: String) -> GithubFailure {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        GithubFailure::Auth
    } else if status == reqwest::StatusCode::FORBIDDEN {
        GithubFailure::Permission(body)
    } else if status == reqwest::StatusCode::NOT_FOUND {
        GithubFailure::NotFound(body)
    } else {
        GithubFailure::Other(body)
    }
}

/// `GET /user` (`github_client.py:112-114`): validates a PAT on connect.
async fn get_authenticated_user(token: &str) -> Result<Value, GithubFailure> {
    let url = format!("{GITHUB_API_BASE}/user");
    let response = github_get(&url, token).await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(classify(status, body));
    }
    // `reqwest/json` is off in this crate (minimal features); decode the
    // text body like `requests`' `.json()` does.
    let text = response
        .text()
        .await
        .map_err(|err| GithubFailure::Other(err.to_string()))?;
    serde_json::from_str::<Value>(&text).map_err(|err| GithubFailure::Other(err.to_string()))
}

/// One page of `GET /user/repos` with the affiliation filter required to
/// surface org repos (`github_client.py:116-130`). Returns the repos plus
/// whether a `rel="next"` link advertises another page.
async fn list_user_repos(token: &str, page: i64) -> Result<(Vec<Value>, bool), GithubFailure> {
    // `urlencode` order (`github_client.py:121-126`): affiliation,
    // per_page, sort, page — commas percent-encoded (`%2C`).
    let url = format!(
        "{GITHUB_API_BASE}/user/repos?affiliation=owner%2Ccollaborator%2Corganization_member&per_page=100&sort=updated&page={page}"
    );
    let response = github_get(&url, token).await?;
    let status = response.status();
    let has_next = has_next_page(response.headers());
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(classify(status, body));
    }
    let text = response
        .text()
        .await
        .map_err(|err| GithubFailure::Other(err.to_string()))?;
    let repos: Vec<Value> =
        serde_json::from_str(&text).map_err(|err| GithubFailure::Other(err.to_string()))?;
    Ok((repos, has_next))
}

/// `_next_url(response) is not None` (`github_client.py:95-104`): the
/// `Link` header's comma-separated parts, `re.match(r'\s*<([^>]+)>;
/// \s*rel="next"')` per part (start-anchored, no end anchor).
fn has_next_page(headers: &reqwest::header::HeaderMap) -> bool {
    headers
        .get_all(reqwest::header::LINK)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|link| link_next_url(link).is_some())
}

fn link_next_url(link: &str) -> Option<&str> {
    for part in link.split(',') {
        let part = part.trim_start();
        let after_open = part.strip_prefix('<')?;
        let end = after_open.find('>')?;
        let (url, rest) = after_open.split_at(end);
        let rest = rest[1..].trim_start();
        if rest
            .strip_prefix(";")
            .map(str::trim_start)
            .is_some_and(|r| r.starts_with("rel=\"next\""))
        {
            return Some(url);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Token crypto + clock
// ---------------------------------------------------------------------------

/// `decrypt_data` (`license/utils/encryption.py:36-47`): empty input reads
/// `""`; failure logs and reads `""` (the ported fail-closed wart the
/// QUIRK-empty-config-token relies on).
fn decrypt_token(state: &AppState, stored: &str) -> String {
    if stored.is_empty() {
        return String::new();
    }
    let keyring =
        pidash_services::license::encryption::Keyring::from_secret(&state.settings().secret_key);
    pidash_services::license::encryption::decrypt_data(&keyring, Some(stored))
}

/// `encrypt_data` for the connect write (`encryption.py:22-33`).
fn encrypt_token(state: &AppState, token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    let keyring =
        pidash_services::license::encryption::Keyring::from_secret(&state.settings().secret_key);
    pidash_services::license::encryption::encrypt_data(&keyring, Some(token))
}

/// `timezone.now().isoformat()`: UTC with microseconds and `+00:00`.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

// ---------------------------------------------------------------------------
// Connect
// ---------------------------------------------------------------------------

/// `POST .../integrations/github/connect/` (`github.py:426-492`): ADMIN at
/// WORKSPACE level (`:429`). Verifies the PAT against GitHub, then stores
/// it encrypted on the workspace integration row (creating the row and its
/// `APIToken` FK shim on first connect) and upserts the PAT provider
/// account. The workspace row is read *after* the live GitHub call
/// (`:448` — a bogus token on a missing workspace answers 401, not 404).
async fn connect(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor_of(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = gate_workspace(
        &pool,
        &slug,
        &actor.id,
        Gate::Workspace { roles: ADMIN_ONLY },
    )
    .await
    {
        return denial.into_response();
    }
    if let Err(denial) = feature_enabled(&state) {
        return denial.into_response();
    }
    // `(request.data.get("token") or "").strip()` (`github.py:434`): a
    // non-object body has no `.get` (`AttributeError` → base 500); falsy
    // values become `""`; a truthy non-string has no `.strip()` (500).
    let data: Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(err) => return Denial::BadDetail(format!("JSON parse error - {err}")).into_response(),
    };
    let map = match data.as_object() {
        Some(map) => map,
        None => return Denial::ServerError.into_response(),
    };
    let token = match map.get("token") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.trim().to_owned(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(n)) if json_number_is_zero(n) => String::new(),
        Some(_) => return Denial::ServerError.into_response(),
    };
    if token.is_empty() {
        return Denial::BadError("GitHub PAT required".to_owned()).into_response();
    }
    let user_info = match get_authenticated_user(&token).await {
        Ok(user_info) => user_info,
        Err(GithubFailure::Auth) => {
            return Denial::UnauthorizedError("GitHub rejected this token".to_owned())
                .into_response();
        }
        Err(GithubFailure::Permission(body) | GithubFailure::NotFound(body)) => {
            return Denial::BadError(format!("GitHub error: {body}")).into_response();
        }
        Err(GithubFailure::Other(body)) => {
            // `log_exception(e)` (`github.py:445`).
            tracing::warn!("github connect: credential verify failed: {body}");
            return Denial::BadGatewayError("Failed to verify GitHub credential".to_owned())
                .into_response();
        }
    };
    let workspace = match workspace_id(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let integration = match ensure_github_integration(&pool).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let wi = match ensure_workspace_integration(&pool, &workspace, &actor.id, &integration).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    // `wi.config = {...}; wi.save(update_fields=["config"])` (`:475-481`):
    // the config column only.
    let login = or_empty(user_info.get("login"));
    let config = serde_json::json!({
        "auth_type": "pat",
        "token": encrypt_token(&state, &token),
        "github_user_login": login,
        "verified_at": now_iso(),
    });
    if let Err(denial) = save_wi_config(&pool, &wi, &config).await {
        return denial.into_response();
    }
    if let Err(denial) =
        upsert_pat_account(&pool, &workspace, &wi, &actor.id, &config, &user_info).await
    {
        return denial.into_response();
    }
    // The response echoes the saved row (`wi.config.get(...)`, `:484-491`).
    json_response(serde_json::json!({
        "connected": true,
        "github_user_login": config.get("github_user_login").and_then(Value::as_str).unwrap_or(""),
        "verified_at": config.get("verified_at").and_then(Value::as_str).unwrap_or(""),
    }))
}

/// `(x or "")` falsiness for JSON numbers: `0`/`0.0` are falsy in Python
/// (`github.py:434`); every other number is truthy (and then fails
/// `.strip()` → 500).
fn json_number_is_zero(n: &serde_json::Number) -> bool {
    if let Some(i) = n.as_i64() {
        return i == 0;
    }
    if let Some(u) = n.as_u64() {
        return u == 0;
    }
    n.as_f64().map(|f| f == 0.0).unwrap_or(false)
}

/// `_get_or_create_github_integration` (`github.py:90-99`): the
/// `provider="github"` row, created with the title/verified/description
/// defaults when missing. A concurrent create surfaces as a unique
/// violation — Django's `get_or_create` re-reads, and so do we.
async fn ensure_github_integration(pool: &sqlx::PgPool) -> Result<uuid::Uuid, Denial> {
    let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM integrations
           WHERE provider = 'github' AND deleted_at IS NULL
           ORDER BY id ASC LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let now = chrono::Utc::now();
    let id = uuid::Uuid::new_v4();
    let description = serde_json::json!({"summary": "Mirror GitHub issues into Pi Dash projects."});
    let created: Result<_, sqlx::Error> = sqlx::query(
        r#"INSERT INTO integrations
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            title, provider, network, description, author,
            webhook_url, webhook_secret, redirect_url, metadata, verified, avatar_url)
           VALUES ($1,$2,$2,NULL,NULL,NULL,'GitHub','github',1,$3,'','','','{}',true,NULL)"#,
    )
    .bind(id)
    .bind(now)
    .bind(&description)
    .execute(pool)
    .await;
    match created {
        Ok(_) => Ok(id),
        Err(_) => {
            // Concurrent first-connect: re-read the winner.
            let winner: Option<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT id FROM integrations
                   WHERE provider = 'github' AND deleted_at IS NULL
                   ORDER BY id ASC LIMIT 1"#,
            )
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            winner.map(|row| row.0).ok_or(Denial::ServerError)
        }
    }
}

/// The connect row (`github.py:451-474`): reuse the existing
/// `WorkspaceIntegration`, else mint the `APIToken` FK shim (inactive,
/// never for auth — see the review of #65 quoted at `:456-460`) and the
/// row itself with an empty `config`.
async fn ensure_workspace_integration(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    integration_id: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM workspace_integrations
           WHERE workspace_id = $1 AND integration_id = $2 AND deleted_at IS NULL
           ORDER BY id ASC LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(integration_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let now = chrono::Utc::now();
    let token_id = uuid::Uuid::new_v4();
    let api_token = format!("pi_dash_api_{}", uuid::Uuid::new_v4().simple());
    let label = format!("github-integration-{workspace_id}");
    sqlx::query(
        r#"INSERT INTO api_tokens
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            label, description, is_active, last_used, token,
            user_id, user_type, workspace_id, expired_at, is_service, allowed_rate_limit)
           VALUES ($1,$2,$2,$3,NULL,NULL,
            $4,'GitHub integration FK shim — not for auth',false,NULL,$5,
            $3,1,$6,NULL,false,'60/min')"#,
    )
    .bind(token_id)
    .bind(now)
    .bind(actor_id)
    .bind(&label)
    .bind(&api_token)
    .bind(workspace_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let wi_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO workspace_integrations
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, actor_id, integration_id, api_token_id, metadata, config)
           VALUES ($1,$2,$2,$3,NULL,NULL,$4,$3,$5,$6,'{}','{}')"#,
    )
    .bind(wi_id)
    .bind(now)
    .bind(actor_id)
    .bind(workspace_id)
    .bind(integration_id)
    .bind(token_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(wi_id)
}

/// `wi.save(update_fields=["config"])` (`github.py:480-481`).
async fn save_wi_config(
    pool: &sqlx::PgPool,
    wi_id: &uuid::Uuid,
    config: &Value,
) -> Result<(), Denial> {
    sqlx::query(r#"UPDATE workspace_integrations SET config = $1 WHERE id = $2"#)
        .bind(config)
        .bind(wi_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `_upsert_git_account_for_pat` (`github.py:282-311`): keyed on
/// `(workspace, github, https://github.com, PAT, "pat:<wi id>")`. The
/// stored `credential_config.token` is the *encrypted* value from the
/// just-saved `wi.config` (`config.get("token")`, `:304`); login/display
/// prefer it over the live `user_info` (`:294-295`); `verified_at` is now
/// (`:308`).
async fn upsert_pat_account(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    wi_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    config: &Value,
    user_info: &Value,
) -> Result<(), Denial> {
    let external_id = format!("pat:{wi_id}");
    let login = config
        .get("github_user_login")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            user_info
                .get("login")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("");
    let display = if login.is_empty() {
        "GitHub PAT".to_owned()
    } else {
        login.to_owned()
    };
    let capabilities = serde_json::json!({
        "read_repositories": true,
        "read_issues": true,
        "write_comments": true,
        "manage_webhooks": false,
        "clone": false,
    });
    let credential_config = serde_json::json!({
        "auth_type": "pat",
        "host_url": "https://github.com",
        "token": config.get("token").and_then(Value::as_str).unwrap_or(""),
    });
    let metadata = serde_json::json!({"identity": user_info});
    let now = chrono::Utc::now();
    let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM git_provider_accounts
           WHERE workspace_id = $1 AND provider = 'github' AND host_url = 'https://github.com'
           AND auth_type = 'pat' AND external_account_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .bind(&external_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match existing {
        Some((id,)) => {
            sqlx::query(
                r#"UPDATE git_provider_accounts
                   SET external_account_login = $1, display_name = $2,
                       capabilities = $3, credential_config = $4,
                       workspace_integration_id = $5, status = 'connected',
                       verified_at = $6, metadata = $7,
                       updated_at = $6, updated_by_id = $8
                   WHERE id = $9"#,
            )
            .bind(login)
            .bind(&display)
            .bind(&capabilities)
            .bind(&credential_config)
            .bind(wi_id)
            .bind(now)
            .bind(&metadata)
            .bind(actor_id)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
        None => {
            sqlx::query(
                r#"INSERT INTO git_provider_accounts
                   (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    workspace_id, provider, host_url, auth_type, external_account_id,
                    external_account_login, display_name, capabilities, credential_config,
                    workspace_integration_id, status, verified_at, last_check_error, metadata)
                   VALUES ($1,$2,$2,$3,NULL,NULL,
                    $4,'github','https://github.com','pat',$5,
                    $6,$7,$8,$9,
                    $10,'connected',$11,'',$12)"#,
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(actor_id)
            .bind(workspace_id)
            .bind(&external_id)
            .bind(login)
            .bind(&display)
            .bind(&capabilities)
            .bind(&credential_config)
            .bind(wi_id)
            .bind(now)
            .bind(&metadata)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_string(denial: Denial) -> (StatusCode, String) {
        denial.status_and_body()
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            body_string(Denial::Unauthorized),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::Forbidden),
            (
                StatusCode::FORBIDDEN,
                r#"{"error":"You don't have the required permissions."}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::NotFoundDetail),
            (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Not found."}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::Disabled),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"GitHub integration is disabled on this instance"}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::BadError("GitHub PAT required".to_owned())),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"GitHub PAT required"}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::UnauthorizedError(
                "GitHub rejected this token".to_owned()
            )),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"error":"GitHub rejected this token"}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::UnauthorizedError(
                "GitHub token rejected".to_owned()
            )),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"error":"GitHub token rejected"}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::ConflictError(
                "GitHub credential is missing".to_owned()
            )),
            (
                StatusCode::CONFLICT,
                r#"{"error":"GitHub credential is missing"}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::BadGatewayError(
                "Failed to verify GitHub credential".to_owned()
            )),
            (
                StatusCode::BAD_GATEWAY,
                r#"{"error":"Failed to verify GitHub credential"}"#.to_owned()
            )
        );
        // Error interpolation is JSON-escaped, like DRF rendering.
        assert_eq!(
            body_string(Denial::BadError("GitHub error: \"nope\"".to_owned())).1,
            r#"{"error":"GitHub error: \"nope\""}"#
        );
        assert_eq!(
            body_string(Denial::ServerError),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":"Something went wrong please try again later"}"#.to_owned()
            )
        );
        assert_eq!(
            body_string(Denial::BadDetail("JSON parse error - foo".to_owned())),
            (
                StatusCode::BAD_REQUEST,
                r#"{"detail":"JSON parse error - foo"}"#.to_owned()
            )
        );
    }

    #[test]
    fn status_body_shapes() {
        // No token reads disconnected (FX-GHA-02 status_get).
        for config in [
            serde_json::json!({}),
            serde_json::json!({"token": ""}),
            serde_json::json!({"token": null}),
            serde_json::json!(null),
        ] {
            assert_eq!(
                status_body(&config).expect("shape"),
                serde_json::json!({"connected": false})
            );
        }
        // A stored token — even one Fernet would reject — reads connected
        // without decrypting (BUG-config-plaintext-status).
        let body = status_body(&serde_json::json!({
            "token": "contract91-seeded-token",
            "github_user_login": "contract-octocat",
            "verified_at": "2026-01-01T00:00:00+00:00",
        }))
        .expect("shape");
        assert_eq!(
            serde_json::to_string(&body).expect("json"),
            r#"{"connected":true,"github_user_login":"contract-octocat","verified_at":"2026-01-01T00:00:00+00:00"}"#
        );
        // `.get` echoes raw values: missing keys render `null`, like DRF.
        let sparse = status_body(&serde_json::json!({"token": "x"})).expect("shape");
        assert_eq!(
            serde_json::to_string(&sparse).expect("json"),
            r#"{"connected":true,"github_user_login":null,"verified_at":null}"#
        );
        // A truthy non-object config has no `.get` (500, like Python).
        assert!(status_body(&serde_json::json!("nope")).is_err());
    }

    #[test]
    fn config_or_empty_matches_python() {
        // Falsy configs read as missing.
        for config in [
            serde_json::json!(null),
            serde_json::json!(false),
            serde_json::json!(""),
            serde_json::json!([]),
        ] {
            assert!(config_or_empty(&config).expect("falsy").is_none());
        }
        let config = serde_json::json!({"token": "x"});
        let map = config_or_empty(&config).expect("object").expect("some");
        assert_eq!(map.get("token"), Some(&serde_json::json!("x")));
        // Truthy non-objects fail.
        assert!(config_or_empty(&serde_json::json!("x")).is_err());
        assert!(config_or_empty(&serde_json::json!([1])).is_err());
    }

    #[test]
    fn or_empty_passes_truthy_through_raw() {
        assert_eq!(or_empty(None), serde_json::json!(""));
        assert_eq!(
            or_empty(Some(&serde_json::json!(null))),
            serde_json::json!("")
        );
        assert_eq!(or_empty(Some(&serde_json::json!(0))), serde_json::json!(""));
        assert_eq!(
            or_empty(Some(&serde_json::json!(""))),
            serde_json::json!("")
        );
        assert_eq!(
            or_empty(Some(&serde_json::json!("x"))),
            serde_json::json!("x")
        );
        // Truthy non-strings pass through raw, like `x or ""`.
        assert_eq!(or_empty(Some(&serde_json::json!(5))), serde_json::json!(5));
    }

    #[test]
    fn page_rule_matches_python() {
        // `max(1, int(...))`; `ValueError` answers 1 (github.py:585-588).
        assert_eq!(parse_page("1"), 1);
        assert_eq!(parse_page("3"), 3);
        assert_eq!(parse_page("0"), 1);
        assert_eq!(parse_page("-2"), 1);
        assert_eq!(parse_page(" 4 "), 4);
        assert_eq!(parse_page("+2"), 2);
        assert_eq!(parse_page(""), 1);
        assert_eq!(parse_page("abc"), 1);
        assert_eq!(parse_page("2.5"), 1);
        assert_eq!(parse_page("99999999999999999999999"), 1);
    }

    #[test]
    fn serialize_repo_keeps_six_keys_in_order() {
        let repo = serde_json::json!({
            "id": 123,
            "owner": {"login": "octo"},
            "name": "r",
            "full_name": "octo/r",
            "default_branch": "main",
            "private": true,
        });
        assert_eq!(
            serde_json::to_string(&serialize_repo(&repo)).expect("json"),
            r#"{"id":123,"owner":"octo","name":"r","full_name":"octo/r","default_branch":"main","private":true}"#
        );
        // Missing owner / blanks degrade to "" (github.py:271-276).
        let sparse = serde_json::json!({"id": null});
        assert_eq!(
            serde_json::to_string(&serialize_repo(&sparse)).expect("json"),
            r#"{"id":null,"owner":"","name":"","full_name":"","default_branch":"","private":false}"#
        );
    }

    #[test]
    fn private_follows_python_truthiness() {
        assert!(!py_truthy(None));
        assert!(!py_truthy(Some(&Value::Null)));
        assert!(py_truthy(Some(&serde_json::json!(true))));
        assert!(!py_truthy(Some(&serde_json::json!(false))));
        assert!(!py_truthy(Some(&serde_json::json!(0))));
        assert!(py_truthy(Some(&serde_json::json!(1))));
        assert!(!py_truthy(Some(&serde_json::json!(""))));
        // `bool("false")` is True in Python (non-empty string).
        assert!(py_truthy(Some(&serde_json::json!("false"))));
    }

    #[test]
    fn link_header_next_detection() {
        assert_eq!(
            link_next_url(
                r#"<https://api.github.com/user/repos?page=2>; rel="next", <https://api.github.com/user/repos?page=5>; rel="last""#
            ),
            Some("https://api.github.com/user/repos?page=2")
        );
        // Last page: no next relation.
        assert_eq!(
            link_next_url(
                r#"<https://api.github.com/user/repos?page=1>; rel="prev", <https://api.github.com/user/repos?page=1>; rel="first""#
            ),
            None
        );
        assert_eq!(link_next_url(""), None);
        // Start-anchored like `re.match` (leading whitespace allowed).
        assert_eq!(
            link_next_url(r#"  <https://x?page=2>; rel="next""#),
            Some("https://x?page=2")
        );
    }

    #[test]
    fn connect_token_coercion() {
        // Mirror the match arms in `connect` for `(x or "").strip()`.
        fn coerce(value: Option<&Value>) -> Result<String, ()> {
            match value {
                None | Some(Value::Null) => Ok(String::new()),
                Some(Value::String(s)) => Ok(s.trim().to_owned()),
                Some(Value::Bool(false)) => Ok(String::new()),
                Some(Value::Number(n)) if json_number_is_zero(n) => Ok(String::new()),
                Some(_) => Err(()),
            }
        }
        assert_eq!(coerce(None).expect("none"), "");
        assert_eq!(
            coerce(Some(&serde_json::json!("  tok "))).expect("str"),
            "tok"
        );
        assert_eq!(coerce(Some(&serde_json::json!(0))).expect("zero"), "");
        assert_eq!(coerce(Some(&serde_json::json!(false))).expect("false"), "");
        // Truthy non-strings fail `.strip()` → 500.
        assert!(coerce(Some(&serde_json::json!(5))).is_err());
        assert!(coerce(Some(&serde_json::json!(true))).is_err());
        assert!(coerce(Some(&serde_json::json!([]))).is_err());
        assert!(coerce(Some(&serde_json::json!({}))).is_err());
    }

    #[test]
    fn now_iso_is_django_isoformat_shape() {
        let rendered = now_iso();
        assert!(rendered.contains('T'), "{rendered}");
        assert!(rendered.ends_with("+00:00"), "{rendered}");
        // Microseconds, always: `...SS.ffffff+00:00`.
        let frac = rendered.split('.').nth(1).expect("fraction");
        assert_eq!(frac.len(), 6 + "+00:00".len(), "{rendered}");
    }

    #[test]
    fn github_failure_classification() {
        use reqwest::StatusCode as GH;
        assert!(matches!(
            classify(GH::UNAUTHORIZED, String::new()),
            GithubFailure::Auth
        ));
        assert!(matches!(
            classify(GH::FORBIDDEN, String::new()),
            GithubFailure::Permission(_)
        ));
        assert!(matches!(
            classify(GH::NOT_FOUND, String::new()),
            GithubFailure::NotFound(_)
        ));
        assert!(matches!(
            classify(GH::BAD_GATEWAY, String::new()),
            GithubFailure::Other(_)
        ));
        assert!(matches!(
            classify(GH::INTERNAL_SERVER_ERROR, String::new()),
            GithubFailure::Other(_)
        ));
    }
}
