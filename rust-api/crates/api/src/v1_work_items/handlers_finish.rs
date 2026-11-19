//! `POST .../agent-runs/<run_id>/finish/` — the one closing command of an
//! issue run (lean agent workflow, design §8.1).
//!
//! `pidash run finish` replaces the four calls a run used to end with
//! (`workpad update`, `comment add`, `issue patch --state`, `run yield`).
//! The endpoint:
//!
//! 1. validates the target state (by exact name, never a stage the project
//!    switched off), the result, and the hand-off rules;
//! 2. merges the outcome into `agent_run.done_payload` — the same keys
//!    `yield` writes, with `stop_ticking` *derived* from the result and
//!    the hand-off stored as `stage_handoff`;
//! 3. posts the comment, attributed to the run;
//! 4. posts the hand-off as a comment when the issue goes to review/test;
//! 5. moves the issue through the v1 patch path with the run as mover, so
//!    the transition (and an enabled stage's entry run) fires as for any
//!    agent move.
//!
//! The steps are not one transaction (the comment and issue operations
//! own theirs). They are ordered so a retry is safe: the comments are
//! skipped once their ids are recorded on the payload, and the move is a
//! no-op when the issue already sits in the target state.
//!
//! Scheduler runs (no issue) are refused; they keep using `yield`.

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_compat::response::json_response;
use pidash_db::entity::{agent_run, issues, states};
use pidash_db::pool::sea_orm_connection;
use pidash_types::orchestration::{
    phase_config_for, stage_runs_automatically, StageSwitches, StateRef, KIND_CODING_TASK,
};

use crate::state::AppState;

use super::handlers_actions::{
    activate_timezone, db_error, fetch_project, fetch_yield_run, is_workspace_member, isoformat,
    preamble, truncate_note, write_done_payload, Denial, HEADER_MISMATCH_BODY,
};
use super::perms::{run_belongs_to, RUN_ID_HEADER};

/// The hand-off cap, in characters (§8.2).
pub const HANDOFF_MAX_CHARS: usize = 20_000;
/// The comment label when the caller names no agent runtime.
pub const DEFAULT_AGENT_LABEL: &str = "AI Agent";

/// `done_payload` key holding the In Progress hand-off. Not `handoff`:
/// that word already names the project-move handoff in orchestration.
pub const STAGE_HANDOFF_KEY: &str = "stage_handoff";
/// `done_payload` key recording the posted finish comment (retry guard).
pub const FINISH_COMMENT_KEY: &str = "finish_comment_id";
/// `done_payload` key recording the posted hand-off comment (retry guard).
pub const HANDOFF_COMMENT_KEY: &str = "handoff_comment_id";

/// The five results a run may finish with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishResult {
    Done,
    Progressed,
    WaitingOnExternal,
    WaitingOnHuman,
    Blocked,
}

impl FinishResult {
    pub const ALL: [FinishResult; 5] = [
        FinishResult::Done,
        FinishResult::Progressed,
        FinishResult::WaitingOnExternal,
        FinishResult::WaitingOnHuman,
        FinishResult::Blocked,
    ];

    pub fn as_wire(self) -> &'static str {
        match self {
            FinishResult::Done => "done",
            FinishResult::Progressed => "progressed",
            FinishResult::WaitingOnExternal => "waiting_on_external",
            FinishResult::WaitingOnHuman => "waiting_on_human",
            FinishResult::Blocked => "blocked",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_wire() == raw.trim())
    }

    /// Whether the clock stops when the run ends (§8.1 table): only the
    /// results that need a person stop it. Everything else keeps ticking,
    /// or stops because the issue left the ticking states.
    pub fn stops_ticking(self) -> bool {
        matches!(self, FinishResult::WaitingOnHuman | FinishResult::Blocked)
    }
}

/// The validated request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishRequest {
    pub to: String,
    pub result: FinishResult,
    pub comment: Option<String>,
    pub handoff: Option<String>,
    pub note: Option<String>,
    pub as_agent: String,
}

/// A 400 refusal: the JSON `{"error": ..., ...}` body, rendered at the
/// handler edge (kept small so the pure checks can return it).
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal(pub Value);

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&self.0).expect("finish body"),
        )
    }
}

/// A 400 with a JSON `{"error": ...}` body plus optional extra keys.
fn bad_request(error: &str, extra: &[(&str, Value)]) -> Refusal {
    let mut map = Map::new();
    map.insert("error".to_owned(), Value::String(error.to_owned()));
    for (key, value) in extra {
        map.insert((*key).to_owned(), value.clone());
    }
    Refusal(Value::Object(map))
}

fn non_blank(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    }
}

/// Parse and type-check the body (step 1, the parts that need no rows).
pub fn parse_finish_body(data: &Value) -> Result<FinishRequest, Refusal> {
    let Some(obj) = data.as_object() else {
        return Err(bad_request("body must be a JSON object", &[]));
    };
    let Some(to) = non_blank(obj.get("to")) else {
        return Err(bad_request("to is required: the target state's name", &[]));
    };
    let allowed: Vec<Value> = FinishResult::ALL
        .iter()
        .map(|r| Value::String(r.as_wire().to_owned()))
        .collect();
    let result = obj
        .get("result")
        .and_then(Value::as_str)
        .and_then(FinishResult::parse)
        .ok_or_else(|| bad_request("result is required", &[("allowed", Value::Array(allowed))]))?;
    let handoff = non_blank(obj.get("handoff"));
    if let Some(text) = handoff.as_deref() {
        if text.chars().count() > HANDOFF_MAX_CHARS {
            return Err(bad_request(
                &format!("handoff is longer than {HANDOFF_MAX_CHARS} characters"),
                &[],
            ));
        }
    }
    Ok(FinishRequest {
        to: to.trim().to_owned(),
        result,
        comment: non_blank(obj.get("comment")),
        handoff,
        note: obj
            .get("note")
            .and_then(Value::as_str)
            .and_then(truncate_note),
        as_agent: non_blank(obj.get("as_agent"))
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|| DEFAULT_AGENT_LABEL.to_owned()),
    })
}

/// One project state the target may resolve to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChoice {
    pub id: Uuid,
    pub name: String,
    pub group: String,
}

impl StateChoice {
    fn as_ref(&self) -> StateRef<'_> {
        StateRef {
            group: &self.group,
            name: &self.name,
        }
    }

    /// A registered phase the project switched off (§6.5).
    pub fn is_disabled_stage(&self, switches: StageSwitches) -> bool {
        phase_config_for(Some(&self.as_ref())).is_some()
            && !stage_runs_automatically(Some(&self.as_ref()), switches)
    }

    /// The review or test phase (a move there needs a hand-off).
    pub fn is_review_or_test(&self) -> bool {
        phase_config_for(Some(&self.as_ref()))
            .is_some_and(|cfg| cfg.template_name != KIND_CODING_TASK)
    }
}

/// The names an agent may finish into: every state except a disabled stage.
pub fn valid_targets(states: &[StateChoice], switches: StageSwitches) -> Vec<String> {
    states
        .iter()
        .filter(|s| !s.is_disabled_stage(switches))
        .map(|s| s.name.clone())
        .collect()
}

/// Resolve the target by exact name and refuse a disabled stage.
pub fn resolve_target<'a>(
    states: &'a [StateChoice],
    to: &str,
    switches: StageSwitches,
) -> Result<&'a StateChoice, Refusal> {
    let names = || {
        Value::Array(
            valid_targets(states, switches)
                .into_iter()
                .map(Value::String)
                .collect(),
        )
    };
    let Some(target) = states.iter().find(|s| s.name == to) else {
        return Err(bad_request(
            &format!("unknown state \"{to}\""),
            &[("available", names())],
        ));
    };
    if target.is_disabled_stage(switches) {
        return Err(bad_request(
            &format!(
                "\"{to}\" does not run an agent in this project; move the issue to a state that does, or finish in the current one"
            ),
            &[("available", names())],
        ));
    }
    Ok(target)
}

/// The hand-off rules (step 1): only an In Progress (`coding-task`) run
/// writes one, and such a run moving to review or test must have one
/// (now or stored earlier on the issue).
pub fn check_handoff(
    phase_kind: &str,
    has_handoff: bool,
    moving_to_review_or_test: bool,
    earlier_handoff_exists: bool,
) -> Result<(), Refusal> {
    let coding = phase_kind == KIND_CODING_TASK;
    if has_handoff && !coding {
        return Err(bad_request(
            "only an In Progress run writes a hand-off; put findings in the comment",
            &[],
        ));
    }
    if coding && moving_to_review_or_test && !has_handoff && !earlier_handoff_exists {
        return Err(bad_request(
            "a move to review or test needs a hand-off: write it to a file and pass --handoff-file",
            &[],
        ));
    }
    Ok(())
}

/// Merge the finish keys into the run's stored payload (step 2).
pub fn merged_payload(
    stored: Option<&Value>,
    req: &FinishRequest,
    yielded_at: &str,
) -> Map<String, Value> {
    let mut payload = match stored {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    payload.insert(
        "status".to_owned(),
        Value::String(req.result.as_wire().to_owned()),
    );
    payload.insert(
        "yielded_at".to_owned(),
        Value::String(yielded_at.to_owned()),
    );
    if let Some(note) = req.note.as_ref() {
        payload.insert("note".to_owned(), Value::String(note.clone()));
    }
    payload.insert(
        "stop_ticking".to_owned(),
        Value::Bool(req.result.stops_ticking()),
    );
    if let Some(handoff) = req.handoff.as_ref() {
        payload.insert(STAGE_HANDOFF_KEY.to_owned(), Value::String(handoff.clone()));
    }
    payload
}

/// The refusal for a run without an issue: scheduled runs keep `yield`.
pub fn scheduler_run_refusal() -> Response {
    bad_request(
        "finish ends an issue run; a scheduled run reports with `pidash run yield`",
        &[],
    )
    .into_response()
}

/// The success body.
pub fn finish_response(run_id: Uuid, state: &str, result: FinishResult, ticking: bool) -> Response {
    json_response(
        StatusCode::OK,
        serde_json::to_string(&json!({
            "run": run_id.to_string(),
            "state": state,
            "result": result.as_wire(),
            "ticking": ticking,
        }))
        .expect("finish body"),
    )
}

/// `POST .../agent-runs/<run_id>/finish/`.
pub async fn post_finish(
    State(state): State<AppState>,
    Path((slug, run_id)): Path<(String, String)>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&run_id) {
        return crate::edge::proxy(State(state), req).await;
    }
    let run_id = run_id.parse::<Uuid>().expect("checked segment");
    let (_parts, body) = req.into_parts();
    let bytes = match pidash_compat::http::read_body(body).await {
        Ok(bytes) => bytes,
        Err(error) => return db_error(error, "read-body").into_response(),
    };
    match finish_inner(&state, &headers, &slug, &run_id, &bytes).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn finish_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    run_id: &Uuid,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    activate_timezone(pre.actor.timezone.as_deref())?;
    let data: Value = if raw_body.is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_slice(raw_body)
            .map_err(|error| Denial::BadDetail(format!("JSON parse error - {error}")))?
    };
    let req = match parse_finish_body(&data) {
        Ok(req) => req,
        Err(refusal) => return Ok(refusal.into_response()),
    };
    let header_run = headers
        .get(RUN_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if !header_run.is_empty() && header_run.to_lowercase() != run_id.to_string().to_lowercase() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            HEADER_MISMATCH_BODY.to_owned(),
        ));
    }
    let run = fetch_yield_run(&pre.pool, slug, run_id)
        .await?
        .ok_or(Denial::NotFoundError("run not found".to_owned()))?;
    if !is_workspace_member(&pre.pool, &pre.actor.id, &run.workspace_id).await? {
        return Err(Denial::NotFoundError("run not found".to_owned()));
    }
    if !run_belongs_to(Some(pre.actor.id), Some(&run.facts)) {
        return Err(Denial::NotFoundError("run not found".to_owned()));
    }
    if !run.facts.status.is_active() {
        return Err(Denial::ConflictError("run is not active".to_owned()));
    }
    let Some(issue_id) = run.facts.work_item_id else {
        return Ok(scheduler_run_refusal());
    };

    let (project_id, current_state_id) = fetch_issue_place(&pre.pool, issue_id).await?;
    let project = fetch_project(&pre.pool, &project_id)
        .await?
        .ok_or(Denial::ServerError)?;
    let switches = project.policy.stage_switches();
    let choices = fetch_state_choices(&pre.pool, project_id).await?;
    let target = match resolve_target(&choices, &req.to, switches) {
        Ok(target) => target.clone(),
        Err(refusal) => return Ok(refusal.into_response()),
    };
    let phase_kind = fetch_phase_kind(&pre.pool, run.facts.id).await?;
    let to_review_or_test = target.is_review_or_test();
    let earlier_handoff =
        if phase_kind == KIND_CODING_TASK && to_review_or_test && req.handoff.is_none() {
            handoff_exists(&pre.pool, issue_id).await?
        } else {
            false
        };
    if let Err(refusal) = check_handoff(
        &phase_kind,
        req.handoff.is_some(),
        to_review_or_test,
        earlier_handoff,
    ) {
        return Ok(refusal.into_response());
    }

    // Step 2: the payload.
    let mut payload = merged_payload(run.done_payload.as_ref(), &req, &isoformat(&Utc::now()));
    let db = sea_orm_connection(&pre.pool);
    write_done_payload(&db, run.facts.id, Value::Object(payload.clone()))
        .await
        .map_err(|error| db_error(error, "finish-write"))?;

    let mut forwarded = headers.clone();
    forwarded.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    forwarded.remove(header::CONTENT_LENGTH);
    forwarded.insert(
        RUN_ID_HEADER,
        HeaderValue::from_str(&run.facts.id.to_string()).map_err(|_| Denial::ServerError)?,
    );
    let project_raw = project_id.to_string();

    // Step 3: the comment.
    if let (Some(comment), false) = (
        req.comment.as_deref(),
        payload.contains_key(FINISH_COMMENT_KEY),
    ) {
        let id = post_agent_comment(
            state,
            &forwarded,
            slug,
            &project_raw,
            issue_id,
            comment,
            &req.as_agent,
            run.facts.id,
        )
        .await?;
        match id {
            Ok(id) => {
                payload.insert(FINISH_COMMENT_KEY.to_owned(), Value::String(id));
                write_done_payload(&db, run.facts.id, Value::Object(payload.clone()))
                    .await
                    .map_err(|error| db_error(error, "finish-write"))?;
            }
            Err(response) => return Ok(response),
        }
    }

    // Step 4: the hand-off, shown to humans where review and test read it.
    if let (Some(handoff), true, false) = (
        req.handoff.as_deref(),
        to_review_or_test,
        payload.contains_key(HANDOFF_COMMENT_KEY),
    ) {
        let id = post_agent_comment(
            state,
            &forwarded,
            slug,
            &project_raw,
            issue_id,
            handoff,
            &req.as_agent,
            run.facts.id,
        )
        .await?;
        match id {
            Ok(id) => {
                payload.insert(HANDOFF_COMMENT_KEY.to_owned(), Value::String(id));
                write_done_payload(&db, run.facts.id, Value::Object(payload.clone()))
                    .await
                    .map_err(|error| db_error(error, "finish-write"))?;
            }
            Err(response) => return Ok(response),
        }
    }

    // Step 5: the move, as an agent state patch.
    if current_state_id != Some(target.id) {
        let body = serde_json::to_vec(&json!({ "state": target.id.to_string() }))
            .map_err(|_| Denial::ServerError)?;
        let response = match super::handlers_core::patch_issue_inner(
            state,
            &forwarded,
            slug,
            &project_raw,
            &issue_id,
            &body,
        )
        .await
        {
            Ok(response) => response,
            Err(denial) => denial.into_response(),
        };
        if !response.status().is_success() {
            return Ok(response);
        }
    }

    let ticking = stage_runs_automatically(Some(&target.as_ref()), switches)
        && !req.result.stops_ticking()
        && project.policy.agent_ticking_enabled.unwrap_or(true);
    Ok(finish_response(
        run.facts.id,
        &target.name,
        req.result,
        ticking,
    ))
}

/// Post one agent comment through the v1 comment-create path. `Ok(Ok(id))`
/// on success; `Ok(Err(response))` passes a refusal through unchanged.
#[allow(clippy::too_many_arguments)]
async fn post_agent_comment(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: &str,
    issue_id: Uuid,
    markdown: &str,
    as_agent: &str,
    run_id: Uuid,
) -> Result<Result<String, Response>, Denial> {
    let body = serde_json::to_vec(&json!({
        "comment_html": pidash_services::assistant::markdown::to_safe_html(Some(markdown)),
        "speaker_type": "agent",
        "speaker_label": as_agent,
        "speaker_agent_run_id": run_id.to_string(),
    }))
    .map_err(|_| Denial::ServerError)?;
    let response = match super::handlers_social::comment_post_inner(
        state,
        headers,
        slug,
        project_raw,
        &issue_id,
        &body,
    )
    .await
    {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    };
    if !response.status().is_success() {
        return Ok(Err(response));
    }
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| Denial::ServerError)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| Denial::ServerError)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or(Denial::ServerError)?
        .to_owned();
    Ok(Ok(id))
}

/// The issue's project and current state.
async fn fetch_issue_place(pool: &PgPool, issue_id: Uuid) -> Result<(Uuid, Option<Uuid>), Denial> {
    let row: Option<(Uuid, Option<Uuid>)> = issues::Entity::find_by_id(issue_id)
        .select_only()
        .columns([issues::Column::ProjectId, issues::Column::StateId])
        .filter(issues::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&sea_orm_connection(pool))
        .await
        .map_err(|error| db_error(error, "finish-issue"))?;
    row.ok_or(Denial::NotFoundError("Work item not found".to_owned()))
}

/// The project's live, non-triage states.
async fn fetch_state_choices(pool: &PgPool, project_id: Uuid) -> Result<Vec<StateChoice>, Denial> {
    use sea_orm::QueryOrder;
    let rows: Vec<(Uuid, String, String)> = states::Entity::find()
        .select_only()
        .columns([
            states::Column::Id,
            states::Column::Name,
            states::Column::Group,
        ])
        .filter(states::Column::ProjectId.eq(project_id))
        .filter(states::Column::DeletedAt.is_null())
        .filter(states::Column::Group.ne("triage"))
        .order_by_asc(states::Column::Sequence)
        .into_tuple()
        .all(&sea_orm_connection(pool))
        .await
        .map_err(|error| db_error(error, "finish-states"))?;
    Ok(rows
        .into_iter()
        .map(|(id, name, group)| StateChoice { id, name, group })
        .collect())
}

async fn fetch_phase_kind(pool: &PgPool, run_id: Uuid) -> Result<String, Denial> {
    let kind: Option<String> = agent_run::Entity::find_by_id(run_id)
        .select_only()
        .column(agent_run::Column::PhaseKind)
        .into_tuple()
        .one(&sea_orm_connection(pool))
        .await
        .map_err(|error| db_error(error, "finish-phase-kind"))?;
    Ok(kind.unwrap_or_default())
}

/// Whether any In Progress run on the issue stored a hand-off.
async fn handoff_exists(pool: &PgPool, issue_id: Uuid) -> Result<bool, Denial> {
    // raw-sql: the jsonb `?` key test has no SeaORM column operator.
    let found: Option<Uuid> = agent_run::Entity::find()
        .select_only()
        .column(agent_run::Column::Id)
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
        .filter(agent_run::Column::PhaseKind.eq(KIND_CODING_TASK))
        .filter(Expr::cust("done_payload ? 'stage_handoff'"))
        .into_tuple()
        .one(&sea_orm_connection(pool))
        .await
        .map_err(|error| db_error(error, "finish-handoff-exists"))?;
    Ok(found.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(name: &str, group: &str) -> StateChoice {
        StateChoice {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            group: group.to_owned(),
        }
    }

    fn project_states() -> Vec<StateChoice> {
        vec![
            choice("Todo", "unstarted"),
            choice("In Progress", "started"),
            choice("In Review", "review"),
            choice("In Test", "test"),
            choice("AI Completed", "completed"),
            choice("Done", "completed"),
        ]
    }

    async fn body_of(response: impl IntoResponse) -> (u16, Value) {
        let response = response.into_response();
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (status, serde_json::from_slice(&bytes).expect("json"))
    }

    #[test]
    fn stop_ticking_is_derived_from_the_result() {
        assert!(!FinishResult::Done.stops_ticking());
        assert!(!FinishResult::Progressed.stops_ticking());
        assert!(!FinishResult::WaitingOnExternal.stops_ticking());
        assert!(FinishResult::WaitingOnHuman.stops_ticking());
        assert!(FinishResult::Blocked.stops_ticking());
        for result in FinishResult::ALL {
            assert_eq!(FinishResult::parse(result.as_wire()), Some(result));
        }
        assert_eq!(FinishResult::parse("approved"), None);
    }

    #[tokio::test]
    async fn body_parsing() {
        let req = parse_finish_body(&json!({
            "to": " In Review ", "result": "done", "comment": "c", "handoff": "h", "note": "  n  "
        }))
        .expect("valid");
        assert_eq!(req.to, "In Review");
        assert_eq!(req.result, FinishResult::Done);
        assert_eq!(req.comment.as_deref(), Some("c"));
        assert_eq!(req.handoff.as_deref(), Some("h"));
        assert_eq!(req.note.as_deref(), Some("n"));
        assert_eq!(req.as_agent, DEFAULT_AGENT_LABEL);

        let (status, body) =
            body_of(parse_finish_body(&json!({"result": "done"})).unwrap_err()).await;
        assert_eq!(status, 400);
        assert!(body["error"].as_str().unwrap().contains("to is required"));

        let (status, body) =
            body_of(parse_finish_body(&json!({"to": "Todo", "result": "nope"})).unwrap_err()).await;
        assert_eq!(status, 400);
        assert_eq!(body["allowed"].as_array().unwrap().len(), 5);

        let long = "x".repeat(HANDOFF_MAX_CHARS + 1);
        let (status, _) = body_of(
            parse_finish_body(&json!({"to": "Todo", "result": "done", "handoff": long}))
                .unwrap_err(),
        )
        .await;
        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn unknown_state_lists_the_valid_targets() {
        let states = project_states();
        let err = resolve_target(&states, "Shipped", StageSwitches::ALL).unwrap_err();
        let (status, body) = body_of(err).await;
        assert_eq!(status, 400);
        assert_eq!(body["available"].as_array().unwrap().len(), 6);
        assert_eq!(
            resolve_target(&states, "In Review", StageSwitches::ALL)
                .unwrap()
                .name,
            "In Review"
        );
    }

    #[tokio::test]
    async fn disabled_stage_target_is_refused() {
        let states = project_states();
        let switches = StageSwitches {
            review: false,
            test: true,
        };
        let (status, body) =
            body_of(resolve_target(&states, "In Review", switches).unwrap_err()).await;
        assert_eq!(status, 400);
        let available: Vec<&str> = body["available"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(!available.contains(&"In Review"));
        assert!(available.contains(&"In Test"));
        assert!(available.contains(&"AI Completed"));
        // In Progress is never switched off.
        assert!(resolve_target(&states, "In Progress", switches).is_ok());
    }

    #[tokio::test]
    async fn handoff_rules() {
        // A hand-off from a review run is refused.
        assert!(check_handoff("review", true, false, false).is_err());
        // A coding run moving to review needs one...
        let (status, body) =
            body_of(check_handoff(KIND_CODING_TASK, false, true, false).unwrap_err()).await;
        assert_eq!(status, 400);
        assert!(body["error"].as_str().unwrap().contains("--handoff-file"));
        // ...unless one was stored earlier on the issue.
        assert!(check_handoff(KIND_CODING_TASK, false, true, true).is_ok());
        assert!(check_handoff(KIND_CODING_TASK, true, true, false).is_ok());
        // A coding run finishing anywhere else needs none.
        assert!(check_handoff(KIND_CODING_TASK, false, false, false).is_ok());
        // Review/test runs moving back need none.
        assert!(check_handoff("review", false, false, false).is_ok());
    }

    #[test]
    fn payload_merge_keeps_retry_guards_and_derives_stop_ticking() {
        let stored = json!({"status": "progressed", "finish_comment_id": "c1", "other": 1});
        let req = parse_finish_body(&json!({
            "to": "In Review", "result": "waiting_on_human", "handoff": "### Hand-off", "note": "n"
        }))
        .unwrap();
        let payload = merged_payload(Some(&stored), &req, "2026-11-19T00:00:00Z");
        assert_eq!(payload["status"], json!("waiting_on_human"));
        assert_eq!(payload["stop_ticking"], json!(true));
        assert_eq!(payload[STAGE_HANDOFF_KEY], json!("### Hand-off"));
        assert_eq!(payload[FINISH_COMMENT_KEY], json!("c1"));
        assert_eq!(payload["other"], json!(1));
        assert_eq!(payload["note"], json!("n"));

        let done = parse_finish_body(&json!({"to": "AI Completed", "result": "done"})).unwrap();
        let payload = merged_payload(None, &done, "t");
        assert_eq!(payload["stop_ticking"], json!(false));
        assert!(!payload.contains_key(STAGE_HANDOFF_KEY));
    }

    static F18_11: &str =
        include_str!("../../../../fixtures/v1_work_items/handlers/F18-11.work_items.json");

    fn fixture_call(name: &str) -> (u16, Value, Value) {
        let fx: Value = serde_json::from_str(F18_11).expect("F18-11 parses");
        let call = fx["calls"][name].clone();
        assert!(!call.is_null(), "fixture call {name}");
        (
            call["status"].as_u64().expect("status") as u16,
            call["request"].clone(),
            call["body"].clone(),
        )
    }

    fn full_states() -> Vec<StateChoice> {
        [
            ("Backlog", "backlog"),
            ("Todo", "unstarted"),
            ("In Progress", "started"),
            ("In Review", "review"),
            ("In Test", "test"),
            ("AI Completed", "completed"),
            ("Done", "completed"),
            ("Cancelled", "cancelled"),
        ]
        .into_iter()
        .map(|(n, g)| choice(n, g))
        .collect()
    }

    #[tokio::test]
    async fn replay_finish_fixture() {
        let run = Uuid::parse_str("640d8cdf-08fb-4772-82d3-2c693d0e105f").unwrap();
        let states = full_states();
        let review_off = StageSwitches {
            review: false,
            test: true,
        };

        for (name, ticking) in [("finish_ok", true), ("finish_waiting_on_human", false)] {
            let (status, request, body) = fixture_call(name);
            let req = parse_finish_body(&request).expect("valid");
            let target = resolve_target(&states, &req.to, StageSwitches::ALL).expect("target");
            let expected_ticking = !req.result.stops_ticking();
            assert_eq!(expected_ticking, ticking, "{name}");
            let got = body_of(finish_response(run, &target.name, req.result, ticking)).await;
            assert_eq!(got, (status, body), "{name}");
        }

        let (status, request, body) = fixture_call("finish_unknown_state");
        let req = parse_finish_body(&request).unwrap();
        let got = body_of(resolve_target(&states, &req.to, StageSwitches::ALL).unwrap_err()).await;
        assert_eq!(got, (status, body));

        let (status, request, body) = fixture_call("finish_disabled_stage");
        let req = parse_finish_body(&request).unwrap();
        let got = body_of(resolve_target(&states, &req.to, review_off).unwrap_err()).await;
        assert_eq!(got, (status, body));

        let (status, request, body) = fixture_call("finish_bad_result");
        let got = body_of(parse_finish_body(&request).unwrap_err()).await;
        assert_eq!(got, (status, body));

        let (status, request, body) = fixture_call("finish_missing_handoff");
        let req = parse_finish_body(&request).unwrap();
        let target = resolve_target(&states, &req.to, StageSwitches::ALL).unwrap();
        let got = body_of(
            check_handoff(
                KIND_CODING_TASK,
                req.handoff.is_some(),
                target.is_review_or_test(),
                false,
            )
            .unwrap_err(),
        )
        .await;
        assert_eq!(got, (status, body));

        let (status, _request, body) = fixture_call("finish_scheduler_run");
        assert_eq!(body_of(scheduler_run_refusal()).await, (status, body));
    }

    #[test]
    fn review_or_test_targets() {
        assert!(choice("In Review", "review").is_review_or_test());
        assert!(choice("In Test", "test").is_review_or_test());
        assert!(!choice("In Progress", "started").is_review_or_test());
        assert!(!choice("AI Completed", "completed").is_review_or_test());
    }
}
