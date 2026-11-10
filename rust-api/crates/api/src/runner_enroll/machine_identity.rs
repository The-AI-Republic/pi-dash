//! Daemon machine identity endpoints (`runner/views/machines.py`):
//! `GET`/`PATCH dev-machines/{id}/`, `GET replace-candidates/` and
//! `POST merge-into/` (#2910/#2912, design §4.10 + §13).

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

use axum::extract::{Extension, Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::entity::{dev_machine, pod, projects, runner as runner_entity, workspaces};
use pidash_services::runner_enroll::{machine_controls, machine_merge, serializers::shapes};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};

use super::auth::{authenticate_machine_token, MachineAuth, ALLOW_GET};
use super::enroll::session_user_id;
use super::{data_get, j_truthy, read_request_data};
use pidash_db::orm_tx::TxConn;

fn strict_uuid(segment: &str) -> Option<Uuid> {
    match segment.parse::<Uuid>() {
        Ok(id) if id.hyphenated().to_string() == segment => Some(id),
        _ => None,
    }
}
use crate::middleware::SessionHandle;
use crate::runner_runs::{json_response, pool_of, server_error};
use crate::state::AppState;
use crate::v1_cycles_modules::json::{self, JVal};

const DAEMON_FIELDS: &[&str] = &["engine_paused_by_owner", "label"];

fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

fn render_dt(at: &DateTime<Utc>) -> String {
    crate::serializer::render_datetime(at)
}

async fn daemon_payload(
    pool: &sqlx::PgPool,
    machine: &dev_machine::Model,
    workspace_id: Uuid,
) -> Result<String, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let now = Utc::now();
    let sign_in_state = pidash_services::domain::engine_credential::current_engine_sign_in_state(
        &machine.engine_sign_in_state,
        machine
            .engine_credential_expires_at
            .map(|at| at.with_timezone(&Utc)),
        now,
    );
    let engine_state = machine.engine_state.clone();
    let id_owned = machine.id.to_string();
    let expires_rendered = machine
        .engine_credential_expires_at
        .as_ref()
        .map(|at| at.with_timezone(&Utc))
        .map(|at| render_dt(&at));
    let reported_rendered = machine
        .engine_state_reported_at
        .as_ref()
        .map(|at| at.with_timezone(&Utc))
        .map(|at| render_dt(&at));
    let merged_rendered = machine.merged_into_id.map(|id| id.to_string());
    let last_seen_rendered = machine
        .last_seen_at
        .as_ref()
        .map(|at| at.with_timezone(&Utc))
        .map(|at| render_dt(&at));
    let revoked_rendered = machine
        .revoked_at
        .as_ref()
        .map(|at| at.with_timezone(&Utc))
        .map(|at| render_dt(&at));
    let created_rendered = render_dt(&machine.created_at.with_timezone(&Utc));
    let updated_rendered = render_dt(&machine.updated_at.with_timezone(&Utc));
    let row = shapes::DevMachineRow {
        id: &id_owned,
        host_label: &machine.host_label,
        label: &machine.label,
        visibility: i64::from(machine.visibility),
        provisioning: &machine.provisioning,
        runner_count: None,
        online_runner_count: None,
        control_online: Some(false),
        engine_paused_by_owner: machine.engine_paused_by_owner,
        engine_auto_pause_reason: &machine.engine_auto_pause_reason,
        engine_team_runs: machine.engine_team_runs,
        engine_sign_in_state: &sign_in_state,
        engine_credential_expires_at: expires_rendered.as_deref(),
        engine_state: &engine_state,
        engine_state_reported_at: reported_rendered.as_deref(),
        merged_into: merged_rendered.as_deref(),
        last_seen_at: last_seen_rendered.as_deref(),
        last_heartbeat_at: None,
        revoked_at: revoked_rendered.as_deref(),
        created_at: &created_rendered,
        updated_at: &updated_rendered,
    };
    let machine_json = shapes::dev_machine_to_representation(&row);
    let machine_value = serde_json::to_value(&machine_json).map_err(|_| server_error())?;

    let runners = runner_entity::Entity::find()
        .filter(runner_entity::Column::DevMachineId.eq(Some(machine.id)))
        .filter(runner_entity::Column::WorkspaceId.eq(workspace_id))
        .order_by_asc(runner_entity::Column::CreatedAt)
        .order_by_asc(runner_entity::Column::Id)
        .all(&db)
        .await
        .map_err(|_| server_error())?;
    let mut daemon_runners = Vec::with_capacity(runners.len());
    for runner in &runners {
        let workspace_slug = workspaces::Entity::find_by_id(runner.workspace_id)
            .one(&db)
            .await
            .map_err(|_| server_error())?
            .map(|ws| ws.slug)
            .unwrap_or_default();
        let pod_row = pod::Entity::find_by_id(runner.pod_id)
            .one(&db)
            .await
            .map_err(|_| server_error())?
            .ok_or_else(server_error)?;
        let project_row = projects::Entity::find_by_id(pod_row.project_id)
            .one(&db)
            .await
            .map_err(|_| server_error())?
            .ok_or_else(server_error)?;
        let agent_kind: String = if runner.provisioning == "desktop_bundled" {
            "engine".to_owned()
        } else {
            match &runner.capabilities {
                Value::Array(caps) => caps
                    .iter()
                    .find_map(|cap| match cap {
                        Value::String(cap) => cap.strip_prefix("agent:").filter(|k| !k.is_empty()),
                        _ => None,
                    })
                    .unwrap_or("")
                    .to_owned(),
                Value::Object(map) => map
                    .keys()
                    .find_map(|key| key.strip_prefix("agent:").filter(|k| !k.is_empty()))
                    .unwrap_or("")
                    .to_owned(),
                _ => String::new(),
            }
        };
        let working_dir = match &runner.dev_metadata {
            Value::Object(map) => match map.get("working_dir") {
                Some(Value::String(working_dir)) => working_dir.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        daemon_runners.push(serde_json::json!({
            "id": runner.id.to_string(),
            "name": runner.name,
            "workspace_slug": workspace_slug,
            "project_id": project_row.id.to_string(),
            "project_identifier": project_row.identifier,
            "pod_id": runner.pod_id.to_string(),
            "agent_kind": agent_kind,
            "provisioning": runner.provisioning,
            "status": runner.status,
            "revoked_at": runner.revoked_at.map(|at| render_dt(&at.with_timezone(&Utc))),
            "revoked_reason": runner.revoked_reason,
            "working_dir": working_dir,
        }));
    }
    let mut payload = machine_value.as_object().cloned().unwrap_or_default();
    payload.insert("runners".to_owned(), Value::Array(daemon_runners));
    serde_json::to_string(&payload).map_err(|_| server_error())
}

fn own_machine_token(auth: &MachineAuth, dev_machine_id: Uuid) -> bool {
    auth.token.dev_machine_id == Some(dev_machine_id)
}

pub async fn daemon_machine_get(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(dev_machine_id) = strict_uuid(&raw_id) else {
        return json_response(
            StatusCode::NOT_FOUND,
            r#"{"detail":"Not found."}"#.to_owned(),
        );
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let auth = match authenticate_machine_token(&pool, secret.as_bytes(), &headers, ALLOW_GET).await
    {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let Some(auth) = auth else {
        return json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"dev_machine_mismatch"}"#.to_owned(),
        );
    };
    if !own_machine_token(&auth, dev_machine_id) {
        return json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"dev_machine_mismatch"}"#.to_owned(),
        );
    }
    let db = pidash_db::pool::sea_orm_connection(&pool);
    let machine = dev_machine::Entity::find_by_id(dev_machine_id)
        .filter(dev_machine::Column::OwnerId.eq(auth.user_id))
        .one(&db)
        .await;
    let machine = match machine {
        Ok(Some(machine)) => machine,
        Ok(None) => {
            return json_response(StatusCode::NOT_FOUND, r#"{"error":"not found"}"#.to_owned())
        }
        Err(_) => return server_error(),
    };
    match daemon_payload(&pool, &machine, auth.token.workspace_id).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

pub async fn daemon_machine_patch(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    headers: axum::http::HeaderMap,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    let Some(dev_machine_id) = strict_uuid(&raw_id) else {
        return json_response(
            StatusCode::NOT_FOUND,
            r#"{"detail":"Not found."}"#.to_owned(),
        );
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let auth = match authenticate_machine_token(&pool, secret.as_bytes(), &headers, ALLOW_GET).await
    {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let Some(auth) = auth else {
        return json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"dev_machine_mismatch"}"#.to_owned(),
        );
    };
    if !own_machine_token(&auth, dev_machine_id) {
        return json_response(
            StatusCode::FORBIDDEN,
            r#"{"error":"dev_machine_mismatch"}"#.to_owned(),
        );
    }
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let JVal::Object(object) = &data else {
        return server_error();
    };
    let map = json::to_serde_publish_map(object);
    let values = match machine_controls::parse_machine_controls(&map, DAEMON_FIELDS) {
        Ok(values) => values,
        Err(message) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&serde_json::json!({"error": message})).expect("renders"),
            )
        }
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let updated = {
        let conn = TxConn::new(&mut tx);
        machine_controls::update_machine_controls(
            &conn,
            dev_machine_id,
            auth.user_id,
            &values,
            now_micros(),
        )
        .await
    };
    match updated {
        Ok(updated) => {
            if tx.commit().await.is_err() {
                return server_error();
            }
            match daemon_payload(&pool, &updated.model, auth.token.workspace_id).await {
                Ok(body) => json_response(StatusCode::OK, body),
                Err(response) => response,
            }
        }
        Err(machine_controls::UpdateError::Failure(failure)) => {
            let _ = tx.rollback().await;
            match failure {
                machine_controls::MachineControlFailure::NotFound => {
                    json_response(StatusCode::NOT_FOUND, r#"{"error":"not found"}"#.to_owned())
                }
                machine_controls::MachineControlFailure::Forbidden => {
                    json_response(StatusCode::FORBIDDEN, r#"{"error":"forbidden"}"#.to_owned())
                }
                machine_controls::MachineControlFailure::Revoked => json_response(
                    StatusCode::CONFLICT,
                    r#"{"error":"dev_machine_revoked"}"#.to_owned(),
                ),
            }
        }
        Err(machine_controls::UpdateError::Db(_)) => server_error(),
    }
}

async fn dual_caller(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(sqlx::PgPool, Uuid, Option<MachineAuth>), Response> {
    let pool = pool_of(state)?.clone();
    let secret = state.settings().secret_key.clone();
    let has_mt = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .strip_prefix("Bearer ")
                .unwrap_or(value)
                .starts_with("mt_")
        })
        .unwrap_or(false);
    if has_mt {
        let auth = authenticate_machine_token(&pool, secret.as_bytes(), headers, ALLOW_GET).await?;
        match auth {
            Some(auth) => return Ok((pool, auth.user_id, Some(auth))),
            None => {
                return Err(json_response(
                    StatusCode::UNAUTHORIZED,
                    crate::license::UNAUTHENTICATED_BODY.to_owned(),
                ))
            }
        }
    }
    let user_id = session_user_id(&pool, secret.as_bytes(), extension).await?;
    match user_id {
        Some(user_id) => Ok((pool, user_id, None)),
        None => Err(json_response(
            StatusCode::UNAUTHORIZED,
            crate::license::UNAUTHENTICATED_BODY.to_owned(),
        )),
    }
}

pub async fn replace_candidates_get(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(dev_machine_id) = strict_uuid(&raw_id) else {
        return json_response(
            StatusCode::NOT_FOUND,
            r#"{"detail":"Not found."}"#.to_owned(),
        );
    };
    let (pool, user_id, machine_auth) = match dual_caller(&state, &headers, extension).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    if let Some(auth) = machine_auth.as_ref() {
        if auth.token.dev_machine_id != Some(dev_machine_id) {
            return json_response(
                StatusCode::FORBIDDEN,
                r#"{"error":"dev_machine_mismatch"}"#.to_owned(),
            );
        }
    }
    let db = pidash_db::pool::sea_orm_connection(&pool);
    let machine = dev_machine::Entity::find_by_id(dev_machine_id)
        .filter(dev_machine::Column::OwnerId.eq(user_id))
        .one(&db)
        .await;
    let machine = match machine {
        Ok(Some(machine)) => machine,
        Ok(None) => {
            return json_response(StatusCode::NOT_FOUND, r#"{"error":"not found"}"#.to_owned())
        }
        Err(_) => return server_error(),
    };
    let candidates = machine_merge::replace_candidates(&db, &machine, now_micros()).await;
    let candidates = match candidates {
        Ok(candidates) => candidates,
        Err(_) => return server_error(),
    };
    let mut items = Vec::with_capacity(candidates.len());
    for candidate in &candidates {
        let live_runner_count = match runner_entity::Entity::find()
            .filter(runner_entity::Column::DevMachineId.eq(Some(candidate.id)))
            .filter(runner_entity::Column::RevokedAt.is_null())
            .count(&db)
            .await
        {
            Ok(count) => count,
            Err(_) => return server_error(),
        };
        let display_name = if !candidate.label.is_empty() {
            candidate.label.clone()
        } else if !candidate.host_label.is_empty() {
            candidate.host_label.clone()
        } else {
            "This computer".to_owned()
        };
        items.push(serde_json::json!({
            "id": candidate.id.to_string(),
            "label": candidate.label,
            "host_label": candidate.host_label,
            "display_name": display_name,
            "last_seen_at": candidate.last_seen_at.map(|at| render_dt(&at.with_timezone(&Utc))),
            "created_at": render_dt(&candidate.created_at.with_timezone(&Utc)),
            "provisioning": candidate.provisioning,
            "live_runner_count": live_runner_count,
        }));
    }
    json_response(
        StatusCode::OK,
        serde_json::to_string(&serde_json::json!({"candidates": items})).expect("renders"),
    )
}

async fn token_may_merge(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<bool, Response> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let target = dev_machine::Entity::find_by_id(target_id)
        .filter(dev_machine::Column::OwnerId.eq(user_id))
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    let source = dev_machine::Entity::find_by_id(source_id)
        .filter(dev_machine::Column::OwnerId.eq(user_id))
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    let (Some(target), Some(source)) = (target, source) else {
        return Ok(true);
    };
    if source.merged_into_id == Some(target.id) {
        return Ok(true);
    }
    Ok(!target.host_label.is_empty() && source.host_label == target.host_label)
}

fn target_string(data: &JVal, key: &str) -> Result<String, Response> {
    let value = data_get(data, key)?;
    let Some(value) = value else {
        return Ok(String::new());
    };
    if !j_truthy(value) {
        return Ok(String::new());
    }
    // `str(...)` coercion (`machines.py:204`): truthy non-strings render
    // via `py_str` (`True`, `123`, repr containers), then UUID-parse 400s.
    let rendered = match value {
        JVal::Str(text) => text.to_lossy_string(),
        other => crate::v1_cycles_modules::json::py_str(other)
            .map(|text| text.to_lossy_string())
            .map_err(|_| server_error())?,
    };
    Ok(pidash_compat::py::py_strip(&rendered).to_owned())
}

pub async fn merge_into_post(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    headers: axum::http::HeaderMap,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    let Some(source_id) = strict_uuid(&raw_id) else {
        return json_response(
            StatusCode::NOT_FOUND,
            r#"{"detail":"Not found."}"#.to_owned(),
        );
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    // `str(request.data.get("target_dev_machine_id") or
    // request.data.get("target") or "").strip()` (`machines.py:204`):
    // truthy non-strings `str()`-coerce, then the UUID parse 400s.
    let first = match target_string(&data, "target_dev_machine_id") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let target_raw = if first.is_empty() {
        match target_string(&data, "target") {
            Ok(value) => value,
            Err(response) => return response,
        }
    } else {
        first
    };
    if target_raw.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            r#"{"error":"target_dev_machine_id is required"}"#.to_owned(),
        );
    }
    let target_id = match target_raw.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                r#"{"error":"invalid_target_dev_machine_id"}"#.to_owned(),
            )
        }
    };
    let (pool, user_id, machine_auth) = match dual_caller(&state, &headers, extension).await {
        Ok(caller) => caller,
        Err(response) => return response,
    };
    if let Some(auth) = machine_auth.as_ref() {
        if auth.token.dev_machine_id != Some(target_id) {
            return json_response(
                StatusCode::FORBIDDEN,
                r#"{"error":"dev_machine_mismatch"}"#.to_owned(),
            );
        }
        let allowed = match token_may_merge(&pool, user_id, source_id, target_id).await {
            Ok(allowed) => allowed,
            Err(response) => return response,
        };
        if !allowed {
            return json_response(
                StatusCode::FORBIDDEN,
                r#"{"error":"not_a_replace_candidate"}"#.to_owned(),
            );
        }
    }
    // The §13.1/§23.4 comments are system-attributed (Django
    // `get_agent_system_user()`); resolved on the pool like the
    // revoke store before the merge transaction begins.
    let system_actor = match pidash_services::domain::identity::agent_system_user_id(&pool).await {
        Ok(actor) => actor,
        Err(_) => return server_error(),
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let now = now_micros();
    let outcome = {
        let conn = TxConn::new(&mut tx);
        machine_merge::merge_dev_machine(&conn, user_id, source_id, target_id, system_actor, now)
            .await
    };
    let outcome = match outcome {
        Ok(outcome) => {
            if tx.commit().await.is_err() {
                return server_error();
            }
            outcome
        }
        Err(err) => {
            let _ = tx.rollback().await;
            let status = StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::BAD_REQUEST);
            return json_response(
                status,
                serde_json::to_string(&serde_json::json!({"error": err.code()})).expect("renders"),
            );
        }
    };
    // Post-commit effects in Django registration order:
    // publishes (per finalized run) → handoffs → drains → bounces →
    // recreates → cleanup (the revoke bundle's firing order; the
    // §23.4 bounces/recreates mirror `fire_revoke_bundle`).
    let ports = crate::runner_runs::LivePorts::new(pool.clone(), &state);
    for run_id in &outcome.publish_run_ids {
        for effect in [
            pidash_services::runner_runs::LifecycleEffect::PublishTerminalEffects {
                run_id: *run_id,
            },
            pidash_services::runner_runs::LifecycleEffect::ApplyTerminalEffectsInline {
                run_id: *run_id,
            },
        ] {
            let _ = pidash_jobs::orchestration::drain_publish_effects(vec![effect], |one| {
                crate::runner_runs::run_endpoints::drain_lifecycle_effects(
                    &pool,
                    &ports,
                    vec![one],
                    Some(user_id),
                )
            })
            .await;
        }
    }
    for (_runner_id, run_id) in &outcome.handoff_pairs {
        if crate::project_move_handoff::complete_project_move_handoff(
            &pool,
            &state,
            *run_id,
            Some(user_id),
        )
        .await
        .is_err()
        {
            tracing::warn!(
                "failed to complete project-move handoff after revoking runner {} (run {})",
                _runner_id,
                run_id
            );
        }
    }
    for pod_id in &outcome.drain_pod_ids {
        let redis = pidash_db::redis::client_from_settings(state.settings());
        let _ = pidash_services::runner_sessions::live_drains::drain_pod_by_id(
            &pool,
            redis.as_ref(),
            &state.settings().runner,
            *pod_id,
        )
        .await;
    }
    // §23.4: E2/E4-failed dedicated issues get the lost-runner bounce
    // (`_bounce_issue_no_eligible_runner`), then the recreate
    // (`_create_and_dispatch_run` via the continuation driver).
    for bounce in &outcome.bounce_effects {
        let name = bounce
            .runner_name
            .clone()
            .unwrap_or_else(|| "the dedicated runner".to_owned());
        let now = chrono::Utc::now();
        match crate::v1_work_items::handlers_actions::dispatch_deps_for_ids(
            &state,
            &pool,
            uuid::Uuid::nil(),
            bounce.issue_id,
            now.timestamp(),
        )
        .await
        {
            Ok((deps, _)) => {
                if let Err(error) = pidash_jobs::orchestration::bounce_issue_no_eligible_runner(
                    &pool,
                    deps,
                    now,
                    0.0,
                    bounce.issue_id,
                    &bounce.triggered_by,
                    &bounce.reason,
                    &name,
                )
                .await
                {
                    tracing::error!(%error, %bounce.issue_id, "dedicated-runner bounce failed");
                }
            }
            Err(_) => {
                tracing::error!(%bounce.issue_id, "dedicated-runner bounce: dispatch deps failed");
            }
        }
    }
    for recreate in &outcome.recreate_effects {
        let now = chrono::Utc::now();
        match crate::v1_work_items::handlers_actions::dispatch_deps_for_ids(
            &state,
            &pool,
            recreate.creator.unwrap_or(uuid::Uuid::nil()),
            recreate.issue_id,
            now.timestamp(),
        )
        .await
        {
            Ok((deps, matcher)) => {
                if let Err(error) = pidash_jobs::orchestration::dispatch_continuation_run(
                    &pool,
                    deps,
                    &matcher,
                    now,
                    0.0,
                    recreate.issue_id,
                    &recreate.trigger,
                    recreate.creator,
                    false,
                    false,
                )
                .await
                {
                    tracing::error!(%error, %recreate.issue_id, "dedicated-runner recreate failed");
                }
            }
            Err(_) => {
                tracing::error!(%recreate.issue_id, "dedicated-runner recreate: dispatch deps failed");
            }
        }
    }
    for runner_id in &outcome.cleanup_runner_ids {
        let redis = pidash_db::redis::client_from_settings(state.settings());
        let _ = pidash_db::runner_sessions::outbox::schedule_stream_cleanup_for_runner(
            redis.as_ref(),
            &state.settings().runner,
            &runner_id.to_string(),
        )
        .await;
    }
    json_response(
        StatusCode::OK,
        serde_json::to_string(&outcome.result).expect("renders"),
    )
}
