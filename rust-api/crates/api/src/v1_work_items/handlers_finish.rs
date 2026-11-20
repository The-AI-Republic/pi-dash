//! `POST .../agent-runs/<run_id>/finish/` — the one closing command of an
//! issue run (lean agent workflow, design §8.1).
//!
//! `pidash run finish` replaces the four calls a run used to end with
//! (`workpad update`, `comment add`, `issue patch --state`, `run yield`).
//! The endpoint:
//!
//! 1. validates the target state (by exact name; never a move into a
//!    stage the project switched off, though staying put is always
//!    allowed), the result, and the hand-off rules;
//! 2. keeps the hand-off as pending in `agent_run.done_payload`;
//! 3. moves the issue through the v1 patch path with the run as mover, so
//!    the transition (and an enabled stage's entry run) fires as for any
//!    agent move — first, so a refused move leaves nothing that claims it
//!    happened;
//! 4. stores the hand-off as `stage_handoff`;
//! 5. posts the comment, attributed to the run;
//! 6. posts the hand-off as a comment when the issue goes to review/test;
//! 7. merges the outcome into `done_payload` — the same keys `yield`
//!    writes, with `stop_ticking` *derived* from the result. Written last,
//!    so a step refused on the way leaves no stop signal behind.
//!
//! The steps are not one transaction (the comment and issue operations
//! own theirs). They are ordered so a retry is safe: a comment is skipped
//! once its id is recorded with the same text (a changed text is posted
//! again), and the move is a no-op when the issue already sits in the
//! target state.
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
    agent_move_refusal, phase_config_for, stage_runs_automatically, AgentMoveRefusal,
    StageSwitches, StateRef, KIND_CODING_TASK,
};

use crate::state::AppState;

use super::handlers_actions::{
    activate_timezone, db_error, fetch_project, fetch_yield_run, is_workspace_member, isoformat,
    preamble, truncate_note, Denial, HEADER_MISMATCH_BODY,
};
use super::perms::{run_belongs_to, RUN_ID_HEADER};

/// The hand-off cap, in characters (§8.2).
pub const HANDOFF_MAX_CHARS: usize = 20_000;
/// The comment label when the caller names no agent runtime.
pub const DEFAULT_AGENT_LABEL: &str = "AI Agent";

/// `done_payload` key holding the In Progress hand-off. Not `handoff`:
/// that word already names the project-move handoff in orchestration.
pub const STAGE_HANDOFF_KEY: &str = "stage_handoff";
/// `done_payload` key holding a hand-off whose move has not happened yet.
/// Nothing reads it: it only becomes [`STAGE_HANDOFF_KEY`] once the issue
/// moved (or stays), so a refused move leaves no hand-off that counts.
pub const PENDING_HANDOFF_KEY: &str = "pending_stage_handoff";
/// `done_payload` key recording the posted finish comment (retry guard).
pub const FINISH_COMMENT_KEY: &str = "finish_comment_id";
/// `done_payload` key recording the posted hand-off comment (retry guard).
pub const HANDOFF_COMMENT_KEY: &str = "handoff_comment_id";
/// `done_payload` key holding the SHA-256 of the posted finish comment, so
/// a retry with different text posts the new text instead of dropping it.
pub const FINISH_COMMENT_DIGEST_KEY: &str = "finish_comment_sha256";
/// `done_payload` key holding the SHA-256 of the posted hand-off comment.
pub const HANDOFF_COMMENT_DIGEST_KEY: &str = "handoff_comment_sha256";

/// Hex SHA-256 of a comment body (the retry guard's text check).
pub fn text_digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Whether `text` still has to be posted: no comment recorded yet, or the
/// recorded one carried different text. A recorded id without a digest
/// counts as posted.
fn comment_owed(payload: &Map<String, Value>, id_key: &str, digest_key: &str, text: &str) -> bool {
    if !payload.contains_key(id_key) {
        return true;
    }
    match payload.get(digest_key).and_then(Value::as_str) {
        Some(stored) => stored != text_digest(text),
        None => false,
    }
}

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
    for key in ["comment", "handoff"] {
        if matches!(obj.get(key), Some(Value::String(text)) if text.trim().is_empty()) {
            return Err(bad_request(
                &format!("{key} is empty: write it, or leave the flag out"),
                &[],
            ));
        }
    }
    let handoff = non_blank(obj.get("handoff"));
    let comment = non_blank(obj.get("comment"));
    if let Some(text) = handoff.as_deref() {
        if text.chars().count() > HANDOFF_MAX_CHARS {
            return Err(bad_request(
                &format!("handoff is longer than {HANDOFF_MAX_CHARS} characters"),
                &[],
            ));
        }
    }
    if result.stops_ticking() && comment.is_none() {
        return Err(bad_request(
            &format!(
                "a {} run must say what the person must do: pass --comment-file",
                result.as_wire()
            ),
            &[],
        ));
    }
    Ok(FinishRequest {
        to: to.trim().to_owned(),
        result,
        comment,
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
        agent_move_refusal(&self.as_ref(), switches) == Some(AgentMoveRefusal::StageOff)
    }

    /// A closed state only a person may move an issue into (§7).
    pub fn is_human_only(&self) -> bool {
        agent_move_refusal(&self.as_ref(), StageSwitches::ALL) == Some(AgentMoveRefusal::HumanOnly)
    }

    /// The review or test phase (a move there needs a hand-off).
    pub fn is_review_or_test(&self) -> bool {
        phase_config_for(Some(&self.as_ref()))
            .is_some_and(|cfg| cfg.template_name != KIND_CODING_TASK)
    }
}

/// The names an agent may finish into: every state except a disabled
/// stage, though the issue's current state is always allowed (a manual
/// run in a switched-off stage may stay where it is, §6.3).
pub fn valid_targets(
    states: &[StateChoice],
    switches: StageSwitches,
    current: Option<Uuid>,
) -> Vec<String> {
    states
        .iter()
        .filter(|s| Some(s.id) == current || !(s.is_disabled_stage(switches) || s.is_human_only()))
        .map(|s| s.name.clone())
        .collect()
}

/// Resolve the target by exact name and refuse a move into a disabled
/// stage. Staying in the current state is never refused.
pub fn resolve_target<'a>(
    states: &'a [StateChoice],
    to: &str,
    switches: StageSwitches,
    current: Option<Uuid>,
) -> Result<&'a StateChoice, Refusal> {
    let names = || {
        Value::Array(
            valid_targets(states, switches, current)
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
    if Some(target.id) != current && target.is_human_only() {
        return Err(bad_request(
            &format!("\"{to}\" is for a person to set; finish in \"AI Completed\" or the current state instead"),
            &[("available", names())],
        ));
    }
    if Some(target.id) != current && target.is_disabled_stage(switches) {
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
/// writes one, and such a run moving to review or test must have one: now,
/// or stored by an In Progress run since the issue's last review or test
/// run (`earlier_handoff_exists`).
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

/// The payload written before the steps that can fail: the stored payload
/// plus the hand-off, but not the result. `status` and `stop_ticking` are
/// written only once every step succeeded, so a refused comment or move
/// never leaves a stop signal behind for a later `yield` to inherit.
pub fn progress_payload(stored: Option<&Value>, req: &FinishRequest) -> Map<String, Value> {
    let mut payload = match stored {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    if let Some(handoff) = req.handoff.as_ref() {
        payload.insert(STAGE_HANDOFF_KEY.to_owned(), Value::String(handoff.clone()));
    }
    payload
}

/// The keys the last write merges in: the result, its time, the note, and
/// the derived `stop_ticking`.
pub fn result_delta(req: &FinishRequest, yielded_at: &str) -> Map<String, Value> {
    let mut delta = Map::new();
    delta.insert(
        "status".to_owned(),
        Value::String(req.result.as_wire().to_owned()),
    );
    delta.insert(
        "yielded_at".to_owned(),
        Value::String(yielded_at.to_owned()),
    );
    if let Some(note) = req.note.as_ref() {
        delta.insert("note".to_owned(), Value::String(note.clone()));
    }
    delta.insert(
        "stop_ticking".to_owned(),
        Value::Bool(req.result.stops_ticking()),
    );
    delta
}

/// The refusal for a run without an issue: scheduled runs keep `yield`.
pub fn scheduler_run_refusal() -> Response {
    bad_request(
        "finish ends an issue run; a scheduled run reports with `pidash run yield`",
        &[],
    )
    .into_response()
}

/// What a `finish` call still has to do, given the run's stored payload
/// (steps 3–5 and the response's `ticking`). Pure, so the retry rules are
/// unit-tested: a step whose id is already recorded is skipped, and the
/// move is skipped when the issue already sits in the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishPlan<'a> {
    /// Step 3: the comment to post, if any is still owed.
    pub comment: Option<&'a str>,
    /// Step 4: the hand-off to post as a comment, if still owed. Only for
    /// a move into review or test, where humans read what the stage reads.
    pub handoff_comment: Option<&'a str>,
    /// Step 5: the state to move to, if the issue is not there yet.
    pub move_to: Option<Uuid>,
    /// Whether the clock keeps running after this run ends.
    pub ticking: bool,
}

/// The issue's clock as `finish` finds it: `None` when the issue has no
/// ticker yet, else whether it can fire again (not switched off by a
/// person, budget left).
pub type ClockView = Option<bool>;

/// Whether the clock runs after this run ends.
///
/// - Moving into a stage that runs: its entry run follows whatever the
///   result says (the run-ended check ignores a stop from a stage the
///   issue left), unless a person switched the clock off or the budget is
///   spent.
/// - Staying in a stage that runs: only when the result does not stop it
///   and the existing ticker can fire.
/// - Anywhere else: no.
pub fn clock_runs_after(
    moved: bool,
    target_runs: bool,
    result: FinishResult,
    project_ticking_enabled: bool,
    clock: ClockView,
) -> bool {
    if !target_runs || !project_ticking_enabled {
        return false;
    }
    match (moved, clock) {
        (true, None) => true,
        (true, Some(can_fire)) => can_fire,
        (false, Some(can_fire)) => can_fire && !result.stops_ticking(),
        (false, None) => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn plan_finish<'a>(
    req: &'a FinishRequest,
    payload: &Map<String, Value>,
    target: &StateChoice,
    current_state: Option<Uuid>,
    switches: StageSwitches,
    project_ticking_enabled: bool,
    clock: ClockView,
) -> FinishPlan<'a> {
    let moved = current_state != Some(target.id);
    FinishPlan {
        comment: req.comment.as_deref().filter(|text| {
            comment_owed(payload, FINISH_COMMENT_KEY, FINISH_COMMENT_DIGEST_KEY, text)
        }),
        handoff_comment: req.handoff.as_deref().filter(|text| {
            target.is_review_or_test()
                && comment_owed(
                    payload,
                    HANDOFF_COMMENT_KEY,
                    HANDOFF_COMMENT_DIGEST_KEY,
                    text,
                )
        }),
        move_to: moved.then_some(target.id),
        ticking: clock_runs_after(
            moved,
            stage_runs_automatically(Some(&target.as_ref()), switches),
            req.result,
            project_ticking_enabled,
            clock,
        ),
    }
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
    let target = match resolve_target(&choices, &req.to, switches, current_state_id) {
        Ok(target) => target.clone(),
        Err(refusal) => return Ok(refusal.into_response()),
    };
    let phase_kind = fetch_phase_kind(&pre.pool, run.facts.id).await?;
    let to_review_or_test = target.is_review_or_test();
    let earlier_handoff =
        if phase_kind == KIND_CODING_TASK && to_review_or_test && req.handoff.is_none() {
            current_handoff_exists(&pre.pool, issue_id).await?
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

    // Step 2: keep the hand-off as pending (the result is written last,
    // step 7). Every write merges only its own keys into the stored
    // payload, so a concurrent `yield` write is never overwritten from a
    // stale snapshot.
    let mut payload = progress_payload(run.done_payload.as_ref(), &req);
    if let Some(handoff) = req.handoff.as_ref() {
        merge_done_payload(
            &pre.pool,
            run.facts.id,
            &json!({ PENDING_HANDOFF_KEY: handoff }),
        )
        .await?;
    }

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

    let clock = fetch_clock_view(&pre.pool, issue_id, &project.policy).await?;
    let plan = plan_finish(
        &req,
        &payload,
        &target,
        current_state_id,
        switches,
        project.policy.agent_ticking_enabled.unwrap_or(true),
        clock,
    );

    // Step 3: the move, as an agent state patch, before anything that says
    // the issue moved. A refused move leaves no comment and no hand-off
    // behind (only the pending copy, which nothing reads).
    if let Some(target_id) = plan.move_to {
        let body = serde_json::to_vec(&json!({ "state": target_id.to_string() }))
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

    // Step 4: the hand-off counts only now that the issue moved (or stays).
    if let Some(handoff) = req.handoff.as_ref() {
        merge_done_payload(
            &pre.pool,
            run.facts.id,
            &json!({ STAGE_HANDOFF_KEY: handoff }),
        )
        .await?;
    }

    // Step 5: the comment.
    if let Some(comment) = plan.comment {
        // A comment this run already posted with the same body (an earlier
        // attempt whose id write failed) is reused, not posted twice.
        let existing = find_run_comment(&pre.pool, issue_id, run.facts.id, comment).await?;
        let id = match existing {
            Some(id) => Ok(id),
            None => {
                post_agent_comment(
                    state,
                    &forwarded,
                    slug,
                    &project_raw,
                    issue_id,
                    comment,
                    &req.as_agent,
                    run.facts.id,
                )
                .await?
            }
        };
        match id {
            Ok(id) => {
                let delta = json!({ FINISH_COMMENT_KEY: id, FINISH_COMMENT_DIGEST_KEY: text_digest(comment) });
                merge_done_payload(&pre.pool, run.facts.id, &delta).await?;
                if let Value::Object(delta) = delta {
                    payload.extend(delta);
                }
            }
            Err(response) => return Ok(response),
        }
    }

    // Step 6: the hand-off, shown to humans where review and test read it.
    if let Some(handoff) = plan.handoff_comment {
        // A comment this run already posted with the same body (an earlier
        // attempt whose id write failed) is reused, not posted twice.
        let existing = find_run_comment(&pre.pool, issue_id, run.facts.id, handoff).await?;
        let id = match existing {
            Some(id) => Ok(id),
            None => {
                post_agent_comment(
                    state,
                    &forwarded,
                    slug,
                    &project_raw,
                    issue_id,
                    handoff,
                    &req.as_agent,
                    run.facts.id,
                )
                .await?
            }
        };
        match id {
            Ok(id) => {
                let delta = json!({ HANDOFF_COMMENT_KEY: id, HANDOFF_COMMENT_DIGEST_KEY: text_digest(handoff) });
                merge_done_payload(&pre.pool, run.facts.id, &delta).await?;
                if let Value::Object(delta) = delta {
                    payload.extend(delta);
                }
            }
            Err(response) => return Ok(response),
        }
    }

    // Step 7: the result, now that every step succeeded.
    merge_done_payload(
        &pre.pool,
        run.facts.id,
        &Value::Object(result_delta(&req, &isoformat(&Utc::now()))),
    )
    .await?;

    Ok(finish_response(
        run.facts.id,
        &target.name,
        req.result,
        plan.ticking,
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

/// Merge `delta`'s keys into `agent_run.done_payload` in one statement
/// (`COALESCE(done_payload, '{}') || delta`), leaving every other key as
/// the database has it.
async fn merge_done_payload(pool: &PgPool, run_id: Uuid, delta: &Value) -> Result<(), Denial> {
    use sea_orm::QueryFilter as _;
    // `cust_with_values` binds the delta as one `$N`.
    // raw-sql: SeaORM cannot express the jsonb `||` merge itself.
    let merged = Expr::cust_with_values(
        "COALESCE(\"done_payload\", '{}'::jsonb) || $1::jsonb",
        [delta.clone()],
    );
    agent_run::Entity::update_many()
        .col_expr(agent_run::Column::DonePayload, merged)
        .filter(agent_run::Column::Id.eq(run_id))
        .exec(&sea_orm_connection(pool))
        .await
        .map(|_| ())
        .map_err(|error| db_error(error, "finish-write"))
}

/// The id of a live comment this run already posted on the issue with
/// exactly this body, if any.
async fn find_run_comment(
    pool: &PgPool,
    issue_id: Uuid,
    run_id: Uuid,
    markdown: &str,
) -> Result<Option<String>, Denial> {
    use pidash_db::entity::issue_comments;
    let html = pidash_services::assistant::markdown::to_safe_html(Some(markdown));
    let found: Option<Uuid> = issue_comments::Entity::find()
        .select_only()
        .column(issue_comments::Column::Id)
        .filter(issue_comments::Column::IssueId.eq(issue_id))
        .filter(issue_comments::Column::SpeakerAgentRunId.eq(run_id))
        .filter(issue_comments::Column::CommentHtml.eq(html))
        .filter(issue_comments::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&sea_orm_connection(pool))
        .await
        .map_err(|error| db_error(error, "finish-find-comment"))?;
    Ok(found.map(|id| id.to_string()))
}

/// The issue's clock for the response's `ticking`: `None` without a
/// ticker, else whether it can fire again.
async fn fetch_clock_view(
    pool: &PgPool,
    issue_id: Uuid,
    policy: &pidash_services::orchestration::clock::ProjectClockPolicy,
) -> Result<ClockView, Denial> {
    let rows = pidash_db::tasks_ticker::models::issue_agent_ticker::tickers_for_issue(
        &sea_orm_connection(pool),
        issue_id,
    )
    .await
    .map_err(|error| db_error(error, "finish-ticker"))?;
    let pool_size = pidash_services::orchestration::clock::pool_size(policy);
    Ok(rows
        .into_iter()
        .find(|ticker| ticker.deleted_at.is_none())
        .map(|ticker| !ticker.user_disabled && !ticker.cap_reached(pool_size)))
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

/// Whether an In Progress run stored a hand-off that still describes the
/// current work: one from a run created after the issue's latest review or
/// test run. After review or test sends the work back, the next In
/// Progress run must write a fresh hand-off; the pre-fix one would show
/// the next review stale "what was produced".
async fn current_handoff_exists(pool: &PgPool, issue_id: Uuid) -> Result<bool, Denial> {
    use sea_orm::QueryOrder;
    let db = sea_orm_connection(pool);
    let last_check: Option<sea_orm::prelude::DateTimeWithTimeZone> = agent_run::Entity::find()
        .select_only()
        .column(agent_run::Column::CreatedAt)
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
        .filter(agent_run::Column::PhaseKind.is_in(["review", "test"]))
        .order_by_desc(agent_run::Column::CreatedAt)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|error| db_error(error, "finish-last-check"))?;
    // The same test as the prompt's hand-off query (`BUNDLE_HANDOFF_SQL`),
    // so a JSON-null key counts as no hand-off in both places.
    let has_handoff =
        pidash_services::orchestration::creation::orm::done_payload_text(STAGE_HANDOFF_KEY);
    let mut query = agent_run::Entity::find()
        .select_only()
        .column(agent_run::Column::Id)
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
        .filter(agent_run::Column::PhaseKind.eq(KIND_CODING_TASK))
        .filter(Expr::expr(has_handoff).is_not_null());
    if let Some(after) = last_check {
        query = query.filter(agent_run::Column::CreatedAt.gt(after));
    }
    let found: Option<Uuid> = query
        .into_tuple()
        .one(&db)
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
        let err = resolve_target(&states, "Shipped", StageSwitches::ALL, None).unwrap_err();
        let (status, body) = body_of(err).await;
        assert_eq!(status, 400);
        // Done and Cancelled are for a person: never offered to an agent.
        assert_eq!(body["available"].as_array().unwrap().len(), 5);
        assert_eq!(
            resolve_target(&states, "In Review", StageSwitches::ALL, None)
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
            body_of(resolve_target(&states, "In Review", switches, None).unwrap_err()).await;
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
        assert!(resolve_target(&states, "In Progress", switches, None).is_ok());
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
    fn progress_keeps_stored_keys_and_the_result_delta_derives_stop_ticking() {
        // What the handler reads before the steps, and the keys its last
        // write merges in (`result_delta`, merged with `||`).
        let stored = json!({"status": "progressed", "finish_comment_id": "c1", "other": 1});
        let req = parse_finish_body(&json!({
            "to": "In Review", "result": "waiting_on_human", "comment": "?",
            "handoff": "### Hand-off", "note": "n"
        }))
        .unwrap();
        let progress = progress_payload(Some(&stored), &req);
        assert_eq!(progress[FINISH_COMMENT_KEY], json!("c1"));
        assert_eq!(progress["other"], json!(1));
        assert_eq!(
            progress["status"],
            json!("progressed"),
            "result not yet written"
        );
        let delta = result_delta(&req, "2026-11-19T00:00:00Z");
        assert_eq!(delta["status"], json!("waiting_on_human"));
        assert_eq!(delta["stop_ticking"], json!(true));
        assert_eq!(delta["note"], json!("n"));
        assert_eq!(delta["yielded_at"], json!("2026-11-19T00:00:00Z"));
        assert!(
            !delta.contains_key(STAGE_HANDOFF_KEY),
            "stored by its own step"
        );

        let done = parse_finish_body(&json!({"to": "AI Completed", "result": "done"})).unwrap();
        let delta = result_delta(&done, "t");
        assert_eq!(delta["stop_ticking"], json!(false));
        assert!(!delta.contains_key("note"));
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
            let target =
                resolve_target(&states, &req.to, StageSwitches::ALL, None).expect("target");
            let expected_ticking = !req.result.stops_ticking();
            assert_eq!(expected_ticking, ticking, "{name}");
            let got = body_of(finish_response(run, &target.name, req.result, ticking)).await;
            assert_eq!(got, (status, body), "{name}");
        }

        let (status, request, body) = fixture_call("finish_unknown_state");
        let req = parse_finish_body(&request).unwrap();
        let got =
            body_of(resolve_target(&states, &req.to, StageSwitches::ALL, None).unwrap_err()).await;
        assert_eq!(got, (status, body));

        let (status, request, body) = fixture_call("finish_disabled_stage");
        let req = parse_finish_body(&request).unwrap();
        let got = body_of(resolve_target(&states, &req.to, review_off, None).unwrap_err()).await;
        assert_eq!(got, (status, body));

        let (status, request, body) = fixture_call("finish_bad_result");
        let got = body_of(parse_finish_body(&request).unwrap_err()).await;
        assert_eq!(got, (status, body));

        let (status, request, body) = fixture_call("finish_missing_handoff");
        let req = parse_finish_body(&request).unwrap();
        let target = resolve_target(&states, &req.to, StageSwitches::ALL, None).unwrap();
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

    fn finish_req(body: Value) -> FinishRequest {
        parse_finish_body(&body).expect("valid")
    }

    #[test]
    fn a_manual_run_in_a_switched_off_stage_may_stay() {
        // §6.3: Run AI in a switched-off In Review must be able to finish
        // `--to "In Review"` (stay, waiting on a person).
        let states = project_states();
        let review = &states[2];
        let switches = StageSwitches {
            review: false,
            test: true,
        };
        let target = resolve_target(&states, "In Review", switches, Some(review.id))
            .expect("staying is allowed");
        assert_eq!(target.id, review.id);
        assert!(valid_targets(&states, switches, Some(review.id)).contains(&"In Review".to_owned()));
        // Moving into it from elsewhere is still refused.
        assert!(resolve_target(&states, "In Review", switches, Some(states[1].id)).is_err());
        assert!(
            !valid_targets(&states, switches, Some(states[1].id)).contains(&"In Review".to_owned())
        );
    }

    #[test]
    fn the_result_is_not_written_before_the_steps() {
        // A refused comment or move must not leave `stop_ticking: true`
        // behind for a later `yield --outcome progressed` to inherit.
        let req = finish_req(json!({
            "to": "In Progress", "result": "waiting_on_human", "comment": "?", "handoff": "h"
        }));
        let stored = json!({"status": "progressed", "stop_ticking": false});
        let progress = progress_payload(Some(&stored), &req);
        assert_eq!(progress["stop_ticking"], json!(false), "untouched");
        assert_eq!(progress["status"], json!("progressed"), "untouched");
        assert_eq!(progress[STAGE_HANDOFF_KEY], json!("h"));
        let done = result_delta(&req, "t");
        assert_eq!(done["stop_ticking"], json!(true));
        assert_eq!(done["status"], json!("waiting_on_human"));
    }

    #[test]
    fn plan_reposts_a_comment_whose_text_changed() {
        let states = project_states();
        let progress = states[1].id;
        let review = &states[2];
        let first = finish_req(json!({
            "to": "In Review", "result": "done", "comment": "old", "handoff": "h1"
        }));
        let mut stored = progress_payload(None, &first);
        stored.insert(FINISH_COMMENT_KEY.to_owned(), json!("c1"));
        stored.insert(
            FINISH_COMMENT_DIGEST_KEY.to_owned(),
            json!(text_digest("old")),
        );
        stored.insert(HANDOFF_COMMENT_KEY.to_owned(), json!("h-1"));
        stored.insert(
            HANDOFF_COMMENT_DIGEST_KEY.to_owned(),
            json!(text_digest("h1")),
        );
        // Same text: nothing to post again.
        let plan = plan_finish(
            &first,
            &stored,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!((plan.comment, plan.handoff_comment), (None, None));
        // Changed text: the new text is posted, not silently dropped.
        let second = finish_req(json!({
            "to": "In Review", "result": "done", "comment": "new", "handoff": "h2"
        }));
        let plan = plan_finish(
            &second,
            &stored,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(plan.comment, Some("new"));
        assert_eq!(plan.handoff_comment, Some("h2"));
        // A recorded id without a digest counts as posted.
        stored.remove(FINISH_COMMENT_DIGEST_KEY);
        let plan = plan_finish(
            &second,
            &stored,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(plan.comment, None);
    }

    #[test]
    fn plan_first_attempt_does_every_step() {
        let states = project_states();
        let progress = states[1].id;
        let review = &states[2];
        let req = finish_req(json!({
            "to": "In Review", "result": "done", "comment": "c", "handoff": "h"
        }));
        let payload = progress_payload(None, &req);
        let plan = plan_finish(
            &req,
            &payload,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(
            plan,
            FinishPlan {
                comment: Some("c"),
                handoff_comment: Some("h"),
                move_to: Some(review.id),
                ticking: true,
            }
        );
    }

    #[test]
    fn plan_retry_skips_what_already_happened() {
        let states = project_states();
        let progress = states[1].id;
        let review = &states[2];
        let req = finish_req(json!({
            "to": "In Review", "result": "done", "comment": "c", "handoff": "h"
        }));
        // First attempt posted the comment, then failed: the retry posts
        // only the hand-off and moves.
        let mut stored = progress_payload(None, &req);
        stored.insert(FINISH_COMMENT_KEY.to_owned(), json!("c1"));
        let payload = progress_payload(Some(&Value::Object(stored.clone())), &req);
        let plan = plan_finish(
            &req,
            &payload,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(plan.comment, None);
        assert_eq!(plan.handoff_comment, Some("h"));
        assert_eq!(plan.move_to, Some(review.id));

        // Both comments posted, move failed: the retry only moves.
        stored.insert(HANDOFF_COMMENT_KEY.to_owned(), json!("h1"));
        let payload = progress_payload(Some(&Value::Object(stored.clone())), &req);
        let plan = plan_finish(
            &req,
            &payload,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!((plan.comment, plan.handoff_comment), (None, None));
        assert_eq!(plan.move_to, Some(review.id));

        // Everything done: re-running after success changes nothing.
        let plan = plan_finish(
            &req,
            &payload,
            review,
            Some(review.id),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(
            (plan.comment, plan.handoff_comment, plan.move_to),
            (None, None, None)
        );
    }

    #[test]
    fn plan_posts_the_hand_off_only_for_review_or_test() {
        let states = project_states();
        let progress = states[1].id;
        let completed = &states[4];
        let req = finish_req(json!({
            "to": "AI Completed", "result": "done", "comment": "answer", "handoff": "h"
        }));
        let payload = progress_payload(None, &req);
        let plan = plan_finish(
            &req,
            &payload,
            completed,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(plan.comment, Some("answer"));
        assert_eq!(plan.handoff_comment, None, "stored, not posted");
        assert_eq!(payload[STAGE_HANDOFF_KEY], json!("h"));
        assert_eq!(plan.move_to, Some(completed.id));
    }

    #[test]
    fn ticking_matrix() {
        use FinishResult::*;
        // Moving into a stage that runs: its entry run follows even for a
        // result that would stop the clock in place.
        assert!(clock_runs_after(true, true, Done, true, None));
        assert!(clock_runs_after(true, true, Blocked, true, Some(true)));
        // ...unless a person switched the clock off or the budget is spent.
        assert!(!clock_runs_after(true, true, Done, true, Some(false)));
        // Staying: only when the result keeps it and the ticker can fire.
        assert!(clock_runs_after(false, true, Progressed, true, Some(true)));
        assert!(clock_runs_after(
            false,
            true,
            WaitingOnExternal,
            true,
            Some(true)
        ));
        assert!(!clock_runs_after(
            false,
            true,
            WaitingOnHuman,
            true,
            Some(true)
        ));
        assert!(!clock_runs_after(false, true, Blocked, true, Some(true)));
        assert!(
            !clock_runs_after(false, true, Progressed, true, Some(false)),
            "at cap"
        );
        assert!(
            !clock_runs_after(false, true, Progressed, true, None),
            "no ticker"
        );
        // A state that does not run, or automated runs off: never.
        assert!(!clock_runs_after(true, false, Done, true, None));
        assert!(!clock_runs_after(true, true, Done, false, None));
    }

    #[test]
    fn a_stop_result_with_a_move_still_reports_the_entry_run() {
        let states = project_states();
        let progress = states[1].id;
        let review = &states[2];
        let req = finish_req(
            json!({"to": "In Review", "result": "blocked", "comment": "c", "handoff": "h"}),
        );
        let payload = progress_payload(None, &req);
        let plan = plan_finish(
            &req,
            &payload,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            None,
        );
        assert!(plan.ticking);
    }

    #[tokio::test]
    async fn waiting_on_a_person_needs_a_comment() {
        for result in ["waiting_on_human", "blocked"] {
            let (status, body) = body_of(
                parse_finish_body(&json!({"to": "In Progress", "result": result})).unwrap_err(),
            )
            .await;
            assert_eq!(status, 400, "{result}");
            assert!(
                body["error"].as_str().unwrap().contains("--comment-file"),
                "{result}"
            );
            assert!(parse_finish_body(
                &json!({"to": "In Progress", "result": result, "comment": "Which API?"})
            )
            .is_ok());
        }
        // The other results may finish quietly.
        for result in ["done", "progressed", "waiting_on_external"] {
            assert!(parse_finish_body(&json!({"to": "In Progress", "result": result})).is_ok());
        }
    }

    #[tokio::test]
    async fn a_blank_comment_or_hand_off_is_refused() {
        for key in ["comment", "handoff"] {
            let (status, body) = body_of(
                parse_finish_body(
                    &json!({"to": "In Progress", "result": "waiting_on_human", key: "  \n"}),
                )
                .unwrap_err(),
            )
            .await;
            assert_eq!(status, 400, "{key}");
            assert!(body["error"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{key} is empty")));
        }
        // Leaving the flag out is fine.
        assert!(parse_finish_body(&json!({"to": "In Progress", "result": "progressed"})).is_ok());
    }

    #[tokio::test]
    async fn done_and_cancelled_are_for_a_person() {
        let mut states = project_states();
        states.push(choice("Cancelled", "cancelled"));
        let progress = states[1].id;
        for name in ["Done", "Cancelled"] {
            let (status, body) = body_of(
                resolve_target(&states, name, StageSwitches::ALL, Some(progress)).unwrap_err(),
            )
            .await;
            assert_eq!(status, 400, "{name}");
            let available: Vec<&str> = body["available"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert!(!available.contains(&name), "{name}");
            assert!(available.contains(&"AI Completed"));
        }
        // AI Completed is the agent's end state.
        assert!(
            resolve_target(&states, "AI Completed", StageSwitches::ALL, Some(progress)).is_ok()
        );
        // An issue a person already closed may stay where it is.
        let done = states[5].id;
        assert!(resolve_target(&states, "Done", StageSwitches::ALL, Some(done)).is_ok());
    }

    #[test]
    fn review_or_test_targets() {
        assert!(choice("In Review", "review").is_review_or_test());
        assert!(choice("In Test", "test").is_review_or_test());
        assert!(!choice("In Progress", "started").is_review_or_test());
        assert!(!choice("AI Completed", "completed").is_review_or_test());
    }
}
