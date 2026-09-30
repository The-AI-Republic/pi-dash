//! D-33 external-integration handlers (stage 5, PIDASHCONV-454).
//!
//! Ports `apps/api/pi_dash/app/views/external/base.py` (243 lines) onto
//! the foundation crates:
//!
//! * `POST workspaces/<slug>/projects/<id>/ai-assistant/`
//!   (`GPTIntegrationEndpoint.post`, `:148-181`)
//! * `POST workspaces/<slug>/ai-assistant/`
//!   (`WorkspaceGPTIntegrationEndpoint.post`, `:184-212`)
//! * `GET unsplash/` (`UnsplashEndpoint.get`, `:215-243`)
//!
//! Only these three paths+methods are registered, so the edge serves
//! exactly this family from Rust while every sibling path keeps proxying
//! to Django — route registration is the cutover granularity, no flag
//! needed. Every other method on the owned paths proxies too, so Django
//! answers its own 405-after-auth and metadata OPTIONS byte for byte
//! (the loop-handlers precedent).
//!
//! Layering: provider tables, config branches, task/prompt falsy logic,
//! the Gemini model rewrite, `response_html`, and the Unsplash URL
//! builders live in [`pidash_services::app_integrations::external`];
//! the `allow_permission` gates live in [`super::gates`]. This module
//! owns the HTTP shell (routes, session auth), the membership-row and
//! lite-detail SQL, the config-row reads, the DRF error rendering, and
//! the outbound provider calls (the `OpenAI` SDK / `requests` clients
//! stay in Python; the wire behavior is what is ported).
//!
//! Handler order (preserved, not redesigned): DRF authentication
//! (`BaseAPIView`: session auth + `IsAuthenticated`,
//! `views/base.py:189-194`) runs before the decorator, the decorator
//! before the body — and the body reads the LLM config, then the task,
//! then calls the provider, and only then fetches the workspace/project
//! rows for the project endpoint (`:153-170`). Anonymous callers never
//! reach a gate (401 `{"detail": ...}`); gate denials answer the
//! allow-style 403.
//!
//! Fixture ids: FX-EXT-01 (`fx-ext-01-llm.json`), FX-PERM-01
//! (`fx-perm-01-permission-matrix.json`, the three external rows).
//!
//! Ported bugs (translate, don't redesign — also listed in the PR):
//!
//! * B3 (`base.py:235`): the Unsplash search URL renders `page=${page}`
//!   with a stray `$`; ported byte-for-byte.
//! * B5 (`base.py:159,163`): `task`/`prompt` default to `False`; a falsy
//!   prompt reaches `get_llm_response` as falsy, so the concatenation
//!   raises `TypeError`, which the `except Exception` swallows — the
//!   caller maps `(None, error)` to the single generic 500. Falsy and
//!   truthy-non-string prompts therefore answer 500 without any
//!   provider call, exactly like Python.
//! * A `None` provider key would raise `AttributeError` on `.lower()`
//!   (unguarded, `base.py:98`); the services layer maps it to the
//!   unsupported-provider 400 instead, since the configured default
//!   (`"openai"`) makes `None` unreachable outside a NULL DB row.

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_types::WorkspaceId;
use serde_json::Value;
use uuid::Uuid;

use pidash_services::app_integrations::external::{
    self, PromptArg, TaskArg, LLM_CONFIG_REQUIRED_BODY, LLM_INTERNAL_ERROR_BODY, TASK_REQUIRED_BODY,
};

use super::gates::{decide_gate, gate_for, tenant_context, GateOutcome};
use crate::license::{json_response, resolve_actor, Denial};
use crate::state::AppState;

/// `app/urls/external.py:15-18` (under the `api/` include).
pub const PROJECT_ASSISTANT_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/ai-assistant/";
/// `app/urls/external.py:20-23`.
pub const WORKSPACE_ASSISTANT_PATH: &str = "/api/workspaces/{slug}/ai-assistant/";
/// `app/urls/external.py:13`.
pub const UNSPLASH_PATH: &str = "/api/unsplash/";

/// Register the three external routes. Nothing else: sibling paths stay
/// unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            PROJECT_ASSISTANT_PATH,
            owned_post(post(post_project_assistant)),
        )
        .route(
            WORKSPACE_ASSISTANT_PATH,
            owned_post(post(post_workspace_assistant)),
        )
        .route(UNSPLASH_PATH, owned_get(get(get_unsplash)))
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

/// A GET-owned path: the GET handler serves from Rust, everything else
/// falls through to Django. `HEAD` rides axum's `get` handling like
/// Django's `GET`-backed `HEAD`.
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

// ---------------------------------------------------------------------------
// shared reads
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `request.user` or the 401.
///
/// Mirrors `BaseAPIView.authentication_classes` + `IsAuthenticated`
/// (`views/base.py:189-194`): bad session, unknown/inactive user, or
/// hash mismatch is anonymous.
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
/// soft-deleted rows excluded, `permissions/base.py:44-51`).
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

fn workspace_facts(slug: &str, role: Option<i32>) -> AllowFacts {
    let allowed = matches!(role, Some(ROLE_ADMIN) | Some(ROLE_MEMBER));
    AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: allowed,
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(ROLE_ADMIN),
    }
}

fn project_facts(slug: &str, ws_role: Option<i32>, project_role: Option<i32>) -> AllowFacts {
    let ws_allowed = matches!(ws_role, Some(ROLE_ADMIN) | Some(ROLE_MEMBER));
    let project_allowed = matches!(project_role, Some(ROLE_ADMIN) | Some(ROLE_MEMBER));
    AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: ws_role.is_some(),
        has_allowed_workspace_role: ws_allowed,
        is_creator: false,
        has_allowed_project_role: project_allowed,
        is_project_member: project_role.is_some(),
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

/// One `instance_configurations` read with the caller's env default —
/// the `get_configuration_value` shape for a db-sourced key
/// (`license/utils/instance_value.py:28-54`): the stored row wins when
/// present (decrypted when `is_encrypted`, via `decrypt_data`), otherwise
/// the default (the `os.environ.get(...)` the view passes in).
/// Soft-deleted rows are invisible (the default manager).
async fn config_value(
    pool: &sqlx::PgPool,
    secret_key: &str,
    key: &str,
    env_default: Option<String>,
) -> Result<Option<String>, Denial> {
    let row: Option<(Option<String>, bool)> = sqlx::query_as(
        r#"SELECT value, is_encrypted FROM instance_configurations
           WHERE key = $1 AND deleted_at IS NULL"#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row {
        Some((Some(value), true)) => {
            let keyring = pidash_db::config::encryption::Keyring::from_secret(secret_key);
            Ok(Some(keyring.decrypt(&value)))
        }
        Some((Some(value), false)) => Ok(Some(value)),
        Some((None, _)) => Ok(env_default),
        None => Ok(env_default),
    }
}

fn env_default(name: &str, fallback: Option<&str>) -> Option<String> {
    match std::env::var(name) {
        Ok(value) => Some(value),
        Err(_) => fallback.map(str::to_owned),
    }
}

/// `get_llm_config` inputs (`base.py:81-96`): DB rows with the
/// `os.environ` fallbacks the view passes as defaults.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn llm_config(
    pool: &sqlx::PgPool,
    secret_key: &str,
) -> Result<(String, String, String), Response> {
    let api_key = config_value(
        pool,
        secret_key,
        "LLM_API_KEY",
        env_default("LLM_API_KEY", None),
    )
    .await
    .map_err(|denial| denial.into_response())?;
    let provider = config_value(
        pool,
        secret_key,
        "LLM_PROVIDER",
        env_default("LLM_PROVIDER", Some("openai")),
    )
    .await
    .map_err(|denial| denial.into_response())?;
    let model = config_value(
        pool,
        secret_key,
        "LLM_MODEL",
        env_default("LLM_MODEL", None),
    )
    .await
    .map_err(|denial| denial.into_response())?;
    external::resolve_llm_config(api_key.as_deref(), provider.as_deref(), model.as_deref())
        .map_err(|_| raw_body(StatusCode::BAD_REQUEST, LLM_CONFIG_REQUIRED_BODY))
}

/// DRF `exception_handler` maps `Http404(*args)` to `NotFound(*args)`, so
/// the `_rewrite_project_kwarg` miss (`Project.resolve`, "Project not
/// found") renders with the resolve message — verified against live
/// Django, not the bare default.
const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;

fn raw_body(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static response")
}

/// Parse a POST body through the shared DRF edge
/// ([`crate::assistant::common::parse_body`]): an empty body is `{}`
/// under any content type; scalars 400 with the exact
/// `non_field_errors` wording. A JSON `null` body arrives as
/// [`ParsedBody::Null`], which the PUT edge maps to `No data provided`;
/// here it degrades to `{}` (missing task → 400), since the view reads
/// `request.data.get(...)` — assumption, documented: no contract test
/// pins a null POST body on these paths.
#[allow(clippy::result_large_err)]
fn parse_object(
    body: &[u8],
    content_type: Option<&str>,
) -> Result<Vec<(String, crate::assistant::common::BodyField)>, Response> {
    use crate::assistant::common::{parse_body, ParsedBody};
    match parse_body(body, content_type).map_err(|failure| failure.into_response())? {
        ParsedBody::Object(fields) => Ok(fields),
        ParsedBody::Scalar(value) => {
            Err(crate::assistant::common::non_field_failure(&value).into_response())
        }
        ParsedBody::Null => Ok(Vec::new()),
    }
}

/// Extract the `task` field or answer the falsy-task 400
/// (`base.py:159-161`). A truthy non-string passes the guard but fails
/// the concatenation — the caller maps it to the 500 (B5).
#[allow(clippy::result_large_err)]
fn task_text(fields: &[(String, crate::assistant::common::BodyField)]) -> Result<String, Response> {
    use crate::assistant::common::BodyField;
    let value =
        fields
            .iter()
            .rev()
            .find(|(name, _)| name == "task")
            .map(|(_, field)| match field {
                BodyField::Json(value) => value.clone(),
                BodyField::File { .. } => Value::Bool(true),
            });
    match external::parse_task(value.as_ref()) {
        TaskArg::Text(task) => Ok(task),
        TaskArg::Missing => Err(raw_body(StatusCode::BAD_REQUEST, TASK_REQUIRED_BODY)),
        TaskArg::NonStringTruthy => Err(raw_body(
            StatusCode::INTERNAL_SERVER_ERROR,
            LLM_INTERNAL_ERROR_BODY,
        )),
    }
}

/// Extract the `prompt` field (`base.py:163`). Anything but a non-empty
/// string answers the swallowed 500 without a provider call (B5).
#[allow(clippy::result_large_err)]
fn prompt_text(
    fields: &[(String, crate::assistant::common::BodyField)],
) -> Result<String, Response> {
    use crate::assistant::common::BodyField;
    let value =
        fields
            .iter()
            .rev()
            .find(|(name, _)| name == "prompt")
            .map(|(_, field)| match field {
                BodyField::Json(value) => value.clone(),
                BodyField::File { .. } => Value::Bool(true),
            });
    match external::parse_prompt(value.as_ref()) {
        PromptArg::Text(prompt) => Ok(prompt),
        PromptArg::Falsy | PromptArg::NonStringTruthy => Err(raw_body(
            StatusCode::INTERNAL_SERVER_ERROR,
            LLM_INTERNAL_ERROR_BODY,
        )),
    }
}

// ---------------------------------------------------------------------------
// outbound provider call (`get_llm_response`, `base.py:123-145`)
// ---------------------------------------------------------------------------

/// Attempt the OpenAI chat-completions call every provider goes through
/// (`OpenAI(api_key=...).chat.completions.create(...)`, `:131-136`).
/// Success returns the first choice's text
/// (`choices[0].message.content`); *any* failure — auth, rate limit,
/// transport, or an unparsable payload — is `Err(())`, and the caller
/// answers the single generic 500, since the handler swallows the
/// mapped detail (`:139-145` → `:164-168`).
async fn call_llm(
    api_key: &str,
    model: &str,
    provider: &str,
    task: &str,
    prompt: &str,
) -> Result<String, ()> {
    let model = external::rewrite_model(provider, model);
    let content = external::final_text(task, prompt);
    let payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": content}],
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| ())?;
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .header(header::CONTENT_TYPE.as_str(), "application/json")
        .body(serde_json::to_string(&payload).map_err(|_| ())?)
        .send()
        .await
        .map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let bytes = response.bytes().await.map_err(|_| ())?;
    let payload: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    payload
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::to_owned)
        .ok_or(())
}

// ---------------------------------------------------------------------------
// lite detail shapes
// ---------------------------------------------------------------------------

/// One `workspaces` row for `WorkspaceLiteSerializer`
/// (`serializers/workspace.py:79-83`): `name`, `slug`, `id`, `logo_url`.
struct WorkspaceLite {
    id: Uuid,
    name: String,
    slug: String,
    logo: Option<String>,
    logo_asset_id: Option<Uuid>,
    logo_asset_entity: Option<String>,
}

/// One `projects` row for `ProjectLiteSerializer`
/// (`serializers/project.py:120-133`).
struct ProjectLite {
    id: Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_asset_id: Option<Uuid>,
    cover_asset_entity: Option<String>,
    logo_props: Value,
    description: String,
    is_default: bool,
}

/// One `workspaces` + `file_assets` row for [`fetch_workspace`].
type WorkspaceRow = (
    Uuid,
    String,
    String,
    Option<String>,
    Option<Uuid>,
    Option<String>,
);

/// One `projects` + `file_assets` row for [`fetch_project`].
#[allow(clippy::type_complexity)]
type ProjectRow = (
    Uuid,
    String,
    String,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Value,
    Option<String>,
    bool,
);

/// `Workspace.objects.get(slug=slug)` (`:170`) — 404 when missing
/// (`handle_exception`: `{"error":"The required object does not exist."}`).
async fn fetch_workspace(pool: &sqlx::PgPool, slug: &str) -> Result<WorkspaceLite, Denial> {
    let row: Option<WorkspaceRow> = sqlx::query_as(
        r#"SELECT w.id, w.name, w.slug, w.logo, w.logo_asset_id, fa.entity_type
               FROM workspaces w LEFT JOIN file_assets fa ON fa.id = w.logo_asset_id
               WHERE w.slug = $1 AND w.deleted_at IS NULL"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (id, name, slug, logo, logo_asset_id, logo_asset_entity) = row.ok_or(Denial::NotFound)?;
    Ok(WorkspaceLite {
        id,
        name,
        slug,
        logo,
        logo_asset_id,
        logo_asset_entity,
    })
}

/// `Project.objects.get(pk=project_id)` (`:171`) — 404 when missing.
async fn fetch_project(pool: &sqlx::PgPool, project_id: &Uuid) -> Result<ProjectLite, Denial> {
    let row: Option<ProjectRow> = sqlx::query_as(
        r#"SELECT p.id, p.identifier, p.name, p.cover_image, p.cover_image_asset_id,
                  fa.entity_type, p.logo_props, p.description, p.is_default
           FROM projects p LEFT JOIN file_assets fa ON fa.id = p.cover_image_asset_id
           WHERE p.id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (
        id,
        identifier,
        name,
        cover_image,
        cover_asset_id,
        cover_asset_entity,
        logo_props,
        description,
        is_default,
    ) = row.ok_or(Denial::NotFound)?;
    Ok(ProjectLite {
        id,
        identifier,
        name,
        cover_image,
        cover_asset_id,
        cover_asset_entity,
        logo_props,
        description: description.unwrap_or_default(),
        is_default,
    })
}

/// `logo_url`: the logo asset's URL, else the `logo` text, else null
/// (`db/models/workspace.py:145-154`).
fn logo_url(row: &WorkspaceLite) -> Value {
    match (row.logo_asset_id, row.logo_asset_entity.as_deref()) {
        (Some(id), Some("WORKSPACE_LOGO")) => Value::String(format!("/api/assets/v2/static/{id}/")),
        (Some(_), _) => Value::Null,
        (None, _) => match row.logo.as_deref() {
            Some(logo) => Value::String(logo.to_owned()),
            None => Value::Null,
        },
    }
}

/// `cover_image_url` (`db/models/project.py:176-183`): the cover asset's
/// URL, else the `cover_image` text, else null.
fn cover_image_url(row: &ProjectLite) -> Value {
    match (row.cover_asset_id, row.cover_asset_entity.as_deref()) {
        (Some(id), Some("PROJECT_COVER")) => Value::String(format!("/api/assets/v2/static/{id}/")),
        (Some(_), _) => Value::Null,
        (None, _) => match row.cover_image.as_deref() {
            Some(cover) => Value::String(cover.to_owned()),
            None => Value::Null,
        },
    }
}

/// `WorkspaceLiteSerializer` order: `name`, `slug`, `id`, `logo_url`.
fn workspace_detail(row: &WorkspaceLite) -> Value {
    let mut map = serde_json::Map::with_capacity(4);
    map.insert("name".to_owned(), Value::String(row.name.clone()));
    map.insert("slug".to_owned(), Value::String(row.slug.clone()));
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert("logo_url".to_owned(), logo_url(row));
    Value::Object(map)
}

/// `ProjectLiteSerializer` order: `id`, `identifier`, `name`,
/// `cover_image`, `cover_image_url`, `logo_props`, `description`,
/// `is_default`.
fn project_detail(row: &ProjectLite) -> Value {
    let mut map = serde_json::Map::with_capacity(8);
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert(
        "identifier".to_owned(),
        Value::String(row.identifier.clone()),
    );
    map.insert("name".to_owned(), Value::String(row.name.clone()));
    map.insert(
        "cover_image".to_owned(),
        row.cover_image
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert("cover_image_url".to_owned(), cover_image_url(row));
    map.insert("logo_props".to_owned(), row.logo_props.clone());
    map.insert(
        "description".to_owned(),
        Value::String(row.description.clone()),
    );
    map.insert("is_default".to_owned(), Value::Bool(row.is_default));
    Value::Object(map)
}

/// Resolve the `project_id` URL kwarg: a UUID passes through, anything
/// else resolves as a workspace-scoped identifier
/// (`_rewrite_project_kwarg`, `views/base.py:49-77` + `Project.resolve`).
/// Unresolvable values answer `{"detail":"Project not found"}` (rendered
/// here, not through `Denial`, whose `NotFound` is the `ObjectDoesNotExist`
/// error body owned by the license domain).
async fn resolve_project_id(pool: &sqlx::PgPool, slug: &str, raw: &str) -> Result<Uuid, Response> {
    if let Ok(id) = raw.parse::<Uuid>() {
        return Ok(id);
    }
    let normalized = raw.trim().to_uppercase();
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

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `POST workspaces/<slug>/projects/<id>/ai-assistant/`
/// (`GPTIntegrationEndpoint.post`, `base.py:148-181`).
async fn post_project_assistant(
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
    let row = gate_for("POST", "workspaces/<slug>/projects/<id>/ai-assistant/")
        .expect("project assistant gate");
    let ws_role = match workspace_role(&pool, &actor.id, &slug).await {
        Ok(role) => role,
        Err(denial) => return denial.into_response(),
    };
    let pm_role = match project_role(&pool, &actor.id, &project_id, &slug).await {
        Ok(role) => role,
        Err(denial) => return denial.into_response(),
    };
    if let Err(response) = enforce(decide_gate(
        &row.gate,
        &tenant_context(&slug),
        &project_facts(&slug, ws_role, pm_role),
    )) {
        return response;
    }
    let (api_key, model, provider) = match llm_config(&pool, &state.settings().secret_key).await {
        Ok(triple) => triple,
        Err(response) => return response,
    };
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let fields = match parse_object(&body, content_type) {
        Ok(fields) => fields,
        Err(response) => return response,
    };
    let task = match task_text(&fields) {
        Ok(task) => task,
        Err(response) => return response,
    };
    let prompt = match prompt_text(&fields) {
        Ok(prompt) => prompt,
        Err(response) => return response,
    };
    let text = match call_llm(&api_key, &model, &provider, &task, &prompt).await {
        Ok(text) => text,
        Err(()) => {
            return raw_body(StatusCode::INTERNAL_SERVER_ERROR, LLM_INTERNAL_ERROR_BODY);
        }
    };
    let workspace = match fetch_workspace(&pool, &slug).await {
        Ok(workspace) => workspace,
        Err(denial) => return denial.into_response(),
    };
    let project = match fetch_project(&pool, &project_id).await {
        Ok(project) => project,
        Err(denial) => return denial.into_response(),
    };
    let mut map = serde_json::Map::with_capacity(4);
    map.insert("response".to_owned(), Value::String(text.clone()));
    map.insert(
        "response_html".to_owned(),
        Value::String(external::response_html(&text)),
    );
    map.insert("project_detail".to_owned(), project_detail(&project));
    map.insert("workspace_detail".to_owned(), workspace_detail(&workspace));
    json_response(&Value::Object(map))
}

/// `POST workspaces/<slug>/ai-assistant/`
/// (`WorkspaceGPTIntegrationEndpoint.post`, `base.py:184-212`): the
/// same guards, without the project/workspace detail pair (`:206-212`).
async fn post_workspace_assistant(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Path(slug): Path<String>,
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
    let row =
        gate_for("POST", "workspaces/<slug>/ai-assistant/").expect("workspace assistant gate");
    let ws_role = match workspace_role(&pool, &actor.id, &slug).await {
        Ok(role) => role,
        Err(denial) => return denial.into_response(),
    };
    if let Err(response) = enforce(decide_gate(
        &row.gate,
        &tenant_context(&slug),
        &workspace_facts(&slug, ws_role),
    )) {
        return response;
    }
    let (api_key, model, provider) = match llm_config(&pool, &state.settings().secret_key).await {
        Ok(triple) => triple,
        Err(response) => return response,
    };
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let fields = match parse_object(&body, content_type) {
        Ok(fields) => fields,
        Err(response) => return response,
    };
    let task = match task_text(&fields) {
        Ok(task) => task,
        Err(response) => return response,
    };
    let prompt = match prompt_text(&fields) {
        Ok(prompt) => prompt,
        Err(response) => return response,
    };
    let text = match call_llm(&api_key, &model, &provider, &task, &prompt).await {
        Ok(text) => text,
        Err(()) => {
            return raw_body(StatusCode::INTERNAL_SERVER_ERROR, LLM_INTERNAL_ERROR_BODY);
        }
    };
    let mut map = serde_json::Map::with_capacity(2);
    map.insert("response".to_owned(), Value::String(text.clone()));
    map.insert(
        "response_html".to_owned(),
        Value::String(external::response_html(&text)),
    );
    json_response(&Value::Object(map))
}

/// `GET unsplash/` (`UnsplashEndpoint.get`, `base.py:215-243`): no
/// decorator, so any authenticated caller reaches the body
/// (FX-PERM-01: `Authenticated`). Without an access key the endpoint
/// answers `[]` (`:226-227`); otherwise it proxies api.unsplash.com
/// and relays the payload with the upstream status verbatim
/// (`:242-243`).
async fn get_unsplash(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = actor(&state, &pool, extension).await {
        return denial.into_response();
    }
    let row = gate_for("GET", "unsplash/").expect("unsplash gate");
    debug_assert!(matches!(row.gate, super::gates::Gate::Authenticated));
    let access_key = match config_value(
        &pool,
        &state.settings().secret_key,
        "UNSPLASH_ACCESS_KEY",
        env_default("UNSPLASH_ACCESS_KEY", None),
    )
    .await
    {
        Ok(key) => key,
        Err(denial) => return denial.into_response(),
    };
    let access_key = access_key.unwrap_or_default();
    if access_key.is_empty() {
        return json_response(&Vec::<Value>::new());
    }
    let url = external::unsplash_url(
        &access_key,
        params.get("query").map(String::as_str),
        params.get("page").map(String::as_str),
        params.get("per_page").map(String::as_str),
    );
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let response = match client
        .get(url)
        .header(header::CONTENT_TYPE.as_str(), "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let payload: Value = match serde_json::from_slice(&bytes) {
        Ok(payload) => payload,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let body = serde_json::to_string(&payload).expect("upstream payload serializes");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("unsplash relay")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_paths_match_django_urls() {
        // `app/urls/external.py:12-24`, under the `api/` include.
        assert_eq!(UNSPLASH_PATH, "/api/unsplash/");
        assert_eq!(
            PROJECT_ASSISTANT_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/ai-assistant/"
        );
        assert_eq!(
            WORKSPACE_ASSISTANT_PATH,
            "/api/workspaces/{slug}/ai-assistant/"
        );
    }

    #[test]
    fn gates_cover_all_three_routes() {
        // FX-PERM-01 rows: project assistant is PROJECT ADMIN/MEMBER,
        // workspace assistant is WORKSPACE ADMIN/MEMBER, unsplash is
        // Authenticated (no decorator).
        assert!(gate_for("POST", "workspaces/<slug>/projects/<id>/ai-assistant/").is_some());
        assert!(gate_for("POST", "workspaces/<slug>/ai-assistant/").is_some());
        assert!(gate_for("GET", "unsplash/").is_some());
    }

    #[test]
    fn project_gate_denies_guests_and_bare_workspace_admin() {
        use pidash_auth::permissions::ROLE_GUEST;
        let row = gate_for("POST", "workspaces/<slug>/projects/<id>/ai-assistant/").expect("gate");
        let scope = tenant_context("acme");
        // Guest project member: denied.
        let denied = project_facts("acme", Some(ROLE_MEMBER), Some(ROLE_GUEST));
        assert_eq!(decide_gate(&row.gate, &scope, &denied), GateOutcome::Deny);
        // Outsider: denied.
        let outsider = project_facts("acme", None, None);
        assert_eq!(decide_gate(&row.gate, &scope, &outsider), GateOutcome::Deny);
        // Member: allowed.
        let member = project_facts("acme", Some(ROLE_MEMBER), Some(ROLE_MEMBER));
        assert_eq!(decide_gate(&row.gate, &scope, &member), GateOutcome::Allow);
        // Workspace admin without a project role still passes through
        // the member branch? No — the override needs a project row, so
        // a bare workspace admin is denied here, matching `decide_allow`.
        let bare_admin = project_facts("acme", Some(ROLE_ADMIN), None);
        assert_eq!(
            decide_gate(&row.gate, &scope, &bare_admin),
            GateOutcome::Deny
        );
    }

    #[test]
    fn workspace_gate_matches_admin_member_only() {
        use pidash_auth::permissions::ROLE_GUEST;
        let row = gate_for("POST", "workspaces/<slug>/ai-assistant/").expect("gate");
        let scope = tenant_context("acme");
        assert_eq!(
            decide_gate(
                &row.gate,
                &scope,
                &workspace_facts("acme", Some(ROLE_ADMIN))
            ),
            GateOutcome::Allow
        );
        assert_eq!(
            decide_gate(
                &row.gate,
                &scope,
                &workspace_facts("acme", Some(ROLE_MEMBER))
            ),
            GateOutcome::Allow
        );
        assert_eq!(
            decide_gate(
                &row.gate,
                &scope,
                &workspace_facts("acme", Some(ROLE_GUEST))
            ),
            GateOutcome::Deny
        );
        assert_eq!(
            decide_gate(&row.gate, &scope, &workspace_facts("acme", None)),
            GateOutcome::Deny
        );
    }

    #[test]
    fn lite_details_keep_serializer_field_order() {
        let workspace = WorkspaceLite {
            id: Uuid::nil(),
            name: "Acme".to_owned(),
            slug: "acme".to_owned(),
            logo: None,
            logo_asset_id: None,
            logo_asset_entity: None,
        };
        let detail = workspace_detail(&workspace);
        let keys: Vec<&str> = detail
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["name", "slug", "id", "logo_url"]);
        assert_eq!(detail["logo_url"], Value::Null);

        let project = ProjectLite {
            id: Uuid::nil(),
            identifier: "CT00001".to_owned(),
            name: "P".to_owned(),
            cover_image: None,
            cover_asset_id: None,
            cover_asset_entity: None,
            logo_props: serde_json::json!({}),
            description: "".to_owned(),
            is_default: false,
        };
        let detail = project_detail(&project);
        let keys: Vec<&str> = detail
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "identifier",
                "name",
                "cover_image",
                "cover_image_url",
                "logo_props",
                "description",
                "is_default"
            ]
        );
    }
}
