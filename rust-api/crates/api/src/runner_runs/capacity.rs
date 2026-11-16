#![allow(clippy::result_large_err)]
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::entity::{agent_run, runner, runner_session};
use pidash_db::orm_tx::TxConn;
use pidash_services::runner_runs::capacity as cap;
use pidash_types::runner_runs::AgentRunStatus;
use sea_orm::{ConnectionTrait, QuerySelect};

use crate::runner_runs::{
    authenticate_daemon, is_uuid_path_segment, json_response, pool_of, server_error, view_response,
    ALLOW_RUN_DAEMON,
};
use crate::state::AppState;
use http_body_util::BodyExt as _;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

fn idempotency_key(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}
fn rfc3339_now_string(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}
fn parse_rfc3339(text: &str) -> Option<DateTime<Utc>> {
    let normalized = text.replace('Z', "+00:00");
    chrono::DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

pub async fn run_capacity(
    State(state): State<AppState>,
    Path(run_id_raw): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    view_response(
        run_capacity_inner(State(state), Path(run_id_raw), req).await,
        ALLOW_RUN_DAEMON,
    )
}

async fn run_capacity_inner(
    State(state): State<AppState>,
    Path(run_id_raw): Path<String>,
    req: Request<axum::body::Body>,
) -> Response {
    if !is_uuid_path_segment(&run_id_raw) {
        return crate::edge::proxy(State(state.clone()), req).await;
    }
    let run_id: Uuid = match run_id_raw.parse() {
        Ok(id) => id,
        Err(_) => {
            return json_response(
                StatusCode::NOT_FOUND,
                r#"{"error":"run_not_found"}"#.to_owned(),
            )
        }
    };
    if let Some(len_str) = req
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
    {
        if let Ok(len) = len_str.trim().parse::<usize>() {
            if len > cap::CAPACITY_BODY_LIMIT_BYTES {
                return json_response(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"body_too_large"}"#.to_owned(),
                );
            }
        }
    }
    let idempotency_key = idempotency_key(req.headers());
    if idempotency_key.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"idempotency_key_required"}"#.to_owned(),
        );
    }
    let pool = match pool_of(&state) {
        Ok(p) => p.clone(),
        Err(resp) => return resp,
    };
    let secret = state.settings().secret_key.clone();
    let runner_opt = match authenticate_daemon(&pool, secret.as_bytes(), req.headers()).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let Some(runner) = runner_opt else {
        return json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"run_not_owned_by_runner"}"#.to_owned(),
        );
    };
    let runner_id = runner.id;
    // Read body with limit
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected,
        Err(_) => return server_error(),
    };
    let bytes = body_bytes.to_bytes();
    if bytes.len() > cap::CAPACITY_BODY_LIMIT_BYTES {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"body_too_large"}"#.to_owned(),
        );
    }
    let data: Value = if bytes.is_empty() {
        Value::Object(Map::new())
    } else {
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => v,
            Err(_) => {
                return json_response(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"invalid_json"}"#.to_owned(),
                )
            }
        }
    };
    let obj = data.as_object().cloned().unwrap_or_default();
    let action_raw = obj
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !cap::CAPACITY_ACTIONS.contains(&action_raw.as_str()) {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_action"}"#.to_owned(),
        );
    }
    let action = action_raw.clone();
    let raw_session = obj
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if raw_session.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"session_id_required"}"#.to_owned(),
        );
    }
    let session_uuid = match cap::parse_uuid(&Value::String(raw_session.clone())) {
        Some(u) => u,
        None => {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"invalid_session_id"}"#.to_owned(),
            )
        }
    };
    let revision_val = obj.get("revision").cloned().unwrap_or(Value::Null);
    let revision = match cap::parse_revision(&revision_val) {
        Some(r) => r,
        None => {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"revision_invalid"}"#.to_owned(),
            )
        }
    };
    let provided_token: Option<Uuid> = if let Some(tok_val) = obj.get("assignment_token") {
        if tok_val.is_null() {
            None
        } else {
            match cap::parse_uuid(tok_val) {
                Some(u) => Some(u),
                None => {
                    return json_response(
                        StatusCode::BAD_REQUEST,
                        r#"{"error":"invalid_assignment_token"}"#.to_owned(),
                    )
                }
            }
        }
    } else {
        None
    };
    if action == "admit" && cap::parse_uuid(&Value::String(idempotency_key.clone())).is_none() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_idempotency_key"}"#.to_owned(),
        );
    }
    let queued_at: Option<String> = obj
        .get("queued_at")
        .and_then(|v| if v.is_null() { None } else { Some(v.clone()) })
        .map(|v| v.as_str().unwrap_or("").to_owned())
        .filter(|s| !s.is_empty() || obj.get("queued_at").is_some_and(|v| !v.is_null()));
    if let Some(ref q) = queued_at {
        if parse_rfc3339(q).is_none() {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"invalid_queued_at"}"#.to_owned(),
            );
        }
    }
    let queue_position: Option<u64> = if let Some(v) = obj.get("queue_position") {
        if v.is_null() {
            None
        } else {
            match cap::validate_count(v, false, true) {
                Some(n) => Some(n),
                None => {
                    return json_response(
                        StatusCode::BAD_REQUEST,
                        r#"{"error":"invalid_queue_position"}"#.to_owned(),
                    )
                }
            }
        }
    } else {
        None
    };
    let active: Option<u64> = if let Some(v) = obj.get("active") {
        if v.is_null() {
            None
        } else {
            match cap::validate_count(v, true, false) {
                Some(n) => Some(n),
                None => {
                    return json_response(
                        StatusCode::BAD_REQUEST,
                        r#"{"error":"invalid_active"}"#.to_owned(),
                    )
                }
            }
        }
    } else {
        None
    };
    let limit: Option<u64> = if let Some(v) = obj.get("limit") {
        if v.is_null() {
            None
        } else {
            match cap::validate_count(v, false, true) {
                Some(n) if (cap::CAPACITY_MIN_LIMIT..=cap::CAPACITY_MAX_LIMIT).contains(&n) => {
                    Some(n)
                }
                _ => {
                    return json_response(
                        StatusCode::BAD_REQUEST,
                        r#"{"error":"invalid_limit"}"#.to_owned(),
                    )
                }
            }
        }
    } else {
        None
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    {
        let conn = TxConn::new(&mut tx);
        let _ = conn
            .execute_unprepared("SET LOCAL lock_timeout = '5000ms'")
            .await;
        let _ = conn
            .execute_unprepared("SET LOCAL statement_timeout = '20000ms'")
            .await;
    }

    // Lock Runner
    let locked_runner_opt = {
        let conn = TxConn::new(&mut tx);
        runner::Entity::find_by_id(runner_id)
            .lock_exclusive()
            .one(&conn)
            .await
    };
    let locked_runner = match locked_runner_opt {
        Ok(Some(r)) => r,
        Ok(None) => {
            let _ = tx.rollback().await;
            return json_response(
                StatusCode::CONFLICT,
                r#"{"error":"session_evicted"}"#.to_owned(),
            );
        }
        Err(_) => {
            let _ = tx.rollback().await;
            return json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                r#"{"error":"runner_state_locked"}"#.to_owned(),
            );
        }
    };
    let _ = locked_runner;

    // Lock active RunnerSession
    let locked_session_opt = {
        let conn = TxConn::new(&mut tx);
        runner_session::Entity::find()
            .filter(runner_session::Column::RunnerId.eq(runner_id))
            .filter(runner_session::Column::RevokedAt.is_null())
            .lock_exclusive()
            .one(&conn)
            .await
    };
    let locked_session = match locked_session_opt {
        Ok(Some(s)) => s,
        Ok(None) => {
            let _ = tx.rollback().await;
            return json_response(
                StatusCode::CONFLICT,
                r#"{"error":"session_evicted"}"#.to_owned(),
            );
        }
        Err(_) => {
            let _ = tx.rollback().await;
            return json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                r#"{"error":"runner_state_locked"}"#.to_owned(),
            );
        }
    };
    if locked_session.id != session_uuid {
        let _ = tx.rollback().await;
        return json_response(
            StatusCode::CONFLICT,
            r#"{"error":"session_evicted"}"#.to_owned(),
        );
    }

    // Lock AgentRun
    let locked_run_opt = {
        let conn = TxConn::new(&mut tx);
        agent_run::Entity::find_by_id(run_id)
            .lock_exclusive()
            .one(&conn)
            .await
    };
    let mut locked_run = match locked_run_opt {
        Ok(Some(r)) => r,
        Ok(None) => {
            let _ = tx.rollback().await;
            return json_response(
                StatusCode::NOT_FOUND,
                r#"{"error":"run_not_found"}"#.to_owned(),
            );
        }
        Err(_) => {
            let _ = tx.rollback().await;
            return json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                r#"{"error":"runner_state_locked"}"#.to_owned(),
            );
        }
    };
    if locked_run.runner_id != Some(runner_id) {
        let _ = tx.rollback().await;
        return json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"run_not_owned_by_runner"}"#.to_owned(),
        );
    }

    let now = Utc::now();
    let (decision, admission_id_opt) = match decide(
        &mut tx,
        &mut locked_run,
        &action,
        provided_token,
        revision,
        &idempotency_key,
        session_uuid,
        queued_at.as_deref(),
        queue_position,
        active,
        limit,
        now,
        &obj,
    )
    .await
    {
        Ok(pair) => pair,
        Err(resp) => {
            let _ = tx.rollback().await;
            return resp;
        }
    };

    let updated_run = {
        let conn = TxConn::new(&mut tx);
        agent_run::Entity::find_by_id(run_id).one(&conn).await
    };
    let updated_run = match updated_run {
        Ok(Some(r)) => r,
        Ok(None) => {
            let _ = tx.rollback().await;
            return json_response(
                StatusCode::NOT_FOUND,
                r#"{"error":"run_not_found"}"#.to_owned(),
            );
        }
        Err(_) => {
            let _ = tx.rollback().await;
            return server_error();
        }
    };

    if tx.commit().await.is_err() {
        return server_error();
    }

    let cw_val = updated_run.capacity_wait.clone().unwrap_or(Value::Null);
    let revision_out = cw_val.get("revision").cloned().unwrap_or(Value::Null);
    let mut body = Map::new();
    body.insert("decision".to_string(), Value::String(decision.clone()));
    body.insert(
        "status".to_string(),
        Value::String(updated_run.status.clone()),
    );
    if let Some(tok) = updated_run.assignment_token {
        body.insert(
            "assignment_token".to_string(),
            Value::String(tok.to_string()),
        );
    } else {
        body.insert("assignment_token".to_string(), Value::Null);
    }
    body.insert(
        "assignment_protocol".to_string(),
        Value::from(updated_run.assignment_protocol),
    );
    body.insert("revision".to_string(), revision_out);
    if decision == "proceed" {
        if let Some(aid) = admission_id_opt {
            body.insert("admission_id".to_string(), Value::String(aid));
        }
    }
    let json_str = serde_json::to_string(&Value::Object(body)).unwrap_or_default();
    json_response(StatusCode::OK, json_str)
}

#[allow(clippy::too_many_arguments)]
async fn decide(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    locked: &mut agent_run::Model,
    action: &str,
    provided_token: Option<Uuid>,
    revision: u64,
    idempotency_key: &str,
    session_uuid: Uuid,
    queued_at: Option<&str>,
    queue_position: Option<u64>,
    active: Option<u64>,
    limit: Option<u64>,
    now: DateTime<Utc>,
    data: &Map<String, Value>,
) -> Result<(String, Option<String>), Response> {
    use crate::runner_runs::run_endpoints::is_terminal_status;
    if is_terminal_status(&locked.status) {
        if locked.capacity_wait.is_some() {
            locked.capacity_wait = None;
            let conn = TxConn::new(&mut *tx);
            use sea_orm::ColumnTrait;
            let _ = agent_run::Entity::update_many()
                .col_expr(
                    agent_run::Column::CapacityWait,
                    sea_orm::sea_query::Expr::val(None::<serde_json::Value>).into(),
                )
                .filter(agent_run::Column::Id.eq(locked.id))
                .exec(&conn)
                .await;
        }
        return Ok(("terminal".to_string(), None));
    }
    if locked.status == AgentRunStatus::CancelRequested.value() {
        return Ok(("cancel".to_string(), None));
    }
    if [
        "running",
        "awaiting_approval",
        "awaiting_reauth",
        "paused_awaiting_input",
    ]
    .contains(&locked.status.as_str())
    {
        return Ok(("already_started".to_string(), None));
    }
    let is_protocol_one = locked.assignment_protocol == cap::PROTOCOL_MACHINE_QUEUE;
    if !is_protocol_one {
        if provided_token.is_some() {
            return Ok(("stale_assignment".to_string(), None));
        }
        if action != "reconcile" {
            return Ok(("legacy_in_flight".to_string(), None));
        }
        let is_assigned = locked.status == AgentRunStatus::Assigned.value()
            || locked.status == "waiting_for_worktree";
        let cw_phase = locked
            .capacity_wait
            .as_ref()
            .and_then(|v| v.get("phase"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !is_assigned || cw_phase == "preparing" {
            return Ok(("legacy_in_flight".to_string(), None));
        }
        let new_token = Uuid::new_v4();
        locked.assignment_token = Some(new_token);
        locked.assignment_protocol = cap::PROTOCOL_MACHINE_QUEUE;
        locked.capacity_wait = None;
        let conn = TxConn::new(&mut *tx);
        use sea_orm::ColumnTrait;
        let _ = agent_run::Entity::update_many()
            .col_expr(
                agent_run::Column::AssignmentToken,
                sea_orm::sea_query::Expr::val(Some(new_token)).into(),
            )
            .col_expr(
                agent_run::Column::AssignmentProtocol,
                sea_orm::sea_query::Expr::val(cap::PROTOCOL_MACHINE_QUEUE).into(),
            )
            .col_expr(
                agent_run::Column::CapacityWait,
                sea_orm::sea_query::Expr::val(None::<serde_json::Value>).into(),
            )
            .filter(agent_run::Column::Id.eq(locked.id))
            .exec(&conn)
            .await;
        return Ok(("retain".to_string(), None));
    }
    if !cap::token_matches(locked.assignment_token, provided_token) {
        return Ok(("stale_assignment".to_string(), None));
    }
    if locked.status != AgentRunStatus::Assigned.value() && locked.status != "waiting_for_worktree"
    {
        return Ok(("stale_assignment".to_string(), None));
    }
    let stored_revision: Option<u64> = locked
        .capacity_wait
        .as_ref()
        .and_then(|v| v.get("revision"))
        .and_then(|v| v.as_u64())
        .or_else(|| {
            locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("revision"))
                .and_then(|v| v.as_i64())
                .map(|i| i as u64)
        });
    match action {
        "waiting" => {
            let stored_phase = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("phase"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if stored_phase == cap::PHASE_PREPARING {
                let mut refreshed = locked
                    .capacity_wait
                    .clone()
                    .unwrap_or(serde_json::json!({}));
                if let Some(obj) = refreshed.as_object_mut() {
                    obj.insert("revision".to_string(), Value::from(revision));
                    obj.insert(
                        "reported_at".to_string(),
                        Value::String(rfc3339_now_string(now)),
                    );
                    if let Some(qp) = queue_position {
                        obj.insert("queue_position".to_string(), Value::from(qp));
                    }
                    if let Some(a) = active {
                        obj.insert("active".to_string(), Value::from(a));
                    }
                    if let Some(l) = limit {
                        obj.insert("limit".to_string(), Value::from(l));
                    }
                }
                locked.capacity_wait = Some(refreshed.clone());
                let conn = TxConn::new(&mut *tx);
                use sea_orm::ColumnTrait;
                let _ = agent_run::Entity::update_many()
                    .col_expr(
                        agent_run::Column::CapacityWait,
                        sea_orm::sea_query::Expr::val(Some(refreshed.clone())).into(),
                    )
                    .filter(agent_run::Column::Id.eq(locked.id))
                    .exec(&conn)
                    .await;
                Ok(("recorded".to_string(), None))
            } else {
                let prior = locked.capacity_wait.clone().unwrap_or(Value::Null);
                let queued_at_str = queued_at.map(|s| s.to_owned()).or_else(|| {
                    prior
                        .get("queued_at")
                        .and_then(Value::as_str)
                        .map(|s| s.to_owned())
                });
                save_wait(
                    tx,
                    locked,
                    session_uuid,
                    revision,
                    queued_at_str.as_deref(),
                    queue_position,
                    active,
                    limit,
                    now,
                    cap::PHASE_WAITING,
                    None,
                    &prior,
                )
                .await;
                Ok(("recorded".to_string(), None))
            }
        }
        "admit" => {
            let stored_phase = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("phase"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let stored_session = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("session_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let stored_admission = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("admission_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if stored_phase == cap::PHASE_PREPARING {
                if stored_session == session_uuid.to_string() && stored_admission == idempotency_key
                {
                    return Ok(("proceed".to_string(), Some(stored_admission)));
                } else {
                    return Ok(("stale_report".to_string(), None));
                }
            }
            if let Some(stored) = stored_revision {
                if revision < stored {
                    return Ok(("stale_report".to_string(), None));
                }
            }
            let admission_id = idempotency_key.to_owned();
            let prior = locked.capacity_wait.clone().unwrap_or(Value::Null);
            let queued_at_str = queued_at.map(|s| s.to_owned()).or_else(|| {
                prior
                    .get("queued_at")
                    .and_then(Value::as_str)
                    .map(|s| s.to_owned())
            });
            save_wait(
                tx,
                locked,
                session_uuid,
                revision,
                queued_at_str.as_deref(),
                None,
                active,
                limit,
                now,
                cap::PHASE_PREPARING,
                Some(admission_id.clone()),
                &prior,
            )
            .await;
            Ok(("proceed".to_string(), Some(admission_id)))
        }
        "defer" => {
            let stored_phase = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("phase"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if stored_phase != cap::PHASE_PREPARING {
                return Ok(("recorded".to_string(), None));
            }
            let raw_admission = data
                .get("admission_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            if raw_admission.is_empty() {
                return Err(json_response(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"admission_id_required"}"#.to_owned(),
                ));
            }
            let admission_uuid = match cap::parse_uuid(&Value::String(raw_admission.clone())) {
                Some(u) => u,
                None => {
                    return Err(json_response(
                        StatusCode::BAD_REQUEST,
                        r#"{"error":"invalid_admission_id"}"#.to_owned(),
                    ))
                }
            };
            let stored_admission = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("admission_id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if admission_uuid.to_string() != stored_admission {
                return Ok(("stale_report".to_string(), None));
            }
            if let Some(stored) = stored_revision {
                if revision <= stored {
                    return Ok(("stale_report".to_string(), None));
                }
            }
            let prior = locked.capacity_wait.clone().unwrap_or(Value::Null);
            let queued_at_str = queued_at.map(|s| s.to_owned()).or_else(|| {
                prior
                    .get("queued_at")
                    .and_then(Value::as_str)
                    .map(|s| s.to_owned())
            });
            save_wait(
                tx,
                locked,
                session_uuid,
                revision,
                queued_at_str.as_deref(),
                queue_position,
                active,
                limit,
                now,
                cap::PHASE_WAITING,
                None,
                &prior,
            )
            .await;
            Ok(("recorded".to_string(), None))
        }
        "reconcile" => {
            let stored_phase = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("phase"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let stored_session = locked
                .capacity_wait
                .as_ref()
                .and_then(|v| v.get("session_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if stored_phase == cap::PHASE_PREPARING && stored_session != session_uuid.to_string() {
                let prior = locked.capacity_wait.clone().unwrap_or(Value::Null);
                let queued_at_str = queued_at.map(|s| s.to_owned()).or_else(|| {
                    prior
                        .get("queued_at")
                        .and_then(Value::as_str)
                        .map(|s| s.to_owned())
                });
                save_wait(
                    tx,
                    locked,
                    session_uuid,
                    revision,
                    queued_at_str.as_deref(),
                    queue_position,
                    active,
                    limit,
                    now,
                    cap::PHASE_WAITING,
                    None,
                    &prior,
                )
                .await;
            }
            Ok(("retain".to_string(), None))
        }
        _ => Ok(("recorded".to_string(), None)),
    }
}

#[allow(clippy::too_many_arguments)]
async fn save_wait(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    locked: &mut agent_run::Model,
    session_uuid: Uuid,
    revision: u64,
    queued_at: Option<&str>,
    queue_position: Option<u64>,
    active: Option<u64>,
    limit: Option<u64>,
    now: DateTime<Utc>,
    phase: &str,
    admission_id: Option<String>,
    prior: &Value,
) {
    let mut cw = serde_json::json!({
        "phase": phase,
        "assignment_token": locked.assignment_token.map(|u| Value::String(u.to_string())).unwrap_or(Value::Null),
        "session_id": session_uuid.to_string(),
        "revision": revision,
        "queued_at": queued_at.map(|s| Value::String(s.to_owned())).or_else(|| prior.get("queued_at").cloned()).unwrap_or(Value::Null),
        "reported_at": rfc3339_now_string(now),
        "queue_position": queue_position.map(Value::from).unwrap_or(Value::Null),
        "active": active.map(Value::from).unwrap_or(Value::Null),
        "limit": limit.map(Value::from).unwrap_or(Value::Null),
    });
    if let Some(aid) = admission_id {
        cw.as_object_mut()
            .unwrap()
            .insert("admission_id".to_string(), Value::String(aid));
    }
    locked.capacity_wait = Some(cw.clone());
    let conn = TxConn::new(&mut *tx);
    use sea_orm::ColumnTrait;
    let _ = agent_run::Entity::update_many()
        .col_expr(
            agent_run::Column::CapacityWait,
            sea_orm::sea_query::Expr::val(Some(cw.clone())).into(),
        )
        .filter(agent_run::Column::Id.eq(locked.id))
        .exec(&conn)
        .await;
}
