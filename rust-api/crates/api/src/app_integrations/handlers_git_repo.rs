#![forbid(unsafe_code)]

//! Git project-repository handlers (stage 5, PIDASHCONV-452).
//!
//! Ports `GitProjectRepositoryEndpoint` (GET/PATCH/DELETE) and
//! `GitProjectRepositoryBindEndpoint` (POST) from
//! `apps/api/pi_dash/app/views/integration/git.py:132-175`, with the
//! `bind_repository` / `get_binding` / `set_binding_sync_enabled` /
//! `unbind_repository` helpers from
//! `apps/api/pi_dash/integrations/git/services.py:297-379`, the
//! `parse_repository_url` fan-out from `registry.py:29-34`, the GitHub URL
//! grammar from `utils/github_client.py:236-254`, and the GitLab URL rules
//! from `adapters/gitlab.py:37-61,214-254`.
//!
//! Routes (from `app/urls/integration.py:101-108`):
//!
//! - `GET workspaces/<slug>/projects/<id>/repository/`
//! - `PATCH workspaces/<slug>/projects/<id>/repository/`
//! - `DELETE workspaces/<slug>/projects/<id>/repository/`
//! - `POST workspaces/<slug>/projects/<id>/repository/bind/`
//!
//! Registration is the cutover granularity (Porting guide F-02): the four
//! owned method+path pairs serve from Rust; every other method on those
//! paths proxies to Django through the fallback, so Django's own
//! `405 Method Not Allowed` bodies stay the contract there.
//!
//! Gates (fixture FX-PERM-01, [`crate::app_integrations::gates`] rows
//! `git.py:133`, `:140`, `:147`, `:157`): the read is the widest in D-33
//! (`PROJECT` level `ADMIN, MEMBER, GUEST`); the three writes are
//! `PROJECT` level `ADMIN`-only. Denials are the allow-style 403; session
//! auth (`BaseAPIView`, `views/base.py:189-194`) runs first (401), then the
//! `_rewrite_project_kwarg` identifier resolution (`views/base.py:49-79`,
//! 404 detail body on miss), then the gate.
//!
//! Layering: the binding read and the bind-resolution queryset come from the
//! queries layer (`pidash_db::app_integrations::queries_git`, PIDASHCONV-433,
//! Done); the gate table from [`crate::app_integrations::gates`]
//! (PIDASHCONV-436, Done). This module owns the HTTP shell (routes, session
//! auth, the project gate), the `serialize_binding` /
//! `serialize_repository` shapes (`services.py:232-295`), the URL parsers,
//! the `_error_response` mapping (`git.py:38-47`), and the bind/toggle/
//! unbind writes with Django's SQL semantics.
//!
//! Fixture ids: FX-GIT-02
//! (`rust-api/fixtures/app_integrations/fx-git-02-repo-bind.json`),
//! FX-PERM-01.
//!
//! Non-obvious faithful corners:
//!
//! - `GET` performs no workspace/project existence check: a missing binding
//!   (or a missing project row past the gate) answers
//!   `200 {"bound": false}` (`git.py:134-138`).
//! - `PATCH` reads `enabled` with `request.data.get("enabled")` and rejects
//!   anything that is not a JSON boolean — the string `"true"` is a 400
//!   (`git.py:142-144`).
//! - `DELETE` on a missing binding is not an error (`services.py:374-379`).
//! - `POST` strips `repo_url` before the blank check (`git.py:160`); the
//!   `201` payload is `serialize_binding` plus `repo_url` appended last
//!   (`git.py:173-175`).
//! - `set_binding_sync_enabled` on a missing binding raises
//!   `ProviderAccountNotFound("Repository is not bound")`, a 404
//!   (`services.py:361-364`), and also flips the legacy
//!   `GithubRepositorySync` rows for github-provider bindings (`:367-370`).
//! - `bind_repository` hard-deletes the existing binding and the legacy
//!   sync rows inside the transaction, creates the binding with
//!   `is_sync_enabled=False`, and only touches `project.repo_url` /
//!   `project.base_branch` when the guarded comparisons pass
//!   (`services.py:324-349`).
//! - GitHub lowercases owner/name; GitLab preserves case
//!   (`github.py:60-68`, `gitlab.py:227-233`).
//! - Token handling differs per adapter: GitHub returns the Fernet-decrypted
//!   value even when it decrypts to `""` (the `except` fallback in
//!   `github.py:87-94` is dead — `decrypt_data` never raises), so a stored
//!   plaintext token becomes `""` and fails as `"empty token"`; GitLab
//!   falls back to the raw token (`gitlab.py:191-198`, `decrypt_data(token)
//!   or token`), so plaintext works there.
//! - The GitLab client refuses non-HTTPS hosts and hosts outside the
//!   allowlist with `GitProviderPermissionError` (`gitlab.py:200-210`).
//!   The allowlist starts at `https://gitlab.com` plus
//!   `GITLAB_ALLOWED_HOSTS`; `GITLAB_HOST` is a Db-tier key the Rust boot
//!   settings do not carry, so a custom `GITLAB_HOST` is not honored here
//!   (all contract environments leave it unset, where the sets agree).
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! - None in this slice: `git.py:132-175` and `services.py:297-379` carry
//!   no intentional-behavior deviation beyond the quirks above, which are
//!   ported as-is.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::SecondsFormat;

use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_db::app_integrations::queries_git::{self, BoundRepository, ProjectBinding};
use pidash_db::integrations::git_models::git_provider_account;
use pidash_db::integrations::git_models::git_repository_binding;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the two project-repository paths with their owned methods.
/// Sibling methods stay unmatched and proxy to Django through the
/// fallback (notably Django's own 405s, e.g. `POST repository/`).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/repository/",
            owned_repository(),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/repository/bind/",
            owned_bind(),
        )
}

/// `repository/`: GET/PATCH/DELETE own their branches; POST/PUT/OPTIONS
/// fall through to Django (its create/update/405s live there).
fn owned_repository() -> axum::routing::MethodRouter<AppState> {
    axum::routing::get(get_repository)
        .patch(patch_repository)
        .delete(delete_repository)
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// `repository/bind/`: POST owns the bind; every other method falls
/// through to Django.
fn owned_bind() -> axum::routing::MethodRouter<AppState> {
    axum::routing::post(post_bind)
        .get(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Fixed bodies
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial
/// (`views/base.py:189-194`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s generic 500 branch (`views/base.py:147-149`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// DRF's default `Http404` body (missing workspace/project row in the
/// bind body).
pub const NOT_FOUND_DETAIL_BODY: &str = r#"{"detail":"Not found."}"#;
/// DRF `exception_handler` maps `Http404(*args)` to `NotFound(*args)`, so
/// the `_rewrite_project_kwarg` miss (`Project.resolve`, "Project not
/// found") renders with the resolve message — verified against live
/// Django, not the bare default.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `get_binding` miss (`git.py:136-137`) and the unbind answer
/// (`git.py:154`).
pub const UNBOUND_BODY: &str = r#"{"bound":false}"#;

// ---------------------------------------------------------------------------
// Denial / error mapping
// ---------------------------------------------------------------------------

/// Handler-level failure: `_error_response` (`git.py:38-47`) plus the DRF
/// dispatch order (401 auth, identifier-rewrite 404, 403 gate, body).
enum RepoDenial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body
    /// (`app/permissions/base.py:80-84`).
    Forbidden,
    /// 404, DRF `Http404` default body.
    NotFoundDetail,
    /// 404, `{"detail":"Project not found"}` (project-kwarg rewrite miss).
    ProjectNotFound,
    /// `{"error": message}` with an explicit status (view-inline 400s,
    /// the `_error_response` mapping, the 409 resolution branches).
    Error(StatusCode, String),
    /// 500, generic branch (`log_exception` + generic body).
    ServerError,
}

impl RepoDenial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            RepoDenial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            RepoDenial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            RepoDenial::NotFoundDetail => (StatusCode::NOT_FOUND, NOT_FOUND_DETAIL_BODY.to_owned()),
            RepoDenial::ProjectNotFound => {
                (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned())
            }
            RepoDenial::Error(status, message) => (*status, message.clone()),
            RepoDenial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }

    /// `{"error": message}` rendered the way DRF renders it (compact,
    /// UTF-8; `JSONRenderer` with `COMPACT_JSON` + `UNICODE_JSON`).
    fn error(status: StatusCode, message: impl Into<String>) -> Self {
        let body = format!(
            "{{\"error\":{}}}",
            serde_json::to_string(message.into().as_str()).expect("error message serializes")
        );
        RepoDenial::Error(status, body)
    }
}

impl IntoResponse for RepoDenial {
    fn into_response(self) -> Response {
        if matches!(self, RepoDenial::ServerError) {
            tracing::warn!("git repo handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("git repo denial response")
    }
}

/// Malformed request body: DRF `ParseError` branch
/// (`{"detail": "JSON parse error - ..."}`).
fn parse_error_response(err: &serde_json::Error) -> Response {
    let body = format!(
        "{{\"detail\":{}}}",
        serde_json::to_string(&format!("JSON parse error - {err}"))
            .expect("parse error serializes")
    );
    json_response(StatusCode::BAD_REQUEST, body)
}

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("git repo json response")
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, RepoDenial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(RepoDenial::ServerError)
}

// ---------------------------------------------------------------------------
// Session auth + project gate (DRF dispatch order)
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, or a non-UUID id means anonymous → 401. (Django PKs are UUIDs;
/// a session id that is not a UUID cannot be a user.)
fn actor_user_id(extension: Option<axum::Extension<SessionHandle>>) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    let raw = session.get("_auth_user_id")?.as_str()?.to_owned();
    raw.parse::<uuid::Uuid>().ok()
}

/// `Project.resolve(workspace_slug, value)`: UUIDs pass through (the row
/// check happens in the view body); other identifiers match
/// `UPPER(identifier)` in the workspace; misses raise `Http404`
/// (`db/models/project.py`, `views/base.py:49-79`).
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, RepoDenial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    row.map(|row| row.0).ok_or(RepoDenial::ProjectNotFound)
}

/// `@allow_permission` at the default `"PROJECT"` level: an active project
/// membership with a listed role, or any active project membership plus an
/// active workspace ADMIN membership (`app/permissions/base.py:53-64`).
/// Everything else (including a valid UUID with no membership row) is the
/// allow-style 403. Soft-deleted memberships do not count
/// (`SoftDeletionManager`).
async fn allow_project(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    roles: &[i16],
) -> Result<(), RepoDenial> {
    let role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    if let Some((role,)) = role {
        if roles.contains(&role) {
            return Ok(());
        }
    }
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    let admin: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = 20
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    if member.is_some() && admin.is_some() {
        Ok(())
    } else {
        Err(RepoDenial::Forbidden)
    }
}

/// The authenticated, gate-checked actor for one project-repository call:
/// 401 before the identifier rewrite (the slug-existence oracle stays
/// closed), the rewrite 404, then the role gate — Django's dispatch order.
async fn resolve_actor(
    pool: &sqlx::PgPool,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<SessionHandle>>,
    roles: &[i16],
) -> Result<(uuid::Uuid, uuid::Uuid), RepoDenial> {
    let user_id = actor_user_id(extension).ok_or(RepoDenial::Unauthorized)?;
    let project_id = resolve_project_id(pool, slug, project_raw).await?;
    allow_project(pool, slug, &project_id, &user_id, roles).await?;
    Ok((user_id, project_id))
}

/// `ROLE` values (`app/permissions/base.py:13-16`) for this slice's gates.
const ROLE_ADMIN: i16 = 20;
const ROLE_MEMBER: i16 = 15;
const ROLE_GUEST: i16 = 5;

// ---------------------------------------------------------------------------
// Serialization (`services.py:232-295`)
// ---------------------------------------------------------------------------

/// `serialize_repository` (`services.py:232-245`): note `id` is the
/// provider-side `external_id`, and the privacy key is `private`.
fn repository_json(repo: &BoundRepository) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::with_capacity(11);
    map.insert(
        "id".to_owned(),
        serde_json::Value::String(repo.external_id.clone()),
    );
    map.insert(
        "provider".to_owned(),
        serde_json::Value::String(repo.provider.clone()),
    );
    map.insert(
        "host_url".to_owned(),
        serde_json::Value::String(repo.host_url.clone()),
    );
    map.insert(
        "namespace".to_owned(),
        serde_json::Value::String(repo.namespace.clone()),
    );
    map.insert(
        "name".to_owned(),
        serde_json::Value::String(repo.name.clone()),
    );
    map.insert(
        "full_name".to_owned(),
        serde_json::Value::String(repo.full_name.clone()),
    );
    map.insert(
        "web_url".to_owned(),
        serde_json::Value::String(repo.web_url.clone()),
    );
    map.insert(
        "clone_url_http".to_owned(),
        serde_json::Value::String(repo.clone_url_http.clone()),
    );
    map.insert(
        "clone_url_ssh".to_owned(),
        serde_json::Value::String(repo.clone_url_ssh.clone()),
    );
    map.insert(
        "default_branch".to_owned(),
        serde_json::Value::String(repo.default_branch.clone()),
    );
    map.insert(
        "private".to_owned(),
        serde_json::Value::Bool(repo.is_private),
    );
    map
}

/// Render an aware datetime the way `Model.__getattribute__` +
/// DRF renders `last_synced_at.isoformat()` / `verified_at.isoformat()`:
/// the stored instant with its offset, microseconds only when nonzero.
/// (`serialize_binding` deliberately bypasses `TimezoneMixin`: no zone
/// shift happens on this path.)
fn isoformat(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

/// `serialize_binding` (`services.py:280-294`), key order as declared.
fn binding_json(binding: &ProjectBinding) -> serde_json::Map<String, serde_json::Value> {
    let degraded = binding.provider_account.status != git_provider_account::STATUS_CONNECTED;
    let mut map = serde_json::Map::with_capacity(12);
    map.insert("bound".to_owned(), serde_json::Value::Bool(true));
    map.insert(
        "id".to_owned(),
        serde_json::Value::String(binding.binding.id.to_string()),
    );
    map.insert(
        "provider".to_owned(),
        serde_json::Value::String(binding.repository.provider.clone()),
    );
    map.insert(
        "provider_account_id".to_owned(),
        serde_json::Value::String(binding.provider_account.id.to_string()),
    );
    map.insert(
        "host_url".to_owned(),
        serde_json::Value::String(binding.repository.host_url.clone()),
    );
    map.insert(
        "repository".to_owned(),
        serde_json::Value::Object(repository_json(&binding.repository)),
    );
    map.insert(
        "is_sync_enabled".to_owned(),
        serde_json::Value::Bool(binding.binding.is_sync_enabled),
    );
    map.insert(
        "clone_auth_mode".to_owned(),
        serde_json::Value::String(binding.binding.clone_auth_mode.clone()),
    );
    map.insert(
        "last_synced_at".to_owned(),
        binding
            .binding
            .last_synced_at
            .as_ref()
            .map(|dt| serde_json::Value::String(isoformat(dt)))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "last_sync_error".to_owned(),
        serde_json::Value::String(binding.binding.last_sync_error.clone()),
    );
    map.insert("degraded".to_owned(), serde_json::Value::Bool(degraded));
    map.insert(
        "degraded_reason".to_owned(),
        serde_json::Value::String(binding.provider_account.last_check_error.clone()),
    );
    map
}

/// Account fields the bind flow needs: auth type and capabilities (the
/// multi-account preference rank), the host, and the stored credential.
/// The candidate read stays in `created_at` order (`services.py:169-175`)
/// so multi-account probing order matches; the timestamp itself never
/// decides (`_account_preference` ranks on the first tuple half only).
struct ResolvableAccount {
    id: uuid::Uuid,
    auth_type: String,
    capabilities: serde_json::Value,
    host_url: String,
    credential_config: serde_json::Value,
}

/// `SELECT` one live account row by id for the explicit-`provider_account_id`
/// branch (`select_provider_account`, `services.py:118-130`): the id must
/// sit inside the CONNECTED/DEGRADED queryset for this host.
async fn fetch_resolution_account(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    provider: &str,
    host_url: &str,
    account_id: uuid::Uuid,
) -> Result<Option<ResolvableAccount>, sqlx::Error> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT a.id, a.auth_type, a.capabilities, a.host_url, a.credential_config
           FROM git_provider_accounts a
           WHERE a.deleted_at IS NULL AND a.workspace_id = $1 AND a.provider = $2
             AND a.host_url = $3 AND a.status = ANY($4) AND a.id = $5
           LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(provider)
    .bind(host_url)
    .bind(vec![
        git_provider_account::STATUS_CONNECTED,
        git_provider_account::STATUS_DEGRADED,
    ])
    .bind(account_id)
    .fetch_optional(pool)
    .await?;
    row.map(|row| map_resolvable_account(&row)).transpose()
}

/// Candidate accounts for the inferred branch (`services.py:169-175`,
/// `created_at` order): the branching itself is handler logic.
async fn fetch_resolution_candidates(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    provider: &str,
    host_url: &str,
) -> Result<Vec<ResolvableAccount>, sqlx::Error> {
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT a.id, a.auth_type, a.capabilities, a.host_url, a.credential_config
           FROM git_provider_accounts a
           WHERE a.deleted_at IS NULL AND a.workspace_id = $1 AND a.provider = $2
             AND a.host_url = $3 AND a.status = ANY($4)
           ORDER BY a.created_at ASC"#,
    )
    .bind(workspace_id)
    .bind(provider)
    .bind(host_url)
    .bind(vec![
        git_provider_account::STATUS_CONNECTED,
        git_provider_account::STATUS_DEGRADED,
    ])
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_resolvable_account).collect()
}

fn map_resolvable_account(row: &sqlx::postgres::PgRow) -> Result<ResolvableAccount, sqlx::Error> {
    use sqlx::Row;
    Ok(ResolvableAccount {
        id: row.try_get("id")?,
        auth_type: row.try_get("auth_type")?,
        capabilities: row.try_get("capabilities")?,
        host_url: row.try_get("host_url")?,
        credential_config: row.try_get("credential_config")?,
    })
}

/// `AuthType` values (`db/models/integration/git.py:18-24`).
const AUTH_TYPE_PAT: &str = "pat";
const AUTH_TYPE_GITHUB_APP: &str = "github_app";

/// `_account_preference` (`services.py:139-150`): the rank half decides;
/// the timestamp half only orders display, so the port keeps the rank.
fn account_rank(account: &ResolvableAccount) -> i32 {
    let write_comments = account
        .capabilities
        .get("write_comments")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if account.auth_type == AUTH_TYPE_PAT && write_comments {
        return 0;
    }
    if write_comments {
        return 1;
    }
    if account.auth_type == AUTH_TYPE_PAT {
        return 2;
    }
    if account.auth_type == AUTH_TYPE_GITHUB_APP {
        return 3;
    }
    4
}

// ---------------------------------------------------------------------------
// GET / PATCH / DELETE `repository/`
// ---------------------------------------------------------------------------

/// `GET repository/` (`git.py:133-138`): no existence check — a missing
/// binding answers `{"bound": false}`.
async fn get_repository(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_raw)): Path<(String, String)>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (_, project_id) = match resolve_actor(
        &pool,
        &slug,
        &project_raw,
        extension,
        &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
    )
    .await
    {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let binding = match queries_git::fetch_binding_for_project(&pool, project_id, &slug).await {
        Ok(binding) => binding,
        Err(_) => return RepoDenial::ServerError.into_response(),
    };
    match binding {
        None => json_response(StatusCode::OK, UNBOUND_BODY.to_owned()),
        Some(binding) => {
            let body = serde_json::Value::Object(binding_json(&binding)).to_string();
            json_response(StatusCode::OK, body)
        }
    }
}

/// `PATCH repository/` (`git.py:140-149`): the `enabled` boolean toggle.
async fn patch_repository(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_raw)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (_, project_id) =
        match resolve_actor(&pool, &slug, &project_raw, extension, &[ROLE_ADMIN]).await {
            Ok(actor) => actor,
            Err(denial) => return denial.into_response(),
        };
    // `request.data.get("enabled")`: anything but a JSON boolean is a 400.
    // A non-object body has no `.get` (`AttributeError` → base 500).
    let data: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(err) => return parse_error_response(&err),
    };
    let map = match data.as_object() {
        Some(map) => map,
        None => return RepoDenial::ServerError.into_response(),
    };
    let enabled = match map.get("enabled") {
        Some(serde_json::Value::Bool(enabled)) => *enabled,
        _ => {
            return RepoDenial::error(StatusCode::BAD_REQUEST, "enabled must be boolean")
                .into_response()
        }
    };
    let binding = match queries_git::fetch_binding_for_project(&pool, project_id, &slug).await {
        Ok(binding) => binding,
        Err(_) => return RepoDenial::ServerError.into_response(),
    };
    let binding = match binding {
        Some(binding) => binding,
        None => {
            return RepoDenial::error(StatusCode::NOT_FOUND, "Repository is not bound")
                .into_response()
        }
    };
    if let Err(denial) = set_binding_sync_enabled(&pool, &slug, &binding, enabled).await {
        return denial.into_response();
    }
    let refreshed = match queries_git::fetch_binding_for_project(&pool, project_id, &slug).await {
        Ok(refreshed) => refreshed,
        Err(_) => return RepoDenial::ServerError.into_response(),
    };
    match refreshed {
        Some(refreshed) => {
            let body = serde_json::Value::Object(binding_json(&refreshed)).to_string();
            json_response(StatusCode::OK, body)
        }
        None => RepoDenial::ServerError.into_response(),
    }
}

/// `set_binding_sync_enabled` (`services.py:361-371`): flip the binding
/// row; github-provider bindings also flip the legacy sync rows (scoped
/// by `workspace__slug`, like the Django filter).
async fn set_binding_sync_enabled(
    pool: &sqlx::PgPool,
    slug: &str,
    binding: &ProjectBinding,
    enabled: bool,
) -> Result<(), RepoDenial> {
    sqlx::query(
        r#"UPDATE git_repository_bindings
           SET is_sync_enabled = $1, updated_at = now()
           WHERE id = $2 AND deleted_at IS NULL"#,
    )
    .bind(enabled)
    .bind(binding.binding.id)
    .execute(pool)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    if binding.repository.provider == "github" {
        sqlx::query(
            r#"UPDATE github_repository_syncs s SET is_sync_enabled = $1
               FROM workspaces w
               WHERE s.workspace_id = w.id AND s.project_id = $2 AND w.slug = $3
                 AND s.deleted_at IS NULL"#,
        )
        .bind(enabled)
        .bind(binding.binding.project_id)
        .bind(slug)
        .execute(pool)
        .await
        .map_err(|_| RepoDenial::ServerError)?;
    }
    Ok(())
}

/// `DELETE repository/` (`git.py:151-154`): hard-delete the binding and the
/// legacy sync rows; a missing binding is not an error.
async fn delete_repository(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_raw)): Path<(String, String)>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (_, project_id) =
        match resolve_actor(&pool, &slug, &project_raw, extension, &[ROLE_ADMIN]).await {
            Ok(actor) => actor,
            Err(denial) => return denial.into_response(),
        };
    if let Err(denial) = unbind_repository(&pool, &slug, &project_id).await {
        return denial.into_response();
    }
    json_response(StatusCode::OK, UNBOUND_BODY.to_owned())
}

/// `unbind_repository` (`services.py:374-378`): hard deletes scoped to the
/// live rows of this workspace slug + project.
async fn unbind_repository(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<(), RepoDenial> {
    sqlx::query(
        r#"DELETE FROM git_repository_bindings b
           USING workspaces w
           WHERE b.workspace_id = w.id AND b.project_id = $1 AND w.slug = $2
             AND b.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .execute(pool)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    sqlx::query(
        r#"DELETE FROM github_repository_syncs s
           USING workspaces w
           WHERE s.workspace_id = w.id AND s.project_id = $1 AND w.slug = $2
             AND s.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .execute(pool)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// `parse_repository_url` (`registry.py:29-34` + adapter grammars)
// ---------------------------------------------------------------------------

/// What one adapter's `parse_repo_url` returns (`dtos.py:14-21`).
#[derive(Debug, Clone, PartialEq)]
struct ParsedRepository {
    provider: String,
    host_url: String,
    namespace: String,
    name: String,
    full_name: String,
    clone_url: String,
}

const GITHUB_HOST: &str = "https://github.com";

/// Strip one trailing `.git` the way `(?:\.git)?` does next to a lazy `name`
/// segment: the suffix only strips when the stem stays non-empty, so a name
/// of exactly `.git` is kept. Callers split `<owner>/<name>` first and apply
/// this to the name segment only (verified against the real regexes:
/// `https://github.com/o/.git` parses as `("o", ".git")`).
fn strip_git_suffix_once(name: &str) -> &str {
    match name.strip_suffix(".git") {
        Some(stem) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// `parse_github_repo_url` (`utils/github_client.py:236-254`): exactly the
/// two anchored grammars (case-sensitive, like the Python `re` patterns).
fn parse_github_repo_url(url: &str) -> Option<(String, String)> {
    let candidate = url.trim();
    if let Some(rest) = candidate
        .strip_prefix("https://github.com/")
        .or_else(|| candidate.strip_prefix("http://github.com/"))
    {
        return split_owner_name(rest);
    }
    if let Some(rest) = candidate.strip_prefix("git@github.com:") {
        // SSH grammar has no trailing-slash allowance (`$` right after
        // the optional suffix); `[^/\s]` forbids all whitespace.
        if rest.contains('/') && !rest.contains(char::is_whitespace) {
            let (owner, name) = rest.split_once('/')?;
            if owner.is_empty() || name.is_empty() || name.contains('/') {
                return None;
            }
            return Some((owner.to_owned(), strip_git_suffix_once(name).to_owned()));
        }
    }
    None
}

/// Shared tail of both GitHub grammars: `<owner>/<name>` with an optional
/// `.git` suffix and (HTTPS only) an optional trailing slash. The Python
/// `[^/\s]+?` segments forbid whitespace and slashes; the HTTPS pattern
/// anchors the end (`/?$`), so `/tree/main` never matches.
fn split_owner_name(rest: &str) -> Option<(String, String)> {
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() || rest.contains(char::is_whitespace) {
        return None;
    }
    let (owner, name) = rest.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some((owner.to_owned(), strip_git_suffix_once(name).to_owned()))
}

/// `parse_repo_url` for the GitHub adapter (`adapters/github.py:55-69`):
/// owner/name lowercased, `clone_url` the stripped input.
fn parse_github_url(url: &str) -> Option<ParsedRepository> {
    let (owner, name) = parse_github_repo_url(url)?;
    let owner = owner.to_lowercase();
    let name = name.to_lowercase();
    Some(ParsedRepository {
        provider: "github".to_owned(),
        host_url: GITHUB_HOST.to_owned(),
        namespace: owner.clone(),
        name: name.clone(),
        full_name: format!("{owner}/{name}"),
        clone_url: url.trim().to_owned(),
    })
}

/// `_normalize_host` (`adapters/gitlab.py:37-47`): the scheme check is
/// case-sensitive, but `urlparse` lowercases the scheme and the return
/// lowercases scheme + netloc — so `HTTP://h/` first gains an `https://`
/// prefix and then collapses to `https://http:` (verified against the
/// real function).
fn gitlab_normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('/');
    if host.is_empty() {
        return "https://gitlab.com".to_owned();
    }
    let host = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_owned()
    } else {
        format!("https://{host}")
    };
    match host.split_once("://") {
        Some((scheme, rest))
            if scheme.to_lowercase() == "http" || scheme.to_lowercase() == "https" =>
        {
            let netloc = rest.split('/').next().unwrap_or("");
            if netloc.is_empty() {
                host.trim_end_matches('/').to_owned()
            } else {
                format!("{}://{}", scheme.to_lowercase(), netloc.to_lowercase())
            }
        }
        _ => host.trim_end_matches('/').to_owned(),
    }
}

/// Split a URL-encoded host's path into `(namespace, name)`
/// (`_split_full_path`, `gitlab.py:78-84`).
fn gitlab_split_full_path(path: &str) -> Option<(String, String)> {
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() || !path.contains('/') {
        return None;
    }
    let (namespace, name) = path.rsplit_once('/')?;
    if namespace.is_empty() || name.is_empty() {
        return None;
    }
    Some((namespace.to_owned(), name.to_owned()))
}

/// `parse_repo_url` for the GitLab adapter (`adapters/gitlab.py:214-254`),
/// with the instance allowlist. Unlike GitHub, case is preserved.
fn parse_gitlab_url(url: &str, allowed_hosts: &[String]) -> Option<ParsedRepository> {
    let candidate = url.trim();
    if candidate.is_empty() {
        return None;
    }
    // SSH form first (`_ssh_repo_re`, `gitlab.py:189,218-234`).
    if let Some(rest) = candidate.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            let host_url = gitlab_normalize_host(host);
            if !allowed_hosts.iter().any(|h| h == &host_url) {
                return None;
            }
            let path = path.strip_suffix(".git").unwrap_or(path);
            let (namespace, name) = gitlab_split_full_path(path)?;
            return Some(ParsedRepository {
                provider: "gitlab".to_owned(),
                host_url: host_url.clone(),
                namespace: namespace.clone(),
                name: name.clone(),
                full_name: format!("{namespace}/{name}"),
                clone_url: candidate.to_owned(),
            });
        }
        return None;
    }
    // URL form (`gitlab.py:236-254`): scheme http/https/ssh, userinfo
    // stripped before the allowlist lookup, `/-/` suffixes dropped.
    // `urlparse` lowercases the scheme before the membership test, so the
    // comparison is case-insensitive (`HTTP://gitlab.com/g/r` parses).
    let (scheme, remainder) = candidate.split_once("://")?;
    let scheme_lower = scheme.to_lowercase();
    if !matches!(scheme_lower.as_str(), "http" | "https" | "ssh") {
        return None;
    }
    let after_scheme = remainder;
    let (authority, path) = match after_scheme.find('/') {
        Some(idx) => (&after_scheme[..idx], &after_scheme[idx..]),
        None => (after_scheme, "/"),
    };
    let bare_host = authority.rsplit('@').next().unwrap_or(authority);
    let host_url = gitlab_normalize_host(bare_host);
    if !allowed_hosts.iter().any(|h| h == &host_url) {
        return None;
    }
    let path = match path.split_once("/-/") {
        Some((before, _)) => before,
        None => path,
    };
    let (namespace, name) = gitlab_split_full_path(path)?;
    Some(ParsedRepository {
        provider: "gitlab".to_owned(),
        host_url: host_url.clone(),
        namespace: namespace.clone(),
        name: name.clone(),
        full_name: format!("{namespace}/{name}"),
        clone_url: candidate.to_owned(),
    })
}

/// The registry fan-out (`registry.py:29-34`): GitHub first, then GitLab;
/// `None` becomes `UnsupportedRepositoryURL`.
fn parse_repository_url(url: &str, gitlab_hosts: &[String]) -> Option<ParsedRepository> {
    if let Some(parsed) = parse_github_url(url) {
        return Some(parsed);
    }
    parse_gitlab_url(url, gitlab_hosts)
}

/// The instance GitLab allowlist (`_allowed_hosts`, `gitlab.py:49-61`):
/// always `https://gitlab.com`, plus the configured extra hosts.
/// (`GITLAB_HOST` is Db-tier and unreadable at boot; every contract
/// environment leaves it unset, where the two sets agree.)
fn gitlab_allowed_hosts(extra: &[String]) -> Vec<String> {
    let mut hosts = vec!["https://gitlab.com".to_owned()];
    for host in extra {
        if host.is_empty() {
            continue;
        }
        hosts.push(gitlab_normalize_host(host));
    }
    hosts
}

// ---------------------------------------------------------------------------
// POST `repository/bind/` (`git.py:157-175`, `services.py:297-350`)
// ---------------------------------------------------------------------------

/// Provider-side failure, mirroring the adapter exception trio
/// (`adapters/base.py:27-37`) plus the unmapped fallthrough.
enum ProviderError {
    /// `GitProviderAuthError` → 401.
    Auth(String),
    /// `GitProviderPermissionError` → 403.
    Permission(String),
    /// `GitProviderNotFoundError` → 404.
    NotFound(String),
    /// Anything else (transport errors, `raise_for_status` leftovers,
    /// bad payloads) → 400 with the message.
    Other(String),
}

impl ProviderError {
    /// `_error_response` (`git.py:38-47`).
    fn denial(self) -> RepoDenial {
        match self {
            ProviderError::Auth(message) => RepoDenial::error(
                StatusCode::UNAUTHORIZED,
                default_message(message, "Provider rejected this credential"),
            ),
            ProviderError::Permission(message) => RepoDenial::error(
                StatusCode::FORBIDDEN,
                default_message(message, "Provider credential lacks permission"),
            ),
            ProviderError::NotFound(message) => RepoDenial::error(
                StatusCode::NOT_FOUND,
                default_message(message, "Repository not found or inaccessible"),
            ),
            ProviderError::Other(message) => RepoDenial::error(StatusCode::BAD_REQUEST, message),
        }
    }
}

fn default_message(message: String, fallback: &str) -> String {
    if message.is_empty() {
        fallback.to_owned()
    } else {
        message
    }
}

/// What the provider fetch returns (`dtos.py:32-46`).
#[derive(Debug, Clone)]
struct RemoteRepository {
    external_id: String,
    namespace: String,
    name: String,
    full_name: String,
    web_url: String,
    clone_url_http: String,
    clone_url_ssh: String,
    default_branch: String,
    is_private: bool,
    metadata: serde_json::Value,
}

/// `account_credential` (`services.py:56-60`): the stored config with the
/// auth type / host defaults filled in.
fn account_credential(account: &ResolvableAccount) -> serde_json::Map<String, serde_json::Value> {
    let mut config = account
        .credential_config
        .as_object()
        .cloned()
        .unwrap_or_default();
    config
        .entry("auth_type")
        .or_insert_with(|| serde_json::Value::String(account.auth_type.clone()));
    config
        .entry("host_url")
        .or_insert_with(|| serde_json::Value::String(account.host_url.clone()));
    config
}

fn credential_token(credential: &serde_json::Map<String, serde_json::Value>) -> String {
    credential
        .get("token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// Python truthiness for a decoded JSON value, for the `X or ""` / `if X`
/// branches (`git.py:160`, `services.py:160`): DRF decodes numbers to
/// int/float, so only an exact zero is falsy.
fn json_is_falsy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Bool(flag) => !flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n == 0.0),
        serde_json::Value::String(text) => text.is_empty(),
        serde_json::Value::Array(items) => items.is_empty(),
        serde_json::Value::Object(fields) => fields.is_empty(),
    }
}

/// GitHub `_token` (`github.py:87-94`): the Fernet-decrypted value is
/// returned even when it decrypts to `""` (`decrypt_data` never raises,
/// so the `except` fallback is dead); only a missing raw token raises.
fn github_token(
    credential: &serde_json::Map<String, serde_json::Value>,
    keyring: &pidash_db::config::encryption::Keyring,
) -> Result<String, ProviderError> {
    let raw = credential_token(credential);
    if raw.is_empty() {
        return Err(ProviderError::Auth("GitHub token is missing".to_owned()));
    }
    let decrypted = keyring.decrypt(&raw);
    if decrypted.is_empty() {
        // `GithubClient("")` → `GithubAuthError("empty token")`.
        return Err(ProviderError::Auth("empty token".to_owned()));
    }
    Ok(decrypted)
}

/// GitLab `_token` (`gitlab.py:191-198`): empty decryptions fall back to
/// the raw token (`decrypt_data(token) or token`).
fn gitlab_token(
    credential: &serde_json::Map<String, serde_json::Value>,
    keyring: &pidash_db::config::encryption::Keyring,
) -> Result<String, ProviderError> {
    let raw = credential_token(credential);
    if raw.is_empty() {
        return Err(ProviderError::Auth("GitLab token is missing".to_owned()));
    }
    let decrypted = keyring.decrypt(&raw);
    let token = if decrypted.is_empty() { raw } else { decrypted };
    // `GitLabClient("")` → `GitProviderAuthError("GitLab token is missing")`.
    if token.is_empty() {
        return Err(ProviderError::Auth("GitLab token is missing".to_owned()));
    }
    Ok(token)
}

/// Percent-encode for `quote(value, safe="")` (`gitlab.py:160-162`):
/// everything outside the unreserved set.
fn quote_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `GithubClient.get_repo` (`github_client.py:116-123`): one
/// `GET /repos/{owner}/{repo}` with the client headers and the
/// 401/403/404 mapping (`_request`, `:62-76`).
async fn fetch_github_repository(
    http: &reqwest::Client,
    token: &str,
    namespace: &str,
    name: &str,
) -> Result<RemoteRepository, ProviderError> {
    let url = format!("https://api.github.com/repos/{namespace}/{name}");
    let response = http
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "pi-dash-github-sync")
        .send()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let status = response.status();
    if status.as_u16() == 401 {
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::Auth(text));
    }
    if status.as_u16() == 403 {
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::Permission(text));
    }
    if status.as_u16() == 404 {
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::NotFound(text));
    }
    if !status.is_success() {
        // `raise_for_status()` → unmapped `HTTPError` → 400.
        let reason = status.canonical_reason().unwrap_or("error");
        return Err(ProviderError::Other(format!(
            "{} {reason} for url: {url}",
            status.as_u16()
        )));
    }
    let text = response
        .text()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let payload: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| ProviderError::Other(err.to_string()))?;
    Ok(map_github_remote(&payload))
}

/// `_remote_repo` for GitHub (`github.py:126-143`).
fn map_github_remote(payload: &serde_json::Value) -> RemoteRepository {
    let owner_value = payload.get("owner");
    let owner_login = owner_value
        .and_then(|owner| owner.get("login"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let owner_string = owner_value
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let owner = if owner_login.is_empty() {
        owner_string
    } else {
        owner_login
    };
    let owner = if owner.is_empty() {
        String::new()
    } else {
        owner.to_lowercase()
    };
    let name = payload
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let fallback_full = format!("{owner}/{name}");
    let full_name = payload
        .get("full_name")
        .and_then(serde_json::Value::as_str)
        .map(|full| {
            if full.is_empty() {
                fallback_full.clone()
            } else {
                full.to_lowercase()
            }
        })
        .unwrap_or(fallback_full);
    let namespace = full_name
        .rsplit_once('/')
        .map(|(namespace, _)| namespace.to_owned())
        .filter(|_| full_name.contains('/'))
        .unwrap_or(owner);
    let str_field = |key: &str| {
        payload
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let external_id = payload
        .get("id")
        .map(|id| {
            if id.is_null() {
                String::new()
            } else if let Some(number) = id.as_i64() {
                number.to_string()
            } else if let Some(text) = id.as_str() {
                if text.is_empty() {
                    String::new()
                } else {
                    text.to_owned()
                }
            } else {
                id.to_string()
            }
        })
        .unwrap_or_default();
    let web_url = {
        let html_url = str_field("html_url");
        if html_url.is_empty() {
            format!("{GITHUB_HOST}/{full_name}")
        } else {
            html_url
        }
    };
    RemoteRepository {
        external_id,
        namespace,
        name,
        full_name,
        web_url,
        clone_url_http: str_field("clone_url"),
        clone_url_ssh: str_field("ssh_url"),
        default_branch: str_field("default_branch"),
        is_private: payload
            .get("private")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        metadata: payload.clone(),
    }
}

/// `GitLabClient.get_project` (`gitlab.py:158-162`): one
/// `GET /projects/{urlencoded}` with no redirect following
/// (`allow_redirects=False`, `:102-114`).
async fn fetch_gitlab_repository(
    http: &reqwest::Client,
    token: &str,
    host_url: &str,
    full_name: &str,
) -> Result<RemoteRepository, ProviderError> {
    let url = format!(
        "{}/api/v4/projects/{}",
        host_url.trim_end_matches('/'),
        quote_path_segment(full_name)
    );
    let response = http
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .header("Accept", "application/json")
        .header("User-Agent", "pi-dash-gitlab-sync")
        .send()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let status = response.status();
    if status.is_redirection() {
        return Err(ProviderError::Permission(
            "GitLab API redirects are not followed".to_owned(),
        ));
    }
    if status.as_u16() == 401 {
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::Auth(text));
    }
    if status.as_u16() == 403 {
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::Permission(text));
    }
    if status.as_u16() == 404 {
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::NotFound(text));
    }
    if !status.is_success() {
        let reason = status.canonical_reason().unwrap_or("error");
        return Err(ProviderError::Other(format!(
            "{} {reason} for url: {url}",
            status.as_u16()
        )));
    }
    let text = response
        .text()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let payload: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| ProviderError::Other(err.to_string()))?;
    Ok(map_gitlab_remote(&payload))
}

/// `_remote_repo` for GitLab (`gitlab.py:294-312`).
fn map_gitlab_remote(payload: &serde_json::Value) -> RemoteRepository {
    let str_field = |key: &str| {
        payload
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let full_name = {
        let namespaced = str_field("path_with_namespace");
        if namespaced.is_empty() {
            str_field("path")
        } else {
            namespaced
        }
    };
    let (namespace, name) =
        gitlab_split_full_path(&full_name).unwrap_or((String::new(), str_field("path")));
    let external_id = payload
        .get("id")
        .map(|id| {
            if id.is_null() {
                String::new()
            } else if let Some(number) = id.as_i64() {
                number.to_string()
            } else if let Some(text) = id.as_str() {
                if text.is_empty() {
                    String::new()
                } else {
                    text.to_owned()
                }
            } else {
                id.to_string()
            }
        })
        .unwrap_or_default();
    RemoteRepository {
        external_id,
        namespace,
        name,
        full_name,
        web_url: str_field("web_url"),
        clone_url_http: str_field("http_url_to_repo"),
        clone_url_ssh: str_field("ssh_url_to_repo"),
        default_branch: str_field("default_branch"),
        is_private: payload
            .get("visibility")
            .and_then(serde_json::Value::as_str)
            .map(|visibility| visibility == "private")
            .unwrap_or(false),
        metadata: payload.clone(),
    }
}

/// `adapter.get_repository(account_credential(account), parsed)` for the
/// resolved account, including the GitLab client host guards
/// (`gitlab.py:200-210`).
async fn fetch_remote_repository(
    http: &reqwest::Client,
    parsed: &ParsedRepository,
    account: &ResolvableAccount,
    keyring: &pidash_db::config::encryption::Keyring,
    allowed_gitlab_hosts: &[String],
) -> Result<RemoteRepository, ProviderError> {
    let credential = account_credential(account);
    match parsed.provider.as_str() {
        "github" => {
            let token = github_token(&credential, keyring)?;
            fetch_github_repository(http, &token, &parsed.namespace, &parsed.name).await
        }
        _ => {
            let host_url = credential
                .get("host_url")
                .and_then(serde_json::Value::as_str)
                .filter(|host| !host.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| "https://gitlab.com".to_owned());
            let normalized = gitlab_normalize_host(&host_url);
            let scheme = normalized.split_once("://").map(|(scheme, _)| scheme);
            if scheme != Some("https") {
                return Err(ProviderError::Permission(
                    "GitLab host must use HTTPS".to_owned(),
                ));
            }
            if !allowed_gitlab_hosts.iter().any(|h| h == &normalized) {
                return Err(ProviderError::Permission(
                    "GitLab host is not allowed; ask an instance admin to add it to GITLAB_ALLOWED_HOSTS.".to_owned(),
                ));
            }
            let token = gitlab_token(&credential, keyring)?;
            fetch_gitlab_repository(http, &token, &normalized, &parsed.full_name).await
        }
    }
}

/// `canonical_clone_url` (`services.py:228-229`): first non-empty wins.
fn canonical_clone_url(remote: &RemoteRepository, raw_url: &str) -> String {
    if !remote.clone_url_http.is_empty() {
        return remote.clone_url_http.clone();
    }
    if !raw_url.is_empty() {
        return raw_url.to_owned();
    }
    remote.web_url.clone()
}

/// `POST repository/bind/` (`git.py:157-175`).
async fn post_bind(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, project_raw)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (user_id, project_id) =
        match resolve_actor(&pool, &slug, &project_raw, extension, &[ROLE_ADMIN]).await {
            Ok(actor) => actor,
            Err(denial) => return denial.into_response(),
        };
    // `request.data`: a non-object body has no `.get` (`AttributeError` →
    // base 500).
    let data: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(err) => return parse_error_response(&err),
    };
    let map = match data.as_object() {
        Some(map) => map,
        None => return RepoDenial::ServerError.into_response(),
    };
    // `(request.data.get("repo_url") or "").strip()` (`git.py:160`): only
    // strings survive — falsy values collapse to the blank check, while a
    // truthy non-string has no `.strip` (`AttributeError` → base 500).
    let repo_url = match map.get("repo_url") {
        Some(serde_json::Value::String(raw)) => raw.trim().to_owned(),
        Some(value) if !json_is_falsy(value) => {
            return RepoDenial::ServerError.into_response();
        }
        _ => String::new(),
    };
    if repo_url.is_empty() {
        return RepoDenial::error(StatusCode::BAD_REQUEST, "repo_url is required").into_response();
    }
    // `resolve_provider_account_repository(..., provider_account_id=...)`
    // branches on truthiness (`services.py:160`): falsy values (missing,
    // null, `""`, `0`, `false`, `[]`, `{}`) take the inferred-account
    // branch; only a truthy value reaches the UUID filter.
    let provider_account_id = match map.get("provider_account_id") {
        None => None,
        Some(value) if json_is_falsy(value) => None,
        Some(serde_json::Value::String(raw)) => match raw.parse::<uuid::Uuid>() {
            Ok(id) => Some(id),
            // `queryset.filter(id=...)` with a bad UUID → `ValidationError`
            // → `{"error": "Please provide valid detail"}`.
            Err(_) => {
                return RepoDenial::error(StatusCode::BAD_REQUEST, "Please provide valid detail")
                    .into_response()
            }
        },
        // Truthy non-string ids never match the UUID filter either.
        Some(_) => {
            return RepoDenial::error(StatusCode::BAD_REQUEST, "Please provide valid detail")
                .into_response()
        }
    };
    let gitlab_hosts = gitlab_allowed_hosts(&state.settings().gitlab_allowed_hosts);
    let parsed = match parse_repository_url(&repo_url, &gitlab_hosts) {
        Some(parsed) => parsed,
        None => {
            return RepoDenial::error(
                StatusCode::BAD_REQUEST,
                "A supported GitHub or GitLab repository URL is required",
            )
            .into_response()
        }
    };
    // `get_object_or_404(Workspace, slug=...)` then
    // `get_object_or_404(Project, pk=..., workspace=...)`: both are
    // `Http404` → the DRF detail body.
    let workspace_row: Option<(uuid::Uuid,)> = match sqlx::query_as(
        r#"SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return RepoDenial::ServerError.into_response(),
    };
    let workspace_id = match workspace_row {
        Some((id,)) => id,
        None => return RepoDenial::NotFoundDetail.into_response(),
    };
    let project_row: Option<(uuid::Uuid, String, String)> = match sqlx::query_as(
        r#"SELECT p.id, p.repo_url, p.base_branch FROM projects p
           WHERE p.id = $1 AND p.workspace_id = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(workspace_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return RepoDenial::ServerError.into_response(),
    };
    let (_, project_repo_url, project_base_branch) = match project_row {
        Some(project) => project,
        None => return RepoDenial::NotFoundDetail.into_response(),
    };
    // Resolve the provider account (`resolve_provider_account_repository`,
    // `services.py:153-200`), then fetch the remote.
    let keyring = pidash_db::config::encryption::Keyring::from_secret(&state.settings().secret_key);
    let http = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(http) => http,
        Err(_) => return RepoDenial::ServerError.into_response(),
    };
    let (account, remote) = match resolve_account_remote(
        &pool,
        &http,
        &workspace_id,
        &parsed,
        provider_account_id,
        &keyring,
        &gitlab_hosts,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(RepoDenial::Error(status, body)) => return json_response(status, body),
        Err(denial) => return denial.into_response(),
    };
    // The transactional writes (`bind_repository`, `services.py:324-350`).
    let bound = match bind_transaction(
        &pool,
        &slug,
        &workspace_id,
        &project_id,
        &user_id,
        &parsed,
        &account,
        &remote,
        &repo_url,
        &project_repo_url,
        &project_base_branch,
    )
    .await
    {
        Ok(bound) => bound,
        Err(denial) => return denial.into_response(),
    };
    let mut payload = binding_json(&bound);
    payload.insert(
        "repo_url".to_owned(),
        serde_json::Value::String(canonical_clone_url(&remote, &parsed.clone_url)),
    );
    // `bind_repository` returns `clone_url` computed from the *parsed*
    // `clone_url` (`services.py:317`), which for GitHub is the stripped
    // input and for GitLab the raw candidate — both equal `parsed.clone_url`.
    let body = serde_json::Value::Object(payload).to_string();
    json_response(StatusCode::CREATED, body)
}

/// Resolve `(account, remote)` for a bind (`select_provider_account` +
/// `resolve_provider_account_repository`, `services.py:118-200`).
async fn resolve_account_remote(
    pool: &sqlx::PgPool,
    http: &reqwest::Client,
    workspace_id: &uuid::Uuid,
    parsed: &ParsedRepository,
    provider_account_id: Option<uuid::Uuid>,
    keyring: &pidash_db::config::encryption::Keyring,
    gitlab_hosts: &[String],
) -> Result<(ResolvableAccount, RemoteRepository), RepoDenial> {
    if let Some(account_id) = provider_account_id {
        let account = fetch_resolution_account(
            pool,
            *workspace_id,
            &parsed.provider,
            &parsed.host_url,
            account_id,
        )
        .await
        .map_err(|_| RepoDenial::ServerError)?;
        let account = match account {
            Some(account) => account,
            None => {
                return Err(RepoDenial::error(
                    StatusCode::NOT_FOUND,
                    "Provider account not found for this repository host",
                ));
            }
        };
        let remote = fetch_remote_repository(http, parsed, &account, keyring, gitlab_hosts)
            .await
            .map_err(ProviderError::denial)?;
        return Ok((account, remote));
    }
    let accounts =
        fetch_resolution_candidates(pool, *workspace_id, &parsed.provider, &parsed.host_url)
            .await
            .map_err(|_| RepoDenial::ServerError)?;
    if accounts.is_empty() {
        return Err(RepoDenial::error(
            StatusCode::CONFLICT,
            "Connect a provider account before binding this repository",
        ));
    }
    if accounts.len() == 1 {
        let mut accounts = accounts;
        let account = accounts.pop().expect("one account");
        let remote = fetch_remote_repository(http, parsed, &account, keyring, gitlab_hosts)
            .await
            .map_err(ProviderError::denial)?;
        return Ok((account, remote));
    }
    let mut matches: Vec<(ResolvableAccount, RemoteRepository)> = Vec::new();
    let mut last_provider_error: Option<ProviderError> = None;
    for account in accounts {
        match fetch_remote_repository(http, parsed, &account, keyring, gitlab_hosts).await {
            Ok(remote) => matches.push((account, remote)),
            Err(ProviderError::Auth(message)) => {
                last_provider_error = Some(ProviderError::Auth(message));
            }
            Err(ProviderError::Permission(message)) => {
                last_provider_error = Some(ProviderError::Permission(message));
            }
            Err(ProviderError::NotFound(message)) => {
                last_provider_error = Some(ProviderError::NotFound(message));
            }
            Err(other) => return Err(other.denial()),
        }
    }
    if matches.is_empty() {
        match last_provider_error {
            Some(err) => return Err(err.denial()),
            None => {
                return Err(RepoDenial::error(
                    StatusCode::CONFLICT,
                    "Connect a provider account before binding this repository",
                ));
            }
        }
    }
    let best_rank = matches
        .iter()
        .map(|(account, _)| account_rank(account))
        .min()
        .expect("matches non-empty");
    let mut best = matches
        .into_iter()
        .filter(|(account, _)| account_rank(account) == best_rank)
        .collect::<Vec<_>>();
    if best.len() == 1 {
        return Ok(best.pop().expect("one best match"));
    }
    Err(RepoDenial::error(
        StatusCode::CONFLICT,
        "Multiple provider accounts can access this repository; choose one",
    ))
}

/// The `bind_repository` writes (`services.py:316-350`) in one
/// transaction: upsert the repository, hard-delete any existing binding
/// and legacy sync rows, create the binding, patch the project row.
/// Returns the fresh binding read (the `select_related` shape the
/// serializer needs).
///
/// The parameter list mirrors `bind_repository`'s keyword arguments
/// one-to-one, hence the arity.
#[allow(clippy::too_many_arguments)]
async fn bind_transaction(
    pool: &sqlx::PgPool,
    slug: &str,
    workspace_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    parsed: &ParsedRepository,
    account: &ResolvableAccount,
    remote: &RemoteRepository,
    raw_url: &str,
    project_repo_url: &str,
    project_base_branch: &str,
) -> Result<ProjectBinding, RepoDenial> {
    let mut tx = pool.begin().await.map_err(|_| RepoDenial::ServerError)?;
    let repository_id = upsert_repository(&mut tx, parsed, remote)
        .await
        .map_err(|_| RepoDenial::ServerError)?;
    let clone_auth_mode = if remote.is_private {
        git_repository_binding::CLONE_AUTH_MODE_RUNNER_MANAGED
    } else {
        git_repository_binding::CLONE_AUTH_MODE_PUBLIC
    };
    sqlx::query(
        r#"DELETE FROM git_repository_bindings
           WHERE project_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    sqlx::query(
        r#"DELETE FROM github_repository_syncs
           WHERE project_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    let binding_id = uuid::Uuid::new_v4();
    let metadata = serde_json::json!({"raw_url": raw_url});
    sqlx::query(
        r#"INSERT INTO git_repository_bindings
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
              project_id, workspace_id, repository_id, provider_account_id, actor_id,
              is_sync_enabled, clone_auth_mode, last_synced_at, last_sync_error, metadata)
           VALUES ($1, now(), now(), $2, $2, NULL,
                   $3, $4, $5, $6, $2,
                   FALSE, $7, NULL, '', $8)"#,
    )
    .bind(binding_id)
    .bind(user_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(repository_id)
    .bind(account.id)
    .bind(clone_auth_mode)
    .bind(metadata)
    .execute(&mut *tx)
    .await
    .map_err(|_| RepoDenial::ServerError)?;
    let clone_url = canonical_clone_url(remote, &parsed.clone_url);
    let mut updates: Vec<(&str, String)> = Vec::new();
    if project_repo_url != clone_url {
        updates.push(("repo_url", clone_url));
    }
    if !remote.default_branch.is_empty() && project_base_branch.is_empty() {
        updates.push(("base_branch", remote.default_branch.clone()));
    }
    for (column, value) in updates {
        let sql = format!("UPDATE projects SET {column} = $1 WHERE id = $2");
        sqlx::query(&sql)
            .bind(value)
            .bind(project_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| RepoDenial::ServerError)?;
    }
    tx.commit().await.map_err(|_| RepoDenial::ServerError)?;
    queries_git::fetch_binding_for_project(pool, *project_id, slug)
        .await
        .map_err(|_| RepoDenial::ServerError)?
        .ok_or(RepoDenial::ServerError)
}

/// `upsert_repository` (`services.py:203-225`): the lookup is
/// provider + host + (`external_id` when set, else `full_name`); the
/// write covers every default column plus `updated_at`.
async fn upsert_repository(
    ex: &mut sqlx::PgConnection,
    parsed: &ParsedRepository,
    remote: &RemoteRepository,
) -> Result<uuid::Uuid, sqlx::Error> {
    let host_url = parsed.host_url.trim_end_matches('/');
    let existing: Option<(uuid::Uuid,)> = if !remote.external_id.is_empty() {
        sqlx::query_as(
            r#"SELECT r.id FROM git_repositories r
               WHERE r.provider = $1 AND r.host_url = $2 AND r.external_id = $3
                 AND r.deleted_at IS NULL LIMIT 1"#,
        )
        .bind(&parsed.provider)
        .bind(host_url)
        .bind(&remote.external_id)
        .fetch_optional(&mut *ex)
        .await?
    } else {
        sqlx::query_as(
            r#"SELECT r.id FROM git_repositories r
               WHERE r.provider = $1 AND r.host_url = $2 AND r.full_name = $3
                 AND r.deleted_at IS NULL LIMIT 1"#,
        )
        .bind(&parsed.provider)
        .bind(host_url)
        .bind(&remote.full_name)
        .fetch_optional(&mut *ex)
        .await?
    };
    if let Some((id,)) = existing {
        sqlx::query(
            r#"UPDATE git_repositories SET external_id = $1, namespace = $2, name = $3,
                      full_name = $4, web_url = $5, clone_url_http = $6, clone_url_ssh = $7,
                      default_branch = $8, is_private = $9, metadata = $10, updated_at = now()
               WHERE id = $11"#,
        )
        .bind(&remote.external_id)
        .bind(&remote.namespace)
        .bind(&remote.name)
        .bind(&remote.full_name)
        .bind(&remote.web_url)
        .bind(&remote.clone_url_http)
        .bind(&remote.clone_url_ssh)
        .bind(&remote.default_branch)
        .bind(remote.is_private)
        .bind(&remote.metadata)
        .bind(id)
        .execute(&mut *ex)
        .await?;
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO git_repositories
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
              provider, host_url, external_id, namespace, name, full_name, web_url,
              clone_url_http, clone_url_ssh, default_branch, is_private, metadata)
           VALUES ($1, now(), now(), NULL, NULL, NULL,
                   $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"#,
    )
    .bind(id)
    .bind(&parsed.provider)
    .bind(host_url)
    .bind(&remote.external_id)
    .bind(&remote.namespace)
    .bind(&remote.name)
    .bind(&remote.full_name)
    .bind(&remote.web_url)
    .bind(&remote.clone_url_http)
    .bind(&remote.clone_url_ssh)
    .bind(&remote.default_branch)
    .bind(remote.is_private)
    .bind(&remote.metadata)
    .execute(&mut *ex)
    .await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::app_integrations::queries_git::BoundAccount;

    fn gitlab_hosts() -> Vec<String> {
        gitlab_allowed_hosts(&[])
    }

    /// FX-GIT-02 `parse_repository_url` executed vectors
    /// (`github_client.py:236-254`, `registry.py:29-34`).
    #[test]
    fn github_url_vectors() {
        let hosts = gitlab_hosts();
        for url in [
            "https://github.com/owner/repo",
            "https://github.com/owner/repo.git",
            "git@github.com:owner/repo.git",
        ] {
            let parsed =
                parse_repository_url(url, &hosts).unwrap_or_else(|| panic!("parses: {url}"));
            assert_eq!(parsed.provider, "github", "{url}");
            assert_eq!(parsed.namespace, "owner", "{url}");
            assert_eq!(parsed.name, "repo", "{url}");
            assert_eq!(parsed.host_url, GITHUB_HOST, "{url}");
        }
        assert_eq!(
            parse_repository_url("https://github.com/owner/repo/tree/main", &hosts),
            None
        );
        assert_eq!(
            parse_repository_url("https://gitlab.com/owner/repo", &hosts)
                .map(|parsed| parsed.provider),
            Some("gitlab".to_owned())
        );
        assert_eq!(
            parse_repository_url("https://bitbucket.org/o/r", &hosts),
            None
        );
        assert_eq!(parse_repository_url("", &hosts), None);
        assert_eq!(parse_repository_url("   ", &hosts), None);
    }

    #[test]
    fn github_case_and_suffix_edges() {
        let hosts = gitlab_hosts();
        // Owner/name lowercase; `clone_url` keeps the stripped input.
        let parsed =
            parse_repository_url("https://github.com/Owner/Repo.git", &hosts).expect("parses");
        assert_eq!(
            (parsed.namespace, parsed.name),
            ("owner".to_owned(), "repo".to_owned())
        );
        assert_eq!(parsed.clone_url, "https://github.com/Owner/Repo.git");
        // SSH has no trailing-slash allowance.
        assert_eq!(parse_repository_url("git@github.com:o/r/", &hosts), None);
        // Case-sensitive host.
        assert_eq!(parse_repository_url("https://GITHUB.COM/o/r", &hosts), None);
        // Whitespace inside segments never matches.
        assert_eq!(
            parse_repository_url("https://github.com/o/r b", &hosts),
            None
        );
    }

    #[test]
    fn gitlab_url_rules() {
        let hosts = gitlab_hosts();
        let parsed =
            parse_repository_url("https://gitlab.com/group/sub/repo", &hosts).expect("parses");
        assert_eq!(parsed.provider, "gitlab");
        assert_eq!(parsed.host_url, "https://gitlab.com");
        // Nested namespaces keep every leading segment; case preserved.
        assert_eq!(parsed.namespace, "group/sub");
        assert_eq!(parsed.name, "repo");
        // `/-/` view suffixes are dropped before the split.
        let parsed =
            parse_repository_url("https://gitlab.com/group/repo/-/merge_requests/1", &hosts)
                .expect("parses");
        assert_eq!(
            (parsed.namespace, parsed.name),
            ("group".to_owned(), "repo".to_owned())
        );
        // SSH form with a non-allowlisted host is rejected.
        assert_eq!(
            parse_repository_url("git@example.com:group/repo.git", &hosts),
            None
        );
        // Custom hosts join the allowlist through settings.
        let custom = gitlab_allowed_hosts(&["git.example.com".to_owned()]);
        let parsed =
            parse_repository_url("https://git.example.com/group/repo", &custom).expect("parses");
        assert_eq!(parsed.host_url, "https://git.example.com");
    }

    #[test]
    fn gitlab_host_normalization() {
        assert_eq!(gitlab_normalize_host(""), "https://gitlab.com");
        assert_eq!(
            gitlab_normalize_host("git.example.com/"),
            "https://git.example.com"
        );
        // Uppercase scheme misses the case-sensitive prefix check, gains
        // `https://`, then collapses (executed against the real Python).
        assert_eq!(
            gitlab_normalize_host("HTTP://git.example.com/"),
            "https://http:"
        );
        assert_eq!(
            gitlab_normalize_host("https://GITLAB.COM/"),
            "https://gitlab.com"
        );
    }

    #[test]
    fn quote_encodes_like_urllib() {
        assert_eq!(quote_path_segment("group/sub"), "group%2Fsub");
        assert_eq!(quote_path_segment("a b~c"), "a%20b~c");
    }

    fn sample_binding() -> ProjectBinding {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap();
        ProjectBinding {
            binding: git_repository_binding::GitRepositoryBinding {
                id: uuid::Uuid::nil(),
                created_at: now,
                updated_at: now,
                created_by_id: None,
                updated_by_id: None,
                deleted_at: None,
                project_id: uuid::Uuid::nil(),
                workspace_id: uuid::Uuid::nil(),
                repository_id: uuid::Uuid::nil(),
                provider_account_id: uuid::Uuid::nil(),
                actor_id: uuid::Uuid::nil(),
                is_sync_enabled: false,
                clone_auth_mode: "public".to_owned(),
                last_synced_at: None,
                last_sync_error: String::new(),
                metadata: serde_json::json!({}),
            },
            repository: BoundRepository {
                id: uuid::Uuid::nil(),
                provider: "github".to_owned(),
                host_url: GITHUB_HOST.to_owned(),
                external_id: "123".to_owned(),
                namespace: "owner".to_owned(),
                name: "repo".to_owned(),
                full_name: "owner/repo".to_owned(),
                web_url: "https://github.com/owner/repo".to_owned(),
                clone_url_http: "https://github.com/owner/repo.git".to_owned(),
                clone_url_ssh: "git@github.com:owner/repo.git".to_owned(),
                default_branch: "main".to_owned(),
                is_private: false,
            },
            provider_account: BoundAccount {
                id: uuid::Uuid::nil(),
                status: "connected".to_owned(),
                last_check_error: String::new(),
            },
        }
    }

    /// `serialize_binding` shape (`services.py:280-294`): exact keys in
    /// declared order, `id` from the provider `external_id`.
    #[test]
    fn serialize_binding_shape() {
        let body = serde_json::Value::Object(binding_json(&sample_binding())).to_string();
        let nil = uuid::Uuid::nil();
        assert_eq!(
            body,
            format!(
                "{{\"bound\":true,\
                 \"id\":\"{nil}\",\
                 \"provider\":\"github\",\
                 \"provider_account_id\":\"{nil}\",\
                 \"host_url\":\"https://github.com\",\
                 \"repository\":{{\"id\":\"123\",\
                 \"provider\":\"github\",\
                 \"host_url\":\"https://github.com\",\
                 \"namespace\":\"owner\",\
                 \"name\":\"repo\",\
                 \"full_name\":\"owner/repo\",\
                 \"web_url\":\"https://github.com/owner/repo\",\
                 \"clone_url_http\":\"https://github.com/owner/repo.git\",\
                 \"clone_url_ssh\":\"git@github.com:owner/repo.git\",\
                 \"default_branch\":\"main\",\
                 \"private\":false}},\
                 \"is_sync_enabled\":false,\
                 \"clone_auth_mode\":\"public\",\
                 \"last_synced_at\":null,\
                 \"last_sync_error\":\"\",\
                 \"degraded\":false,\
                 \"degraded_reason\":\"\"}}"
            )
        );
    }

    #[test]
    fn serialize_binding_degraded_and_synced_at() {
        use chrono::TimeZone;
        let mut binding = sample_binding();
        binding.provider_account.status = "degraded".to_owned();
        binding.provider_account.last_check_error = "stale".to_owned();
        binding.binding.last_synced_at = Some(
            chrono::Utc
                .with_ymd_and_hms(2026, 9, 29, 12, 34, 56)
                .unwrap(),
        );
        let body = serde_json::Value::Object(binding_json(&binding)).to_string();
        assert!(body.contains("\"degraded\":true"), "{body}");
        assert!(body.contains("\"degraded_reason\":\"stale\""), "{body}");
        assert!(
            body.contains("\"last_synced_at\":\"2026-09-29T12:34:56+00:00\""),
            "{body}"
        );
    }

    /// `_error_response` status mapping (`git.py:38-47`).
    #[test]
    fn provider_error_status_map() {
        let (status, _) = ProviderError::Auth("x".to_owned())
            .denial()
            .status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = ProviderError::Permission("x".to_owned())
            .denial()
            .status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = ProviderError::NotFound("x".to_owned())
            .denial()
            .status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, body) = ProviderError::Other("boom".to_owned())
            .denial()
            .status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"error":"boom"}"#);
        // Empty provider messages fall back to the defaults.
        let (_, body) = ProviderError::Auth(String::new())
            .denial()
            .status_and_body();
        assert_eq!(body, r#"{"error":"Provider rejected this credential"}"#);
    }

    /// Review fix: the `.git` suffix strips from the name segment only, so
    /// a repo literally named `.git` parses like the Python regexes do
    /// (`("o", ".git")`, verified against `utils/github_client.py:236-254`).
    #[test]
    fn github_dot_git_name_kept() {
        let hosts = gitlab_hosts();
        for url in [
            "https://github.com/o/.git",
            "https://github.com/o/.git/",
            "git@github.com:o/.git",
        ] {
            let parsed =
                parse_repository_url(url, &hosts).unwrap_or_else(|| panic!("parses: {url}"));
            assert_eq!(parsed.provider, "github", "{url}");
            assert_eq!(parsed.namespace, "o", "{url}");
            assert_eq!(parsed.name, ".git", "{url}");
        }
        // Ordinary suffix stripping is unchanged.
        let parsed = parse_repository_url("https://github.com/o/r.git", &hosts).expect("parses");
        assert_eq!(parsed.name, "r");
        let parsed =
            parse_repository_url("https://github.com/o/r.git.git", &hosts).expect("parses");
        assert_eq!(parsed.name, "r.git");
    }

    /// Review fix: `urlparse` lowercases the scheme before the membership
    /// test, so uppercase-scheme GitLab URLs parse (`adapters/gitlab.py:236`).
    #[test]
    fn gitlab_uppercase_scheme_parses() {
        let hosts = gitlab_hosts();
        let parsed = parse_repository_url("HTTP://gitlab.com/group/repo", &hosts).expect("parses");
        assert_eq!(parsed.provider, "gitlab");
        assert_eq!(parsed.namespace, "group");
        assert_eq!(parsed.name, "repo");
        assert_eq!(
            parse_repository_url("FTP://gitlab.com/group/repo", &hosts),
            None
        );
    }

    /// Review fix: Python truthiness behind `or ""` / `if X`
    /// (`git.py:160`, `services.py:160`).
    #[test]
    fn json_truthiness_matches_python() {
        for falsy in [
            serde_json::Value::Null,
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!(0.0),
            serde_json::json!(""),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            assert!(json_is_falsy(&falsy), "{falsy}");
        }
        for truthy in [
            serde_json::json!(true),
            serde_json::json!(123),
            serde_json::json!("x"),
            serde_json::json!("   "),
            serde_json::json!([0]),
            serde_json::json!({"a": 0}),
        ] {
            assert!(!json_is_falsy(&truthy), "{truthy}");
        }
    }

    #[test]
    fn account_preference_ranks() {
        let mut account = ResolvableAccount {
            id: uuid::Uuid::nil(),
            auth_type: "pat".to_owned(),
            capabilities: serde_json::json!({"write_comments": true}),
            host_url: GITHUB_HOST.to_owned(),
            credential_config: serde_json::json!({}),
        };
        assert_eq!(account_rank(&account), 0);
        account.auth_type = "oauth".to_owned();
        assert_eq!(account_rank(&account), 1);
        account.capabilities = serde_json::json!({});
        account.auth_type = "pat".to_owned();
        assert_eq!(account_rank(&account), 2);
        account.auth_type = "github_app".to_owned();
        assert_eq!(account_rank(&account), 3);
        account.auth_type = "oauth".to_owned();
        assert_eq!(account_rank(&account), 4);
    }
}
