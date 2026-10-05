//! D-33 GitHub project bind/status handlers (stage 5, PIDASHCONV-450).
//!
//! Ports `apps/api/pi_dash/app/views/integration/github.py:962-1205`
//! (`GithubProjectBindEndpoint.post`, `GithubProjectStatusEndpoint`
//! get/patch/delete) onto the foundation crates:
//!
//! * `POST workspaces/<slug>/projects/<id>/github/bind/` (`:981-1082`)
//! * `GET workspaces/<slug>/projects/<id>/github/` (`:1093-1146`)
//! * `PATCH workspaces/<slug>/projects/<id>/github/` (`:1148-1172`)
//! * `DELETE workspaces/<slug>/projects/<id>/github/` (`:1174-1205`)
//!
//! Only these four path+method pairs are registered, so the edge serves
//! exactly this family from Rust while every sibling path keeps proxying
//! to Django — route registration is the cutover granularity, no flag
//! needed. Every other method on the owned paths proxies too, so Django
//! answers its own 405-after-auth and metadata OPTIONS byte for byte
//! (the loop-handlers precedent).
//!
//! Handler order (preserved, not redesigned): DRF authentication
//! (`BaseAPIView`: session auth + `IsAuthenticated`,
//! `views/base.py:189-194`) runs before the decorator, the decorator
//! before the body — and the body validates `repo_url`, then reads the
//! workspace/project rows, then the workspace-integration, then calls
//! GitHub. Anonymous callers never reach a gate (401 `{"detail": ...}`);
//! gate denials answer the allow-style 403. The `_feature_enabled` 404
//! (`github.py:82-87`) fires only after a passing gate. The
//! `_rewrite_project_kwarg` identifier resolution
//! (`views/base.py:49-77` + `Project.resolve`) runs before the gate for
//! authenticated callers: a UUID passes through unchecked, anything else
//! resolves as a workspace-scoped upper-cased identifier, and an
//! unresolvable value answers `{"detail":"Project not found"}` (probed
//! live; note: no trailing period).
//!
//! Fixture ids: FX-GHA-03
//! (`fx-gha-03-project.json`), FX-PERM-01
//! (`fx-perm-01-permission-matrix.json`, the three project-github rows).
//!
//! Status reads join `workspaces`/`github_repositories`/`git_repositories`
//! with NO `deleted_at` filter on the joined rows — only the base
//! sync/binding row carries the default-manager filter. Probed live
//! (probe W: a soft-deleted workspace still reports its binding): the
//! Done-layer `repository_sync_for_project_sql` /
//! `github_binding_for_project_sql` builders filter the joined workspace
//! row and would answer `{bound: False}` there, so this module owns its
//! two status SELECTs. Foundation crates are read-only; the builders are
//! left untouched.
//!
//! Ported bugs and quirks (translate, don't redesign — also listed in
//! the PR):
//!
//! * BUG stale comment (`github.py:1021-1029`): the rebind comment cites
//!   a `OneToOneField` uniqueness on `repository`, but the model is a
//!   plain `ForeignKey` with only the conditional unique on `project`.
//!   The BEHAVIOUR (synchronous hard delete of the existing sync, FK
//!   cascades hard-deleting dependent rows) is ported, not the comment.
//! * `int(verified.get("id") or 0)` (`:1014`) runs outside the `try`, so
//!   a truthy non-numeric id is an unhandled `ValueError`/`TypeError`
//!   (500), while a falsy one answers the 502. [`python_repo_id`]
//!   mirrors both.
//! * `(request.data.get("repo_url") or "").strip()` (`:986`): a truthy
//!   non-string (number, `true`, non-empty list/dict) raises
//!   `AttributeError` (500); a JSON `null` body makes `request.data`
//!   `None`, so `.get` raises too (500, probed). Only falsy values fall
//!   through to the 400. [`repo_url_field`] mirrors this.
//! * `get_repo` maps 401 → `GithubAuthError` (401) and 404 →
//!   `GithubNotFoundError` (404); `GithubPermissionError` (403) is a
//!   plain `Exception` sibling, so a 403 answers the generic 502, and
//!   every other failure (transport, `raise_for_status`, bad JSON)
//!   answers it too (`:1010-1012`).
//! * `save(update_fields=[...])` leaves `updated_at`/`updated_by`
//!   untouched (probed: PATCH toggle and the project `repo_url` save
//!   write only the named column), and `QuerySet.update()` touches only
//!   its named column. Only creates, `update_or_create` hits (full save)
//!   and instance soft deletes touch the audit columns.
//! * `get_object_or_404` misses render
//!   `{"detail":"No <VerboseName> matches the given query."}` (probed:
//!   `No Workspace ...` / `No Project ...`), not the generic 404 body.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::ROLE_ADMIN;
use pidash_types::WorkspaceId;
use serde_json::Value;
use uuid::Uuid;

use pidash_db::app_integrations::queries_github::{
    fetch_integration_id_by_provider, fetch_workspace_integration,
};

use super::gates::{decide_gate, gate_for, tenant_context, GateOutcome, GITHUB_DISABLED_BODY};
use crate::assistant::common::{body_get, parse_body, BodyField, ParsedBody};
use crate::license::{json_response, resolve_actor, Denial};
use crate::state::AppState;

/// `app/urls/integration.py` under the `api/` include: project status
/// (`github-project-status`).
pub const STATUS_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/github/";
/// `app/urls/integration.py`: project bind (`github-project-bind`).
pub const BIND_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/github/bind/";

/// `github.py:986-988`.
const REPO_REQUIRED_BODY: &str = r#"{"error":"repo_url is required"}"#;
/// `github.py:989-994`.
const BAD_URL_BODY: &str =
    r#"{"error":"Only github.com URLs are supported (e.g. https://github.com/owner/repo)"}"#;
/// `github.py:999-1001` (409).
const NOT_CONNECTED_BODY: &str = r#"{"error":"Workspace GitHub integration is not connected"}"#;
/// `github.py:1006-1007` (404).
const REPO_NOT_FOUND_BODY: &str = r#"{"error":"Repository not found or token has no access"}"#;
/// `github.py:1008-1009` (401).
const TOKEN_REJECTED_BODY: &str = r#"{"error":"GitHub token rejected"}"#;
/// `github.py:1010-1012` (502).
const FAILED_VERIFY_BODY: &str = r#"{"error":"Failed to verify repository"}"#;
/// `github.py:1014-1016` (502).
const NO_REPO_ID_BODY: &str = r#"{"error":"GitHub did not return a repository id"}"#;
/// `github.py:1066-1070` (409).
const BIND_CONFLICT_BODY: &str =
    r#"{"error":"Could not bind: concurrent change detected, please retry"}"#;
/// `github.py:1153-1154` (400).
const ENABLED_REQUIRED_BODY: &str = r#"{"error":"enabled (bool) is required"}"#;
/// `github.py:1162-1163` (404).
const NO_BINDING_BODY: &str = r#"{"error":"No GitHub binding for this project"}"#;
/// `get_object_or_404(Workspace, ...)` miss (404, probed live).
const NO_WORKSPACE_BODY: &str = r#"{"detail":"No Workspace matches the given query."}"#;
/// `get_object_or_404(Project, ...)` miss (404, probed live).
const NO_PROJECT_BODY: &str = r#"{"detail":"No Project matches the given query."}"#;
/// `Project.resolve` miss via `_rewrite_project_kwarg` (404, probed
/// live — verbatim, no trailing period).
const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// Unbound GET/DELETE shape (`github.py:1113-1114,1189,1205`).
const UNBOUND_BODY: &str = r#"{"bound":false}"#;

/// `GithubClient` default (`utils/github_client.py:22`).
const GITHUB_API_BASE: &str = "https://api.github.com";
/// `GithubClient.__init__` default timeout (`github_client.py:23,39`).
const GITHUB_TIMEOUT_SECS: u64 = 30;
/// `GithubProviderAccount` host looked up for the bind account
/// (`github.py:362-369`).
const GITHUB_HOST_URL: &str = "https://github.com";

/// Register the four project-github routes. Nothing else: sibling paths
/// stay unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            STATUS_PATH,
            get(get_status)
                .patch(patch_status)
                .delete(delete_status)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            BIND_PATH,
            post(post_bind)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// shared reads
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `request.user` or the 401. Mirrors `BaseAPIView.authentication_classes`
/// + `IsAuthenticated` (`views/base.py:189-194`).
async fn actor(
    state: &AppState,
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    match resolve_actor(pool, state.settings().secret_key.as_bytes(), extension).await {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(Denial::Unauthorized),
        Err(_) => Err(Denial::ServerError),
    }
}

/// Active workspace role for `(user, slug)`, or `None` (no row).
/// Mirrors the `allow_permission` workspace lookup (`is_active=True`,
/// soft-deleted rows excluded, `permissions/base.py:44-51`). The joined
/// `workspaces` row carries no deleted filter, like Django's join.
async fn workspace_role(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    slug: &str,
) -> Result<Option<i32>, Denial> {
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

/// Active project role for `(user, project_id, slug)`, or `None`.
/// Mirrors the project-level lookup (`permissions/base.py:53-64`).
async fn project_role(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    slug: &str,
) -> Result<Option<i32>, Denial> {
    let row: Option<(i16,)> = sqlx::query_as(
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
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(role,)| i32::from(role)))
}

/// Membership facts for one `Gate::Project` row: the allowed-role flags
/// are computed against that row's `roles` (the kernel's
/// `has_allowed_project_role` is gate-relative), while the
/// workspace-admin override (`is_project_member && is_workspace_admin`,
/// `permissions/base.py:56-64`) is role-independent.
fn project_facts(
    slug: &str,
    roles: &[i32],
    ws_role: Option<i32>,
    pm_role: Option<i32>,
) -> AllowFacts {
    AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: ws_role.is_some(),
        has_allowed_workspace_role: ws_role.is_some_and(|role| roles.contains(&role)),
        is_creator: false,
        has_allowed_project_role: pm_role.is_some_and(|role| roles.contains(&role)),
        is_project_member: pm_role.is_some(),
        is_workspace_admin: ws_role == Some(ROLE_ADMIN),
    }
}

/// Enforce one D-33 gate row: allow runs, deny answers the allow-style
/// 403 (`{"error":"You don't have the required permissions."}`).
#[allow(clippy::result_large_err)]
fn enforce(outcome: GateOutcome) -> Result<(), Response> {
    match outcome {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Deny => Err(crate::permissions::PermissionDenied.into_response()),
        GateOutcome::Unauthenticated => Err(Denial::Unauthorized.into_response()),
    }
}

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a non-UUID identifier for the equality lookup
/// (`db/models/project.py:210`): `str(value).strip().upper()`.
fn normalize_resolve_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// Resolve the `project_id` URL kwarg for an authenticated caller
/// (`_rewrite_project_kwarg`, `views/base.py:49-77` + `Project.resolve`,
/// `db/models/project.py:190-217`): a UUID passes through unchecked,
/// anything else resolves as a workspace-scoped upper-cased identifier
/// (soft-deleted projects excluded); an unresolvable value answers the
/// verbatim `{"detail":"Project not found"}` 404 (probed live).
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn resolve_project_id(pool: &sqlx::PgPool, slug: &str, raw: &str) -> Result<Uuid, Response> {
    if let Ok(id) = raw.parse::<Uuid>() {
        return Ok(id);
    }
    let normalized = normalize_resolve_identifier(raw);
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(normalized)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    row.map(|(id,)| id)
        .ok_or_else(|| raw_body(StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY))
}

fn raw_body(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static response")
}

fn created_json(value: &Value) -> Response {
    let body = serde_json::to_string(value).expect("serializable response");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("created response")
}

// ---------------------------------------------------------------------------
// pure helpers (unit-tested below)
// ---------------------------------------------------------------------------

/// JSON truthiness as Python's `or` sees it: only null, false, zero,
/// `""`, `[]` and `{}` are falsy.
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            number.as_i64().is_some_and(|n| n != 0) || number.as_f64().is_some_and(|n| n != 0.0)
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `parse_github_repo_url` (`utils/github_client.py:236-254`): github.com
/// only (case-sensitive), owner/name with no `/` or whitespace,
/// optional single `.git` suffix, optional single trailing slash on the
/// https form, sub-paths rejected. The `.git` strip keeps a bare
/// `".git"` name intact, matching the lazy regex (`[^/\s]+?` must match
/// at least one char before the optional suffix).
fn parse_github_repo_url(url: &str) -> Option<(String, String)> {
    let candidate = url.trim();
    if candidate.is_empty() {
        return None;
    }
    for prefix in ["https://github.com/", "http://github.com/"] {
        if let Some(rest) = candidate.strip_prefix(prefix) {
            return parse_repo_rest(rest, true);
        }
    }
    if let Some(rest) = candidate.strip_prefix("git@github.com:") {
        return parse_repo_rest(rest, false);
    }
    None
}

fn parse_repo_rest(rest: &str, allow_trailing_slash: bool) -> Option<(String, String)> {
    let mut parts: Vec<&str> = rest.split('/').collect();
    if allow_trailing_slash && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() != 2 {
        return None;
    }
    let (owner, mut name) = (parts[0], parts[1]);
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    if owner.chars().any(char::is_whitespace) || name.chars().any(char::is_whitespace) {
        return None;
    }
    if name.len() > 4 && name.ends_with(".git") {
        name = &name[..name.len() - 4];
    }
    Some((owner.to_owned(), name.to_owned()))
}

/// `int(verified.get("id") or 0)` (`github.py:1014`): falsy inputs
/// (missing, null, `0`, `""`, `false`, `[]`, `{}`) are `0` — which the
/// caller answers with the 502. Truthy numbers truncate toward zero
/// and truthy strings parse like Python's `int(s, 10)` (surrounding
/// whitespace, `+`/`-`, single underscores between digits); anything
/// else (`Err(())`) is the unhandled `ValueError`/`TypeError` the view
/// lets bubble to the 500.
fn python_repo_id(value: Option<&Value>) -> Result<i64, ()> {
    let value = match value {
        None => return Ok(0),
        Some(value) => value,
    };
    if !json_truthy(value) {
        return Ok(0);
    }
    match value {
        Value::Bool(true) => Ok(1),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int);
            }
            match number.as_f64() {
                Some(float) if float.is_finite() && float.abs() < i64::MAX as f64 => {
                    Ok(float.trunc() as i64)
                }
                _ => Err(()),
            }
        }
        Value::String(text) => python_int(text),
        _ => Err(()),
    }
}

/// `int(s, 10)` for the strings `python_repo_id` accepts.
fn python_int(text: &str) -> Result<i64, ()> {
    let trimmed = text.trim();
    let (sign, digits) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    if digits.is_empty() {
        return Err(());
    }
    let mut value: i64 = 0;
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return Err(());
            }
            prev_underscore = true;
            continue;
        }
        let digit = ch.to_digit(10).ok_or(())?;
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit as i64))
            .ok_or(())?;
        prev_underscore = false;
    }
    if prev_underscore {
        return Err(());
    }
    Ok(if sign { -value } else { value })
}

/// `gh_repo.get(key) or ""` (`_serialize_repo`, `github.py:270-280`):
/// falsy values render `""`, truthy non-strings pass through (e.g. a
/// numeric `name` renders as-is — no crash, unlike the owner branch).
fn or_empty(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(_)) => value.cloned().unwrap_or(Value::Null),
        Some(other) if json_truthy(other) => other.clone(),
        _ => Value::String(String::new()),
    }
}

/// `(gh_repo.get("owner") or {}).get("login") or ""`
/// (`github.py:271`): a missing/null owner renders `""`, a dict owner
/// renders its login (or `""`), and a truthy non-dict owner is the
/// `AttributeError` the response build lets bubble to the 500.
fn owner_login(verified: &serde_json::Map<String, Value>) -> Result<Value, ()> {
    match verified.get("owner") {
        None | Some(Value::Null) => Ok(Value::String(String::new())),
        Some(Value::Object(owner)) => Ok(or_empty(owner.get("login"))),
        Some(other) if !json_truthy(other) => Ok(Value::String(String::new())),
        _ => Err(()),
    }
}

/// `_serialize_repo` (`github.py:270-280`): fixed key order `id`,
/// `owner`, `name`, `full_name`, `default_branch`, `private`.
fn serialize_repo(verified: &serde_json::Map<String, Value>) -> Result<Value, ()> {
    let mut map = serde_json::Map::with_capacity(6);
    map.insert(
        "id".to_owned(),
        verified.get("id").cloned().unwrap_or(Value::Null),
    );
    map.insert("owner".to_owned(), owner_login(verified)?);
    map.insert("name".to_owned(), or_empty(verified.get("name")));
    map.insert("full_name".to_owned(), or_empty(verified.get("full_name")));
    map.insert(
        "default_branch".to_owned(),
        or_empty(verified.get("default_branch")),
    );
    map.insert(
        "private".to_owned(),
        Value::Bool(verified.get("private").is_some_and(json_truthy)),
    );
    Ok(Value::Object(map))
}

/// `last_synced_at.isoformat()` on the model instance
/// (`github.py:1127,1143`): UTC `+00:00` suffix (never `Z` — this is
/// `datetime.isoformat`, not DRF rendering), microseconds only when
/// nonzero. Postgres `timestamptz` has microsecond resolution, so the
/// fraction is exactly six digits.
fn isoformat(dt: &DateTime<Utc>) -> String {
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = dt.timestamp_subsec_nanos();
    if nanos == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{:06}+00:00", nanos / 1000)
    }
}

/// Run the gate for one route+method: fetch both membership rows and
/// decide through the kernel. The gate row must exist (all four do).
#[allow(clippy::result_large_err)]
async fn check_gate(
    pool: &sqlx::PgPool,
    actor_id: &Uuid,
    slug: &str,
    project_id: &Uuid,
    method: &str,
    path: &str,
) -> Result<(), Response> {
    let row = gate_for(method, path).expect("D-33 project-github gate");
    let roles = match row.gate {
        super::gates::Gate::Project { roles } => roles,
        _ => return Err(Denial::ServerError.into_response()),
    };
    let ws_role = match workspace_role(pool, actor_id, slug).await {
        Ok(role) => role,
        Err(denial) => return Err(denial.into_response()),
    };
    let pm_role = match project_role(pool, actor_id, project_id, slug).await {
        Ok(role) => role,
        Err(denial) => return Err(denial.into_response()),
    };
    enforce(decide_gate(
        &row.gate,
        &tenant_context(slug),
        &project_facts(slug, roles, ws_role, pm_role),
    ))
}

/// The disabled-flag 404 when `GITHUB_SYNC_ENABLED` is off
/// (`github.py:82-87`), checked after a passing gate.
#[allow(clippy::result_large_err)]
fn check_feature(state: &AppState) -> Result<(), Response> {
    if state.settings().github_sync_enabled {
        Ok(())
    } else {
        Err(raw_body(StatusCode::NOT_FOUND, GITHUB_DISABLED_BODY))
    }
}

/// Parse a POST/PATCH JSON-or-form body through the shared DRF edge.
/// Unlike the external assistants (where `null` degrades to `{}`), a
/// `null`/scalar body here reaches `request.data.get` on `None`/a scalar
/// and raises — the 500 the view lets bubble (probed live).
#[allow(clippy::result_large_err)]
fn parse_object(
    body: &[u8],
    content_type: Option<&str>,
) -> Result<Vec<(String, BodyField)>, Response> {
    match parse_body(body, content_type).map_err(|failure| failure.into_response())? {
        ParsedBody::Object(fields) => Ok(fields),
        ParsedBody::Scalar(_) | ParsedBody::Null => Err(Denial::ServerError.into_response()),
    }
}

// ---------------------------------------------------------------------------
// status reads (github.py:1097-1146)
// ---------------------------------------------------------------------------

/// One legacy-sync status row: `(sync id, enabled, synced_at, error,
/// repo id, owner, name, url)`.
type LegacyRow = (
    Uuid,
    bool,
    Option<DateTime<Utc>>,
    String,
    i64,
    String,
    String,
    Option<String>,
);

/// One legacy sync with its repository for the `bound: True` branch.
struct LegacyStatus {
    id: Uuid,
    is_sync_enabled: bool,
    last_synced_at: Option<DateTime<Utc>>,
    last_sync_error: String,
    repository_id: i64,
    owner: String,
    name: String,
    url: Option<String>,
}

/// One fallback status row: `(binding id, enabled, synced_at, error,
/// external id, namespace, name, web url)`.
type FallbackRow = (
    Uuid,
    bool,
    Option<DateTime<Utc>>,
    String,
    String,
    String,
    String,
    String,
);

/// One github-provider `GitRepositoryBinding` with its repository for
/// the fallback branch.
struct FallbackStatus {
    id: Uuid,
    is_sync_enabled: bool,
    last_synced_at: Option<DateTime<Utc>>,
    last_sync_error: String,
    external_id: String,
    namespace: String,
    name: String,
    web_url: String,
}

/// `GithubRepositorySync.objects.filter(project_id, workspace__slug)
/// .select_related("repository").first()` (`github.py:1097-1102`). The
/// joined rows carry no deleted filter (probed live); ordering is the
/// model default (`-created_at`).
async fn fetch_legacy_status(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    slug: &str,
) -> Result<Option<LegacyStatus>, Denial> {
    let row: Option<LegacyRow> = sqlx::query_as(
        r#"SELECT s.id, s.is_sync_enabled, s.last_synced_at, s.last_sync_error,
                      r.repository_id, r.owner, r.name, r.url
               FROM github_repository_syncs s
               JOIN workspaces w ON w.id = s.workspace_id
               JOIN github_repositories r ON r.id = s.repository_id
               WHERE s.deleted_at IS NULL AND s.project_id = $1 AND w.slug = $2
               ORDER BY s.created_at DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(
        |(
            id,
            is_sync_enabled,
            last_synced_at,
            last_sync_error,
            repository_id,
            owner,
            name,
            url,
        )| {
            LegacyStatus {
                id,
                is_sync_enabled,
                last_synced_at,
                last_sync_error,
                repository_id,
                owner,
                name,
                url,
            }
        },
    ))
}

/// `GitRepositoryBinding.objects.filter(project_id, workspace__slug,
/// repository__provider="github").select_related("repository").first()`
/// (`github.py:1104-1112`), shared by the GET fallback and the
/// PATCH/DELETE no-sync branches. Same join-filter rule as above.
async fn fetch_fallback_status(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    slug: &str,
) -> Result<Option<FallbackStatus>, Denial> {
    let row: Option<FallbackRow> = sqlx::query_as(
        r#"SELECT b.id, b.is_sync_enabled, b.last_synced_at, b.last_sync_error,
                  r.external_id, r.namespace, r.name, r.web_url
           FROM git_repository_bindings b
           JOIN workspaces w ON w.id = b.workspace_id
           JOIN git_repositories r ON r.id = b.repository_id
           WHERE b.deleted_at IS NULL AND b.project_id = $1 AND w.slug = $2
           AND r.provider = 'github'
           ORDER BY b.created_at DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(
        |(
            id,
            is_sync_enabled,
            last_synced_at,
            last_sync_error,
            external_id,
            namespace,
            name,
            web_url,
        )| {
            FallbackStatus {
                id,
                is_sync_enabled,
                last_synced_at,
                last_sync_error,
                external_id,
                namespace,
                name,
                web_url,
            }
        },
    ))
}

fn synced_at_text(last_synced_at: &Option<DateTime<Utc>>) -> Value {
    last_synced_at
        .as_ref()
        .map(isoformat)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

/// Legacy `bound: True` shape (`github.py:1131-1146`): `repository.id`
/// is the integer `repository_id`.
fn legacy_body(status: &LegacyStatus) -> Value {
    let mut repository = serde_json::Map::with_capacity(4);
    repository.insert("id".to_owned(), Value::from(status.repository_id));
    repository.insert("owner".to_owned(), Value::String(status.owner.clone()));
    repository.insert("name".to_owned(), Value::String(status.name.clone()));
    repository.insert(
        "url".to_owned(),
        status.url.clone().map(Value::String).unwrap_or(Value::Null),
    );
    let mut map = serde_json::Map::with_capacity(6);
    map.insert("bound".to_owned(), Value::Bool(true));
    map.insert("id".to_owned(), Value::String(status.id.to_string()));
    map.insert("repository".to_owned(), Value::Object(repository));
    map.insert(
        "is_sync_enabled".to_owned(),
        Value::Bool(status.is_sync_enabled),
    );
    map.insert(
        "last_synced_at".to_owned(),
        synced_at_text(&status.last_synced_at),
    );
    map.insert(
        "last_sync_error".to_owned(),
        Value::String(status.last_sync_error.clone()),
    );
    Value::Object(map)
}

/// Fallback `bound: True` shape (`github.py:1115-1130`):
/// `repository.id` is the string `external_id`.
fn fallback_body(status: &FallbackStatus) -> Value {
    let mut repository = serde_json::Map::with_capacity(4);
    repository.insert("id".to_owned(), Value::String(status.external_id.clone()));
    repository.insert("owner".to_owned(), Value::String(status.namespace.clone()));
    repository.insert("name".to_owned(), Value::String(status.name.clone()));
    repository.insert("url".to_owned(), Value::String(status.web_url.clone()));
    let mut map = serde_json::Map::with_capacity(6);
    map.insert("bound".to_owned(), Value::Bool(true));
    map.insert("id".to_owned(), Value::String(status.id.to_string()));
    map.insert("repository".to_owned(), Value::Object(repository));
    map.insert(
        "is_sync_enabled".to_owned(),
        Value::Bool(status.is_sync_enabled),
    );
    map.insert(
        "last_synced_at".to_owned(),
        synced_at_text(&status.last_synced_at),
    );
    map.insert(
        "last_sync_error".to_owned(),
        Value::String(status.last_sync_error.clone()),
    );
    Value::Object(map)
}

/// `GET workspaces/<slug>/projects/<id>/github/`
/// (`GithubProjectStatusEndpoint.get`, `github.py:1093-1146`).
async fn get_status(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path((slug, project_id_raw)): Path<(String, String)>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_id_raw).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    if let Err(response) = check_gate(
        &pool,
        &actor.id,
        &slug,
        &project_id,
        "GET",
        "workspaces/<slug>/projects/<id>/github/",
    )
    .await
    {
        return response;
    }
    if let Err(response) = check_feature(&state) {
        return response;
    }
    let legacy = match fetch_legacy_status(&pool, &project_id, &slug).await {
        Ok(status) => status,
        Err(denial) => return denial.into_response(),
    };
    if let Some(status) = legacy {
        return json_response(&legacy_body(&status));
    }
    let fallback = match fetch_fallback_status(&pool, &project_id, &slug).await {
        Ok(status) => status,
        Err(denial) => return denial.into_response(),
    };
    match fallback {
        Some(status) => json_response(&fallback_body(&status)),
        None => raw_body(StatusCode::OK, UNBOUND_BODY),
    }
}

/// `enabled` extraction (`github.py:1153-1154`): only a JSON bool
/// passes; anything else (missing, null, number, string, file) answers
/// the 400.
#[allow(clippy::result_large_err)]
fn enabled_flag(fields: &Vec<(String, BodyField)>) -> Result<bool, Response> {
    match body_get(fields, "enabled") {
        Some(BodyField::Json(Value::Bool(enabled))) => Ok(*enabled),
        _ => Err(raw_body(StatusCode::BAD_REQUEST, ENABLED_REQUIRED_BODY)),
    }
}

/// `PATCH workspaces/<slug>/projects/<id>/github/`
/// (`GithubProjectStatusEndpoint.patch`, `github.py:1148-1172`): with a
/// legacy sync, save `is_sync_enabled` on it (audit columns untouched,
/// probed) and mirror to the github-provider bindings; without one,
/// update the bindings directly. Both `UPDATE`s touch only the named
/// column. No sync and no updated binding row is the 404.
async fn patch_status(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path((slug, project_id_raw)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_id_raw).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    if let Err(response) = check_gate(
        &pool,
        &actor.id,
        &slug,
        &project_id,
        "PATCH",
        "workspaces/<slug>/projects/<id>/github/",
    )
    .await
    {
        return response;
    }
    if let Err(response) = check_feature(&state) {
        return response;
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let fields = match parse_object(&body, content_type) {
        Ok(fields) => fields,
        Err(response) => return response,
    };
    let enabled = match enabled_flag(&fields) {
        Ok(enabled) => enabled,
        Err(response) => return response,
    };
    let sync_id: Option<(Uuid,)> = match sqlx::query_as(
        r#"SELECT s.id FROM github_repository_syncs s
           JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.deleted_at IS NULL AND s.project_id = $1 AND w.slug = $2
           ORDER BY s.created_at DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if let Some((id,)) = sync_id {
        if sqlx::query("UPDATE github_repository_syncs SET is_sync_enabled = $1 WHERE id = $2")
            .bind(enabled)
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
        {
            return Denial::ServerError.into_response();
        }
    }
    let mirrored = match sqlx::query(
        r#"UPDATE git_repository_bindings b SET is_sync_enabled = $3
           FROM workspaces w, git_repositories r
           WHERE b.workspace_id = w.id AND b.repository_id = r.id
           AND b.deleted_at IS NULL AND b.project_id = $1 AND w.slug = $2
           AND r.provider = 'github'"#,
    )
    .bind(project_id)
    .bind(&slug)
    .bind(enabled)
    .execute(&pool)
    .await
    {
        Ok(result) => result.rows_affected(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    if sync_id.is_none() && mirrored == 0 {
        return raw_body(StatusCode::NOT_FOUND, NO_BINDING_BODY);
    }
    json_response(&serde_json::json!({"is_sync_enabled": enabled}))
}

// ---------------------------------------------------------------------------
// unbind (github.py:1174-1205)
// ---------------------------------------------------------------------------

/// `soft_delete_related_objects.delay("db", <model>, <pk>, "default")`
/// (`db/mixins.py:72-78`): positional args, no kwargs. Published through
/// the Postgres queue for the worker to forward to the broker
/// (Python-owned task), best-effort after commit like the space intake
/// precedent — without it the response still stands.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &'static str, id: &Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(id.to_string()),
            Value::String("default".to_owned()),
        ],
        Default::default(),
    );
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// The binding-side unbind shared by both DELETE branches
/// (`github.py:1184-1188,1199-1203`): per github-provider binding, soft
/// delete its comment syncs, then its issue syncs (queryset deletes —
/// `deleted_at` only), then soft delete the binding itself (instance
/// delete — `deleted_at`, `updated_at`, `updated_by`, plus the cascade
/// task). Returns the soft-deleted binding ids for task enqueue.
async fn unbind_git_bindings(
    pool: &sqlx::PgPool,
    actor_id: &Uuid,
    project_id: &Uuid,
    slug: &str,
) -> Result<Vec<Uuid>, Denial> {
    let ids: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT b.id FROM git_repository_bindings b
           JOIN workspaces w ON w.id = b.workspace_id
           JOIN git_repositories r ON r.id = b.repository_id
           WHERE b.deleted_at IS NULL AND b.project_id = $1 AND w.slug = $2
           AND r.provider = 'github'
           ORDER BY b.created_at DESC"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    for (id,) in &ids {
        sqlx::query(
            r#"UPDATE git_comment_syncs SET deleted_at = now()
               WHERE deleted_at IS NULL AND issue_sync_id IN (
                   SELECT id FROM git_issue_syncs WHERE binding_id = $1 AND deleted_at IS NULL
               )"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        sqlx::query(
            r#"UPDATE git_issue_syncs SET deleted_at = now()
               WHERE binding_id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        sqlx::query(
            r#"UPDATE git_repository_bindings
               SET deleted_at = now(), updated_at = now(), updated_by_id = $1
               WHERE id = $2"#,
        )
        .bind(actor_id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    Ok(ids.into_iter().map(|(id,)| id).collect())
}

/// `DELETE workspaces/<slug>/projects/<id>/github/`
/// (`GithubProjectStatusEndpoint.delete`, `github.py:1174-1205`): without
/// a legacy sync, unbind the github-provider bindings; with one, do it
/// atomically after synchronously soft-deleting the dependent sync rows
/// (queryset deletes) and the sync itself (instance delete + task), so
/// the §6.8 lock predicate releases with the unbind.
async fn delete_status(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path((slug, project_id_raw)): Path<(String, String)>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_id_raw).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    if let Err(response) = check_gate(
        &pool,
        &actor.id,
        &slug,
        &project_id,
        "DELETE",
        "workspaces/<slug>/projects/<id>/github/",
    )
    .await
    {
        return response;
    }
    if let Err(response) = check_feature(&state) {
        return response;
    }
    let sync_id: Option<(Uuid,)> = match sqlx::query_as(
        r#"SELECT s.id FROM github_repository_syncs s
           JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.deleted_at IS NULL AND s.project_id = $1 AND w.slug = $2
           ORDER BY s.created_at DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut tasks: Vec<(&'static str, Uuid)> = Vec::new();
    if let Some((id,)) = sync_id {
        // `transaction.atomic()` (`github.py:1190`): the three writes
        // below commit or roll back together.
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let failed = sqlx::query(
            r#"UPDATE github_comment_syncs SET deleted_at = now()
               WHERE deleted_at IS NULL AND issue_sync_id IN (
                   SELECT id FROM github_issue_syncs
                   WHERE repository_sync_id = $1 AND deleted_at IS NULL
               )"#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .is_err()
            || sqlx::query(
                r#"UPDATE github_issue_syncs SET deleted_at = now()
                   WHERE repository_sync_id = $1 AND deleted_at IS NULL"#,
            )
            .bind(id)
            .execute(&mut *tx)
            .await
            .is_err()
            || sqlx::query(
                r#"UPDATE github_repository_syncs
                   SET deleted_at = now(), updated_at = now(), updated_by_id = $1
                   WHERE id = $2"#,
            )
            .bind(actor.id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .is_err()
            || tx.commit().await.is_err();
        if failed {
            return Denial::ServerError.into_response();
        }
        tasks.push(("githubrepositorysync", id));
    }
    match unbind_git_bindings(&pool, &actor.id, &project_id, &slug).await {
        Ok(ids) => {
            for id in &ids {
                tasks.push(("gitrepositorybinding", *id));
            }
        }
        Err(denial) => return denial.into_response(),
    }
    for (model, id) in &tasks {
        enqueue_soft_delete(&pool, model, id).await;
    }
    raw_body(StatusCode::OK, UNBOUND_BODY)
}

// ---------------------------------------------------------------------------
// bind (github.py:962-1082)
// ---------------------------------------------------------------------------

/// `(request.data.get("repo_url") or "").strip()` (`github.py:986`):
/// missing and falsy values fall through to the blank 400, a string is
/// stripped (blank after strip is the same 400), and anything else
/// truthy (number, `true`, non-empty list/dict, file part) is the
/// `AttributeError` the view lets bubble to the 500 (probed live).
#[allow(clippy::result_large_err)]
fn repo_url_field(fields: &Vec<(String, BodyField)>) -> Result<String, Response> {
    match body_get(fields, "repo_url") {
        None => Err(raw_body(StatusCode::BAD_REQUEST, REPO_REQUIRED_BODY)),
        Some(BodyField::File { .. }) => Err(Denial::ServerError.into_response()),
        Some(BodyField::Json(Value::String(text))) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Err(raw_body(StatusCode::BAD_REQUEST, REPO_REQUIRED_BODY))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        Some(BodyField::Json(other)) if !json_truthy(other) => {
            Err(raw_body(StatusCode::BAD_REQUEST, REPO_REQUIRED_BODY))
        }
        _ => Err(Denial::ServerError.into_response()),
    }
}

/// `GithubClient(token).get_repo(owner, name)` (`github.py:1003-1012`):
/// an empty token raises `GithubAuthError` in `__init__` (inside the
/// `try`, so it is the 401); 401 → 401, 404 → 404, 403 (a plain
/// `GithubPermissionError`, not caught) and every other failure
/// (transport, non-2xx, bad JSON) → the logged 502.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn verify_repo(
    token: &str,
    owner: &str,
    name: &str,
) -> Result<serde_json::Map<String, Value>, Response> {
    if token.is_empty() {
        return Err(raw_body(StatusCode::UNAUTHORIZED, TOKEN_REJECTED_BODY));
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(GITHUB_TIMEOUT_SECS))
        .build()
    {
        Ok(client) => client,
        Err(_) => return Err(raw_body(StatusCode::BAD_GATEWAY, FAILED_VERIFY_BODY)),
    };
    let response = match client
        .get(format!("{GITHUB_API_BASE}/repos/{owner}/{name}"))
        .header(header::AUTHORIZATION.as_str(), format!("Bearer {token}"))
        .header(header::ACCEPT.as_str(), "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header(header::USER_AGENT.as_str(), "pi-dash-github-sync")
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Err(raw_body(StatusCode::BAD_GATEWAY, FAILED_VERIFY_BODY)),
    };
    match response.status().as_u16() {
        401 => return Err(raw_body(StatusCode::UNAUTHORIZED, TOKEN_REJECTED_BODY)),
        404 => return Err(raw_body(StatusCode::NOT_FOUND, REPO_NOT_FOUND_BODY)),
        status if !(200..300).contains(&status) => {
            return Err(raw_body(StatusCode::BAD_GATEWAY, FAILED_VERIFY_BODY));
        }
        _ => {}
    }
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return Err(raw_body(StatusCode::BAD_GATEWAY, FAILED_VERIFY_BODY)),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(raw_body(StatusCode::BAD_GATEWAY, FAILED_VERIFY_BODY)),
    }
}

/// Failure of the bind transaction: a unique-violation anywhere inside
/// the atomic block is the 409 (`github.py:1066-1070`); anything else
/// (including a `get_or_create` multi-hit, Django's
/// `MultipleObjectsReturned`) is the 500.
enum TxnError {
    Conflict,
    Server,
}

impl TxnError {
    fn into_response(self) -> Response {
        match self {
            TxnError::Conflict => raw_body(StatusCode::CONFLICT, BIND_CONFLICT_BODY),
            TxnError::Server => Denial::ServerError.into_response(),
        }
    }
}

fn txn_error(error: sqlx::Error) -> TxnError {
    if let Some(db_error) = error.as_database_error() {
        if db_error.code().as_deref() == Some("23505") {
            return TxnError::Conflict;
        }
    }
    TxnError::Server
}

/// `GithubRepository.objects.get_or_create(project, repository_id,
/// defaults=...)` (`github.py:1034-1044`): a hit reuses the row with no
/// write; a miss inserts (audit `created_by` set, the
/// `get_or_create` full-save shape).
#[allow(clippy::too_many_arguments)]
async fn get_or_create_repo(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    repository_id: i64,
    owner: &str,
    name: &str,
    canonical_url: &str,
) -> Result<Uuid, TxnError> {
    let hits: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM github_repositories
           WHERE project_id = $1 AND repository_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(repository_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(txn_error)?;
    match hits.as_slice() {
        [(id,)] => Ok(*id),
        [] => {
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO github_repositories
                   (id, created_at, updated_at, created_by_id, project_id, workspace_id,
                    name, url, config, repository_id, owner)
                   VALUES ($1, now(), now(), $2, $3, $4, $5, $6, '{}', $7, $8)"#,
            )
            .bind(id)
            .bind(actor_id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(name)
            .bind(canonical_url)
            .bind(repository_id)
            .bind(owner)
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
            Ok(id)
        }
        _ => Err(TxnError::Server),
    }
}

/// `Label.objects.get_or_create(project, name="github",
/// defaults={workspace, color})` (`github.py:1045-1049`): a hit reuses
/// the row; a miss inserts with `Label.save()`'s `sort_order`
/// (`MAX(sort_order)` for the project + 10000, else the 65535 default —
/// probed live) and the blank defaults (`description`/`external_id`/
/// `external_source` all `""`, `parent` null, probed live).
async fn get_or_create_label(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<Uuid, TxnError> {
    let hits: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM labels
           WHERE project_id = $1 AND name = 'github' AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(txn_error)?;
    match hits.as_slice() {
        [(id,)] => Ok(*id),
        [] => {
            let max: Option<(Option<f64>,)> = sqlx::query_as(
                r#"SELECT MAX(sort_order) FROM labels
                   WHERE project_id = $1 AND deleted_at IS NULL"#,
            )
            .bind(project_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(txn_error)?;
            let sort_order = max
                .and_then(|(value,)| value)
                .map(|value| value + 10000.0)
                .unwrap_or(65535.0);
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO labels
                   (id, created_at, updated_at, created_by_id, project_id, workspace_id,
                    name, description, color, sort_order, external_id, external_source)
                   VALUES ($1, now(), now(), $2, $3, $4, 'github', '', '#1f2328', $5, '', '')"#,
            )
            .bind(id)
            .bind(actor_id)
            .bind(project_id)
            .bind(workspace_id)
            .bind(sort_order)
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
            Ok(id)
        }
        _ => Err(TxnError::Server),
    }
}

/// `_upsert_git_binding_for_github_sync` (`github.py:355-420`): the first
/// PAT-capable account for the workspace-integration (`ORDER BY
/// auth_type`, else a legacy `DEGRADED` PAT row), the `GitRepository`
/// `update_or_create` (a hit is a full save: defaults plus `updated_at`
/// / `updated_by`), hard delete of the existing project binding, then the
/// fresh binding mirroring the sync (`RUNNER_MANAGED` for private repos,
/// else `PUBLIC`).
#[allow(clippy::too_many_arguments)]
async fn upsert_git_binding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    workspace_integration_id: &Uuid,
    raw_token: &str,
    sync_id: &Uuid,
    verified: &serde_json::Map<String, Value>,
    owner: &str,
    name: &str,
) -> Result<(), TxnError> {
    let account: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM git_provider_accounts
           WHERE workspace_id = $1 AND provider = 'github' AND host_url = $2
           AND workspace_integration_id = $3 AND deleted_at IS NULL
           ORDER BY auth_type ASC LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(GITHUB_HOST_URL)
    .bind(workspace_integration_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(txn_error)?;
    let account_id = match account {
        Some((id,)) => id,
        None => {
            let id = Uuid::new_v4();
            let credential = serde_json::json!({
                "auth_type": "pat",
                "host_url": GITHUB_HOST_URL,
                "token": raw_token,
            });
            sqlx::query(
                r#"INSERT INTO git_provider_accounts
                   (id, created_at, updated_at, created_by_id, workspace_id,
                    provider, host_url, auth_type, external_account_id,
                    external_account_login, display_name, capabilities,
                    credential_config, workspace_integration_id,
                    status, last_check_error, metadata)
                   VALUES ($1, now(), now(), $2, $3, 'github', $4, 'pat', $5,
                           '', 'Legacy GitHub integration', '{}', $6, $7, 'degraded',
                           'Backfilled while binding legacy GitHub repository', '{}')"#,
            )
            .bind(id)
            .bind(actor_id)
            .bind(workspace_id)
            .bind(GITHUB_HOST_URL)
            .bind(format!("legacy:{workspace_integration_id}"))
            .bind(credential)
            .bind(workspace_integration_id)
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
            id
        }
    };
    let external_id = verified
        .get("id")
        .map(|id| {
            if let Some(text) = id.as_str() {
                text.to_owned()
            } else {
                id.to_string()
            }
        })
        .unwrap_or_default();
    let lower_owner = owner.to_lowercase();
    let lower_name = name.to_lowercase();
    let full_name = format!("{lower_owner}/{lower_name}");
    let (web_url, clone_http, clone_ssh, default_branch) = (
        verified
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or(""),
        verified
            .get("clone_url")
            .and_then(Value::as_str)
            .unwrap_or(""),
        verified
            .get("ssh_url")
            .and_then(Value::as_str)
            .unwrap_or(""),
        verified
            .get("default_branch")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let is_private = verified.get("private").is_some_and(json_truthy);
    let repo_hits: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM git_repositories
           WHERE provider = 'github' AND host_url = $1 AND external_id = $2
           AND deleted_at IS NULL"#,
    )
    .bind(GITHUB_HOST_URL)
    .bind(&external_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(txn_error)?;
    let repo_id = match repo_hits.as_slice() {
        [(id,)] => {
            sqlx::query(
                r#"UPDATE git_repositories
                   SET namespace = $1, name = $2, full_name = $3, web_url = $4,
                       clone_url_http = $5, clone_url_ssh = $6, default_branch = $7,
                       is_private = $8, metadata = $9,
                       updated_at = now(), updated_by_id = $10
                   WHERE id = $11"#,
            )
            .bind(&lower_owner)
            .bind(&lower_name)
            .bind(&full_name)
            .bind(web_url)
            .bind(clone_http)
            .bind(clone_ssh)
            .bind(default_branch)
            .bind(is_private)
            .bind(Value::Object(verified.clone()))
            .bind(actor_id)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
            *id
        }
        [] => {
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO git_repositories
                   (id, created_at, updated_at, created_by_id,
                    provider, host_url, external_id, namespace, name, full_name,
                    web_url, clone_url_http, clone_url_ssh, default_branch,
                    is_private, metadata)
                   VALUES ($1, now(), now(), $2,
                           'github', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"#,
            )
            .bind(id)
            .bind(actor_id)
            .bind(GITHUB_HOST_URL)
            .bind(&external_id)
            .bind(&lower_owner)
            .bind(&lower_name)
            .bind(&full_name)
            .bind(web_url)
            .bind(clone_http)
            .bind(clone_ssh)
            .bind(default_branch)
            .bind(is_private)
            .bind(Value::Object(verified.clone()))
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
            id
        }
        _ => return Err(TxnError::Server),
    };
    let existing: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM git_repository_bindings
           WHERE project_id = $1 AND deleted_at IS NULL
           ORDER BY created_at DESC LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(txn_error)?;
    if let Some((id,)) = existing {
        sqlx::query("DELETE FROM git_repository_bindings WHERE id = $1")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
    }
    let metadata = serde_json::json!({"legacy_github_repository_sync_id": sync_id.to_string()});
    let clone_auth_mode = if is_private {
        "runner_managed"
    } else {
        "public"
    };
    sqlx::query(
        r#"INSERT INTO git_repository_bindings
           (id, created_at, updated_at, created_by_id, project_id, workspace_id,
            repository_id, provider_account_id, actor_id, is_sync_enabled,
            clone_auth_mode, last_sync_error, metadata)
           VALUES ($1, now(), now(), $2, $3, $4, $5, $6, $7, false, $8, '', $9)"#,
    )
    .bind(Uuid::new_v4())
    .bind(actor_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(repo_id)
    .bind(account_id)
    .bind(actor_id)
    .bind(clone_auth_mode)
    .bind(metadata)
    .execute(&mut **tx)
    .await
    .map_err(txn_error)?;
    Ok(())
}

/// `POST workspaces/<slug>/projects/<id>/github/bind/`
/// (`GithubProjectBindEndpoint.post`, `github.py:981-1082`): verify the
/// URL upstream, then — inside one transaction — rebind (hard delete the
/// existing sync so the conditional unique on `project` never trips),
/// get-or-create the repository and the `github` label, create the sync
/// (always `is_sync_enabled=False`, even on rebind), upsert the
/// `GitRepositoryBinding` mirror, and persist the canonical URL on the
/// project when it differs (Bind doubles as the General-Settings save).
async fn post_bind(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path((slug, project_id_raw)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_id_raw).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    if let Err(response) = check_gate(
        &pool,
        &actor.id,
        &slug,
        &project_id,
        "POST",
        "workspaces/<slug>/projects/<id>/github/bind/",
    )
    .await
    {
        return response;
    }
    if let Err(response) = check_feature(&state) {
        return response;
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let fields = match parse_object(&body, content_type) {
        Ok(fields) => fields,
        Err(response) => return response,
    };
    let repo_url = match repo_url_field(&fields) {
        Ok(repo_url) => repo_url,
        Err(response) => return response,
    };
    let (owner, name) = match parse_github_repo_url(&repo_url) {
        Some(parsed) => parsed,
        None => return raw_body(StatusCode::BAD_REQUEST, BAD_URL_BODY),
    };
    let workspace_id: Uuid = match sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL",
    )
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some((id,))) => id,
        Ok(None) => return raw_body(StatusCode::NOT_FOUND, NO_WORKSPACE_BODY),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let project: Option<(Uuid, String)> = match sqlx::query_as(
        "SELECT id, repo_url FROM projects WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(workspace_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let (_, current_repo_url) = match project {
        Some(project) => project,
        None => return raw_body(StatusCode::NOT_FOUND, NO_PROJECT_BODY),
    };
    let integration_id = match fetch_integration_id_by_provider(&pool).await {
        Ok(id) => id,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let workspace_integration = match integration_id {
        Some(id) => match fetch_workspace_integration(&pool, workspace_id, id).await {
            Ok(wi) => wi,
            Err(_) => return Denial::ServerError.into_response(),
        },
        None => None,
    };
    // `wi is None or not (wi.config or {}).get("token")`
    // (`github.py:999-1001`): the emptiness check is Python truthiness
    // on the raw value (a non-string token proceeds and fails closed in
    // `decrypt_data`), not the string-only helper.
    let Some(wi) =
        workspace_integration.filter(|wi| wi.config.get("token").is_some_and(json_truthy))
    else {
        return raw_body(StatusCode::CONFLICT, NOT_CONNECTED_BODY);
    };
    let workspace_integration_id = wi.id;
    let raw_token = wi.config.get("token").and_then(Value::as_str).unwrap_or("");
    let keyring = pidash_db::config::encryption::Keyring::from_secret(&state.settings().secret_key);
    let token = keyring.decrypt(raw_token);
    let verified = match verify_repo(&token, &owner, &name).await {
        Ok(verified) => verified,
        Err(response) => return response,
    };
    let repository_id = match python_repo_id(verified.get("id")) {
        Ok(0) => return raw_body(StatusCode::BAD_GATEWAY, NO_REPO_ID_BODY),
        Ok(id) => id,
        Err(()) => return Denial::ServerError.into_response(),
    };
    let canonical: Value = match verified.get("html_url") {
        Some(url) if json_truthy(url) => url.clone(),
        _ => Value::String(format!("https://github.com/{owner}/{name}")),
    };
    let canonical_text = canonical
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| canonical.to_string());
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let outcome = bind_transaction(
        &mut tx,
        &actor.id,
        &project_id,
        &workspace_id,
        &workspace_integration_id,
        raw_token,
        &current_repo_url,
        &canonical_text,
        repository_id,
        &owner,
        &name,
        &verified,
    )
    .await;
    let sync_id = match outcome {
        Ok(sync_id) => sync_id,
        Err(error) => return error.into_response(),
    };
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    let repository = match serialize_repo(&verified) {
        Ok(repository) => repository,
        Err(()) => return Denial::ServerError.into_response(),
    };
    let mut map = serde_json::Map::with_capacity(6);
    map.insert("id".to_owned(), Value::String(sync_id.to_string()));
    map.insert("repository".to_owned(), repository);
    map.insert("is_sync_enabled".to_owned(), Value::Bool(false));
    map.insert("last_synced_at".to_owned(), Value::Null);
    map.insert("last_sync_error".to_owned(), Value::String(String::new()));
    map.insert("repo_url".to_owned(), canonical);
    created_json(&Value::Object(map))
}

/// The bind writes inside `transaction.atomic()` (`github.py:1026-1065`),
/// returning the new sync id. Any unique violation is the 409.
#[allow(clippy::too_many_arguments)]
async fn bind_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    workspace_integration_id: &Uuid,
    raw_token: &str,
    current_repo_url: &str,
    canonical_url: &str,
    repository_id: i64,
    owner: &str,
    name: &str,
    verified: &serde_json::Map<String, Value>,
) -> Result<Uuid, TxnError> {
    let existing: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM github_repository_syncs
           WHERE project_id = $1 AND deleted_at IS NULL
           ORDER BY created_at DESC LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(txn_error)?;
    if let Some((id,)) = existing {
        sqlx::query("DELETE FROM github_repository_syncs WHERE id = $1")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
    }
    let repo_id = get_or_create_repo(
        tx,
        actor_id,
        project_id,
        workspace_id,
        repository_id,
        owner,
        name,
        canonical_url,
    )
    .await?;
    let label_id = get_or_create_label(tx, actor_id, project_id, workspace_id).await?;
    let sync_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO github_repository_syncs
           (id, created_at, updated_at, created_by_id, project_id, workspace_id,
            repository_id, credentials, actor_id, workspace_integration_id,
            label_id, is_sync_enabled, last_sync_error)
           VALUES ($1, now(), now(), $2, $3, $4, $5, '{}', $6, $7, $8, false, '')"#,
    )
    .bind(sync_id)
    .bind(actor_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(repo_id)
    .bind(actor_id)
    .bind(workspace_integration_id)
    .bind(label_id)
    .execute(&mut **tx)
    .await
    .map_err(txn_error)?;
    upsert_git_binding(
        tx,
        actor_id,
        project_id,
        workspace_id,
        workspace_integration_id,
        raw_token,
        &sync_id,
        verified,
        owner,
        name,
    )
    .await?;
    if current_repo_url != canonical_url {
        sqlx::query("UPDATE projects SET repo_url = $1 WHERE id = $2")
            .bind(canonical_url)
            .bind(project_id)
            .execute(&mut **tx)
            .await
            .map_err(txn_error)?;
    }
    Ok(sync_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};

    fn decide(method: &str, ws_role: Option<i32>, pm_role: Option<i32>) -> GateOutcome {
        let path = if method == "POST" {
            "workspaces/<slug>/projects/<id>/github/bind/"
        } else {
            "workspaces/<slug>/projects/<id>/github/"
        };
        let row = gate_for(method, path).expect("gate");
        let roles = match row.gate {
            super::super::gates::Gate::Project { roles } => roles,
            _ => panic!("project gate"),
        };
        let scope = tenant_context("acme");
        decide_gate(
            &row.gate,
            &scope,
            &project_facts("acme", roles, ws_role, pm_role),
        )
    }

    #[test]
    fn owned_paths_match_django_urls() {
        // `app/urls/integration.py`, under the `api/` include.
        assert_eq!(
            STATUS_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/github/"
        );
        assert_eq!(
            BIND_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/github/bind/"
        );
    }

    #[test]
    fn gates_cover_all_four_routes() {
        // FX-PERM-01 rows: GET is ADMIN/MEMBER/GUEST, the three writes
        // are ADMIN-only, all at PROJECT level.
        assert!(gate_for("GET", "workspaces/<slug>/projects/<id>/github/").is_some());
        assert!(gate_for("PATCH", "workspaces/<slug>/projects/<id>/github/").is_some());
        assert!(gate_for("DELETE", "workspaces/<slug>/projects/<id>/github/").is_some());
        assert!(gate_for("POST", "workspaces/<slug>/projects/<id>/github/bind/").is_some());
    }

    #[test]
    fn guest_reads_but_cannot_write() {
        assert_eq!(
            decide("GET", Some(ROLE_GUEST), Some(ROLE_GUEST)),
            GateOutcome::Allow
        );
        for method in ["POST", "PATCH", "DELETE"] {
            assert_eq!(
                decide(method, Some(ROLE_GUEST), Some(ROLE_GUEST)),
                GateOutcome::Deny,
                "{method}"
            );
        }
    }

    #[test]
    fn member_reads_but_cannot_write() {
        assert_eq!(
            decide("GET", Some(ROLE_MEMBER), Some(ROLE_MEMBER)),
            GateOutcome::Allow
        );
        for method in ["POST", "PATCH", "DELETE"] {
            assert_eq!(
                decide(method, Some(ROLE_MEMBER), Some(ROLE_MEMBER)),
                GateOutcome::Deny,
                "{method}"
            );
        }
    }

    #[test]
    fn admin_can_do_everything() {
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            assert_eq!(
                decide(method, Some(ROLE_ADMIN), Some(ROLE_ADMIN)),
                GateOutcome::Allow,
                "{method}"
            );
        }
    }

    #[test]
    fn workspace_admin_override_covers_guest_project_members() {
        // `permissions/base.py:56-64`: any project membership plus a
        // workspace ADMIN membership passes regardless of project role.
        assert_eq!(
            decide("POST", Some(ROLE_ADMIN), Some(ROLE_GUEST)),
            GateOutcome::Allow
        );
        // Without a project row the override does not fire.
        assert_eq!(decide("POST", Some(ROLE_ADMIN), None), GateOutcome::Deny);
        // Outsiders never pass.
        assert_eq!(decide("GET", None, None), GateOutcome::Deny);
    }

    #[test]
    fn url_parser_accepts_github_forms() {
        assert_eq!(
            parse_github_repo_url("https://github.com/octo/hello"),
            Some(("octo".to_owned(), "hello".to_owned()))
        );
        assert_eq!(
            parse_github_repo_url("http://github.com/octo/hello/"),
            Some(("octo".to_owned(), "hello".to_owned()))
        );
        assert_eq!(
            parse_github_repo_url("https://github.com/octo/hello.git"),
            Some(("octo".to_owned(), "hello".to_owned()))
        );
        assert_eq!(
            parse_github_repo_url("  https://github.com/octo/hello  "),
            Some(("octo".to_owned(), "hello".to_owned()))
        );
        assert_eq!(
            parse_github_repo_url("git@github.com:octo/hello"),
            Some(("octo".to_owned(), "hello".to_owned()))
        );
        assert_eq!(
            parse_github_repo_url("git@github.com:octo/hello.git"),
            Some(("octo".to_owned(), "hello".to_owned()))
        );
        // A bare `.git` name stays intact (lazy-regex parity).
        assert_eq!(
            parse_github_repo_url("https://github.com/octo/.git"),
            Some(("octo".to_owned(), ".git".to_owned()))
        );
        // One `.git` suffix strips; two strip once.
        assert_eq!(
            parse_github_repo_url("https://github.com/octo/r.git.git"),
            Some(("octo".to_owned(), "r.git".to_owned()))
        );
    }

    #[test]
    fn url_parser_rejects_non_github() {
        assert_eq!(parse_github_repo_url(""), None);
        assert_eq!(parse_github_repo_url("   "), None);
        assert_eq!(parse_github_repo_url("https://gitlab.com/o/r"), None);
        assert_eq!(
            parse_github_repo_url("https://github.com/o/r/tree/main"),
            None
        );
        assert_eq!(parse_github_repo_url("https://github.com/o/"), None);
        assert_eq!(parse_github_repo_url("https://github.com//r"), None);
        assert_eq!(parse_github_repo_url("https://github.com/o/r/extra/"), None);
        assert_eq!(parse_github_repo_url("https://github.com/o c/r"), None);
        assert_eq!(parse_github_repo_url("HTTPS://github.com/o/r"), None);
        assert_eq!(parse_github_repo_url("git@github.com:o/r/"), None);
        assert_eq!(parse_github_repo_url("git@gitlab.com:o/r"), None);
    }

    #[test]
    fn repo_id_mirrors_int_or_zero() {
        assert_eq!(python_repo_id(None), Ok(0));
        assert_eq!(python_repo_id(Some(&Value::Null)), Ok(0));
        assert_eq!(python_repo_id(Some(&Value::from(0))), Ok(0));
        assert_eq!(python_repo_id(Some(&Value::from(""))), Ok(0));
        assert_eq!(python_repo_id(Some(&Value::from(false))), Ok(0));
        assert_eq!(python_repo_id(Some(&Value::from(true))), Ok(1));
        assert_eq!(python_repo_id(Some(&Value::from(99887766))), Ok(99887766));
        assert_eq!(python_repo_id(Some(&Value::from(12.7))), Ok(12));
        assert_eq!(python_repo_id(Some(&Value::from("99887766"))), Ok(99887766));
        assert_eq!(python_repo_id(Some(&Value::from("  42  "))), Ok(42));
        assert_eq!(python_repo_id(Some(&Value::from("1_0"))), Ok(10));
        // Truthy garbage is the unhandled ValueError/TypeError (500).
        assert_eq!(python_repo_id(Some(&Value::from("abc"))), Err(()));
        assert_eq!(python_repo_id(Some(&Value::from(""))), Ok(0));
        assert_eq!(python_repo_id(Some(&serde_json::json!([1]))), Err(()));
        assert_eq!(python_repo_id(Some(&serde_json::json!({"a": 1}))), Err(()));
    }

    #[test]
    fn python_int_parses_like_builtin() {
        assert_eq!(python_int("42"), Ok(42));
        assert_eq!(python_int("  -7  "), Ok(-7));
        assert_eq!(python_int("+12"), Ok(12));
        assert_eq!(python_int("1_000_000"), Ok(1000000));
        assert_eq!(python_int(""), Err(()));
        assert_eq!(python_int("  "), Err(()));
        assert_eq!(python_int("1_"), Err(()));
        assert_eq!(python_int("_1"), Err(()));
        assert_eq!(python_int("1__2"), Err(()));
        assert_eq!(python_int("12.5"), Err(()));
        assert_eq!(python_int("0x10"), Err(()));
    }

    #[test]
    fn serialize_repo_shape_and_order() {
        let verified: serde_json::Map<String, Value> = serde_json::from_value(serde_json::json!({
            "id": 99887766,
            "owner": {"login": "octo"},
            "name": "hello",
            "full_name": "octo/hello",
            "default_branch": "main",
            "private": true,
        }))
        .expect("map");
        let repo = serialize_repo(&verified).expect("serializes");
        let keys: Vec<&str> = repo
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "owner",
                "name",
                "full_name",
                "default_branch",
                "private"
            ]
        );
        assert_eq!(repo["owner"], Value::String("octo".to_owned()));
        assert_eq!(repo["private"], Value::Bool(true));

        // Missing/null owner renders "".
        let bare: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"id": 1})).expect("map");
        let repo = serialize_repo(&bare).expect("serializes");
        assert_eq!(repo["owner"], Value::String(String::new()));
        assert_eq!(repo["name"], Value::String(String::new()));
        assert_eq!(repo["id"], Value::from(1));

        // Truthy non-dict owner is the 500.
        let bad: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"owner": "octo"})).expect("map");
        assert_eq!(serialize_repo(&bad), Err(()));
    }

    #[test]
    fn status_shapes_keep_key_order() {
        let legacy = LegacyStatus {
            id: Uuid::nil(),
            is_sync_enabled: false,
            last_synced_at: None,
            last_sync_error: String::new(),
            repository_id: 7,
            owner: "o".to_owned(),
            name: "r".to_owned(),
            url: Some("https://github.com/o/r".to_owned()),
        };
        let body = legacy_body(&legacy);
        let keys: Vec<&str> = body
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "bound",
                "id",
                "repository",
                "is_sync_enabled",
                "last_synced_at",
                "last_sync_error"
            ]
        );
        assert_eq!(body["repository"]["id"], Value::from(7));

        let fallback = FallbackStatus {
            id: Uuid::nil(),
            is_sync_enabled: true,
            last_synced_at: None,
            last_sync_error: String::new(),
            external_id: "7".to_owned(),
            namespace: "o".to_owned(),
            name: "r".to_owned(),
            web_url: "https://github.com/o/r".to_owned(),
        };
        let body = fallback_body(&fallback);
        assert_eq!(body["repository"]["id"], Value::String("7".to_owned()));
        assert_eq!(body["is_sync_enabled"], Value::Bool(true));
    }

    #[test]
    fn isoformat_matches_datetime_isoformat() {
        // `datetime(2026,9,29,12,0,0,tzinfo=utc).isoformat()`.
        let plain = DateTime::parse_from_rfc3339("2026-09-29T12:00:00Z")
            .expect("parse")
            .with_timezone(&Utc);
        assert_eq!(isoformat(&plain), "2026-09-29T12:00:00+00:00");
        // With microseconds: six digits, `+00:00`.
        let micro = DateTime::parse_from_rfc3339("2026-09-29T12:00:00.123456Z")
            .expect("parse")
            .with_timezone(&Utc);
        assert_eq!(isoformat(&micro), "2026-09-29T12:00:00.123456+00:00");
    }

    #[test]
    fn error_bodies_are_byte_exact() {
        // Every inline body, exactly as probed live against Django.
        assert_eq!(REPO_REQUIRED_BODY, r#"{"error":"repo_url is required"}"#);
        assert_eq!(
            BAD_URL_BODY,
            r#"{"error":"Only github.com URLs are supported (e.g. https://github.com/owner/repo)"}"#
        );
        assert_eq!(
            NOT_CONNECTED_BODY,
            r#"{"error":"Workspace GitHub integration is not connected"}"#
        );
        assert_eq!(
            REPO_NOT_FOUND_BODY,
            r#"{"error":"Repository not found or token has no access"}"#
        );
        assert_eq!(TOKEN_REJECTED_BODY, r#"{"error":"GitHub token rejected"}"#);
        assert_eq!(
            FAILED_VERIFY_BODY,
            r#"{"error":"Failed to verify repository"}"#
        );
        assert_eq!(
            NO_REPO_ID_BODY,
            r#"{"error":"GitHub did not return a repository id"}"#
        );
        assert_eq!(
            BIND_CONFLICT_BODY,
            r#"{"error":"Could not bind: concurrent change detected, please retry"}"#
        );
        assert_eq!(
            ENABLED_REQUIRED_BODY,
            r#"{"error":"enabled (bool) is required"}"#
        );
        assert_eq!(
            NO_BINDING_BODY,
            r#"{"error":"No GitHub binding for this project"}"#
        );
        assert_eq!(
            NO_WORKSPACE_BODY,
            r#"{"detail":"No Workspace matches the given query."}"#
        );
        assert_eq!(
            NO_PROJECT_BODY,
            r#"{"detail":"No Project matches the given query."}"#
        );
        assert_eq!(PROJECT_NOT_FOUND_BODY, r#"{"detail":"Project not found"}"#);
        assert_eq!(UNBOUND_BODY, r#"{"bound":false}"#);
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_resolve_identifier;

    #[test]
    fn resolve_identifier_strips_py_whitespace() {
        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736):
        // `%1C`-padded identifiers must resolve, not 404.
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_resolve_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_resolve_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_resolve_identifier("\u{85}eng\u{85}"), "ENG");
    }
}
