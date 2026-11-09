#![forbid(unsafe_code)]

//! Runner LLM model catalog (PDASHOSS01-364) — Django-owned table, Rust
//! read/write surface for both route groups.
//!
//! Ports `RunnerModelCatalog` (`apps/api/pi_dash/runner/models.py`) and the
//! two Python views (`runner/views/model_catalog.py` for the picker read and
//! `license/api/views/runner_models.py` for the admin CRUD). The contract is
//! frozen byte-for-byte with the Django side:
//!
//! * `GET /api/runners/model-catalog/` (RunnerWeb, any signed-in user):
//!   `{"agents": {agent: [row,...]}}` with every agent key present, only
//!   `is_enabled` rows, ordered by `sort_order`, `created_at`. Row keys in
//!   declaration order: `id, agent, model, label, reasoning_efforts,
//!   sort_order, is_enabled, is_preselected, created_at, updated_at`.
//! * `GET /api/instances/runner-models/` (License, admin only): flat list
//!   ordered by `agent`, `sort_order`, `created_at`, all rows.
//! * `POST /api/instances/runner-models/` → 201 + row; `PATCH
//!   /api/instances/runner-models/<id>/` → 200 + row; `DELETE …` → 204.
//!   Missing row → 404 `{"error":"not found"}`. Validation and error codes
//!   mirror `RunnerModelCatalogSerializer`.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch},
    Router,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::license::{require_admin, resolve_actor, Denial, UNAUTHENTICATED_BODY};

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}
use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_db::entity::runner_model_catalog;
use pidash_db::pool::sea_orm_connection;

/// The six CLI agent identifiers the catalog is grouped by (kept in code;
/// adding agents is out of scope). Order matches `RUNNER_MODEL_AGENTS`.
pub const RUNNER_AGENTS: [&str; 6] = [
    "claude-code",
    "codex",
    "cursor-agent",
    "open-claw",
    "grok",
    "muse-code",
];

/// The reasoning-effort vocabulary the catalog accepts. Codex rows use the
/// full set; other agents store an empty list.
pub const ALLOWED_REASONING_EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];

const PUBLIC_PATH: &str = "/api/runners/model-catalog/";
const ADMIN_LIST_PATH: &str = "/api/instances/runner-models/";
const ADMIN_DETAIL_PATH: &str = "/api/instances/runner-models/{id}/";

fn is_known_agent(agent: &str) -> bool {
    RUNNER_AGENTS.contains(&agent)
}

// ---------------------------------------------------------------------------
// Row shape (declaration order is the wire order)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct CatalogRow {
    id: String,
    agent: String,
    model: String,
    label: String,
    reasoning_efforts: Vec<String>,
    sort_order: i32,
    is_enabled: bool,
    is_preselected: bool,
    created_at: String,
    updated_at: String,
}

fn parse_efforts(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn render_row(row: &runner_model_catalog::Model, timezone: &chrono_tz::Tz) -> CatalogRow {
    CatalogRow {
        id: row.id.to_string(),
        agent: row.agent.clone(),
        model: row.model.clone(),
        label: row.label.clone(),
        reasoning_efforts: parse_efforts(&row.reasoning_efforts),
        sort_order: row.sort_order,
        is_enabled: row.is_enabled,
        is_preselected: row.is_preselected,
        created_at: crate::serializer::render_datetime_in(&row.created_at, timezone),
        updated_at: crate::serializer::render_datetime_in(&row.updated_at, timezone),
    }
}

fn json_body(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .expect("catalog json response")
}

/// DRF field-error envelope: `{"field": ["msg"]}` in insertion order.
fn field_error(field: &str, message: &str) -> Response {
    json_body(StatusCode::BAD_REQUEST, json!({ field: [message] }))
}

fn detail_error(errors: Value) -> Response {
    json_body(StatusCode::BAD_REQUEST, errors)
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct CatalogWrite {
    agent: Option<Value>,
    model: Option<Value>,
    label: Option<Value>,
    reasoning_efforts: Option<Value>,
    sort_order: Option<Value>,
    is_enabled: Option<Value>,
    is_preselected: Option<Value>,
}

/// Extract a trimmed required string (`trim_whitespace` + non-empty).
/// DRF CharField `allow_blank=False` rejects `""` after trim; a
/// whitespace-only string becomes `""` and is likewise rejected.
fn required_string(value: Option<&Value>) -> Result<String, &'static str> {
    match value {
        None | Some(Value::Null) => Err("blank"),
        Some(Value::String(s)) => {
            let trimmed = s.trim().to_owned();
            if trimmed.is_empty() {
                Err("blank")
            } else {
                Ok(trimmed)
            }
        }
        Some(other) => {
            // Non-string scalars stringify like DRF CharField `to_python`;
            // numbers/bools are accepted as their `str`, composites as `repr`.
            let text = match other {
                Value::Bool(true) => "True".to_owned(),
                Value::Bool(false) => "False".to_owned(),
                Value::Number(n) => n.to_string(),
                Value::Array(_) | Value::Object(_) => {
                    serde_json::to_string(other).unwrap_or_default()
                }
                Value::String(s) => s.clone(),
                Value::Null => return Err("blank"),
            };
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Err("blank")
            } else {
                Ok(trimmed.to_owned())
            }
        }
    }
}

#[allow(clippy::result_large_err)]
fn parse_efforts_request(
    value: Option<&Value>,
    required: bool,
) -> Result<Option<Vec<String>>, Response> {
    match value {
        None => {
            if required {
                Ok(Some(Vec::new()))
            } else {
                Ok(None)
            }
        }
        Some(Value::Null) => Ok(Some(Vec::new())),
        Some(Value::Array(items)) => {
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let Some(s) = item.as_str() else {
                    return Err(field_error(
                        "reasoning_efforts",
                        "reasoning_efforts entries must be strings",
                    ));
                };
                if !ALLOWED_REASONING_EFFORTS.contains(&s) {
                    let allowed = ALLOWED_REASONING_EFFORTS.join(", ");
                    return Err(field_error(
                        "reasoning_efforts",
                        &format!("reasoning_efforts entries must be one of: {allowed}"),
                    ));
                }
                if !seen.insert(s.to_owned()) {
                    return Err(field_error(
                        "reasoning_efforts",
                        "reasoning_efforts entries must be unique",
                    ));
                }
                out.push(s.to_owned());
            }
            Ok(Some(out))
        }
        Some(_) => Err(field_error(
            "reasoning_efforts",
            "reasoning_efforts must be a list",
        )),
    }
}

#[allow(clippy::result_large_err)]
fn parse_sort_order(value: Option<&Value>) -> Result<Option<i32>, Response> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                if i < 0 {
                    return Err(field_error(
                        "sort_order",
                        "Ensure this value is greater than or equal to 0.",
                    ));
                }
                if i > i32::MAX as i64 {
                    return Err(field_error(
                        "sort_order",
                        "Ensure this value is less than or equal to 2147483647.",
                    ));
                }
                Ok(Some(i as i32))
            } else {
                Err(field_error("sort_order", "A valid integer is required."))
            }
        }
        Some(Value::String(s)) => match s.parse::<i64>() {
            Ok(i) if i >= 0 && i <= i32::MAX as i64 => Ok(Some(i as i32)),
            Ok(_) => Err(field_error(
                "sort_order",
                "Ensure this value is greater than or equal to 0.",
            )),
            Err(_) => Err(field_error("sort_order", "A valid integer is required.")),
        },
        Some(_) => Err(field_error("sort_order", "A valid integer is required.")),
    }
}

#[allow(clippy::result_large_err)]
fn parse_bool_field(value: Option<&Value>, field: &'static str) -> Result<Option<bool>, Response> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(field_error(field, "Must be a valid boolean.")),
    }
}

fn decode_body(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return json!({});
    }
    match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => json!({}),
    }
}

// ---------------------------------------------------------------------------
// Public catalog — GET /api/runners/model-catalog/
// ---------------------------------------------------------------------------

/// Register the picker source under `RunnerWeb`. Any signed-in user.
pub fn public_routes() -> Router<AppState> {
    Router::new().route(
        PUBLIC_PATH,
        crate::license::owned(get(public_catalog), &["GET"]),
    )
}

async fn public_catalog(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let actor = {
        let pool = match pool_of(&state) {
            Ok(p) => p.clone(),
            Err(d) => return d.into_response(),
        };
        match resolve_actor(&pool, state.settings().secret_key.as_bytes(), extension).await {
            Ok(Some(a)) => a,
            Ok(None) => {
                return Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(UNAUTHENTICATED_BODY))
                    .expect("401 catalog")
            }
            Err(e) => return e.into_response(),
        }
    };
    let pool = match pool_of(&state) {
        Ok(p) => p,
        Err(d) => return d.into_response(),
    };
    let db = sea_orm_connection(&pool);
    let rows = match runner_model_catalog::Entity::find()
        .filter(runner_model_catalog::Column::IsEnabled.eq(true))
        .order_by(runner_model_catalog::Column::Agent, sea_orm::Order::Asc)
        .order_by(runner_model_catalog::Column::SortOrder, sea_orm::Order::Asc)
        .order_by(runner_model_catalog::Column::CreatedAt, sea_orm::Order::Asc)
        .all(&db)
        .await
    {
        Ok(r) => r,
        Err(_) => return server_error(),
    };
    // Group with every agent key present, in RUNNER_AGENTS order.
    let mut grouped: serde_json::Map<String, Value> = serde_json::Map::new();
    for agent in RUNNER_AGENTS {
        grouped.insert(agent.to_owned(), json!([]));
    }
    for row in &rows {
        let rendered = render_row(row, &actor.timezone);
        let key = rendered.agent.clone();
        if let Some(Value::Array(items)) = grouped.get_mut(&key) {
            items.push(serde_json::to_value(&rendered).expect("row value"));
        }
    }
    json_body(StatusCode::OK, json!({ "agents": grouped }))
}

fn server_error() -> Response {
    let _ = Denial::ServerError;
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(r#"{"error":"server error"}"#))
        .expect("500")
}

// ---------------------------------------------------------------------------
// Admin CRUD — /api/instances/runner-models/
// ---------------------------------------------------------------------------

/// Register the admin list/create under `License`.
pub fn admin_list_routes() -> Router<AppState> {
    Router::new().route(
        ADMIN_LIST_PATH,
        crate::license::owned(get(admin_list).post(admin_create), &["GET", "POST"]),
    )
}

/// Register the admin detail under `License`.
pub fn admin_detail_routes() -> Router<AppState> {
    Router::new().route(
        ADMIN_DETAIL_PATH,
        crate::license::owned(
            patch(admin_patch).delete(admin_delete),
            &["PATCH", "DELETE"],
        ),
    )
}

async fn admin_list(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let actor = match require_admin(&state, extension).await {
        Ok(a) => a,
        Err(d) => return d.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(p) => p.clone(),
        Err(d) => return d.into_response(),
    };
    let db = sea_orm_connection(&pool);
    let rows = match runner_model_catalog::Entity::find()
        .order_by(runner_model_catalog::Column::Agent, sea_orm::Order::Asc)
        .order_by(runner_model_catalog::Column::SortOrder, sea_orm::Order::Asc)
        .order_by(runner_model_catalog::Column::CreatedAt, sea_orm::Order::Asc)
        .all(&db)
        .await
    {
        Ok(r) => r,
        Err(_) => return server_error(),
    };
    let out: Vec<Value> = rows
        .iter()
        .map(|r| serde_json::to_value(render_row(r, &actor.timezone)).expect("row value"))
        .collect();
    json_body(StatusCode::OK, json!(out))
}

async fn admin_create(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let actor = match require_admin(&state, extension).await {
        Ok(a) => a,
        Err(d) => return d.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(p) => p.clone(),
        Err(d) => return d.into_response(),
    };
    // Same body gate as the DRF view: a non-object body falls through to
    // serializer errors, not a 500 — decode leniently and let validation
    // speak. Content-type check mirrors DRF's media-type tolerance.
    let raw = decode_body(&body);
    // Empty body with a JSON content-type is an empty dict (no fields).
    let _ = headers;
    if !raw.is_object() && !raw.is_null() {
        return detail_error(json!({
            "model": ["This field is required."],
            "agent": ["This field is required."],
            "label": ["This field is required."],
        }));
    }
    let v = raw.as_object().cloned().unwrap_or_default();
    let get = |k: &str| v.get(k);

    // agent (required, known)
    let agent_raw = get("agent");
    let agent = match agent_raw {
        None | Some(Value::Null) => {
            return field_error("agent", "This field is required.");
        }
        Some(Value::String(s)) => {
            if !is_known_agent(s) {
                return field_error("agent", &format!("\"{s}\" is not a valid choice."));
            }
            s.clone()
        }
        Some(other) => {
            let s = other.as_str().unwrap_or("").to_owned();
            if !is_known_agent(&s) {
                return field_error("agent", &format!("\"{s}\" is not a valid choice."));
            }
            s
        }
    };
    let model = match required_string(get("model")) {
        Ok(m) => m,
        Err(_) => return field_error("model", "This field may not be blank."),
    };
    let label = match required_string(get("label")) {
        Ok(l) => l,
        Err(_) => return field_error("label", "This field may not be blank."),
    };
    let reasoning_efforts = match parse_efforts_request(get("reasoning_efforts"), true) {
        Ok(v) => v.unwrap_or_default(),
        Err(resp) => return resp,
    };
    let sort_order = match parse_sort_order(get("sort_order")) {
        Ok(v) => v.unwrap_or(0),
        Err(resp) => return resp,
    };
    let is_enabled = match parse_bool_field(get("is_enabled"), "is_enabled") {
        Ok(v) => v.unwrap_or(true),
        Err(resp) => return resp,
    };
    let is_preselected = match parse_bool_field(get("is_preselected"), "is_preselected") {
        Ok(v) => v.unwrap_or(false),
        Err(resp) => return resp,
    };

    let db = sea_orm_connection(&pool);
    // Duplicate (agent, model) → 400. Check first for a friendly field
    // error; the DB partial unique is the backstop (→ 400, never 500).
    let dup = match runner_model_catalog::Entity::find()
        .filter(runner_model_catalog::Column::Agent.eq(agent.clone()))
        .filter(runner_model_catalog::Column::Model.eq(model.clone()))
        .one(&db)
        .await
    {
        Ok(r) => r,
        Err(_) => return server_error(),
    };
    if dup.is_some() {
        return field_error("model", "A model with this agent and model already exists.");
    }

    let now = chrono::Utc::now().fixed_offset();
    let id = Uuid::new_v4();
    let created_at = now;
    let updated_at = now;
    let efforts_json = json!(reasoning_efforts);

    // Same-transaction preselect clearing + insert.
    let txn = match db.begin().await {
        Ok(t) => t,
        Err(_) => return server_error(),
    };
    if is_preselected {
        let _ = runner_model_catalog::Entity::update_many()
            .col_expr(
                runner_model_catalog::Column::IsPreselected,
                sea_orm::prelude::Expr::value(false),
            )
            .filter(runner_model_catalog::Column::Agent.eq(agent.clone()))
            .filter(runner_model_catalog::Column::IsPreselected.eq(true))
            .exec(&txn)
            .await;
    }
    let insert = runner_model_catalog::ActiveModel {
        id: Set(id),
        agent: Set(agent),
        model: Set(model),
        label: Set(label),
        reasoning_efforts: Set(efforts_json),
        sort_order: Set(sort_order),
        is_enabled: Set(is_enabled),
        is_preselected: Set(is_preselected),
        created_at: Set(created_at),
        updated_at: Set(updated_at),
    };
    let created = match insert.insert(&txn).await {
        Ok(r) => r,
        Err(e) => {
            let _ = txn.rollback().await;
            if is_duplicate_error(&e) {
                return field_error("model", "A model with this agent and model already exists.");
            }
            return server_error();
        }
    };
    let _ = txn.commit().await;
    json_body(
        StatusCode::CREATED,
        serde_json::to_value(render_row(&created, &actor.timezone)).expect("row"),
    )
}

async fn admin_patch(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let actor = match require_admin(&state, extension).await {
        Ok(a) => a,
        Err(d) => return d.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(p) => p.clone(),
        Err(d) => return d.into_response(),
    };
    let row_id = match id.parse::<Uuid>() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };
    let db = sea_orm_connection(&pool);
    let existing = match runner_model_catalog::Entity::find_by_id(row_id)
        .one(&db)
        .await
    {
        Ok(r) => r,
        Err(_) => return server_error(),
    };
    let Some(model) = existing else {
        return not_found();
    };
    let raw = decode_body(&body);
    let obj = match raw.as_object() {
        Some(m) => m.clone(),
        None => {
            return field_error(
                "detail",
                "Invalid data. Expected a dictionary, but got a list.",
            )
        }
    };
    let get = |k: &str| obj.get(k);

    // Agent if present.
    let mut new_agent = model.agent.clone();
    if let Some(v) = get("agent") {
        if v.is_null() {
            return field_error("agent", "This field may not be null.");
        }
        let s = v.as_str().unwrap_or("").to_owned();
        if !is_known_agent(&s) {
            return field_error("agent", &format!("\"{s}\" is not a valid choice."));
        }
        new_agent = s;
    }
    let mut new_model = model.model.clone();
    if let Some(v) = get("model") {
        if v.is_null() {
            return field_error("model", "This field may not be null.");
        }
        match required_string(Some(v)) {
            Ok(m) => new_model = m,
            Err(_) => return field_error("model", "This field may not be blank."),
        }
    }
    let mut new_label = model.label.clone();
    if let Some(v) = get("label") {
        if v.is_null() {
            return field_error("label", "This field may not be null.");
        }
        match required_string(Some(v)) {
            Ok(l) => new_label = l,
            Err(_) => return field_error("label", "This field may not be blank."),
        }
    }
    // reasoning_efforts: PATCH with an explicit null clears to [] (Django JSONField).
    let mut new_efforts: Option<Vec<String>> = None;
    if let Some(v) = get("reasoning_efforts") {
        if v.is_null() {
            new_efforts = Some(Vec::new());
        } else {
            match parse_efforts_request(Some(v), false) {
                Ok(val) => new_efforts = val,
                Err(resp) => return resp,
            }
        }
    }
    let mut new_sort: Option<i32> = None;
    if get("sort_order").is_some() {
        match parse_sort_order(get("sort_order")) {
            Ok(v) => new_sort = v,
            Err(resp) => return resp,
        }
    }
    let mut new_enabled: Option<bool> = None;
    if let Some(v) = get("is_enabled") {
        if v.is_null() {
            return field_error("is_enabled", "This field may not be null.");
        }
        match parse_bool_field(Some(v), "is_enabled") {
            Ok(val) => new_enabled = val,
            Err(resp) => return resp,
        }
    }
    let mut new_presel: Option<bool> = None;
    if let Some(v) = get("is_preselected") {
        if v.is_null() {
            return field_error("is_preselected", "This field may not be null.");
        }
        match parse_bool_field(Some(v), "is_preselected") {
            Ok(val) => new_presel = val,
            Err(resp) => return resp,
        }
    }

    // Duplicate check on the final (agent, model) when it would collide.
    let final_agent = new_agent.clone();
    let final_model = new_model.clone();
    let dup = match runner_model_catalog::Entity::find()
        .filter(runner_model_catalog::Column::Agent.eq(final_agent))
        .filter(runner_model_catalog::Column::Model.eq(final_model))
        .filter(runner_model_catalog::Column::Id.ne(model.id))
        .one(&db)
        .await
    {
        Ok(r) => r,
        Err(_) => return server_error(),
    };
    if dup.is_some() {
        return field_error("model", "A model with this agent and model already exists.");
    }

    let now = chrono::Utc::now().fixed_offset();
    let want_preselect = new_presel.unwrap_or(model.is_preselected);
    let agent_for_clear = new_agent.clone();
    let txn = match db.begin().await {
        Ok(t) => t,
        Err(_) => return server_error(),
    };
    if want_preselect {
        let _ = runner_model_catalog::Entity::update_many()
            .col_expr(
                runner_model_catalog::Column::IsPreselected,
                sea_orm::prelude::Expr::value(false),
            )
            .filter(runner_model_catalog::Column::Agent.eq(agent_for_clear))
            .filter(runner_model_catalog::Column::Id.ne(model.id))
            .filter(runner_model_catalog::Column::IsPreselected.eq(true))
            .exec(&txn)
            .await;
    }
    // Preserve unchanged columns by using the fetched model as the base.
    let existing_active = runner_model_catalog::ActiveModel {
        id: Set(model.id),
        agent: Set(new_agent.clone()),
        model: Set(new_model),
        label: Set(new_label),
        reasoning_efforts: Set(new_efforts
            .map(|v| json!(v))
            .unwrap_or_else(|| model.reasoning_efforts.clone())),
        sort_order: Set(new_sort.unwrap_or(model.sort_order)),
        is_enabled: Set(new_enabled.unwrap_or(model.is_enabled)),
        is_preselected: Set(new_presel.unwrap_or(model.is_preselected)),
        created_at: Set(model.created_at),
        updated_at: Set(now),
    };
    let updated = match existing_active.update(&txn).await {
        Ok(r) => r,
        Err(e) => {
            let _ = txn.rollback().await;
            if is_duplicate_error(&e) {
                return field_error("model", "A model with this agent and model already exists.");
            }
            // Preselect unique violation (concurrent set) → friendly 400.
            if is_preselect_violation(&e) {
                return field_error(
                    "is_preselected",
                    "Only one model per agent may be preselected.",
                );
            }
            return server_error();
        }
    };
    let _ = txn.commit().await;
    // updated row's datetimes are already in UTC-Tz; re-render in actor zone.
    json_body(
        StatusCode::OK,
        serde_json::to_value(render_row(&updated, &actor.timezone)).expect("row"),
    )
}

async fn admin_delete(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(id): Path<String>,
) -> Response {
    let _actor = match require_admin(&state, extension).await {
        Ok(a) => a,
        Err(d) => return d.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(p) => p.clone(),
        Err(d) => return d.into_response(),
    };
    let row_id = match id.parse::<Uuid>() {
        Ok(v) => v,
        Err(_) => return not_found(),
    };
    let db = sea_orm_connection(&pool);
    match runner_model_catalog::Entity::delete_by_id(row_id)
        .exec(&db)
        .await
    {
        Ok(res) if res.rows_affected > 0 => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .expect("204"),
        Ok(_) => not_found(),
        Err(_) => server_error(),
    }
}

fn not_found() -> Response {
    json_body(StatusCode::NOT_FOUND, json!({ "error": "not found" }))
}

fn is_duplicate_error(err: &sea_orm::DbErr) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("unique") || s.contains("duplicate") || s.contains("runner_model_unique_per_agent")
}

fn is_preselect_violation(err: &sea_orm::DbErr) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("runner_model_one_preselected_per_agent")
}
