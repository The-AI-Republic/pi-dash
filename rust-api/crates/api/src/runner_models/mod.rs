#![forbid(unsafe_code)]

//! Runner LLM model catalog (NEWPIDASH-169).
//!
//! Instance admins manage, per agent, the LLM models the add-runner
//! picker offers. The table is Rust-owned (`rust_runner_model_catalog`,
//! see [`schema`]); the runner itself accepts any model slug, so the
//! catalog is advisory and deleting a row never breaks a runner already
//! using that model.
//!
//! [`catalog`] owns the signed-in read (`GET /api/runners/model-catalog/`,
//! merged into the RunnerWeb group); [`admin`] owns the instance-admin
//! CRUD (`/api/instances/runner-models/`, merged into the License group).

pub mod admin;
pub mod catalog;
pub mod guards;
pub mod schema;

use axum::extract::Extension;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{instance_admins, instances, rust_runner_model_catalog};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::middleware::SessionHandle;
use crate::permissions::DEFAULT_DENIED_BODY;
use crate::state::AppState;

/// DRF `NotAuthenticated` bytes for anonymous callers (same as
/// `crate::space::UNAUTHENTICATED_BODY`; duplicated here so this module
/// stays self-contained).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// Row lookup failure: `{"error":"not_found"}`.
pub const NOT_FOUND_BODY: &str = r#"{"error":"not_found"}"#;
/// Unexpected failures: the generic 500 shape.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"server_error"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub(crate) enum Denial {
    /// 401, DRF `NotAuthenticated` (no valid session).
    Unauthorized,
    /// 403, DRF-default permission denial (not an instance admin).
    Forbidden,
    /// 404 `{"error":"not_found"}`.
    NotFound,
    /// 400 `{"error":"<code>"}` (validation).
    BadRequest(&'static str),
    /// 500, unexpected database failures.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, &'static str) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY),
            Denial::Forbidden => (StatusCode::FORBIDDEN, DEFAULT_DENIED_BODY),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY),
            Denial::BadRequest(_) => (StatusCode::BAD_REQUEST, ""),
            Denial::ServerError => (StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            Denial::BadRequest(code) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({ "error": code }).to_string(),
            ),
            _ => {
                let (status, body) = self.status_and_body();
                (status, body.to_owned())
            }
        };
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

/// Authenticated actor or 401. Delegates to
/// [`crate::license::resolve_actor`], the shared license plumbing
/// (mirrors `SessionAuthentication`); the cookie carried depends on the
/// path (ordinary `session-id` for `/api/runners/`, `admin-session-id`
/// for `/api/instances/`) — the middleware decides.
pub(crate) async fn authed_actor(
    state: &AppState,
    pool: &PgPool,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Uuid, Denial> {
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor.id),
        Ok(None) => Err(Denial::Unauthorized),
        Err(_) => Err(Denial::ServerError),
    }
}

/// `InstanceAdminPermission.has_permission` (same shape as
/// `crate::r#loop::admin::require_instance_admin`): the instance's first
/// row plus an `InstanceAdmin` with `role >= 15` for `(instance, user)`.
pub(crate) async fn require_instance_admin(pool: &PgPool, user_id: &Uuid) -> Result<(), Denial> {
    let db = pidash_db::sea_orm_connection(pool);
    let instance: Option<instances::Model> = instances::Entity::find()
        .filter(instances::Column::DeletedAt.is_null())
        .order_by_desc(instances::Column::CreatedAt)
        .limit(1)
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(instance) = instance else {
        return Err(Denial::Forbidden);
    };
    let found = instance_admins::Entity::find()
        .filter(instance_admins::Column::InstanceId.eq(instance.id))
        .filter(instance_admins::Column::UserId.eq(*user_id))
        .filter(instance_admins::Column::Role.gte(15))
        .filter(instance_admins::Column::DeletedAt.is_null())
        .exists(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    if !found {
        return Err(Denial::Forbidden);
    }
    Ok(())
}

pub(crate) fn pools(state: &AppState) -> Result<PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `SELECT` of the whole catalog, ordered for the picker: agent, then
/// sort order, then model. `enabled_only` filters to
/// `is_enabled = true`.
pub(crate) async fn all_rows(
    pool: &PgPool,
    enabled_only: bool,
) -> Result<Vec<rust_runner_model_catalog::Model>, Denial> {
    let db = pidash_db::sea_orm_connection(pool);
    let mut query = rust_runner_model_catalog::Entity::find();
    if enabled_only {
        query = query.filter(rust_runner_model_catalog::Column::IsEnabled.eq(true));
    }
    query
        .order_by_asc(rust_runner_model_catalog::Column::Agent)
        .order_by_asc(rust_runner_model_catalog::Column::SortOrder)
        .order_by_asc(rust_runner_model_catalog::Column::Model)
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)
}

fn render_timestamp(value: &chrono::DateTime<chrono::FixedOffset>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// One catalog row as rendered on the wire. Key order is fixed.
pub(crate) fn render_row(row: &rust_runner_model_catalog::Model) -> Value {
    let mut map = Map::new();
    map.insert("id".to_owned(), row.id.to_string().into());
    map.insert("agent".to_owned(), row.agent.clone().into());
    map.insert("model".to_owned(), row.model.clone().into());
    map.insert("label".to_owned(), row.label.clone().into());
    map.insert(
        "reasoning_efforts".to_owned(),
        Value::Array(
            row.reasoning_efforts
                .iter()
                .map(|effort| effort.clone().into())
                .collect(),
        ),
    );
    map.insert("sort_order".to_owned(), row.sort_order.into());
    map.insert("is_enabled".to_owned(), row.is_enabled.into());
    map.insert("is_preselected".to_owned(), row.is_preselected.into());
    map.insert(
        "created_at".to_owned(),
        render_timestamp(&row.created_at).into(),
    );
    map.insert(
        "updated_at".to_owned(),
        render_timestamp(&row.updated_at).into(),
    );
    Value::Object(map)
}

/// `{"agents": {<agent>: [rows…]}}` with every known agent key present
/// (empty when it has no rows in the filtered set), rows in picker order.
pub(crate) fn render_grouped(rows: &[rust_runner_model_catalog::Model]) -> Value {
    let mut agents = Map::new();
    for agent in guards::KNOWN_AGENTS {
        let group: Vec<Value> = rows
            .iter()
            .filter(|row| row.agent == agent)
            .map(render_row)
            .collect();
        agents.insert(agent.to_owned(), Value::Array(group));
    }
    let mut root = Map::new();
    root.insert("agents".to_owned(), Value::Object(agents));
    Value::Object(root)
}

pub(crate) fn json_response(status: StatusCode, value: &Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(value.to_string()))
        .expect("static json response")
}

/// Read a request body as JSON. Empty bodies count as `{}`; malformed
/// JSON or non-object JSON is `invalid_body`.
pub(crate) fn parse_json_body(body: &[u8]) -> Result<Value, Denial> {
    if body.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(Value::Object(Map::new()));
    }
    let parsed: Value =
        serde_json::from_slice(body).map_err(|_| Denial::BadRequest("invalid_body"))?;
    if !parsed.is_object() {
        return Err(Denial::BadRequest("invalid_body"));
    }
    Ok(parsed)
}
