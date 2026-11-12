//! Approval web endpoints (D-15, stage 5, PIDASHCONV-542).
//!
//! Ports `apps/api/pi_dash/runner/views/approvals.py:1-105`:
//!
//! * `ApprovalListEndpoint.get` (`:29-41`) → [`list_approvals`]:
//!   PENDING approvals routed to the run creator, optional
//!   `?project=` scope through the run's pod, `-requested_at`, first
//!   200, bare JSON array. Fully owned.
//! * `ApprovalDecideEndpoint.post` (`:48-105`) → [`decide_approval`]:
//!   the `ApprovalDecisionSerializer` 400s are owned (L3); every valid
//!   decision proxies to Django, which raises the ported
//!   `NotSupportedError` (`select_for_update` over the nullable
//!   `agent_run__runner` join, `approvals.py:58`) and answers 500 — the
//!   lock, the 404/409s, the AWAITING→RUNNING flip and the decide
//!   fan-out are all unreachable on Postgres (FX-RUN-08 `decide_bug`,
//!   pinned by `test_web_approvals.py`).

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use sea_orm::sea_query::{Alias, Expr};
use sea_orm::{
    ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::entity as orm;
use pidash_db::pool::sea_orm_connection;
use pidash_db::runner_runs::ApprovalRequest;
use pidash_services::runner_runs::shape;
use pidash_types::runner_runs::{ApprovalKind, ApprovalStatus};

use super::runs::RowError;
use super::{
    is_uuid_path_segment, json_response, pool_of, proxy_with_body, view_response, ALLOW_APPROVALS,
    ALLOW_APPROVAL_DECIDE,
};
use crate::license::{query_last, resolve_actor, QueryMap, UNAUTHENTICATED_BODY};
use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_compat::response::server_error;

/// Approval list (`approvals.py:31-40`): creator-routed PENDING rows,
/// `-requested_at`, `LIMIT 200`. `project_scope` is the raw
/// `?project=` string, compared through a `uuid` cast
/// (`agent_run__pod__project_id`, so the pod joins): invalid text
/// errors like Django's `ValidationError`, a 500 either way.
pub fn approvals_list_select(
    actor_id: Uuid,
    project_scope: Option<&str>,
) -> sea_orm::Select<orm::agent_run_approval::Entity> {
    use orm::agent_run_approval::{Column, Entity, Relation};
    let mut select = Entity::find()
        .join(JoinType::InnerJoin, Relation::AgentRun.def())
        .filter(orm::agent_run::Column::CreatedById.eq(actor_id))
        .filter(Column::Status.eq(ApprovalStatus::Pending.value()));
    if let Some(raw) = project_scope {
        select = select
            .join(
                JoinType::InnerJoin,
                orm::agent_run::Entity::belongs_to(orm::pod::Entity)
                    .from(orm::agent_run::Column::PodId)
                    .to(orm::pod::Column::Id)
                    .into(),
            )
            .filter(
                Expr::col((orm::pod::Entity, orm::pod::Column::ProjectId))
                    .eq(Expr::val(raw.to_owned()).cast_as(Alias::new("uuid"))),
            );
    }
    select.order_by_desc(Column::RequestedAt).limit(200)
}

/// The L2 approval off its entity row (the choice columns parse to
/// their enums; a model-violating value is [`RowError::Choice`]).
pub fn approval_from_model(
    model: orm::agent_run_approval::Model,
) -> Result<ApprovalRequest, RowError> {
    let kind = ApprovalKind::from_value(&model.kind).ok_or_else(|| RowError::Choice {
        column: "approval.kind",
        value: model.kind.clone(),
    })?;
    let status = ApprovalStatus::from_value(&model.status).ok_or_else(|| RowError::Choice {
        column: "approval.status",
        value: model.status.clone(),
    })?;
    Ok(ApprovalRequest {
        id: model.id,
        agent_run_id: model.agent_run_id,
        kind,
        payload: model.payload,
        reason: model.reason,
        status,
        decision_source: model.decision_source,
        decided_by_id: model.decided_by_id,
        requested_at: model.requested_at.with_timezone(&Utc),
        expires_at: model.expires_at.map(|at| at.with_timezone(&Utc)),
        decided_at: model.decided_at.map(|at| at.with_timezone(&Utc)),
    })
}

/// 401 for anonymous callers (DRF `NotAuthenticated`).
fn unauthorized() -> Response {
    json_response(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned())
}

/// `GET /api/runners/approvals/` — pending approvals routed to the run
/// creator (decision #6, design §5.2), bare JSON array. Fully owned.
pub async fn list_approvals(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    view_response(
        list_approvals_inner(State(state), Query(query), extension).await,
        ALLOW_APPROVALS,
    )
}

async fn list_approvals_inner(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let actor = match resolve_actor(pool, state.settings().secret_key.as_bytes(), extension).await {
        Ok(Some(actor)) => actor,
        Ok(None) => return unauthorized(),
        Err(_) => return server_error(),
    };
    // `if project_id:` — an empty string scopes nothing.
    let project_scope = query_last(&query, "project").filter(|value| !value.is_empty());
    let records = match approvals_list_select(actor.id, project_scope.as_deref())
        .all(&sea_orm_connection(pool))
        .await
    {
        Ok(records) => records,
        Err(_) => return server_error(),
    };
    let mut body = Vec::with_capacity(records.len());
    for record in records {
        let row = match approval_from_model(record) {
            Ok(row) => row,
            Err(_) => return server_error(),
        };
        let view = shape::approval_to_representation(&row);
        body.push(serde_json::to_value(&view).expect("serializable approval view"));
    }
    (StatusCode::OK, Json(Value::Array(body))).into_response()
}

/// `POST /api/runners/approvals/<id>/decide/` — validate the decision
/// (owned 400s), then proxy: every valid POST raises Django's
/// `NotSupportedError` before the creator gate (the ported decide bug),
/// so the 500 must be Django's own.
pub async fn decide_approval(
    State(state): State<AppState>,
    Path(approval_raw): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    view_response(
        decide_approval_inner(State(state), Path(approval_raw), extension, req).await,
        ALLOW_APPROVAL_DECIDE,
    )
}

async fn decide_approval_inner(
    State(state): State<AppState>,
    Path(approval_raw): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if !is_uuid_path_segment(&approval_raw) {
        return crate::edge::proxy(State(state), req).await;
    }
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    match resolve_actor(pool, state.settings().secret_key.as_bytes(), extension).await {
        Ok(Some(_)) => {}
        Ok(None) => return unauthorized(),
        Err(_) => return server_error(),
    }
    let (parts, body) = req.into_parts();
    let bytes = {
        use http_body_util::BodyExt;
        match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => return server_error(),
        }
    };
    let parsed: Value = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => return proxy_with_body(state, parts, bytes).await,
    };
    match shape::validate_approval_decision(&parsed) {
        Ok(_) => proxy_with_body(state, parts, bytes).await,
        Err(body) => (StatusCode::BAD_REQUEST, Json(body)).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx08() -> Value {
        let raw =
            include_str!("../../../../fixtures/runner_runs/fx-run-08-handlers-web.golden.json");
        serde_json::from_str(raw).expect("valid fixture")
    }

    #[test]
    fn list_statement_shapes() {
        let pg = |select: sea_orm::Select<orm::agent_run_approval::Entity>| {
            sea_orm::QueryTrait::build(&select, sea_orm::DbBackend::Postgres).to_string()
        };
        let bare = pg(approvals_list_select(Uuid::nil(), None));
        // All 11 approval columns selected (the entity's), then the run
        // join and the creator / pending predicates.
        assert!(bare.starts_with(
            "SELECT \"agent_run_approval\".\"id\", \"agent_run_approval\".\"kind\", \
             \"agent_run_approval\".\"payload\", \"agent_run_approval\".\"reason\", \
             \"agent_run_approval\".\"status\", \"agent_run_approval\".\"decision_source\", \
             \"agent_run_approval\".\"requested_at\", \"agent_run_approval\".\"expires_at\", \
             \"agent_run_approval\".\"decided_at\", \"agent_run_approval\".\"agent_run_id\", \
             \"agent_run_approval\".\"decided_by_id\" FROM \"agent_run_approval\" \
             INNER JOIN \"agent_run\" ON \"agent_run_approval\".\"agent_run_id\" = \"agent_run\".\"id\" \
             WHERE \"agent_run\".\"created_by_id\" = '00000000-0000-0000-0000-000000000000' AND \
             \"agent_run_approval\".\"status\" = 'pending'"
        ), "{bare}");
        assert!(bare.ends_with("ORDER BY \"agent_run_approval\".\"requested_at\" DESC LIMIT 200"));
        assert!(!bare.contains("\"pod\""));
        let scoped = pg(approvals_list_select(Uuid::nil(), Some("pr")));
        assert!(
            scoped
                .contains("INNER JOIN \"pod\" ON \"agent_run\".\"pod_id\" = \"pod\".\"id\" WHERE"),
            "{scoped}"
        );
        assert!(
            scoped.contains("AND \"pod\".\"project_id\" = CAST('pr' AS uuid)"),
            "{scoped}"
        );
    }

    #[test]
    fn decide_validation_matches_fx08_400s() {
        let fx = fx08();
        let body = |pointer: &str| fx.pointer(pointer).expect(pointer).to_string();
        // L3 owns the kernel; the handler maps `Err` → 400 verbatim.
        let bad = shape::validate_approval_decision(&serde_json::json!({"decision": "maybe"}));
        assert_eq!(
            bad.expect_err("bad choice").to_string(),
            body("/decide/bad_choice/body")
        );
        let missing = shape::validate_approval_decision(&serde_json::json!({}));
        assert_eq!(
            missing.expect_err("missing").to_string(),
            body("/decide/missing_field/body")
        );
        assert!(
            shape::validate_approval_decision(&serde_json::json!({"decision": "accept"})).is_ok()
        );
        // The bug note pins the proxy decision: every valid POST raises
        // before the gate, so nothing past validation is reachable.
        assert!(fx["decide_bug"]
            .as_str()
            .expect("bug note")
            .contains("unreachable on Postgres"));
    }

    #[test]
    fn list_keys_match_fx08() {
        let fx = fx08();
        let keys: Vec<String> = fx["approvals_list"]["ok"]["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|key| key.as_str().expect("str").to_owned())
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "agent_run",
                "kind",
                "payload",
                "reason",
                "status",
                "decision_source",
                "requested_at",
                "decided_at",
                "expires_at"
            ]
        );
        assert_eq!(
            keys.as_slice(),
            pidash_services::runner_runs::shape::APPROVAL_WIRE_FIELDS
        );
    }
}
