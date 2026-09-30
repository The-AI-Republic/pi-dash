//! Workspace webhook handlers (D-33, stage 5, PIDASHCONV-453).
//!
//! Port of `apps/api/pi_dash/app/views/webhook/base.py` (all 126 lines)
//! with routes from `app/urls/webhook.py:15-30`:
//!
//! * `POST workspaces/<slug>/webhooks/` (`WebhookEndpoint.post`, `:22-37`)
//! * `GET workspaces/<slug>/webhooks/` (`WebhookEndpoint.get` pk=None,
//!   `:39-58`)
//! * `GET workspaces/<slug>/webhooks/<uuid>/` (`WebhookEndpoint.get`,
//!   `:59-76`)
//! * `PATCH workspaces/<slug>/webhooks/<uuid>/` (`WebhookEndpoint.patch`,
//!   `:78-102`)
//! * `DELETE workspaces/<slug>/webhooks/<uuid>/`
//!   (`WebhookEndpoint.delete`, `:104-108`)
//! * `POST workspaces/<slug>/webhooks/<uuid>/regenerate/`
//!   (`WebhookSecretRegenerateEndpoint.post`, `:111-118`)
//! * `GET workspaces/<slug>/webhook-logs/<uuid>/`
//!   (`WebhookLogsEndpoint.get`, `:121-126`)
//!
//! One closure: the collection + detail paths share `WebhookEndpoint`, so
//! the list/detail/patch/delete handlers share the gate and the lookup.
//!
//! Layering (all sibling layers merged — PIDASHCONV-365/380/428/436/439):
//! field + guard validation in
//! [`pidash_services::app_integrations::serializers`], SQL in
//! [`pidash_db::app_integrations::queries_webhook`], row types in
//! [`pidash_db::app_integrations::models_webhook`], the gate table in
//! [`super::gates`]. This module owns the HTTP shell (routes, session
//! auth, the `@allow_permission([ADMIN], WORKSPACE)` gate), the DRF
//! body-validation mirrors for the non-URL fields, and the row rendering.
//!
//! Fixture ids: FX-WEB-01 (`fx-web-01-webhook-crud.json`), FX-WEB-02
//! (`fx-web-02-secret-regenerate.json`), FX-WEB-03
//! (`fx-web-03-webhook-log-queries.json`), FX-PERM-01 (gate rows).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG B2 (`base.py:84`): PATCH builds `context={request: request}`
//!   (the request object as the dict key), so
//!   `self.context.get("request")` is always `None` on update and the
//!   request host is never appended to the disallowed list. Ported as-is:
//!   [`validate_update_url`] is called with `request_host = None`
//!   ([`pidash_services::app_integrations::serializers`] pins this).
//! * `fields=(...)` (`base.py:42-57,81-85`): accepted, then discarded —
//!   `DynamicBaseSerializer.__init__` overwrites it with `self.expand`
//!   (empty), so every read renders the full `__all__` shape, `secret_key`
//!   included (the contract pins this; the serializer kernel ports it).
//! * `WebhookLog.webhook` is a plain UUID column, not a FK — the logs
//!   predicate is a bare equality and unknown ids answer `200 []`
//!   (no existence check, `base.py:124`).
//! * `WebhookLog.response_status` stores mixed types as text; the column
//!   stays `TEXT`.
//! * NOTE (fixture-vs-code): FX-WEB-01 says a non-"already exists"
//!   `IntegrityError` on POST answers 500, but `base.py:36` re-raises
//!   into `handle_exception`'s `IntegrityError` branch, which answers 400
//!   `{"error": "The payload is not valid"}`. The code wins: other
//!   SQLSTATE-23xxx failures map to that 400.
//!
//! # Task delivery
//!
//! `serve` carries no AMQP publisher (only the worker does), so the
//! `soft_delete_related_objects.delay("db", "webhook", pk, "default")`
//! call on delete (`db/mixins.py:77-78`) enqueues a
//! [`pidash_jobs::queue::NewJob`] into `rust_job_queue`; the worker
//! forwards the Python-owned name to the broker. Enqueue is best-effort
//! after commit: a missing queue table must not turn the 204 into a 500,
//! so failures are traced and the response stands (the D-02 `space`
//! precedent).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::app_integrations::models_webhook::webhook::Webhook;
use pidash_db::app_integrations::models_webhook::webhook_log::WebhookLog;
use pidash_db::app_integrations::queries_webhook;
use pidash_services::app_integrations::serializers::{
    validate_create_url, validate_update_url, SystemResolver, WebhookUrlError,
};

use super::{actor, owned, parse_id, pool_of, require_workspace_admin, workspace_id_or_404};
use super::{json_response, Denial};
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// Gate-table paths for the four webhook routes, in
/// [`super::gates::GATES`] spelling.
const PATH_COLLECTION: &str = "workspaces/<slug>/webhooks/";
const PATH_DETAIL: &str = "workspaces/<slug>/webhooks/<uuid>/";
const PATH_REGENERATE: &str = "workspaces/<slug>/webhooks/<uuid>/regenerate/";
const PATH_LOGS: &str = "workspaces/<slug>/webhook-logs/<uuid>/";

/// Register the four owned webhook routes. Every other method on these
/// paths proxies to Django (its 405-after-auth responses live there).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/webhooks/",
            owned(
                axum::routing::get(list_webhooks).post(create_webhook),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/webhooks/{pk}/",
            owned(
                axum::routing::get(retrieve_webhook)
                    .patch(partial_update_webhook)
                    .delete(destroy_webhook),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/webhooks/{pk}/regenerate/",
            owned(
                axum::routing::post(regenerate_secret),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/webhook-logs/{webhook_id}/",
            owned(
                axum::routing::get(list_webhook_logs),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
}

// ---------------------------------------------------------------------------
// Request context
// ---------------------------------------------------------------------------

/// The authenticated ADMIN actor plus the request host for the create
/// guard (`request.get_host().split(":")[0]`,
/// `serializers/webhook.py:46-48`).
struct Context {
    actor_id: Uuid,
    timezone: chrono_tz::Tz,
    request_host: Option<String>,
}

/// Session auth + the workspace-ADMIN gate, in Django's order: anonymous
/// is rejected 401 before anything else; the decorator 403s before the
/// view body runs, so lookups never leak existence to non-admins.
async fn context(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: &HeaderMap,
    slug: &str,
    method: &str,
    path: &str,
) -> Result<Context, Denial> {
    let pool = pool_of(state)?;
    let resolved = actor(state, extension).await?;
    require_workspace_admin(pool, slug, &resolved.id, method, path).await?;
    Ok(Context {
        actor_id: resolved.id,
        timezone: resolved.timezone,
        request_host: request_host_of(headers),
    })
}

/// `request.get_host().split(":")[0]` (`serializers/webhook.py:47`):
/// the `Host` header without any port. `None` when the header is absent
/// (Django would raise `DisallowedHost` there; the header is always
/// present on the wire, so this is unreachable in practice).
fn request_host_of(headers: &HeaderMap) -> Option<String> {
    let host = headers.get("host")?.to_str().ok()?;
    Some(host.split(':').next().unwrap_or("").to_owned())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/webhooks/` (`base.py:39-58`, pk=None branch):
/// newest-first live webhooks for the workspace, full shape each, 200.
async fn list_webhooks(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    Path((slug,)): axum::extract::Path<(String,)>,
) -> Response {
    let ctx = match context(&state, extension, &headers, &slug, "GET", PATH_COLLECTION).await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let rows = match queries_webhook::fetch_webhook_list(pool, &slug).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let items: Vec<Value> = rows
        .iter()
        .map(|row| render_webhook(row, &ctx.timezone))
        .collect();
    json_response(StatusCode::OK, Value::Array(items).to_string())
}

/// `POST workspaces/<slug>/webhooks/` (`base.py:22-37`): validate through
/// `WebhookSerializer` (field chain, then the DNS/SSRF guards with the
/// request host), force `workspace` server-side (read-only), 201 with the
/// full shape. Duplicate URLs answer the 409 branch (matched on the
/// Postgres unique-violation detail, which is what `"already exists" in
/// str(e)` matches); other integrity failures answer the 400 branch.
async fn create_webhook(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    Path((slug,)): axum::extract::Path<(String,)>,
    body: bytes::Bytes,
) -> Response {
    let ctx = match context(&state, extension, &headers, &slug, "POST", PATH_COLLECTION).await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // Field level first (`is_valid`), collecting every field's failures.
    let mut input = match validate_write_fields(&data, false) {
        Ok(input) => input,
        Err(errors) => return field_errors(errors),
    };
    // Then the guard chain (`create()` inside `save()`), off the executor:
    // `socket.getaddrinfo` blocks in Python too. `url` is always present
    // here: POST field validation rejects a missing/invalid one above.
    let Some(url) = input.url.as_deref() else {
        return Denial::ServerError.into_response();
    };
    let url = match run_create_guards(url, ctx.request_host.as_deref()).await {
        Ok(url) => url,
        Err(error) => return guard_error(error),
    };
    input.url = Some(url);
    // `Workspace.objects.get(slug=slug)` — 404 when missing (`base.py:23`).
    // The gate already passed, so the slug names a workspace the caller
    // administers; this lookup only races deletion.
    let workspace_id = match workspace_id_or_404(pool, &slug).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let now = Utc::now();
    let row = Webhook {
        id: Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        // `BaseModel.save` on create (`db/models/base.py:37-39`, CRUM):
        // `created_by` is the actor, `updated_by` stays `None`.
        created_by_id: Some(ctx.actor_id),
        updated_by_id: None,
        deleted_at: None,
        workspace_id,
        url: input.url.expect("guarded url"),
        // Model defaults (`db/models/webhook.py:37-46`): active, flags
        // off, `version` "v1".
        is_active: input.is_active.unwrap_or(true),
        // Read-only: the model default fires (`generate_token`).
        secret_key: pidash_db::app_integrations::models_webhook::generate_secret_key(),
        project: input.project.unwrap_or(false),
        issue: input.issue.unwrap_or(false),
        module: input.module.unwrap_or(false),
        cycle: input.cycle.unwrap_or(false),
        issue_comment: input.issue_comment.unwrap_or(false),
        is_internal: input.is_internal.unwrap_or(false),
        version: input.version.unwrap_or_else(|| "v1".to_owned()),
    };
    if let Err(error) = queries_webhook::create_webhook(pool, &row).await {
        return insert_error(error);
    }
    json_response(
        StatusCode::CREATED,
        render_webhook(&row, &ctx.timezone).to_string(),
    )
}

/// `GET workspaces/<slug>/webhooks/<uuid>/` (`base.py:59-76`): one live
/// webhook scoped to the slug, full shape, 200; misses answer 404.
async fn retrieve_webhook(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    Path((slug, pk)): axum::extract::Path<(String, String)>,
) -> Response {
    let ctx = match context(&state, extension, &headers, &slug, "GET", PATH_DETAIL).await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let pk = match parse_id(&pk) {
        Ok(pk) => pk,
        Err(response) => return response,
    };
    let row = match queries_webhook::fetch_webhook_detail(pool, pk, &slug).await {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    match row {
        Some(row) => json_response(
            StatusCode::OK,
            render_webhook(&row, &ctx.timezone).to_string(),
        ),
        None => Denial::NotFound.into_response(),
    }
}

/// `PATCH workspaces/<slug>/webhooks/<uuid>/` (`base.py:78-102`):
/// partial update through `WebhookSerializer` (guards only when `url` is
/// present, with BUG B2's missing request host), full-row `save()`
/// (`updated_at` advances, `updated_by` is the actor), 200.
async fn partial_update_webhook(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    Path((slug, pk)): axum::extract::Path<(String, String)>,
    body: bytes::Bytes,
) -> Response {
    let ctx = match context(&state, extension, &headers, &slug, "PATCH", PATH_DETAIL).await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let pk = match parse_id(&pk) {
        Ok(pk) => pk,
        Err(response) => return response,
    };
    let row = match queries_webhook::fetch_webhook_detail(pool, pk, &slug).await {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(mut row) = row else {
        return Denial::NotFound.into_response();
    };
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let input = match validate_write_fields(&data, true) {
        Ok(input) => input,
        Err(errors) => return field_errors(errors),
    };
    // `update()` runs the guards only `if url:` — with BUG B2's context,
    // so the request host is never appended (`base.py:84`).
    if let Some(url) = input.url.as_deref() {
        match run_update_guards(url).await {
            Ok(guarded) => row.url = guarded,
            Err(error) => return guard_error(error),
        }
    }
    if let Some(is_active) = input.is_active_opt() {
        row.is_active = is_active;
    }
    if let Some(project) = input.project_opt() {
        row.project = project;
    }
    if let Some(issue) = input.issue_opt() {
        row.issue = issue;
    }
    if let Some(module) = input.module_opt() {
        row.module = module;
    }
    if let Some(cycle) = input.cycle_opt() {
        row.cycle = cycle;
    }
    if let Some(issue_comment) = input.issue_comment_opt() {
        row.issue_comment = issue_comment;
    }
    if let Some(is_internal) = input.is_internal_opt() {
        row.is_internal = is_internal;
    }
    if let Some(version) = input.version_opt() {
        row.version = version;
    }
    // Full-row `save()`: `auto_now` advances `updated_at`, CRUM sets
    // `updated_by` (`db/models/base.py:41-42`).
    row.updated_at = Utc::now();
    row.updated_by_id = Some(ctx.actor_id);
    if let Err(error) = queries_webhook::update_webhook_full(pool, &row).await {
        return update_error(error);
    }
    json_response(
        StatusCode::OK,
        render_webhook(&row, &ctx.timezone).to_string(),
    )
}

/// `DELETE workspaces/<slug>/webhooks/<uuid>/` (`base.py:104-108`):
/// soft delete (`deleted_at` + full `save()` with the actor as
/// `updated_by`), then the related-objects task, 204 with an empty body.
async fn destroy_webhook(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    Path((slug, pk)): axum::extract::Path<(String, String)>,
) -> Response {
    let ctx = match context(&state, extension, &headers, &slug, "DELETE", PATH_DETAIL).await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let pk = match parse_id(&pk) {
        Ok(pk) => pk,
        Err(response) => return response,
    };
    let row = match queries_webhook::fetch_webhook_detail(pool, pk, &slug).await {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(mut row) = row else {
        return Denial::NotFound.into_response();
    };
    queries_webhook::stamp_soft_delete(&mut row);
    // `delete()` goes through `save()`, so CRUM stamps `updated_by`
    // exactly like a patch (`db/models/base.py:41-42`).
    row.updated_by_id = Some(ctx.actor_id);
    if let Err(error) = queries_webhook::update_webhook_full(pool, &row).await {
        return update_error(error);
    }
    enqueue_soft_delete(pool, &pk).await;
    StatusCode::NO_CONTENT.into_response()
}

/// `POST workspaces/<slug>/webhooks/<uuid>/regenerate/`
/// (`base.py:111-118`): rotate `secret_key` via `generate_token()`, full
/// `save()` (CRUM stamps `updated_by`, `auto_now` advances `updated_at`),
/// 200 with the full shape carrying the new secret.
async fn regenerate_secret(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    Path((slug, pk)): axum::extract::Path<(String, String)>,
) -> Response {
    let ctx = match context(&state, extension, &headers, &slug, "POST", PATH_REGENERATE).await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let pk = match parse_id(&pk) {
        Ok(pk) => pk,
        Err(response) => return response,
    };
    let row = match queries_webhook::fetch_webhook_detail(pool, pk, &slug).await {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(mut row) = row else {
        return Denial::NotFound.into_response();
    };
    queries_webhook::stamp_secret_regenerate(&mut row);
    // `save()` stamps `updated_by` through CRUM, like every update.
    row.updated_by_id = Some(ctx.actor_id);
    if let Err(error) = queries_webhook::update_webhook_full(pool, &row).await {
        return update_error(error);
    }
    json_response(
        StatusCode::OK,
        render_webhook(&row, &ctx.timezone).to_string(),
    )
}

/// `GET workspaces/<slug>/webhook-logs/<uuid>/` (`base.py:121-126`):
/// newest-first log rows for the webhook scoped to the slug, 200. There
/// is no existence check on the webhook itself: unknown ids answer `[]`.
async fn list_webhook_logs(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    Path((slug, webhook_id)): axum::extract::Path<(String, String)>,
) -> Response {
    let ctx = match context(&state, extension, &headers, &slug, "GET", PATH_LOGS).await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let webhook_id = match parse_id(&webhook_id) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let rows = match queries_webhook::fetch_webhook_logs(pool, &slug, webhook_id).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let items: Vec<Value> = rows
        .iter()
        .map(|row| render_webhook_log(row, &ctx.timezone))
        .collect();
    json_response(StatusCode::OK, Value::Array(items).to_string())
}

// ---------------------------------------------------------------------------
// Bodies and field validation
// ---------------------------------------------------------------------------

/// Parse the request body the way DRF does for JSON posts: empty → `{}`;
/// malformed → `ParseError` 400 (`JSONParser`: `"JSON parse error - …"`);
/// non-object JSON → the `to_internal_value` mapping failure 400.
#[allow(clippy::result_large_err)]
fn parse_body(raw: &[u8]) -> Result<Map<String, Value>, Response> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => {
            let datatype = match &other {
                Value::Null => "NoneType",
                Value::Bool(_) => "bool",
                Value::Number(_) => {
                    if other.is_i64() || other.is_u64() {
                        "int"
                    } else {
                        "float"
                    }
                }
                Value::String(_) => "str",
                Value::Array(_) => "list",
                Value::Object(_) => "dict",
            };
            Err(field_errors(single_field_error(
                "non_field_errors",
                &format!("Invalid data. Expected a dictionary, but got {datatype}."),
            )))
        }
        Err(error) => {
            let mut map = Map::new();
            map.insert(
                "detail".to_owned(),
                Value::String(format!("JSON parse error - {error}")),
            );
            Err(Denial::BadJson(Value::Object(map)).into_response())
        }
    }
}

/// One validated write body. Every field is optional here: POST fills the
/// model defaults afterwards, PATCH skips absent keys (`partial=True`).
#[derive(Debug, Default)]
struct WriteInput {
    url: Option<String>,
    is_active: Option<bool>,
    project: Option<bool>,
    issue: Option<bool>,
    module: Option<bool>,
    cycle: Option<bool>,
    issue_comment: Option<bool>,
    is_internal: Option<bool>,
    version: Option<String>,
}

impl WriteInput {
    fn is_active_opt(&self) -> Option<bool> {
        self.is_active
    }
    fn project_opt(&self) -> Option<bool> {
        self.project
    }
    fn issue_opt(&self) -> Option<bool> {
        self.issue
    }
    fn module_opt(&self) -> Option<bool> {
        self.module
    }
    fn cycle_opt(&self) -> Option<bool> {
        self.cycle
    }
    fn issue_comment_opt(&self) -> Option<bool> {
        self.issue_comment
    }
    fn is_internal_opt(&self) -> Option<bool> {
        self.is_internal
    }
    fn version_opt(&self) -> Option<String> {
        self.version.clone()
    }
}

/// Coerced JSON scalar for `CharField` inputs (`url`, `version`): DRF
/// `CharField.to_internal_value` accepts strings and numbers (str()'d)
/// and rejects booleans and structured values with "Not a valid string."
fn coerce_string(value: &Value) -> Result<String, &'static str> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        _ => Err("Not a valid string."),
    }
}

/// `BooleanField.to_internal_value` (DRF 3.15 exact sets): JSON booleans
/// verbatim; 1/1.0 and 0/0.0; the documented string spellings;
/// everything else fails with "Must be a valid boolean."
fn coerce_bool(value: &Value) -> Result<bool, &'static str> {
    const TRUTHY: &[&str] = &[
        "t", "T", "y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", "on", "On", "ON", "1",
    ];
    const FALSY: &[&str] = &[
        "f", "F", "n", "N", "no", "No", "NO", "false", "False", "FALSE", "off", "Off", "OFF", "0",
    ];
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_f64() == Some(1.0) {
                Ok(true)
            } else if number.as_i64() == Some(0) || number.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err("Must be a valid boolean.")
            }
        }
        Value::String(text) => {
            if TRUTHY.contains(&text.as_str()) {
                Ok(true)
            } else if FALSY.contains(&text.as_str()) {
                Ok(false)
            } else {
                Err("Must be a valid boolean.")
            }
        }
        _ => Err("Must be a valid boolean."),
    }
}

/// Validate one boolean field: absent → `None` (caller defaults or
/// skips); explicit null → "may not be null" (`allow_null=False`).
fn validate_bool_field(
    data: &Map<String, Value>,
    key: &str,
    errors: &mut Map<String, Value>,
) -> Option<bool> {
    let value = data.get(key)?;
    if value.is_null() {
        errors.insert(
            key.to_owned(),
            Value::Array(vec![Value::String(
                "This field may not be null.".to_owned(),
            )]),
        );
        return None;
    }
    match coerce_bool(value) {
        Ok(flag) => Some(flag),
        Err(message) => {
            errors.insert(
                key.to_owned(),
                Value::Array(vec![Value::String(message.to_owned())]),
            );
            None
        }
    }
}

/// Validate the `version` `CharField` (`max_length=50`, `allow_blank`
/// default `False`): absent → `None`; null → null error; blank (after
/// DRF's strip) fails fast with the blank error; then max-length and the
/// NUL check.
fn validate_version_field(
    data: &Map<String, Value>,
    errors: &mut Map<String, Value>,
) -> Option<String> {
    let value = data.get("version")?;
    if value.is_null() {
        errors.insert(
            "version".to_owned(),
            Value::Array(vec![Value::String(
                "This field may not be null.".to_owned(),
            )]),
        );
        return None;
    }
    let text = match coerce_string(value) {
        Ok(text) => text,
        Err(message) => {
            errors.insert(
                "version".to_owned(),
                Value::Array(vec![Value::String(message.to_owned())]),
            );
            return None;
        }
    };
    // `trim_whitespace=True`: same strip the URL chain uses.
    let stripped = text
        .trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
        .to_owned();
    if stripped.is_empty() {
        errors.insert(
            "version".to_owned(),
            Value::Array(vec![Value::String(
                "This field may not be blank.".to_owned(),
            )]),
        );
        return None;
    }
    let mut field_errors = Vec::new();
    if stripped.chars().count() > 50 {
        field_errors.push("Ensure this field has no more than 50 characters.".to_owned());
    }
    if stripped.contains('\0') {
        field_errors.push("Null characters are not allowed.".to_owned());
    }
    if field_errors.is_empty() {
        Some(stripped)
    } else {
        errors.insert(
            "version".to_owned(),
            Value::Array(field_errors.into_iter().map(Value::String).collect()),
        );
        None
    }
}

/// Field level for POST (`partial=false`: `url` required) and PATCH
/// (`partial=true`: absent keys skipped), in serializer field order —
/// the `errors` dict preserves it. `url` goes through the shared
/// `validate_url_field` chain; the guards run afterwards in the handler.
/// The remaining fields mirror their DRF field types. Unknown and
/// read-only keys (`workspace`, `secret_key`, `deleted_at`, `id`, audit
/// datetimes, and client-sent `created_by`/`updated_by`) are ignored:
/// `to_internal_value` only iterates writable fields, and `save()`
/// overwrites the audit pair from CRUM anyway.
fn validate_write_fields(
    data: &Map<String, Value>,
    partial: bool,
) -> Result<WriteInput, Map<String, Value>> {
    let mut errors = Map::new();
    // `url` first (serializer field order), then the flags and `version`.
    let url = validate_url_presence(data, partial, &mut errors);
    let mut input = WriteInput {
        url,
        ..WriteInput::default()
    };
    input.is_active = validate_bool_field(data, "is_active", &mut errors);
    input.project = validate_bool_field(data, "project", &mut errors);
    input.issue = validate_bool_field(data, "issue", &mut errors);
    input.module = validate_bool_field(data, "module", &mut errors);
    input.cycle = validate_bool_field(data, "cycle", &mut errors);
    input.issue_comment = validate_bool_field(data, "issue_comment", &mut errors);
    input.is_internal = validate_bool_field(data, "is_internal", &mut errors);
    input.version = validate_version_field(data, &mut errors);
    if errors.is_empty() {
        Ok(input)
    } else {
        Err(errors)
    }
}

/// Presence + field chain for `url`: absent on POST → required (via the
/// shared chain's `None`); absent on PATCH → skipped (`None`, no error);
/// explicit null → null error; numbers str() through `CharField`;
/// bools/structured values → "Not a valid string."
fn validate_url_presence(
    data: &Map<String, Value>,
    partial: bool,
    errors: &mut Map<String, Value>,
) -> Option<String> {
    let Some(value) = data.get("url") else {
        if partial {
            return None;
        }
        match pidash_services::app_integrations::serializers::validate_url_field(None) {
            Ok(_) => return None,
            Err(messages) => {
                errors.insert(
                    "url".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
                return None;
            }
        }
    };
    if value.is_null() {
        match pidash_services::app_integrations::serializers::validate_url_field(Some(None)) {
            Ok(_) => return None,
            Err(messages) => {
                errors.insert(
                    "url".to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
                return None;
            }
        }
    }
    let text = match coerce_string(value) {
        Ok(text) => text,
        Err(message) => {
            errors.insert(
                "url".to_owned(),
                Value::Array(vec![Value::String(message.to_owned())]),
            );
            return None;
        }
    };
    match pidash_services::app_integrations::serializers::validate_url_field(Some(Some(&text))) {
        Ok(trimmed) => Some(trimmed),
        Err(messages) => {
            errors.insert(
                "url".to_owned(),
                Value::Array(messages.into_iter().map(Value::String).collect()),
            );
            None
        }
    }
}

fn single_field_error(key: &str, message: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        key.to_owned(),
        Value::Array(vec![Value::String(message.to_owned())]),
    );
    map
}

fn field_errors(errors: Map<String, Value>) -> Response {
    Denial::BadJson(Value::Object(errors)).into_response()
}

fn guard_error(error: WebhookUrlError) -> Response {
    Denial::BadJson(error.body()).into_response()
}

/// `create()` guard chain with the request host appended
/// (`serializers/webhook.py:44-53`), run off the async executor:
/// `socket.getaddrinfo` blocks in Python too.
async fn run_create_guards(
    url: &str,
    request_host: Option<&str>,
) -> Result<String, WebhookUrlError> {
    let url = url.to_owned();
    let request_host = request_host.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let resolver = SystemResolver;
        validate_create_url(Some(Some(url.as_str())), request_host.as_deref(), &resolver)
    })
    .await
    .map_err(|_| WebhookUrlError::Guard("Hostname could not be resolved.".to_owned()))?
}

/// `update()` guard chain (`serializers/webhook.py:61-88`) with BUG B2's
/// context: the request host is never appended (`base.py:84`).
async fn run_update_guards(url: &str) -> Result<String, WebhookUrlError> {
    let url = url.to_owned();
    tokio::task::spawn_blocking(move || {
        let resolver = SystemResolver;
        match validate_update_url(Some(Some(url.as_str())), None, &resolver) {
            Ok(Some(trimmed)) => Ok(trimmed),
            Ok(None) => Err(WebhookUrlError::Guard(
                "Invalid URL: No hostname found.".to_owned(),
            )),
            Err(error) => Err(error),
        }
    })
    .await
    .map_err(|_| WebhookUrlError::Guard("Hostname could not be resolved.".to_owned()))?
}

// ---------------------------------------------------------------------------
// Database failures
// ---------------------------------------------------------------------------

/// True when the failure is the duplicate-URL branch: a unique violation
/// (`base.py:31` tests `"already exists" in str(e)`, and Postgres renders
/// every unique violation with a `Key (…) already exists` detail, which
/// is what psycopg's `str` matches on). The webhooks INSERT can only
/// violate the workspace+url uniqueness (the partial unique; the legacy
/// `unique_together` never fires for live rows since `deleted_at` NULLs
/// are distinct), so any unique violation here IS the duplicate-URL case.
/// (`sqlx` exposes no `detail()` on the error trait, and its `Display`
/// renders the message only — hence the kind check instead of a string
/// match.)
fn is_duplicate_url(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db_error) = error else {
        return false;
    };
    db_error.is_unique_violation()
}

/// Map an INSERT failure onto the POST branches (`base.py:30-36`):
/// duplicate URL → 409; any other integrity failure → the
/// `handle_exception` 400 branch (the re-raised `IntegrityError` lands
/// there — the fixture's "500" note contradicts the code); transport
/// failures → 500.
fn insert_error(error: sqlx::Error) -> Response {
    if is_duplicate_url(&error) {
        return Denial::Conflict.into_response();
    }
    update_error(error)
}

/// Map a write failure onto the `handle_exception` matrix: integrity
/// violations (SQLSTATE 23xxx) → 400 `{"error": "The payload is not
/// valid"}`; anything else → the 500 envelope (the D-02 `space`
/// `db_error` precedent).
fn update_error(error: sqlx::Error) -> Response {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref().unwrap_or("").starts_with("23") {
            return json_response(
                StatusCode::BAD_REQUEST,
                super::INVALID_PAYLOAD_BODY.to_owned(),
            );
        }
    }
    Denial::ServerError.into_response()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn opt_uuid(value: &Option<Uuid>) -> Value {
    match value {
        Some(id) => Value::String(id.to_string()),
        None => Value::Null,
    }
}

fn opt_datetime<Tz: chrono::TimeZone>(value: &Option<DateTime<Tz>>, zone: &chrono_tz::Tz) -> Value
where
    Tz::Offset: std::fmt::Display,
{
    match value {
        Some(dt) => Value::String(crate::serializer::render_datetime_in(dt, zone)),
        None => Value::Null,
    }
}

fn opt_string(value: &Option<String>) -> Value {
    match value {
        Some(text) => Value::String(text.clone()),
        None => Value::Null,
    }
}

/// `WebhookSerializer(webhook).data`: the full `__all__` shape (the
/// `fields=(...)` kwarg is discarded by `DynamicBaseSerializer`), in DRF
/// `get_default_field_names` order — declared fields first (`id` from
/// `BaseSerializer`, `url` from `WebhookSerializer`), then the concrete
/// model fields, then the forward relations (`created_by`, `updated_by`,
/// `workspace`, which DRF appends after `model_info.fields`). Verified
/// byte order against live Django.
/// Datetimes render in the request's zone (`TimezoneMixin`); FKs render
/// as pk strings (`PrimaryKeyRelatedField`).
fn render_webhook(row: &Webhook, zone: &chrono_tz::Tz) -> Value {
    let mut map = Map::with_capacity(17);
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert("url".to_owned(), Value::String(row.url.clone()));
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&row.created_at, zone)),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&row.updated_at, zone)),
    );
    map.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at, zone));
    map.insert("is_active".to_owned(), Value::Bool(row.is_active));
    map.insert(
        "secret_key".to_owned(),
        Value::String(row.secret_key.clone()),
    );
    map.insert("project".to_owned(), Value::Bool(row.project));
    map.insert("issue".to_owned(), Value::Bool(row.issue));
    map.insert("module".to_owned(), Value::Bool(row.module));
    map.insert("cycle".to_owned(), Value::Bool(row.cycle));
    map.insert("issue_comment".to_owned(), Value::Bool(row.issue_comment));
    map.insert("is_internal".to_owned(), Value::Bool(row.is_internal));
    map.insert("version".to_owned(), Value::String(row.version.clone()));
    map.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    map.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    map.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    Value::Object(map)
}

/// `WebhookLogSerializer(log).data`: full `__all__` shape in the same
/// DRF order — no declared fields, so concrete fields first (`webhook`
/// is a plain UUID column, not a relation), then the forward relations.
/// `response_status` stays text (ported BUG: mixed types).
fn render_webhook_log(row: &WebhookLog, zone: &chrono_tz::Tz) -> Value {
    let mut map = Map::with_capacity(16);
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&row.created_at, zone)),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&row.updated_at, zone)),
    );
    map.insert("deleted_at".to_owned(), opt_datetime(&row.deleted_at, zone));
    map.insert("webhook".to_owned(), Value::String(row.webhook.to_string()));
    map.insert("event_type".to_owned(), opt_string(&row.event_type));
    map.insert("request_method".to_owned(), opt_string(&row.request_method));
    map.insert(
        "request_headers".to_owned(),
        opt_string(&row.request_headers),
    );
    map.insert("request_body".to_owned(), opt_string(&row.request_body));
    map.insert(
        "response_status".to_owned(),
        opt_string(&row.response_status),
    );
    map.insert(
        "response_headers".to_owned(),
        opt_string(&row.response_headers),
    );
    map.insert("response_body".to_owned(), opt_string(&row.response_body));
    map.insert(
        "retry_count".to_owned(),
        Value::Number(row.retry_count.into()),
    );
    map.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    map.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    map.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// `soft_delete_related_objects.delay("db", "webhook", pk, "default")`
/// (`db/mixins.py:77-78`): positional args, no kwargs. Best-effort after
/// commit — without it the response still stands.
async fn enqueue_soft_delete(pool: &PgPool, webhook_id: &Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("webhook".to_owned()),
            Value::String(webhook_id.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn body(data: serde_json::Value) -> Map<String, Value> {
        match data {
            Value::Object(map) => map,
            _ => panic!("object body"),
        }
    }

    fn error_body(errors: Map<String, Value>) -> Value {
        Value::Object(errors)
    }

    // -- parse_body ---------------------------------------------------------

    #[test]
    fn empty_body_is_empty_object() {
        assert!(parse_body(b"").expect("empty").is_empty());
    }

    #[test]
    fn object_body_passes_through() {
        let map = parse_body(br#"{"url":"https://example.com/hook"}"#).expect("object");
        assert_eq!(map.get("url").expect("url"), "https://example.com/hook");
    }

    // -- validate_write_fields: url presence --------------------------------

    #[test]
    fn post_missing_url_is_required() {
        let Err(errors) = validate_write_fields(&body(serde_json::json!({})), false) else {
            panic!("must fail");
        };
        assert_eq!(
            error_body(errors),
            serde_json::json!({"url": ["This field is required."]})
        );
    }

    #[test]
    fn patch_missing_url_is_skipped() {
        let input = validate_write_fields(&body(serde_json::json!({"is_active": false})), true)
            .expect("partial skips url");
        assert!(input.url.is_none());
        assert_eq!(input.is_active, Some(false));
    }

    #[test]
    fn explicit_null_url_fails() {
        for partial in [false, true] {
            let Err(errors) =
                validate_write_fields(&body(serde_json::json!({"url": null})), partial)
            else {
                panic!("must fail");
            };
            assert_eq!(
                error_body(errors),
                serde_json::json!({"url": ["This field may not be null."]}),
                "partial={partial}"
            );
        }
    }

    #[test]
    fn bool_url_fails_type_check() {
        let Err(errors) = validate_write_fields(&body(serde_json::json!({"url": true})), false)
        else {
            panic!("must fail");
        };
        assert_eq!(
            error_body(errors),
            serde_json::json!({"url": ["Not a valid string."]})
        );
    }

    // -- validate_write_fields: flags + version ------------------------------

    #[test]
    fn bool_fields_accept_json_booleans_and_defaults() {
        let input = validate_write_fields(
            &body(serde_json::json!({"url": "https://example.com/hook", "issue": true})),
            false,
        )
        .expect("valid");
        assert_eq!(input.issue, Some(true));
        assert_eq!(input.is_active, None);
        assert_eq!(input.project, None);
    }

    #[test]
    fn bool_fields_reject_garbage_and_null() {
        let Err(errors) = validate_write_fields(
            &body(serde_json::json!({"url": "https://example.com/hook", "is_active": "maybe"})),
            false,
        ) else {
            panic!("must fail");
        };
        assert_eq!(
            error_body(errors),
            serde_json::json!({"is_active": ["Must be a valid boolean."]})
        );
        let Err(errors) = validate_write_fields(
            &body(serde_json::json!({"url": "https://example.com/hook", "project": null})),
            false,
        ) else {
            panic!("must fail");
        };
        assert_eq!(
            error_body(errors),
            serde_json::json!({"project": ["This field may not be null."]})
        );
    }

    #[test]
    fn version_validates_length_and_blank() {
        let input = validate_write_fields(
            &body(serde_json::json!({"url": "https://example.com/hook", "version": "v2"})),
            false,
        )
        .expect("valid");
        assert_eq!(input.version.as_deref(), Some("v2"));
        let Err(errors) = validate_write_fields(
            &body(serde_json::json!({"url": "https://example.com/hook", "version": ""})),
            false,
        ) else {
            panic!("must fail");
        };
        assert_eq!(
            error_body(errors),
            serde_json::json!({"version": ["This field may not be blank."]})
        );
        let long = "v".repeat(51);
        let Err(errors) = validate_write_fields(
            &body(serde_json::json!({"url": "https://example.com/hook", "version": long})),
            false,
        ) else {
            panic!("must fail");
        };
        assert_eq!(
            error_body(errors),
            serde_json::json!({"version": ["Ensure this field has no more than 50 characters."]})
        );
    }

    #[test]
    fn unknown_and_read_only_keys_are_ignored() {
        let input = validate_write_fields(
            &body(serde_json::json!({
                "url": "https://example.com/hook",
                "workspace": "00000000-0000-0000-0000-000000000000",
                "secret_key": "forged",
                "nope": 1,
            })),
            false,
        )
        .expect("ignored keys pass");
        assert!(input.url.is_some());
    }

    // -- coerce_bool ----------------------------------------------------------

    #[test]
    fn bool_coercion_matches_drf_sets() {
        assert_eq!(coerce_bool(&serde_json::json!(true)), Ok(true));
        assert_eq!(coerce_bool(&serde_json::json!(false)), Ok(false));
        assert_eq!(coerce_bool(&serde_json::json!(1)), Ok(true));
        assert_eq!(coerce_bool(&serde_json::json!(0)), Ok(false));
        assert_eq!(coerce_bool(&serde_json::json!("True")), Ok(true));
        assert_eq!(coerce_bool(&serde_json::json!("OFF")), Ok(false));
        assert!(coerce_bool(&serde_json::json!("maybe")).is_err());
        assert!(coerce_bool(&serde_json::json!(2)).is_err());
        assert!(coerce_bool(&serde_json::json!([])).is_err());
    }

    // -- request_host_of ------------------------------------------------------

    #[test]
    fn host_header_strips_port() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "example.com:8000".parse().expect("header"));
        assert_eq!(request_host_of(&headers).as_deref(), Some("example.com"));
        assert!(request_host_of(&HeaderMap::new()).is_none());
    }

    // -- rendering ------------------------------------------------------------

    fn sample_webhook() -> Webhook {
        let re_parse = |s: &str| s.parse::<DateTime<Utc>>().expect("dt");
        Webhook {
            id: Uuid::nil(),
            created_at: re_parse("2026-01-01T00:00:00Z"),
            updated_at: re_parse("2026-01-02T00:00:00Z"),
            created_by_id: Some(Uuid::nil()),
            updated_by_id: None,
            deleted_at: None,
            workspace_id: Uuid::nil(),
            url: "https://example.com/hook".to_owned(),
            is_active: true,
            secret_key: "pi_dash_wh_abc".to_owned(),
            project: false,
            issue: true,
            module: false,
            cycle: false,
            issue_comment: false,
            is_internal: false,
            version: "v1".to_owned(),
        }
    }

    #[test]
    fn webhook_renders_full_shape_in_field_order() {
        let value = render_webhook(&sample_webhook(), &chrono_tz::UTC);
        let map = value.as_object().expect("object");
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        // DRF `get_default_field_names`: declared (`id`, `url`), then
        // concrete fields, then forward relations — verified against
        // live Django.
        assert_eq!(
            keys,
            vec![
                "id",
                "url",
                "created_at",
                "updated_at",
                "deleted_at",
                "is_active",
                "secret_key",
                "project",
                "issue",
                "module",
                "cycle",
                "issue_comment",
                "is_internal",
                "version",
                "created_by",
                "updated_by",
                "workspace",
            ]
        );
        assert_eq!(map.get("url").expect("url"), "https://example.com/hook");
        assert_eq!(map.get("issue").expect("issue"), &Value::Bool(true));
        assert_eq!(
            map.get("created_by").expect("actor"),
            "00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(map.get("updated_by").expect("no updater"), &Value::Null);
        assert_eq!(map.get("created_at").expect("dt"), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn webhook_log_renders_full_shape() {
        let re_parse = |s: &str| s.parse::<DateTime<Utc>>().expect("dt");
        let row = WebhookLog {
            id: Uuid::nil(),
            created_at: re_parse("2026-01-01T00:00:00Z"),
            updated_at: re_parse("2026-01-01T00:00:00Z"),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: Uuid::nil(),
            webhook: Uuid::nil(),
            event_type: Some("push".to_owned()),
            request_method: Some("POST".to_owned()),
            request_headers: Some("{}".to_owned()),
            request_body: Some("{}".to_owned()),
            response_status: Some("200".to_owned()),
            response_headers: Some("{}".to_owned()),
            response_body: Some("{}".to_owned()),
            retry_count: 0,
        };
        let value = render_webhook_log(&row, &chrono_tz::UTC);
        let map = value.as_object().expect("object");
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        // No declared fields: concrete fields first, then forward
        // relations — verified against live Django.
        assert_eq!(
            keys,
            vec![
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "webhook",
                "event_type",
                "request_method",
                "request_headers",
                "request_body",
                "response_status",
                "response_headers",
                "response_body",
                "retry_count",
                "created_by",
                "updated_by",
                "workspace",
            ]
        );
        assert_eq!(map.get("event_type").expect("event"), "push");
        assert_eq!(map.get("retry_count").expect("retries"), 0);
        assert_eq!(
            map.get("webhook").expect("hook"),
            "00000000-0000-0000-0000-000000000000"
        );
    }
}
