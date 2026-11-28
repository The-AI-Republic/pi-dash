//! `POST .../agent-runs/<run_id>/finish/` — the one closing command of an
//! issue run (lean agent workflow, design §8.1).
//!
//! `pidash run finish` replaces the four calls a run used to end with
//! (`workpad update`, `comment add`, `issue patch --state`, `run yield`).
//! The endpoint:
//!
//! 1. validates the target state (by exact name; never a move into a
//!    stage the project switched off, though staying put is always
//!    allowed), the result, and the workpad gate (an In Progress run must
//!    have updated the workpad before it moves the issue to review or
//!    test; `.ai_design/unified_workpad/design.md` §8.2);
//! 2. moves the issue through the issue-update operation with the run as
//!    mover, so the transition (and an enabled stage's entry run) fires
//!    as for any agent move — first, so a refused move leaves nothing
//!    that claims it happened;
//! 3. posts the comment, attributed to the run;
//! 4. posts a legacy hand-off as a comment when the issue goes to
//!    review/test (compatibility window only: prompts no longer send
//!    one, and nothing stores it);
//! 5. merges the outcome into `done_payload` — the same keys `yield`
//!    writes, with `stop_ticking` *derived* from the result. Written last,
//!    so a step refused on the way leaves no stop signal behind.
//!
//! The steps are not one transaction (the comment and issue operations
//! own theirs). They are ordered so a retry is safe: each comment carries
//! a run-finish external id, so a retry that already posted hits the
//! comment operation's own dedupe (`409`) and continues instead of posting
//! twice (a changed text carries a new digest, so it is posted again),
//! and the move is a no-op when the issue already sits in the target state.
//!
//! Scheduler runs (no issue) are refused; they keep using `yield`.

use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_compat::response::json_response;
use pidash_db::entity::{agent_run, issues, states};
use pidash_db::pool::sea_orm_connection;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{WriteActor, WriteCtx, WriteScope};
use pidash_services::operations;
use pidash_services::orchestration::creation;
use pidash_types::orchestration::{
    agent_move_refusal, phase_config_for, stage_runs_automatically, template_name_for,
    AgentMoveRefusal, StageSwitches, StateRef, KIND_CODING_TASK, OUTCOME_BLOCKED, OUTCOME_DONE,
    OUTCOME_PROGRESSED, OUTCOME_WAITING_ON_EXTERNAL, OUTCOME_WAITING_ON_HUMAN,
};

use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::state::AppState;

use super::handlers_actions::{
    activate_timezone, db_error, fetch_project, fetch_yield_run, is_workspace_member, isoformat,
    preamble, truncate_note, Denial, HEADER_MISMATCH_BODY,
};
use super::handlers_core::{
    app_origin, drain_outcome_pod, fetch_issue_for_write, fetch_project_for_write, now_utc,
    render_write_response, require_gate,
};
use super::perms::{run_belongs_to, V1WorkItemsRoute, RUN_ID_HEADER};

/// The hand-off cap, in characters (§8.2).
pub const HANDOFF_MAX_CHARS: usize = 20_000;
/// The comment label when the caller names no agent runtime.
pub const DEFAULT_AGENT_LABEL: &str = "AI Agent";

/// `done_payload` key that held the In Progress hand-off before the
/// unified workpad (`.ai_design/unified_workpad/design.md` U2, U8). Nothing
/// writes or reads it any more; old `done_payload` rows may still carry it.
pub const STAGE_HANDOFF_KEY: &str = "stage_handoff";
/// The comment operation's dedupe source for `finish` posts (NEWPIDASH-338
/// item 6): retries carry the same external id, so a retry that already
/// posted hits the `409` instead of posting twice.
pub const RUN_FINISH_SOURCE: &str = "run-finish";

/// Hex SHA-256 of a comment body: the retry guard's text check, carried in
/// the external id (same text retries dedupe; changed text posts again).
pub fn text_digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// The comment operation's dedupe id for one `finish` post: the run, the
/// slot (`comment` or `handoff`), and the text digest.
pub fn finish_external_id(run_id: Uuid, slot: &str, text: &str) -> String {
    format!("{run_id}:{slot}:{}", text_digest(text))
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

    /// The wire names, derived from the one vocabulary in
    /// `pidash_types::orchestration` (NEWPIDASH-338 item 10): the api list
    /// can no longer drift from the types crate. Order of [`FinishResult::ALL`]
    /// is untouched (the F18-11 golden pins the `allowed` order).
    pub fn as_wire(self) -> &'static str {
        match self {
            FinishResult::Done => OUTCOME_DONE,
            FinishResult::Progressed => OUTCOME_PROGRESSED,
            FinishResult::WaitingOnExternal => OUTCOME_WAITING_ON_EXTERNAL,
            FinishResult::WaitingOnHuman => OUTCOME_WAITING_ON_HUMAN,
            FinishResult::Blocked => OUTCOME_BLOCKED,
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
    // NEWPIDASH-338 item 5: identical texts would collapse into one
    // comment (the hand-off lookup matches the just-posted comment by
    // body), so refuse them with a clear message instead of silently
    // posting once.
    if let (Some(comment_text), Some(handoff_text)) = (comment.as_deref(), handoff.as_deref()) {
        if comment_text.trim() == handoff_text.trim() {
            return Err(bad_request(
                "comment and handoff are identical: write them separately, or leave one flag out",
                &[],
            ));
        }
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
        // NEWPIDASH-338 item 4: never name a state the project does not
        // have. With no "AI Completed" state the agent retries the named
        // state, gets "unknown state", and burns retries without recording
        // a result — so name staying + `waiting_on_human` instead, as the
        // lifecycle prompt does.
        use pidash_types::orchestration::AI_COMPLETED_STATE_NAME;
        let names_ai_completed = states
            .iter()
            .any(|s| s.group == "completed" && s.name == AI_COMPLETED_STATE_NAME);
        let guidance = if names_ai_completed {
            "finish in \"AI Completed\" or the current state instead".to_owned()
        } else {
            "stay in the current state and finish with `waiting_on_human` instead".to_owned()
        };
        return Err(bad_request(
            &format!("\"{to}\" is for a person to set; {guidance}"),
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

/// The workpad gate (step 1; `.ai_design/unified_workpad/design.md` §8.2):
/// an In Progress (`coding-task`) run that moves the issue into review or
/// test must have changed the workpad since the In Progress pass started,
/// so the next stage finds a note that describes this pass. The caller
/// applies it only to that move, and not when a legacy hand-off is sent
/// (it is posted as a comment, which review and test read).
///
/// `stamped_sha` is the workpad hash when the pass started
/// ([`fetch_workpad_hashes`]); a run created before NEWPIDASH-320 carries
/// none and is not gated.
pub fn check_workpad_written(stamped_sha: Option<&str>, current_sha: &str) -> Result<(), Refusal> {
    match stamped_sha {
        Some(stamped) if stamped == current_sha => Err(bad_request(WORKPAD_UNCHANGED_MESSAGE, &[])),
        _ => Ok(()),
    }
}

/// The [`check_workpad_written`] refusal.
pub const WORKPAD_UNCHANGED_MESSAGE: &str =
    "update the workpad before moving to review or test: note where the work is and how to check it";

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

/// What a `finish` call still has to do (steps 3–5 and the response's
/// `ticking`). Pure, so the step rules are unit-tested: the comments post
/// whenever the request carries them (retries dedupe through the
/// comment operation's external id, not through this plan), and the move
/// is skipped when the issue already sits in the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishPlan<'a> {
    /// Step 3: the comment to post, if the request carries one.
    pub comment: Option<&'a str>,
    /// Step 4: a legacy hand-off to post as a comment, if the request carries
    /// one. Only for a move into review or test, where humans read what
    /// the stage reads.
    pub handoff_comment: Option<&'a str>,
    /// Step 2: the state to move to, if the issue is not there yet.
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

pub fn plan_finish<'a>(
    req: &'a FinishRequest,
    target: &StateChoice,
    current_state: Option<Uuid>,
    switches: StageSwitches,
    project_ticking_enabled: bool,
    clock: ClockView,
) -> FinishPlan<'a> {
    let moved = current_state != Some(target.id);
    FinishPlan {
        comment: req.comment.as_deref(),
        handoff_comment: req
            .handoff
            .as_deref()
            .filter(|_| target.is_review_or_test()),
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
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let data: Value = if raw_body.is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_slice(raw_body)
            .map_err(|error| Denial::BadDetail(format!("JSON parse error - {error}")))?
    };
    let mut req = match parse_finish_body(&data) {
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
    // The run row is already loaded: no second select for its phase kind.
    // A project-move replacement run stamps no kind but renders the prompt
    // for the issue's state (`create_project_move_handoff_run`), so an
    // empty kind is read from the state the run is finishing in.
    let phase_kind = if run.phase_kind.is_empty() {
        let current = current_state_id.and_then(|id| choices.iter().find(|c| c.id == id));
        template_name_for(current.map(StateChoice::as_ref).as_ref()).to_owned()
    } else {
        run.phase_kind.clone()
    };
    // A legacy hand-off (a prompt rendered before the unified workpad) is
    // stored nowhere. An In Progress run's is still posted as a comment on
    // a move to review or test (step 4); any other run's is dropped.
    if req.handoff.is_some() {
        tracing::info!(run_id = %run.facts.id, phase_kind = %phase_kind, "finish: legacy hand-off received; not stored");
        if phase_kind != KIND_CODING_TASK {
            req.handoff = None;
        }
    }
    let moving_to_review_or_test =
        current_state_id != Some(target.id) && target.is_review_or_test();
    if phase_kind == KIND_CODING_TASK && moving_to_review_or_test && req.handoff.is_none() {
        let (stamped, current) = fetch_workpad_hashes(&pre.pool, run.facts.id, issue_id).await?;
        if let Err(refusal) = check_workpad_written(stamped.as_deref(), &current) {
            return Ok(refusal.into_response());
        }
    }

    let clock = fetch_clock_view(&pre.pool, issue_id, &project.policy).await?;
    let plan = plan_finish(
        &req,
        &target,
        current_state_id,
        switches,
        project.policy.agent_ticking_enabled.unwrap_or(true),
        clock,
    );

    // The gates the composed handler inners used to run, kept now that
    // `finish` calls the operations directly (NEWPIDASH-338 item 7). The
    // move is gated exactly like the issue-patch path (its gate plus the
    // archived check); each posted comment like the comment-create path.
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    // The core helpers answer the core `Denial`: render it directly, as
    // the handler inners did, instead of converting into this module's.
    if plan.move_to.is_some() {
        if let Err(denial) = require_gate(
            &pre.pool,
            &workspace_id,
            slug,
            &pre.actor.id,
            Some(&project_id),
            None,
            V1WorkItemsRoute::IssueDetail,
            "PATCH",
        )
        .await
        {
            return Ok(denial.into_response());
        }
        refuse_if_archived(&pre.pool, project_id).await?;
    }
    if plan.comment.is_some() || plan.handoff_comment.is_some() {
        if let Err(denial) = require_gate(
            &pre.pool,
            &workspace_id,
            slug,
            &pre.actor.id,
            Some(&project_id),
            None,
            V1WorkItemsRoute::CommentList,
            "POST",
        )
        .await
        {
            return Ok(denial.into_response());
        }
        // The comment endpoint's own gate carries the archived check, so
        // a stay that only posts still refuses on an archived project,
        // exactly like the composed `comment_post_inner` did.
        refuse_if_archived(&pre.pool, project_id).await?;
    }

    // One shared operation context for the move and both comments: the
    // same actor, scope and hooks the handler inners built per call.
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = WriteCtx::new(WriteActor {
        user_id: pre.actor.id,
        timezone: pre.actor.timezone.clone(),
    })
    .with_scope(WriteScope {
        workspace_id: pre.workspace_id,
        project_id: Some(project_id),
    });
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let deps = operations::Deps {
        pool: pre.pool.clone(),
        orm: orm.clone(),
        transition,
        enqueue,
    };
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let origin = app_origin(&state.settings().urls).ok();
    // The `created_at` override resolver: `finish` never sends one, so the
    // comment is stamped now, exactly like the endpoint's absent-override
    // arm.
    let now = now_utc();
    let rendered_now = crate::serializer::render_datetime_in(&now, &tz);
    let resolve_created_at =
        move |_: Option<&Value>| -> Result<(DateTime<Utc>, String), operations::Error> {
            Ok((now, rendered_now.clone()))
        };

    // Step 2: the move, through the issue-update operation with the run as
    // mover, before anything that says the issue moved. A refused move
    // leaves no comment behind.
    //
    // Attribution and gates preserved from the patch path: `moved_by_run`
    // is this run (the inner path inferred the same id from the forwarded
    // run header); the work-type lock cannot fire (`finish` never sends
    // `work_type`); the `issue_run_policy` gate skips agent self-actions
    // exactly as before (`moved_by_run` set); and `resolve_target` above
    // enforces the same rule as the agent-move gate on the same switches.
    if let Some(target_id) = plan.move_to {
        let current =
            match fetch_issue_for_write(&pre.pool, slug, &project_id, &issue_id, &tz).await {
                Ok(current) => current,
                Err(denial) => return Ok(denial.into_response()),
            };
        let write_project = match fetch_project_for_write(&pre.pool, &project_id).await {
            Ok(project) => project,
            Err(denial) => return Ok(denial.into_response()),
        };
        let current_rendered = match render_write_response(
            &pre.pool,
            &current,
            &tz,
            web_base.as_deref(),
            pre.actor.id,
        )
        .await
        {
            Ok(rendered) => rendered,
            Err(denial) => return Ok(denial.into_response()),
        };
        let current_text =
            pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&current_rendered);
        let move_body = json!({ "state": target_id.to_string() });
        let write = operations::issues::IssueWrite {
            slug,
            project_id,
            workspace_id,
            project: write_project,
            body: &move_body,
            from_form: false,
            origin: origin.clone(),
            web_base: web_base.clone(),
        };
        let update_target = operations::issues::UpdateTarget {
            current: &current,
            current_instance: current_text,
            moved_by_run: Some(run.facts.id),
        };
        let outcome = match operations::issues::update(
            &deps,
            &ctx,
            issue_id,
            &write,
            &update_target,
            operations::issues::UpdateOptions {
                emit_activity: true,
                pod_once: pidash_services::orchestration::creation::pod_once_requested(&move_body),
            },
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => return Ok(op_failure(error)),
        };
        if let Err(denial) = drain_outcome_pod(state, &outcome).await {
            return Ok(denial.into_response());
        }
    }

    // Step 3: the comment.
    if let Some(comment) = plan.comment {
        if let Some(response) = post_finish_comment(
            &deps,
            &ctx,
            &resolve_created_at,
            slug,
            project_id,
            issue_id,
            origin.clone(),
            web_base.clone(),
            comment,
            &req.as_agent,
            run.facts.id,
            "comment",
        )
        .await
        {
            return Ok(response);
        }
    }

    // Step 4: a legacy hand-off (compatibility window), posted where
    // review and test look for one when the workpad does not say.
    if let Some(handoff) = plan.handoff_comment {
        if let Some(response) = post_finish_comment(
            &deps,
            &ctx,
            &resolve_created_at,
            slug,
            project_id,
            issue_id,
            origin.clone(),
            web_base.clone(),
            handoff,
            &req.as_agent,
            run.facts.id,
            "handoff",
        )
        .await
        {
            return Ok(response);
        }
    }

    // Step 5: the result, now that every step succeeded.
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

/// The archived-project refusal the composed handler inners ran: the
/// patch path checked it after its gate and the comment endpoint's own
/// gate carries it. Each `finish` branch checks it after its gate, so a
/// move and a comment-only stay are each refused on an archived project,
/// exactly as before.
async fn refuse_if_archived(pool: &PgPool, project_id: Uuid) -> Result<(), Denial> {
    if let Some(info) = crate::project_archived::archived_by_id(pool, project_id)
        .await
        .map_err(|_| Denial::ServerError)?
    {
        return Err(Denial::ProjectArchived(
            crate::project_archived::archived_body(&info),
        ));
    }
    Ok(())
}

/// Map an operation failure onto the exact status and body the v1
/// endpoint would have answered (the operation's own mapping): the move
/// and the comments no longer pass through their handler inners, but a
/// refused step still answers byte-identically.
fn op_failure(error: operations::Error) -> Response {
    let (status, body) = error.status_and_body();
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    json_response(status, pidash_compat::response::escape_u2028(&body))
}

/// Post one `finish` comment through the comment-create operation (steps
/// 5 and 6 share this helper). The body carries the run-finish external
/// id, so a retry that already posted hits the operation's own dedupe
/// (`409 Conflict`) and continues instead of posting twice; any other
/// failure answers with the operation's own status and body, exactly as
/// the comment endpoint would. Returns the refusal response when the
/// post failed, `None` on success.
#[allow(clippy::too_many_arguments)]
async fn post_finish_comment(
    deps: &operations::Deps,
    ctx: &WriteCtx,
    resolve_created_at: &operations::comments::CreatedAtResolver,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    origin: Option<String>,
    web_base: Option<String>,
    markdown: &str,
    as_agent: &str,
    run_id: Uuid,
    slot: &str,
) -> Option<Response> {
    let body = json!({
        "comment_html": pidash_services::assistant::markdown::to_safe_html(Some(markdown)),
        "speaker_type": "agent",
        "speaker_label": as_agent,
        "speaker_agent_run_id": run_id.to_string(),
        "external_source": RUN_FINISH_SOURCE,
        "external_id": finish_external_id(run_id, slot, markdown),
    });
    let write = operations::comments::CommentWrite {
        slug,
        project_id,
        issue_id,
        body: &body,
        from_form: false,
        origin,
        web_base,
    };
    match operations::comments::create(
        deps,
        ctx,
        &write,
        resolve_created_at,
        operations::comments::CommentCreateOptions {
            emit_activity: true,
            emit_model_activity: true,
        },
    )
    .await
    {
        Ok(_) => None,
        // Already posted by an earlier attempt: the retry dedupe hit.
        Err(operations::Error::Conflict(_)) => None,
        Err(error) => Some(op_failure(error)),
    }
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

/// The workpad hash when the run's In Progress pass started and the hash of
/// the issue's workpad now, for [`check_workpad_written`]. The start hash is
/// `run_config.stage_start_workpad_sha256` (a continuation carries its
/// parent's, [`creation::STAGE_START_WORKPAD_SHA256`]), else the run's own
/// `workpad_sha256`; `None` for a run created before NEWPIDASH-320.
async fn fetch_workpad_hashes(
    pool: &PgPool,
    run_id: Uuid,
    issue_id: Uuid,
) -> Result<(Option<String>, String), Denial> {
    let db = sea_orm_connection(pool);
    let run_config: Option<Value> = agent_run::Entity::find_by_id(run_id)
        .select_only()
        .column(agent_run::Column::RunConfig)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|error| db_error(error, "finish-run-config"))?;
    let stamped = run_config
        .as_ref()
        .and_then(|config| {
            config
                .get(creation::STAGE_START_WORKPAD_SHA256)
                .or_else(|| config.get("workpad_sha256"))
        })
        .and_then(Value::as_str)
        .map(str::to_owned);
    let workpad: Option<String> = issues::Entity::find_by_id(issue_id)
        .select_only()
        .column(issues::Column::Workpad)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|error| db_error(error, "finish-workpad"))?;
    let current = creation::workpad_sha256(workpad.as_deref().unwrap_or_default());
    Ok((stamped, current))
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
    fn finish_result_names_come_from_the_types_crate() {
        // NEWPIDASH-338 item 10: the wire names are the OUTCOME_* consts,
        // not literals that can drift.
        assert_eq!(FinishResult::Done.as_wire(), OUTCOME_DONE);
        assert_eq!(FinishResult::Progressed.as_wire(), OUTCOME_PROGRESSED);
        assert_eq!(
            FinishResult::WaitingOnExternal.as_wire(),
            OUTCOME_WAITING_ON_EXTERNAL
        );
        assert_eq!(
            FinishResult::WaitingOnHuman.as_wire(),
            OUTCOME_WAITING_ON_HUMAN
        );
        assert_eq!(FinishResult::Blocked.as_wire(), OUTCOME_BLOCKED);
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
    async fn workpad_gate() {
        // A coding run moving to review with the workpad unchanged since
        // the run was created is refused...
        let (status, body) =
            body_of(check_workpad_written(Some("same"), "same").unwrap_err()).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"], json!(WORKPAD_UNCHANGED_MESSAGE));
        // ...and allowed once the workpad changed.
        assert!(check_workpad_written(Some("old"), "new").is_ok());
        // A run created before NEWPIDASH-320 carries no stamped hash.
        assert!(check_workpad_written(None, "same").is_ok());
    }

    #[test]
    fn finish_external_ids_carry_run_slot_and_digest() {
        // NEWPIDASH-338 item 6: retries dedupe through the comment
        // operation's external id. Same text retries the same id (the
        // `409` continues); changed text carries a new digest (posted
        // again); the two slots never collide.
        let run = Uuid::new_v4();
        let comment = finish_external_id(run, "comment", "same text");
        assert_eq!(comment, finish_external_id(run, "comment", "same text"));
        assert_ne!(comment, finish_external_id(run, "comment", "changed text"));
        assert_ne!(
            comment,
            finish_external_id(run, "handoff", "same text"),
            "slots differ even for identical texts"
        );
        assert_ne!(
            comment,
            finish_external_id(Uuid::new_v4(), "comment", "same text"),
            "runs differ"
        );
        assert!(comment.starts_with(&format!("{run}:comment:")));
    }

    #[test]
    fn result_delta_derives_stop_ticking() {
        // The last write merges in (`result_delta`, merged with `||`):
        // the result, its time, the note, and the derived stop signal.
        let req = parse_finish_body(&json!({
            "to": "In Review", "result": "waiting_on_human", "comment": "?",
            "handoff": "### Hand-off", "note": "n"
        }))
        .unwrap();
        let delta = result_delta(&req, "2026-11-19T00:00:00Z");
        assert_eq!(delta["status"], json!("waiting_on_human"));
        assert_eq!(delta["stop_ticking"], json!(true));
        assert_eq!(delta["note"], json!("n"));
        assert_eq!(delta["yielded_at"], json!("2026-11-19T00:00:00Z"));
        assert!(
            !delta.contains_key(STAGE_HANDOFF_KEY),
            "the hand-off is never stored"
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

        let (status, request, body) = fixture_call("finish_workpad_unchanged");
        let req = parse_finish_body(&request).unwrap();
        let target = resolve_target(&states, &req.to, StageSwitches::ALL, None).unwrap();
        assert!(target.is_review_or_test() && req.handoff.is_none());
        let got = body_of(check_workpad_written(Some("unchanged"), "unchanged").unwrap_err()).await;
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
        // behind for a later `yield --outcome progressed` to inherit:
        // step 5 writes the result only after every step succeeded.
        let req = finish_req(json!({
            "to": "In Progress", "result": "waiting_on_human", "comment": "?", "handoff": "h"
        }));
        let delta = result_delta(&req, "t");
        assert_eq!(delta["stop_ticking"], json!(true));
        assert_eq!(delta["status"], json!("waiting_on_human"));
        assert!(
            !delta.contains_key(STAGE_HANDOFF_KEY),
            "the hand-off is never stored"
        );
    }

    #[test]
    fn plan_posts_whatever_the_request_carries() {
        // NEWPIDASH-338 item 6: the plan no longer tracks posted ids —
        // retries dedupe through the comment operation's external id.
        // Same request, same plan; a retry posts the same external ids
        // and the `409` continues.
        let states = project_states();
        let progress = states[1].id;
        let review = &states[2];
        let first = finish_req(json!({
            "to": "In Review", "result": "done", "comment": "old", "handoff": "h1"
        }));
        let plan = plan_finish(
            &first,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(plan.comment, Some("old"));
        assert_eq!(plan.handoff_comment, Some("h1"));
        // A retry of the same request plans the same posts.
        let plan = plan_finish(
            &first,
            review,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(
            (plan.comment, plan.handoff_comment),
            (Some("old"), Some("h1"))
        );
        // The stay case: already in the target, only the comments post.
        let plan = plan_finish(
            &first,
            review,
            Some(review.id),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(
            (plan.comment, plan.handoff_comment, plan.move_to),
            (Some("old"), Some("h1"), None)
        );
    }

    #[test]
    fn plan_first_attempt_does_every_step() {
        let states = project_states();
        let progress = states[1].id;
        let review = &states[2];
        let req = finish_req(json!({
            "to": "In Review", "result": "done", "comment": "c", "handoff": "h"
        }));
        let plan = plan_finish(
            &req,
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
    fn plan_posts_the_hand_off_only_for_review_or_test() {
        let states = project_states();
        let progress = states[1].id;
        let completed = &states[4];
        let req = finish_req(json!({
            "to": "AI Completed", "result": "done", "comment": "answer", "handoff": "h"
        }));
        let plan = plan_finish(
            &req,
            completed,
            Some(progress),
            StageSwitches::ALL,
            true,
            Some(true),
        );
        assert_eq!(plan.comment, Some("answer"));
        assert_eq!(plan.handoff_comment, None, "stored, not posted");
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
        let plan = plan_finish(&req, review, Some(progress), StageSwitches::ALL, true, None);
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
    async fn identical_comment_and_hand_off_are_refused() {
        // NEWPIDASH-338 item 5: same text twice would post once (the
        // hand-off lookup finds the just-posted comment), so refuse it.
        let (status, body) = body_of(
            parse_finish_body(&json!({
                "to": "In Review", "result": "done",
                "comment": "same text", "handoff": "same text",
            }))
            .unwrap_err(),
        )
        .await;
        assert_eq!(status, 400);
        assert!(
            body["error"].as_str().expect("error").contains("identical"),
            "{body}"
        );
        // Different texts still pass.
        assert!(parse_finish_body(&json!({
            "to": "In Review", "result": "done",
            "comment": "the answer", "handoff": "### Hand-off",
        }))
        .is_ok());
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

        // NEWPIDASH-338 item 4: in a project with no "AI Completed" state
        // the refusal names only states that exist — never "AI Completed".
        let mut bare = project_states();
        bare.retain(|s| s.name != "AI Completed");
        let progress = bare[1].id;
        let (status, body) =
            body_of(resolve_target(&bare, "Done", StageSwitches::ALL, Some(progress)).unwrap_err())
                .await;
        assert_eq!(status, 400);
        let error = body["error"].as_str().expect("error");
        assert!(
            error.contains("waiting_on_human"),
            "stay guidance, not a missing state: {error}"
        );
        assert!(
            !error.contains("AI Completed"),
            "never names a state the project lacks: {error}"
        );
        assert!(
            !body["available"]
                .as_array()
                .expect("available")
                .iter()
                .any(|v| v == "AI Completed"),
            "available lists only real states"
        );
        // With the state present the old guidance stays.
        let (status, body) = body_of(
            resolve_target(&states, "Done", StageSwitches::ALL, Some(progress)).unwrap_err(),
        )
        .await;
        assert_eq!(status, 400);
        assert!(body["error"]
            .as_str()
            .expect("error")
            .contains("AI Completed"));
    }

    #[test]
    fn review_or_test_targets() {
        assert!(choice("In Review", "review").is_review_or_test());
        assert!(choice("In Test", "test").is_review_or_test());
        assert!(!choice("In Progress", "started").is_review_or_test());
        assert!(!choice("AI Completed", "completed").is_review_or_test());
    }
}
