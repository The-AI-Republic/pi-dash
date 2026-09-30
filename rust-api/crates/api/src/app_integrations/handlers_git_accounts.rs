//! Git provider + provider-account handlers (D-33, stage 5, PIDASHCONV-451).
//!
//! Ports `apps/api/pi_dash/app/views/integration/git.py:34-130` with routes
//! from `app/urls/integration.py`:
//!
//! * `GitProvidersEndpoint.get` (`:50-54`): workspace ADMIN/MEMBER gate,
//!   `get_object_or_404(Workspace, slug)`, static [`PROVIDERS_BODY`]
//!   (`registry.py:45-53`; github `51-53`, gitlab `185-187`).
//! * `GitProviderAccountListCreateEndpoint.get` (`:57-67`): ADMIN/MEMBER,
//!   `filter(workspace).order_by("provider","host_url","display_name")`
//!   via [`pidash_db::app_integrations::queries_git`], 200
//!   `{"accounts": [...]}`.
//! * `post` (`:69-94`): ADMIN-only; `provider` allowlist then `token`
//!   presence (both view-inline 400s); `auth_type` defaults to `"pat"`;
//!   `host_url` defaults to `https://github.com` / the gitlab host with a
//!   trailing-`/` strip; then `create_provider_account`
//!   (`services.py:72-116`) with [`map_provider_error`] for the
//!   `_error_response` table (`:38-47`); 201 serialized.
//! * `GitProviderAccountDetailEndpoint.get` (`:97-101`) / `delete`
//!   (`:103-114`): ADMIN/MEMBER read, ADMIN-only revoke. Lookup is
//!   `get_object_or_404(GitProviderAccount, id, workspace__slug)` — the
//!   404 renders `No GitProviderAccount matches the given query.`
//!   ([`ACCOUNT_NOT_FOUND_BODY`]). Revoke sets `status=REVOKED`,
//!   `last_check_error`, wipes the stored token (other credential keys
//!   kept), bumps `updated_at`, and disables the account's bindings;
//!   200 `{"connected":false}`.
//! * `GitProviderAccountReposEndpoint.get` (`:117-130`): ADMIN/MEMBER,
//!   the shared `max(1, int(page or "1"))` rule with `ValueError -> 1`
//!   ([`pidash_db::app_integrations::queries_git::parse_page_param`]),
//!   then `list_account_repositories` (`services.py:381-389`); 200
//!   `{"repos","page","has_next_page"}`.
//!
//! Fixtures: FX-GIT-01
//! (`rust-api/fixtures/app_integrations/fx-git-01-provider-account.json`),
//! FX-PERM-01 (gates, [`super::gates`]).
//!
//! The project-repository endpoints (`git.py:132-175`: get/patch/delete +
//! bind) belong to a sibling issue and keep proxying to Django — no route
//! here, so the edge fallback serves them.
//!
//! Live provider calls (create-verify, repo listing) go through reqwest
//! with the same endpoints/headers/params as the Python clients
//! (`utils/github_client.py`, `adapters/gitlab.py`) and the same status
//! mapping. Non-GET/POST/DELETE methods on owned paths proxy so Django's
//! 405-after-auth (and OPTIONS metadata) survive byte for byte.
//!
//! Ported bugs and quirks (translate, don't redesign):
//!
//! * `host_url` pre-normalization (`git.py:79-82`) only strips trailing
//!   `/` — no scheme prepend; `normalize_host_url` (scheme prepend,
//!   `services.py:43-47`) runs inside `create_provider_account`.
//! * `auth_type` is stripped but never validated or lowercased
//!   (`git.py:78`): any string stores and serializes back.
//! * Revoke is not atomic (save, then bindings update, `git.py:106-113`).
//! * Rust `Settings` carries no `GITLAB_HOST`, so the gitlab default host
//!   is the `https://gitlab.com` constant (Django falls back to
//!   `settings.GITLAB_HOST`). The allowlist check additionally seeds only
//!   `https://gitlab.com` plus `Settings.gitlab_allowed_hosts`.

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::Router;
use chrono::{SecondsFormat, Utc};

use pidash_db::app_integrations::queries_git;
use pidash_db::config::encryption::Keyring;
use pidash_db::integrations::git_models::git_provider_account::GitProviderAccount;

use super::gates::{decide_gate, tenant_context, Gate, GateOutcome};
use crate::state::AppState;

/// Exact bytes of the providers payload (`registry.py:45-53`).
pub const PROVIDERS_BODY: &str = r#"{"providers":[{"key":"github","display_name":"GitHub","code_review_term":"pull request"},{"key":"gitlab","display_name":"GitLab","code_review_term":"merge request"}]}"#;
/// DRF `IsAuthenticated` denial (anonymous before any gate).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// Allow-style gate denial (`app/permissions/base.py:80-84`).
pub const PERMISSION_DENIED_BODY: &str = r#"{"error":"You don't have the required permissions."}"#;
/// DRF default `Http404` body (missing workspace slug).
pub const WORKSPACE_NOT_FOUND_BODY: &str = r#"{"detail":"Not found."}"#;
/// `get_object_or_404(GitProviderAccount, ...)` body (`git.py:100,105,120`).
pub const ACCOUNT_NOT_FOUND_BODY: &str =
    r#"{"detail":"No GitProviderAccount matches the given query."}"#;
/// Generic 500 (`views/base.py:99-103`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// GitHub API base (`utils/github_client.py:22`): the credential host is
/// never consulted for github — verification and listing always hit
/// `api.github.com`.
const GITHUB_API_BASE: &str = "https://api.github.com";
/// Default gitlab host (`git.py:34-35` Django-default branch).
const GITLAB_DEFAULT_HOST: &str = "https://gitlab.com";
/// Default github host (`git.py:81`).
const GITHUB_DEFAULT_HOST: &str = "https://github.com";
/// Upstream timeout (`DEFAULT_TIMEOUT_SECONDS = 30`, both clients).
const UPSTREAM_TIMEOUT_SECS: u64 = 30;

/// Owned git provider-account routes. Sibling `projects/<id>/repository/`
/// paths stay unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/integrations/git/providers/",
            owned(axum::routing::get(get_providers)),
        )
        .route(
            "/api/workspaces/{slug}/integrations/git/accounts/",
            axum::routing::get(list_accounts)
                .post(create_account)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/integrations/git/accounts/{account_id}/",
            axum::routing::get(get_account)
                .delete(revoke_account)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/integrations/git/accounts/{account_id}/repos/",
            owned(axum::routing::get(list_repos)),
        )
}

/// A GET-only path: reads serve from Rust, everything else falls through
/// to Django (its 405-after-auth and OPTIONS metadata live there).
fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Denials and JSON responses
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded route).
    Unauthorized,
    /// 403, allow-style gate denial.
    Forbidden,
    /// 404, missing workspace slug (DRF default `Http404` body).
    WorkspaceNotFound,
    /// 404, missing provider account (model `get_object_or_404` body).
    AccountNotFound,
    /// 400, view-inline `{"error": ...}`.
    BadError(String),
    /// 400, upstream transport/parse failure (`str(exc)` branch).
    BadUpstream(String),
    /// 401/403/404 from the provider with the `_error_response` default.
    Provider(u16, String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, PERMISSION_DENIED_BODY.to_owned()),
            Denial::WorkspaceNotFound => {
                (StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND_BODY.to_owned())
            }
            Denial::AccountNotFound => (StatusCode::NOT_FOUND, ACCOUNT_NOT_FOUND_BODY.to_owned()),
            Denial::BadError(message) | Denial::BadUpstream(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::Provider(status, message) => (
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST),
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static json response")
}

fn denial_response(denial: Denial) -> Response {
    let (status, body) = denial.status_and_body();
    json_response(status, body)
}

// ---------------------------------------------------------------------------
// Auth + gates (decorator order: session auth, then allow_permission)
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, or a non-UUID id means anonymous → 401.
fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    let raw = session.get("_auth_user_id")?.as_str()?.to_owned();
    raw.parse::<uuid::Uuid>().ok()
}

/// Authenticate, then authorize through the [`Gate`] table
/// (`super::gates`, FX-PERM-01) so the role matrix stays in one place:
/// the membership facts are fetched here (the `tenant_context` half) and
/// decided through `decide_gate`.
async fn resolve_workspace_gate(
    pool: &sqlx::PgPool,
    slug: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    gate: &Gate,
) -> Result<uuid::Uuid, Denial> {
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let allowed: &[i32] = match gate {
        Gate::Workspace { roles } => roles,
        _ => return Err(Denial::ServerError),
    };
    let scope = tenant_context(slug);
    // `role` is a smallint (`ROLE`: 20/15/5); decode as `i16` like the
    // pilot handlers do.
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL AND w.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let role = row.map(|(role,)| role as i32);
    let facts = pidash_auth::permissions::allow::AllowFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.map(|role| allowed.contains(&role)).unwrap_or(false),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role
            .map(|role| role == pidash_auth::permissions::ROLE_ADMIN)
            .unwrap_or(false),
    };
    match decide_gate(gate, &scope, &facts) {
        GateOutcome::Allow => Ok(user_id),
        GateOutcome::Deny => Err(Denial::Forbidden),
        GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

/// `get_object_or_404(Workspace, slug=slug)`: the id, or the DRF default
/// 404. Runs after the gate (decorator order), so reachable only when a
/// membership row already proved the workspace exists.
async fn workspace_id(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::WorkspaceNotFound)
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Serialization (services.py:264-279, 250-262)
// ---------------------------------------------------------------------------

/// `verified_at.isoformat() if verified_at else None`
/// (`services.py:273`): Python renders a UTC-aware datetime as
/// `...+00:00` with microseconds only when nonzero — exactly
/// `AutoSi` without `Z`.
fn isoformat_or_none(dt: Option<chrono::DateTime<Utc>>) -> serde_json::Value {
    dt.map(|dt| serde_json::Value::String(dt.to_rfc3339_opts(SecondsFormat::AutoSi, false)))
        .unwrap_or(serde_json::Value::Null)
}

/// `serialize_provider_account` (`services.py:264-279`): the 11 keys in
/// source order. `credential_config` and `metadata` are never serialized
/// (secret hygiene, FX-GIT-01).
fn serialize_provider_account(account: &GitProviderAccount) -> serde_json::Value {
    let mut body = serde_json::Map::with_capacity(11);
    body.insert(
        "id".to_owned(),
        serde_json::Value::String(account.id.to_string()),
    );
    body.insert(
        "provider".to_owned(),
        serde_json::Value::String(account.provider.clone()),
    );
    body.insert(
        "host_url".to_owned(),
        serde_json::Value::String(account.host_url.clone()),
    );
    body.insert(
        "auth_type".to_owned(),
        serde_json::Value::String(account.auth_type.clone()),
    );
    body.insert(
        "external_account_id".to_owned(),
        serde_json::Value::String(account.external_account_id.clone()),
    );
    body.insert(
        "external_account_login".to_owned(),
        serde_json::Value::String(account.external_account_login.clone()),
    );
    body.insert(
        "display_name".to_owned(),
        serde_json::Value::String(account.display_name.clone()),
    );
    body.insert("capabilities".to_owned(), account.capabilities.clone());
    body.insert(
        "status".to_owned(),
        serde_json::Value::String(account.status.clone()),
    );
    body.insert(
        "verified_at".to_owned(),
        isoformat_or_none(account.verified_at),
    );
    body.insert(
        "last_check_error".to_owned(),
        serde_json::Value::String(account.last_check_error.clone()),
    );
    serde_json::Value::Object(body)
}

/// `serialize_remote_repository` (`services.py:250-262`): the 11 keys in
/// source order, with the caller's `host_url` (the account's).
fn serialize_remote_repository(
    repo: &pidash_types::integrations::RemoteRepository,
    host_url: &str,
) -> serde_json::Value {
    let mut body = serde_json::Map::with_capacity(11);
    body.insert(
        "id".to_owned(),
        serde_json::Value::String(repo.external_id.clone()),
    );
    body.insert(
        "provider".to_owned(),
        serde_json::Value::String(repo.provider.clone()),
    );
    body.insert(
        "host_url".to_owned(),
        serde_json::Value::String(host_url.to_owned()),
    );
    body.insert(
        "namespace".to_owned(),
        serde_json::Value::String(repo.namespace.clone()),
    );
    body.insert(
        "name".to_owned(),
        serde_json::Value::String(repo.name.clone()),
    );
    body.insert(
        "full_name".to_owned(),
        serde_json::Value::String(repo.full_name.clone()),
    );
    body.insert(
        "web_url".to_owned(),
        serde_json::Value::String(repo.web_url.clone()),
    );
    body.insert(
        "clone_url_http".to_owned(),
        serde_json::Value::String(repo.clone_url_http.clone()),
    );
    body.insert(
        "clone_url_ssh".to_owned(),
        serde_json::Value::String(repo.clone_url_ssh.clone()),
    );
    body.insert(
        "default_branch".to_owned(),
        serde_json::Value::String(repo.default_branch.clone()),
    );
    body.insert(
        "private".to_owned(),
        serde_json::Value::Bool(repo.is_private),
    );
    serde_json::Value::Object(body)
}

// ---------------------------------------------------------------------------
// Pure ports: create flow, capabilities, identity, remote mapping
// ---------------------------------------------------------------------------

/// Provider error mapped through `_error_response` (`git.py:38-47`).
/// `str(exc) or <default>`: an empty message falls back to the default.
fn map_provider_error(kind: &str, message: &str) -> Denial {
    let with_default = |default: &str| {
        if message.is_empty() {
            default.to_owned()
        } else {
            message.to_owned()
        }
    };
    match kind {
        "auth" => Denial::Provider(401, with_default("Provider rejected this credential")),
        "permission" => Denial::Provider(403, with_default("Provider credential lacks permission")),
        "notfound" => Denial::Provider(404, with_default("Repository not found or inaccessible")),
        _ => Denial::BadUpstream(message.to_owned()),
    }
}

/// `credential_capabilities(...).as_dict()` for github
/// (`adapters/github.py:116-123`): pure in `auth_type`
/// (`credential.get("auth_type") or "pat"`).
fn github_capabilities(auth_type: &str) -> serde_json::Value {
    let auth_type = if auth_type.is_empty() {
        "pat"
    } else {
        auth_type
    };
    serde_json::json!({
        "read_repositories": true,
        "read_issues": true,
        "write_comments": auth_type == "pat",
        "manage_webhooks": auth_type == "github_app",
        "clone": false,
    })
}

/// `credential_capabilities(...).as_dict()` for gitlab
/// (`adapters/gitlab.py:287-293`): constant.
fn gitlab_capabilities() -> serde_json::Value {
    serde_json::json!({
        "read_repositories": true,
        "read_issues": true,
        "write_comments": true,
        "manage_webhooks": true,
        "clone": false,
    })
}

/// `str(value or "")` for a scalar JSON value: Python falsiness first
/// (`0`, `""`, `false`, `null` → `""`), then `str()`. Ids from both
/// providers are positive ints or strings; the falsy arms exist so `0`
/// maps to `""` exactly like `str(payload.get("id") or "")`.
fn py_str(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Bool(true)) => "True".to_owned(),
        Some(serde_json::Value::Bool(false)) => String::new(),
        Some(serde_json::Value::Number(n)) => {
            if n.as_i64() == Some(0) || n.as_u64() == Some(0) {
                String::new()
            } else {
                n.to_string()
            }
        }
        Some(serde_json::Value::Array(_)) | Some(serde_json::Value::Object(_)) => String::new(),
    }
}

/// `bool(value)`: truthiness for the `is_private` flag.
fn py_bool(value: Option<&serde_json::Value>) -> bool {
    match value {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => {
            n.as_i64().is_some_and(|i| i != 0)
                || n.as_u64().is_some_and(|u| u != 0)
                || n.as_f64().is_some_and(|f| f != 0.0)
        }
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(serde_json::Value::Array(a)) => !a.is_empty(),
        Some(serde_json::Value::Object(o)) => !o.is_empty(),
    }
}

/// First non-empty string in `or`-chain order (`a or b or ""`).
fn first_present(values: &[String]) -> String {
    values
        .iter()
        .find(|v| !v.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// `create_provider_account` identity mapping (`services.py:88-93`):
/// `external_id = str(id or username or login or "")`,
/// `login = login or username or name or ""`,
/// `display_name = login or f"{display} account"`.
fn account_identity(identity: &serde_json::Value, display_name: &str) -> (String, String, String) {
    let get = |key: &str| py_str(identity.get(key));
    let external_id = first_present(&[get("id"), get("username"), get("login")]);
    let login = first_present(&[get("login"), get("username"), get("name")]);
    let display = if login.is_empty() {
        format!("{display_name} account")
    } else {
        login.clone()
    };
    (external_id, login, display)
}

/// GitHub `_remote_repo` (`adapters/github.py:125-143`).
fn github_remote_repo(payload: &serde_json::Value) -> pidash_types::integrations::RemoteRepository {
    let owner_value = payload.get("owner");
    let owner_login = owner_value
        .and_then(|o| o.as_object())
        .and_then(|o| o.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let owner_str = owner_value.and_then(|v| v.as_str()).unwrap_or("");
    let owner = [owner_login, owner_str]
        .into_iter()
        .find(|v| !v.is_empty())
        .unwrap_or("")
        .to_lowercase();
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let full_name = payload
        .get("full_name")
        .and_then(|v| v.as_str())
        .map(|v| v.to_lowercase())
        .unwrap_or_else(|| format!("{owner}/{name}").to_lowercase());
    let namespace = full_name
        .rsplit_once('/')
        .map(|(ns, _)| ns.to_owned())
        .unwrap_or_else(|| owner.clone());
    let str_field = |key: &str| {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    pidash_types::integrations::RemoteRepository {
        provider: "github".to_owned(),
        external_id: py_str(payload.get("id")),
        namespace,
        name,
        full_name: full_name.clone(),
        web_url: {
            let url = str_field("html_url");
            if url.is_empty() {
                format!("{GITHUB_DEFAULT_HOST}/{full_name}")
            } else {
                url
            }
        },
        clone_url_http: str_field("clone_url"),
        clone_url_ssh: str_field("ssh_url"),
        default_branch: str_field("default_branch"),
        is_private: py_bool(payload.get("private")),
        metadata: payload.clone(),
    }
}

/// `_split_full_path` (`adapters/gitlab.py:78-83`): strip `/`, strip
/// `.git`, rsplit on `/` — in that order (`_strip_git_suffix` runs on
/// the already slash-stripped path).
fn split_full_path(path: &str) -> Option<(String, String)> {
    let stripped = path.trim_matches('/');
    let stripped = stripped.strip_suffix(".git").unwrap_or(stripped);
    if stripped.is_empty() || !stripped.contains('/') {
        return None;
    }
    let (ns, name) = stripped.rsplit_once('/')?;
    Some((ns.to_owned(), name.to_owned()))
}

/// GitLab `_remote_repo` (`adapters/gitlab.py:295-310`).
fn gitlab_remote_repo(payload: &serde_json::Value) -> pidash_types::integrations::RemoteRepository {
    let str_field = |key: &str| {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    let full_name = {
        let v = str_field("path_with_namespace");
        if v.is_empty() {
            str_field("path")
        } else {
            v
        }
    };
    let (namespace, name) =
        split_full_path(&full_name).unwrap_or_else(|| (String::new(), str_field("path")));
    pidash_types::integrations::RemoteRepository {
        provider: "gitlab".to_owned(),
        external_id: py_str(payload.get("id")),
        namespace,
        name,
        full_name,
        web_url: str_field("web_url"),
        clone_url_http: str_field("http_url_to_repo"),
        clone_url_ssh: str_field("ssh_url_to_repo"),
        default_branch: str_field("default_branch"),
        is_private: payload.get("visibility").and_then(|v| v.as_str()) == Some("private"),
        metadata: payload.clone(),
    }
}

/// `_normalize_host` (`adapters/gitlab.py:37-46`): strip, trailing-`/`
/// strip, empty → gitlab.com, scheme prepend (case-sensitive, like
/// `normalize_host_url`), then lowercase scheme + netloc with the path
/// dropped (`f"{scheme}://{netloc}"`, not scheme + netloc + tail).
fn gitlab_normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('/');
    if host.is_empty() {
        return GITLAB_DEFAULT_HOST.to_owned();
    }
    let host = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_owned()
    } else {
        format!("https://{host}")
    };
    let host = host.trim_end_matches('/');
    match host.split_once("://") {
        Some((scheme, rest))
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            let netloc = rest.split('/').next().unwrap_or("");
            if netloc.is_empty() {
                return host.to_owned();
            }
            format!("{}://{}", scheme.to_lowercase(), netloc.to_lowercase())
        }
        _ => host.to_owned(),
    }
}

/// Lowercase scheme of a normalized host (`urlparse(...).scheme`).
fn url_scheme(host: &str) -> String {
    host.split_once("://")
        .map(|(scheme, _)| scheme.to_lowercase())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Live provider calls (adapters + utils/github_client.py)
// ---------------------------------------------------------------------------

/// Upstream failure classified the way `_map_error` + `_request` do:
/// 401 → auth, 403 → permission, 404 → notfound, redirect-mismatch →
/// permission, anything else (5xx, transport, bad JSON) → generic 400.
enum UpstreamError {
    Auth(String),
    Permission(String),
    NotFound(String),
    Generic(String),
}

impl UpstreamError {
    fn denial(self) -> Denial {
        match self {
            UpstreamError::Auth(message) => map_provider_error("auth", &message),
            UpstreamError::Permission(message) => map_provider_error("permission", &message),
            UpstreamError::NotFound(message) => map_provider_error("notfound", &message),
            UpstreamError::Generic(message) => Denial::BadUpstream(message),
        }
    }
}

fn upstream_client() -> Result<reqwest::Client, UpstreamError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(UPSTREAM_TIMEOUT_SECS))
        .build()
        .map_err(|err| UpstreamError::Generic(err.to_string()))
}

fn classify_status(status: u16, body: &str) -> Result<(), UpstreamError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    // `GitLabClient._request`: redirects are never followed → permission.
    if (300..400).contains(&status) {
        return Err(UpstreamError::Permission(
            "GitLab API redirects are not followed".to_owned(),
        ));
    }
    match status {
        401 => Err(UpstreamError::Auth(body.to_owned())),
        403 => Err(UpstreamError::Permission(body.to_owned())),
        404 => Err(UpstreamError::NotFound(body.to_owned())),
        _ => Err(UpstreamError::Generic(format!(
            "upstream status {status}: {body}"
        ))),
    }
}

async fn read_json(response: reqwest::Response) -> Result<serde_json::Value, UpstreamError> {
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|err| UpstreamError::Generic(err.to_string()))?;
    classify_status(status, &body)?;
    serde_json::from_str(&body).map_err(|err| UpstreamError::Generic(err.to_string()))
}

/// `GithubClient._headers` (`utils/github_client.py:52-58`).
fn github_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {token}")
            .parse()
            .unwrap_or_else(|_| header::HeaderValue::from_static("Bearer")),
    );
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        header::HeaderValue::from_static("2022-11-28"),
    );
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static("pi-dash-github-sync"),
    );
    headers
}

/// `GitLabClient._headers` (`adapters/gitlab.py:95-100`).
fn gitlab_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "PRIVATE-TOKEN",
        token
            .parse()
            .unwrap_or_else(|_| header::HeaderValue::from_static("")),
    );
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static("pi-dash-gitlab-sync"),
    );
    headers
}

/// `verify_provider_account` for github: `GET {api}/user`
/// (`utils/github_client.py:99-102`).
async fn github_verify(token: &str) -> Result<serde_json::Value, UpstreamError> {
    let client = upstream_client()?;
    let response = client
        .get(format!("{GITHUB_API_BASE}/user"))
        .headers(github_headers(token))
        .send()
        .await
        .map_err(|err| UpstreamError::Generic(err.to_string()))?;
    read_json(response).await
}

/// `verify_provider_account` for gitlab: `GET {host}/api/v4/user`
/// (`adapters/gitlab.py:136-137`), after the `_client` guards
/// (`:204-213`): https-only scheme and the host allowlist.
async fn gitlab_verify(
    token: &str,
    host_url: &str,
    allowed_hosts: &[String],
) -> Result<serde_json::Value, UpstreamError> {
    let normalized = gitlab_normalize_host(host_url);
    if url_scheme(&normalized) != "https" {
        return Err(UpstreamError::Permission(
            "GitLab host must use HTTPS".to_owned(),
        ));
    }
    if !allowed_hosts
        .iter()
        .any(|h| gitlab_normalize_host(h) == normalized)
    {
        return Err(UpstreamError::Permission(
            "GitLab host is not allowed; ask an instance admin to add it to GITLAB_ALLOWED_HOSTS."
                .to_owned(),
        ));
    }
    let client = upstream_client()?;
    let response = client
        .get(format!("{normalized}/api/v4/user"))
        .headers(gitlab_headers(token))
        .send()
        .await
        .map_err(|err| UpstreamError::Generic(err.to_string()))?;
    read_json(response).await
}

/// `list_repositories` for github (`github.py:145-154` +
/// `list_user_repos`, `github_client.py:103-114`): one page of
/// `/user/repos` with `has_next` from the `Link: rel="next"` header.
async fn github_list_repos(
    token: &str,
    page: i64,
) -> Result<(Vec<pidash_types::integrations::RemoteRepository>, bool), UpstreamError> {
    let client = upstream_client()?;
    let url = format!(
        "{GITHUB_API_BASE}/user/repos?affiliation=owner%2Ccollaborator%2Corganization_member&per_page=100&sort=updated&page={page}"
    );
    let response = client
        .get(url)
        .headers(github_headers(token))
        .send()
        .await
        .map_err(|err| UpstreamError::Generic(err.to_string()))?;
    let has_next = has_next_link(response.headers());
    let body = read_json(response).await?;
    let repos = body
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(github_remote_repo)
        .collect();
    Ok((repos, has_next))
}

/// `_next_url` (`github_client.py:74-83`): the `next` URL from a
/// paginated response's `Link` header.
fn has_next_link(headers: &HeaderMap) -> bool {
    let link = headers
        .get(header::LINK)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    link.split(',').any(|part| {
        let part = part.trim();
        part.starts_with('<') && part.contains(r#";"#) && part.contains(r#"rel="next""#)
    })
}

/// `list_repositories` for gitlab (`gitlab.py:313-320` + `list_projects`
/// `:139-153`): one page of `/projects`, `has_next` from `X-Next-Page`.
async fn gitlab_list_repos(
    token: &str,
    host_url: &str,
    allowed_hosts: &[String],
    page: i64,
) -> Result<(Vec<pidash_types::integrations::RemoteRepository>, bool), UpstreamError> {
    let normalized = gitlab_normalize_host(host_url);
    if url_scheme(&normalized) != "https" {
        return Err(UpstreamError::Permission(
            "GitLab host must use HTTPS".to_owned(),
        ));
    }
    if !allowed_hosts
        .iter()
        .any(|h| gitlab_normalize_host(h) == normalized)
    {
        return Err(UpstreamError::Permission(
            "GitLab host is not allowed; ask an instance admin to add it to GITLAB_ALLOWED_HOSTS."
                .to_owned(),
        ));
    }
    let client = upstream_client()?;
    let url = format!(
        "{normalized}/api/v4/projects?membership=true&simple=true&order_by=last_activity_at&sort=desc&page={page}&per_page=100"
    );
    let response = client
        .get(url)
        .headers(gitlab_headers(token))
        .send()
        .await
        .map_err(|err| UpstreamError::Generic(err.to_string()))?;
    let has_next = response
        .headers()
        .get("X-Next-Page")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| !v.is_empty());
    let body = read_json(response).await?;
    let repos = body
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(gitlab_remote_repo)
        .collect();
    Ok((repos, has_next))
}

/// Stored credential for one account (`account_credential`,
/// `services.py:50-54`): `credential_config` with `auth_type` then
/// `host_url` defaults, then the token (decrypted like `_token`:
/// decrypt-or-plaintext, `github.py:89-95`, `gitlab.py:192-198`).
fn stored_token(account: &GitProviderAccount, keyring: &Keyring) -> Result<String, Denial> {
    let token = account
        .credential_config
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if token.is_empty() {
        // `_token` per adapter (`github.py:95`, `gitlab.py:198`).
        let missing = if account.provider == "gitlab" {
            "GitLab token is missing"
        } else {
            "GitHub token is missing"
        };
        return Err(map_provider_error("auth", missing));
    }
    let decrypted = keyring.decrypt(token);
    Ok(if decrypted.is_empty() {
        token.to_owned()
    } else {
        decrypted
    })
}

// ---------------------------------------------------------------------------
// Request-data helpers (`request.data.get(...) or ...`, git.py:72-82)
// ---------------------------------------------------------------------------

/// One `request.data` field: falsy (missing, null, `""`, `0`, `false`,
/// empty containers) → `None` (the `or <default>` branch); a string →
/// its content; any other truthy value → 500 (`AttributeError` on
/// `.strip()`, the generic `handle_exception` branch).
fn data_field(data: &serde_json::Value, key: &str) -> Result<Option<String>, Denial> {
    match data.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        // `""` is falsy, so `(request.data.get(key) or <default>)` takes
        // the default — including for `auth_type` (`"" or "pat"`, git.py:78).
        Some(serde_json::Value::String(s)) if s.is_empty() => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(serde_json::Value::Bool(false)) => Ok(None),
        Some(serde_json::Value::Number(n)) if n.as_i64() == Some(0) || n.as_u64() == Some(0) => {
            Ok(None)
        }
        Some(serde_json::Value::Array(a)) if a.is_empty() => Ok(None),
        Some(serde_json::Value::Object(o)) if o.is_empty() => Ok(None),
        _ => Err(Denial::ServerError),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GitProvidersEndpoint.get` (`git.py:50-54`).
async fn get_providers(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial_response(denial),
    };
    let gate = match super::gates::gate_for("GET", "workspaces/<slug>/integrations/git/providers/")
    {
        Some(row) => &row.gate,
        None => return denial_response(Denial::ServerError),
    };
    if let Err(denial) = resolve_workspace_gate(&pool, &slug, extension, gate).await {
        return denial_response(denial);
    }
    if let Err(denial) = workspace_id(&pool, &slug).await {
        return denial_response(denial);
    }
    json_response(StatusCode::OK, PROVIDERS_BODY.to_owned())
}

/// `GitProviderAccountListCreateEndpoint.get` (`git.py:57-67`).
async fn list_accounts(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial_response(denial),
    };
    let gate = match super::gates::gate_for("GET", "workspaces/<slug>/integrations/git/accounts/") {
        Some(row) => &row.gate,
        None => return denial_response(Denial::ServerError),
    };
    if let Err(denial) = resolve_workspace_gate(&pool, &slug, extension, gate).await {
        return denial_response(denial);
    }
    let workspace = match workspace_id(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial_response(denial),
    };
    let accounts = match queries_git::fetch_provider_account_list(&pool, workspace).await {
        Ok(accounts) => accounts,
        Err(_) => return denial_response(Denial::ServerError),
    };
    let rendered: Vec<serde_json::Value> =
        accounts.iter().map(serialize_provider_account).collect();
    let mut body = serde_json::Map::with_capacity(1);
    body.insert("accounts".to_owned(), serde_json::Value::Array(rendered));
    json_response(StatusCode::OK, serde_json::Value::Object(body).to_string())
}

/// `GitProviderAccountListCreateEndpoint.post` (`git.py:69-94`).
async fn create_account(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Option<axum::Json<serde_json::Value>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial_response(denial),
    };
    let gate = match super::gates::gate_for("POST", "workspaces/<slug>/integrations/git/accounts/")
    {
        Some(row) => &row.gate,
        None => return denial_response(Denial::ServerError),
    };
    let actor = match resolve_workspace_gate(&pool, &slug, extension, gate).await {
        Ok(actor) => actor,
        Err(denial) => return denial_response(denial),
    };
    let workspace = match workspace_id(&pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial_response(denial),
    };
    let data = body.map(|b| b.0).unwrap_or(serde_json::Value::Null);
    let data = if data.is_null() {
        serde_json::json!({})
    } else {
        data
    };
    // `(request.data.get("provider") or "").strip().lower()` (`:72`).
    let provider = match data_field(&data, "provider") {
        Ok(value) => value.unwrap_or_default().trim().to_lowercase(),
        Err(denial) => return denial_response(denial),
    };
    if provider != "github" && provider != "gitlab" {
        return denial_response(Denial::BadError(
            "provider must be github or gitlab".to_owned(),
        ));
    }
    // `(request.data.get("token") or "").strip()` (`:75-77`).
    let token = match data_field(&data, "token") {
        Ok(value) => value.unwrap_or_default().trim().to_owned(),
        Err(denial) => return denial_response(denial),
    };
    if token.is_empty() {
        return denial_response(Denial::BadError("token is required".to_owned()));
    }
    // `(request.data.get("auth_type") or "pat").strip()` (`:78`): never
    // validated or lowercased — any string stores and serializes back.
    let auth_type = match data_field(&data, "auth_type") {
        Ok(value) => value.unwrap_or_else(|| "pat".to_owned()).trim().to_owned(),
        Err(denial) => return denial_response(denial),
    };
    // `(request.data.get("host_url") or <default>).rstrip("/")`
    // (`:79-82`): trailing-slash strip only, no scheme prepend here.
    let default_host = if provider == "github" {
        GITHUB_DEFAULT_HOST
    } else {
        GITLAB_DEFAULT_HOST
    };
    // `(request.data.get("host_url") or <default>).rstrip("/")`: the `or`
    // runs before the strip, so an all-slash value (`"///"`, truthy) strips
    // down to `""` instead of falling back to the default.
    let host_url = match data_field(&data, "host_url") {
        Ok(value) => value
            .unwrap_or_else(|| default_host.to_owned())
            .trim_end_matches('/')
            .to_owned(),
        Err(denial) => return denial_response(denial),
    };
    // `create_provider_account` (`services.py:72-116`).
    let normalized_host = queries_git::normalize_host_url(&host_url);
    let allowed_hosts = {
        let mut hosts = vec![GITLAB_DEFAULT_HOST.to_owned()];
        hosts.extend(state.settings().gitlab_allowed_hosts.iter().cloned());
        hosts
    };
    let identity = if provider == "github" {
        match github_verify(&token).await {
            Ok(identity) => identity,
            Err(err) => return denial_response(err.denial()),
        }
    } else {
        match gitlab_verify(&token, &normalized_host, &allowed_hosts).await {
            Ok(identity) => identity,
            Err(err) => return denial_response(err.denial()),
        }
    };
    let capabilities = if provider == "github" {
        github_capabilities(&auth_type)
    } else {
        gitlab_capabilities()
    };
    let display_name = if provider == "github" {
        "GitHub"
    } else {
        "GitLab"
    };
    let (external_id, login, display) = account_identity(&identity, display_name);
    let keyring = Keyring::from_secret(&state.settings().secret_key);
    let encrypted_token = keyring.encrypt(&token);
    let now = Utc::now();
    let account_id = uuid::Uuid::new_v4();
    let mut credential_config = serde_json::Map::with_capacity(3);
    credential_config.insert(
        "auth_type".to_owned(),
        serde_json::Value::String(auth_type.clone()),
    );
    credential_config.insert(
        "host_url".to_owned(),
        serde_json::Value::String(normalized_host.clone()),
    );
    credential_config.insert(
        "token".to_owned(),
        serde_json::Value::String(encrypted_token),
    );
    let mut metadata = serde_json::Map::with_capacity(1);
    metadata.insert("identity".to_owned(), identity);
    // `RETURNING` is mapped through the db layer's row reader (the row
    // struct carries no `FromRow`; same as `queries_git`).
    let inserted = sqlx::query(
        r#"INSERT INTO git_provider_accounts
           (id, created_at, updated_at, created_by_id, updated_by_id, workspace_id,
            provider, host_url, auth_type, external_account_id, external_account_login,
            display_name, capabilities, credential_config, status, verified_at,
            last_check_error, metadata)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,'connected',$15,'',$16)
           RETURNING id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, provider, host_url, auth_type, external_account_id,
            external_account_login, display_name, capabilities, credential_config,
            workspace_integration_id, status, verified_at, last_check_error, metadata"#,
    )
    .bind(account_id)
    .bind(now)
    .bind(now)
    .bind(actor)
    .bind(actor)
    .bind(workspace)
    .bind(&provider)
    .bind(&normalized_host)
    .bind(&auth_type)
    .bind(&external_id)
    .bind(&login)
    .bind(&display)
    .bind(&capabilities)
    .bind(serde_json::Value::Object(credential_config))
    .bind(now)
    .bind(serde_json::Value::Object(metadata))
    .fetch_one(&pool)
    .await;
    match inserted {
        Ok(row) => match queries_git::map_git_provider_account_row(&row) {
            Ok(account) => json_response(
                StatusCode::CREATED,
                serialize_provider_account(&account).to_string(),
            ),
            Err(_) => denial_response(Denial::ServerError),
        },
        Err(_) => denial_response(Denial::ServerError),
    }
}

/// Shared detail lookup for get/delete/repos:
/// `get_object_or_404(GitProviderAccount, id, workspace__slug)`
/// (`git.py:100,105,120`). A non-UUID `account_id` proxies to Django —
/// its `<uuid:>` converter 404s there exactly as before the cutover.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn detail_or_proxy(
    state: &AppState,
    slug: &str,
    account_id: &str,
    req: axum::extract::Request,
) -> Result<GitProviderAccount, Response> {
    let account_uuid = match account_id.parse::<uuid::Uuid>() {
        Ok(id) => id,
        Err(_) => return Err(crate::edge::proxy(State(state.clone()), req).await),
    };
    let pool = pool_of(state).map_err(denial_response)?;
    match queries_git::fetch_provider_account_detail(&pool, account_uuid, slug).await {
        Ok(Some(account)) => Ok(account),
        Ok(None) => Err(denial_response(Denial::AccountNotFound)),
        Err(_) => Err(denial_response(Denial::ServerError)),
    }
}

/// `GitProviderAccountDetailEndpoint.get` (`git.py:97-101`).
async fn get_account(
    State(state): State<AppState>,
    Path((slug, account_id)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial_response(denial),
    };
    let gate = match super::gates::gate_for(
        "GET",
        "workspaces/<slug>/integrations/git/accounts/<uuid>/",
    ) {
        Some(row) => &row.gate,
        None => return denial_response(Denial::ServerError),
    };
    if let Err(denial) = resolve_workspace_gate(&pool, &slug, extension, gate).await {
        return denial_response(denial);
    }
    match detail_or_proxy(&state, &slug, &account_id, req).await {
        Ok(account) => json_response(
            StatusCode::OK,
            serialize_provider_account(&account).to_string(),
        ),
        Err(response) => response,
    }
}

/// `GitProviderAccountDetailEndpoint.delete` (`git.py:103-114`): revoke —
/// `status=REVOKED`, `last_check_error`, token wiped (other credential
/// keys kept), `updated_at` bumped; bindings disabled. Not atomic, like
/// Python (save, then update).
async fn revoke_account(
    State(state): State<AppState>,
    Path((slug, account_id)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial_response(denial),
    };
    let gate = match super::gates::gate_for(
        "DELETE",
        "workspaces/<slug>/integrations/git/accounts/<uuid>/",
    ) {
        Some(row) => &row.gate,
        None => return denial_response(Denial::ServerError),
    };
    if let Err(denial) = resolve_workspace_gate(&pool, &slug, extension, gate).await {
        return denial_response(denial);
    }
    let account = match detail_or_proxy(&state, &slug, &account_id, req).await {
        Ok(account) => account,
        Err(response) => return response,
    };
    let revoked: Result<(), sqlx::Error> = async {
        sqlx::query(
            r#"UPDATE git_provider_accounts
               SET status = 'revoked',
                   last_check_error = 'Provider account disconnected',
                   credential_config = COALESCE(credential_config, '{}'::jsonb) || '{"token": ""}'::jsonb,
                   updated_at = now()
               WHERE id = $1"#,
        )
        .bind(account.id)
        .execute(&pool)
        .await?;
        sqlx::query(
            r#"UPDATE git_repository_bindings
               SET is_sync_enabled = false,
                   last_sync_error = 'Provider account disconnected'
               WHERE provider_account_id = $1 AND deleted_at IS NULL"#,
        )
        .bind(account.id)
        .execute(&pool)
        .await?;
        Ok(())
    }
    .await;
    match revoked {
        Ok(()) => json_response(StatusCode::OK, r#"{"connected":false}"#.to_owned()),
        Err(_) => denial_response(Denial::ServerError),
    }
}

/// `GitProviderAccountReposEndpoint.get` (`git.py:117-130`).
async fn list_repos(
    State(state): State<AppState>,
    Path((slug, account_id)): Path<(String, String)>,
    Query(query): Query<Vec<(String, String)>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial_response(denial),
    };
    let gate = match super::gates::gate_for(
        "GET",
        "workspaces/<slug>/integrations/git/accounts/<uuid>/repos/",
    ) {
        Some(row) => &row.gate,
        None => return denial_response(Denial::ServerError),
    };
    if let Err(denial) = resolve_workspace_gate(&pool, &slug, extension, gate).await {
        return denial_response(denial);
    }
    let account = match detail_or_proxy(&state, &slug, &account_id, req).await {
        Ok(account) => account,
        Err(response) => return response,
    };
    // `max(1, int(query page or "1"))`, `ValueError -> 1` (`:121-124`):
    // last-wins like `QueryDict.get`, over the raw pairs.
    let raw_page = query
        .iter()
        .rev()
        .find(|(key, _)| key == "page")
        .map(|(_, value)| value.as_str());
    let page = queries_git::parse_page_param(raw_page);
    let keyring = Keyring::from_secret(&state.settings().secret_key);
    let token = match stored_token(&account, &keyring) {
        Ok(token) => token,
        Err(denial) => return denial_response(denial),
    };
    let allowed_hosts = {
        let mut hosts = vec![GITLAB_DEFAULT_HOST.to_owned()];
        hosts.extend(state.settings().gitlab_allowed_hosts.iter().cloned());
        hosts
    };
    let (repos, has_next) = if account.provider == "gitlab" {
        match gitlab_list_repos(&token, &account.host_url, &allowed_hosts, page).await {
            Ok(result) => result,
            Err(err) => return denial_response(err.denial()),
        }
    } else {
        match github_list_repos(&token, page).await {
            Ok(result) => result,
            Err(err) => return denial_response(err.denial()),
        }
    };
    // `{"repos","page","has_next_page"}` (`services.py:381-389`).
    let rendered: Vec<serde_json::Value> = repos
        .iter()
        .map(|repo| serialize_remote_repository(repo, &account.host_url))
        .collect();
    let mut body = serde_json::Map::with_capacity(3);
    body.insert("repos".to_owned(), serde_json::Value::Array(rendered));
    body.insert("page".to_owned(), serde_json::Value::Number(page.into()));
    body.insert(
        "has_next_page".to_owned(),
        serde_json::Value::Bool(has_next),
    );
    json_response(StatusCode::OK, serde_json::Value::Object(body).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_body_is_byte_exact() {
        let body: serde_json::Value = serde_json::from_str(PROVIDERS_BODY).expect("valid json");
        assert_eq!(
            body,
            serde_json::json!({
                "providers": [
                    {"key": "github", "display_name": "GitHub", "code_review_term": "pull request"},
                    {"key": "gitlab", "display_name": "GitLab", "code_review_term": "merge request"},
                ]
            })
        );
        assert!(PROVIDERS_BODY.contains(r#""key":"github","display_name":"GitHub""#));
    }

    #[test]
    fn provider_error_defaults_match_error_response() {
        // `str(exc) or <default>` (git.py:39-47).
        assert!(
            matches!(map_provider_error("auth", ""), Denial::Provider(401, m) if m == "Provider rejected this credential")
        );
        assert!(
            matches!(map_provider_error("permission", ""), Denial::Provider(403, m) if m == "Provider credential lacks permission")
        );
        assert!(
            matches!(map_provider_error("notfound", ""), Denial::Provider(404, m) if m == "Repository not found or inaccessible")
        );
        assert!(
            matches!(map_provider_error("auth", "bad"), Denial::Provider(401, m) if m == "bad")
        );
        assert!(matches!(
            map_provider_error("other", "x"),
            Denial::BadUpstream(_)
        ));
    }

    #[test]
    fn identity_prefers_id_username_login_then_display_fallback() {
        let (external, login, display) =
            account_identity(&serde_json::json!({"id": 42, "login": "octocat"}), "GitHub");
        assert_eq!(
            (external.as_str(), login.as_str(), display.as_str()),
            ("42", "octocat", "octocat")
        );
        let (external, login, display) =
            account_identity(&serde_json::json!({"username": "u", "name": "N"}), "GitLab");
        assert_eq!(
            (external.as_str(), login.as_str(), display.as_str()),
            ("u", "u", "u")
        );
        let (external, login, display) = account_identity(&serde_json::json!({}), "GitHub");
        assert_eq!(
            (external.as_str(), login.as_str(), display.as_str()),
            ("", "", "GitHub account")
        );
        // Falsy `0` behaves like Python `0 or ""`.
        let (external, _, _) = account_identity(&serde_json::json!({"id": 0}), "GitHub");
        assert_eq!(external, "");
    }

    #[test]
    fn capabilities_follow_auth_type() {
        assert_eq!(github_capabilities("pat")["write_comments"], true);
        assert_eq!(github_capabilities("github_app")["manage_webhooks"], true);
        assert_eq!(github_capabilities("oauth")["write_comments"], false);
        assert_eq!(gitlab_capabilities()["write_comments"], true);
    }

    #[test]
    fn gitlab_host_normalization_vectors() {
        // FX-GIT-01 `normalize_host_url` vectors plus the gitlab lowercasing.
        assert_eq!(gitlab_normalize_host(""), GITLAB_DEFAULT_HOST);
        assert_eq!(
            gitlab_normalize_host("gitlab.example.com/"),
            "https://gitlab.example.com"
        );
        // `_normalize_host` drops the path (`f"{scheme}://{netloc}"`,
        // `adapters/gitlab.py:44-46`), it does not keep the tail.
        assert_eq!(
            gitlab_normalize_host("https://Git.Example.COM/x/"),
            "https://git.example.com"
        );
        assert_eq!(
            gitlab_normalize_host("http://git.internal:8080/"),
            "http://git.internal:8080"
        );
        assert_eq!(url_scheme("https://gitlab.com"), "https");
        assert_eq!(url_scheme("http://git.internal:8080"), "http");
    }

    #[test]
    fn data_field_mirrors_or_semantics() {
        let data = serde_json::json!({"a": " x ", "b": 0, "c": false, "d": 7, "e": ""});
        assert_eq!(data_field(&data, "a").unwrap(), Some(" x ".to_owned()));
        assert_eq!(data_field(&data, "b").unwrap(), None);
        assert_eq!(data_field(&data, "c").unwrap(), None);
        assert_eq!(data_field(&data, "missing").unwrap(), None);
        // `""` is falsy: `("" or "pat")` takes the default (git.py:78).
        assert_eq!(data_field(&data, "e").unwrap(), None);
        assert!(data_field(&data, "d").is_err());
    }

    #[test]
    fn github_remote_repo_lowercases_and_falls_back() {
        let repo = github_remote_repo(&serde_json::json!({
            "id": 1, "name": "R", "owner": {"login": "O"},
            "private": true, "clone_url": "c", "ssh_url": "s",
        }));
        assert_eq!(repo.full_name, "o/r");
        assert_eq!(repo.namespace, "o");
        assert_eq!(repo.web_url, "https://github.com/o/r");
        assert!(repo.is_private);
        let rendered = serialize_remote_repository(&repo, "https://github.com");
        let keys: Vec<&str> = rendered
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "provider",
                "host_url",
                "namespace",
                "name",
                "full_name",
                "web_url",
                "clone_url_http",
                "clone_url_ssh",
                "default_branch",
                "private"
            ]
        );
    }

    #[test]
    fn gitlab_remote_repo_splits_path() {
        let repo = gitlab_remote_repo(&serde_json::json!({
            "id": 9, "path_with_namespace": "g/sub/r", "visibility": "private",
        }));
        assert_eq!(
            (repo.namespace.as_str(), repo.name.as_str()),
            ("g/sub", "r")
        );
        assert!(repo.is_private);
        // Slashes strip before the `.git` suffix (`_split_full_path`,
        // `adapters/gitlab.py:78-83`): `"g/r.git/"` splits to `("g", "r")`.
        assert_eq!(
            split_full_path("g/r.git/"),
            Some(("g".to_owned(), "r".to_owned()))
        );
    }
}
