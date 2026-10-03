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
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::runner_runs::approval;
use pidash_db::runner_runs::ApprovalRequest;
use pidash_services::runner_runs::shape;
use pidash_types::runner_runs::{ApprovalKind, ApprovalStatus};

use super::runs::RowError;
use super::{is_uuid_path_segment, json_response, pool_of, proxy_with_body, SERVER_ERROR_BODY};
use crate::license::{query_last, resolve_actor, QueryMap, UNAUTHENTICATED_BODY};
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// Approval list (`approvals.py:31-40`): creator-routed PENDING rows,
/// `-requested_at`, `LIMIT 200`. Params: `$1` requester; `$2` the raw
/// project scope when present (`::uuid` cast: invalid text errors like
/// Django's `ValidationError`, a 500 either way).
pub fn approvals_list_sql(has_project_scope: bool) -> String {
    let columns = approval::COLUMNS
        .iter()
        .map(|column| format!(r#""agent_run_approval"."{column}""#))
        .collect::<Vec<_>>()
        .join(", ");
    let mut from = r#"FROM "agent_run_approval" INNER JOIN "agent_run" ON ("agent_run_approval"."agent_run_id" = "agent_run"."id")"#.to_owned();
    let mut extra = String::new();
    if has_project_scope {
        // `agent_run__pod__project_id`: Django joins the pod table.
        from.push_str(r#" INNER JOIN "pod" ON ("agent_run"."pod_id" = "pod"."id")"#);
        extra.push_str(r#" AND "pod"."project_id" = $3::uuid"#);
    }
    format!(
        concat!(
            r#"SELECT {columns} {from} "#,
            r#"WHERE ("agent_run"."created_by_id" = $1 AND "agent_run_approval"."status" = $2){extra} "#,
            r#"ORDER BY "agent_run_approval"."requested_at" DESC LIMIT 200"#,
        ),
        columns = columns,
        from = from,
        extra = extra,
    )
}

/// One approval row in [`approval::COLUMNS`] order.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApprovalRecord {
    pub id: Uuid,
    pub agent_run_id: Uuid,
    pub kind: String,
    pub payload: Value,
    pub reason: String,
    pub status: String,
    pub decision_source: String,
    pub decided_by_id: Option<Uuid>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub decided_at: Option<DateTime<Utc>>,
}

impl ApprovalRecord {
    pub fn into_approval(self) -> Result<ApprovalRequest, RowError> {
        let kind = ApprovalKind::from_value(&self.kind).ok_or_else(|| RowError::Choice {
            column: "approval.kind",
            value: self.kind.clone(),
        })?;
        let status = ApprovalStatus::from_value(&self.status).ok_or_else(|| RowError::Choice {
            column: "approval.status",
            value: self.status.clone(),
        })?;
        Ok(ApprovalRequest {
            id: self.id,
            agent_run_id: self.agent_run_id,
            kind,
            payload: self.payload,
            reason: self.reason,
            status,
            decision_source: self.decision_source,
            decided_by_id: self.decided_by_id,
            requested_at: self.requested_at,
            expires_at: self.expires_at,
            decided_at: self.decided_at,
        })
    }
}

/// 401 for anonymous callers (DRF `NotAuthenticated`).
fn unauthorized() -> Response {
    json_response(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned())
}

/// 500 for transport / model-violation failures on owned branches.
fn server_error() -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        SERVER_ERROR_BODY.to_owned(),
    )
}

/// `GET /api/runners/approvals/` — pending approvals routed to the run
/// creator (decision #6, design §5.2), bare JSON array. Fully owned.
pub async fn list_approvals(
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
    let list_sql = approvals_list_sql(project_scope.is_some());
    let mut list_query = sqlx::query_as::<_, ApprovalRecord>(&list_sql)
        .bind(actor.id)
        .bind(ApprovalStatus::Pending.value());
    if let Some(scope) = &project_scope {
        list_query = list_query.bind(scope.as_str());
    }
    let records: Vec<ApprovalRecord> = match list_query.fetch_all(pool).await {
        Ok(records) => records,
        Err(_) => return server_error(),
    };
    let mut body = Vec::with_capacity(records.len());
    for record in records {
        let row = match record.into_approval() {
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
    fn list_sql_shapes() {
        let bare = approvals_list_sql(false);
        assert!(bare.contains(
            r#"INNER JOIN "agent_run" ON ("agent_run_approval"."agent_run_id" = "agent_run"."id")"#
        ));
        assert!(bare.contains(
            r#"WHERE ("agent_run"."created_by_id" = $1 AND "agent_run_approval"."status" = $2)"#
        ));
        assert!(bare.ends_with(r#"ORDER BY "agent_run_approval"."requested_at" DESC LIMIT 200"#));
        assert!(!bare.contains("$3"));
        let scoped = approvals_list_sql(true);
        assert!(scoped.contains(r#""pod"."project_id" = $3::uuid"#));
        // All 11 approval columns selected, fixture order.
        for column in approval::COLUMNS {
            assert!(
                bare.contains(&format!(r#""agent_run_approval"."{column}""#)),
                "selects {column}"
            );
        }
        assert_eq!(approval::COLUMNS.len(), 11);
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
