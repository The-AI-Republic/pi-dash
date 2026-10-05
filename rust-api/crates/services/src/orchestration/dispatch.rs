#![forbid(unsafe_code)]

//! Dispatch remainder (D-12 L8, stage 5): preflight + bounce, creator/pod
//! resolvers, continuation dispatch, the Run AI trio, Re-tick, wait, the
//! deferred cap-hit pause.
//!
//! The services half of `orchestration/scheduling.py:650-1470`: every
//! function below is Python control flow over the [`DispatchSeam`] trait
//! (which extends the L6 [`CreationSeam`][creation::CreationSeam] /
//! [`FinalizeAgentRunSeam`][creation::FinalizeAgentRunSeam] seams, so the
//! guards, builders and clock writes share one insertion transaction —
//! the single-transaction grant+dispatch with rollback). The jobs-side
//! `fire_tick_seam` module implements the seam over live SQL plus the
//! `tasks_ticker::FireTickSeam` shim; this module never names
//! `pidash_jobs` (jobs → services exists — it would cycle).
//!
//! Reuse, never fork: guards and builders come from
//! [`creation`][crate::orchestration::creation] (`active_run_for`,
//! `latest_prior_run`, `parent_for_next_run`, `resolve_fallback_creator`,
//! `resolve_pod_for_issue`, `create_and_dispatch_run`,
//! `create_continuation_run`), clock math and events from
//! [`clock`][crate::orchestration::clock] (`reconcile`, `on_retick` via
//! the event, `reset_ticker_after_comment_and_run`, `disarm_ticker`,
//! `compute_next_run_at`, `effective_interval_seconds`), executor policy
//! from L3/L4 ([`effective_executor_for_issue`][crate::dispatch::effective_executor_for_issue],
//! [`user_has_llm_config`][crate::dispatch::user_has_llm_config],
//! [`LlmProfile`][crate::dispatch::LlmProfile],
//! [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`][crate::dispatch::ENROLLED_MANAGED_RUNNERS_EXISTS_SQL])
//! and the role verdict from
//! [`check_project_role`][pidash_auth::permissions::membership::check_project_role].
//!
//! Signal side-effects (the state saves below) become explicit
//! [`clock::reconcile`] calls — a bounce / deferred-pause move is
//! `left_bucket`, the Re-tick paused-move is `entered_bucket` with
//! `want_run=false` (the `dispatch_immediate=False` save) — executed on
//! the same seam, with the [`reconcile_log_line`][clock::reconcile_log_line]
//! collected like any other log line. No L7 edge.
//!
//! Logging: Python logs through the `scheduling` logger mid-flow; the
//! port collects [`LogLine`]s on each outcome in Python order and the
//! jobs drivers emit them via `tracing` after commit. Fixtures judge the
//! values, not the mechanism.
//!
//! [`PodRunnerMatcher`] is the one-method D-14 seam (`runner D-14`,
//! downstream of D-12 — no edge, it would deadlock): D-14 implements it
//! later; drivers take it as `&dyn` per call and tests script it.
//!
//! Fixture id replayed by the suite below: FX-ORCH-08
//! (`rust-api/fixtures/orchestration/fx08_dispatch/`); the
//! builder-success paths replay live in jobs.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Ported bugs and quirks (translate, don't redesign):
//!
//! * `update_fields` writes ONLY the listed columns (Django 4.2
//!   `_save_table` — no `auto_now` bump beyond `updated_at`, and the
//!   `updated_by = bot/creator` assignments on the pause / Re-tick moves
//!   never reach the row, so U11's `updated_by` stays NULL).
//! * The bounce bodies are `format_html` calls with no args — plain
//!   literals, pinned byte-verbatim ([`BOUNCE_BODY_DEFAULT`],
//!   [`BOUNCE_BODY_NO_LLM_CONFIG`]).

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use pidash_auth::permissions::membership::{check_project_role, ProjectRoleFacts};
use pidash_db::orchestration::workpad::AgentUserCollisionError;
use pidash_db::tasks_ticker::models::issue_agent_ticker::{
    IssueAgentTicker, COLUMNS as TICKER_COLUMNS,
};
use pidash_db::tasks_ticker::{pool_size_or_default, TickerDisarmReason};
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use pidash_types::orchestration::{
    auto_pauses_on_cap, is_ticking_state, ticking_state_names_by_group, MACHINE_TRIGGERS,
    RUN_AI_ACTIVE_RUN_EXISTS, RUN_AI_NO_ELIGIBLE_RUNNER, RUN_AI_NO_POD, TRIGGER_RUN_AI,
    WAIT_CAP_REACHED, WAIT_GRANTED, WAIT_INFINITE_POOL, WAIT_NO_TICKER, WAIT_REARMABLE_DISARMS,
};
use pidash_types::orchestration::{StateRef, TickerEvent};

use super::clock::{self, ClockIssue, ClockOutcome, ClockWrite, ProjectClockPolicy};
use super::creation::{
    self, ContinuationRequest, CreateDispatchRequest, CreationError, CreationSeam,
    FinalizeAgentRunSeam, IssueView, StateView,
};
use crate::dispatch::{effective_executor_for_issue, user_has_llm_config, LlmProfile, UserFlags};

// ---------------------------------------------------------------------------
// Reasons + bodies
// ---------------------------------------------------------------------------

/// Default `_bounce_issue_no_eligible_runner` reason (`:1029`).
pub const REASON_NO_ELIGIBLE_RUNNER: &str = "no-eligible-runner";
/// Cloud `_bounce_issue_no_eligible_runner` reason (`:985`): the execution
/// principal holds no usable LLM config.
pub const REASON_NO_LLM_CONFIG: &str = "no-llm-config";
/// Managed `_bounce_issue_no_eligible_runner` reason (`:998`, `:1014`):
/// no creator, or nobody enrolled.
pub const REASON_NO_MANAGED_RUNNER: &str = "no-managed-runner";

/// The `no-llm-config` bounce body (`:1064-1070`), byte-verbatim.
/// `format_html` with no args is a plain literal.
pub const BOUNCE_BODY_NO_LLM_CONFIG: &str = "<p><strong>Agent run skipped — no AI provider configured.</strong></p><p>This project uses the Pi Dash Cloud Agent, which runs against the triggering user's AI provider. Configure one in Pi Dash AI settings, or assign this issue to a member who has one configured.</p>";

/// The default bounce body (`:1072-1077`), byte-verbatim: every reason
/// except `no-llm-config` (including the managed reasons, B2b).
pub const BOUNCE_BODY_DEFAULT: &str = "<p><strong>Agent run skipped — no eligible runner.</strong></p><p>No runner is registered in this pod that can serve this issue. Add a runner under your account, or assign this issue to a workspace member whose runner is registered here.</p>";

/// Pick the bounce body (`:1064-1077`): only `no-llm-config` differs.
pub fn bounce_body(reason: &str) -> &'static str {
    if reason == REASON_NO_LLM_CONFIG {
        BOUNCE_BODY_NO_LLM_CONFIG
    } else {
        BOUNCE_BODY_DEFAULT
    }
}

// ---------------------------------------------------------------------------
// Handler-executed SQL
// ---------------------------------------------------------------------------

/// Re-tick issue lock (`:662-667`): `Issue.all_objects` (no deleted
/// guard) `select_for_update(of=("self",))` by pk. The `OF issues` is
/// kept though the port selects no joins (same row locked). `$1` is the
/// issue id; the seam follows with the unlocked [`IssueView`] read.
pub const RETICK_ISSUE_LOCK_SQL: &str = "SELECT id FROM issues WHERE id = $1 FOR UPDATE OF issues";

/// Deferred-pause issue lock (`:1455`): plain `select_for_update()` by
/// pk (no `of=`, unlike the Re-tick lock). `$1` is the issue id.
pub const PAUSE_ISSUE_LOCK_SQL: &str = "SELECT id FROM issues WHERE id = $1 FOR UPDATE";

/// Unlocked full-row ticker read by issue (`maybe_apply_deferred_pause`
/// `:1372`, the Re-tick post-dispatch re-read `:705`):
/// `...filter(issue=...).first()` — the default-manager soft-delete
/// scope plus the pk ordering `.first()` adds on an unordered model.
/// `$1` is the issue id.
pub fn ticker_select_by_issue_sql() -> String {
    format!(
        "SELECT {} FROM issue_agent_ticker WHERE issue_id = $1 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1",
        TICKER_COLUMNS.join(", "),
    )
}

/// Deferred-pause ticker re-check (`:1442`):
/// `select_for_update().filter(pk).first()` — plain `FOR UPDATE` (no
/// `of=`). `$1` is the ticker id.
pub fn ticker_lock_by_id_sql() -> String {
    format!(
        "SELECT {} FROM issue_agent_ticker WHERE id = $1 AND deleted_at IS NULL ORDER BY id ASC LIMIT 1 FOR UPDATE",
        TICKER_COLUMNS.join(", "),
    )
}

/// Bounce Backlog target (`:1082-1089`): `State.objects` (soft-delete
/// scope; the `group` filter already excludes triage)
/// `filter(project_id, group).order_by("-default", "sequence").first()`.
/// `$1` project id, `$2` the group (`backlog`).
pub const BACKLOG_TARGET_SQL: &str = "SELECT id, name, \"group\" FROM states WHERE project_id = $1 AND \"group\" = $2 AND deleted_at IS NULL ORDER BY \"default\" DESC, sequence ASC LIMIT 1";

/// `_in_progress_state_for` (`:717-726`): `State.all_state_objects`
/// (plain manager + the explicit `deleted_at` filter)
/// `filter(project_id, group, name).order_by("sequence").first()`.
/// `$1` project id, `$2` group, `$3` state name. Text pinned by Q1/Q2.
pub const IN_PROGRESS_STATE_SQL: &str = "SELECT id, name, \"group\" FROM states WHERE project_id = $1 AND \"group\" = $2 AND name = $3 AND deleted_at IS NULL ORDER BY sequence ASC LIMIT 1";

/// Deferred-pause parking lookup (`:1417-1425`):
/// `all_state_objects.filter(project, name).first()` under `Meta.ordering
/// = ("sequence",)`. `$1` project id, `$2` the name (`Paused`).
pub const PAUSED_STATE_SQL: &str = "SELECT id, name, \"group\" FROM states WHERE project_id = $1 AND name = $2 AND deleted_at IS NULL ORDER BY sequence ASC LIMIT 1";

/// Bounce fallback (`:1096`): `issue.project.default_state_id`. `$1` is
/// the project id; the seam follows with the state-row read.
pub const PROJECT_DEFAULT_STATE_SQL: &str = "SELECT default_state_id FROM projects WHERE id = $1";

/// The project columns the dispatch drivers read for clock policy
/// (`issue.project` follows in `reconcile` / `wait_ticker`). `$1` is the
/// project id.
pub const PROJECT_CLOCK_POLICY_SQL: &str = "SELECT agent_ticking_enabled, agent_default_max_ticks, agent_default_interval_seconds, agent_review_default_interval_seconds, agent_test_default_interval_seconds FROM projects WHERE id = $1";

/// The state-move write (`:1109`, `:696`, `:1463`):
/// `save(update_fields=["state", "updated_at"])` — ONLY these columns
/// (the `updated_by` assignments never reach the row). `$1` state id,
/// `$2` now, `$3` issue id.
pub const ISSUE_STATE_UPDATE_SQL: &str =
    "UPDATE issues SET state_id = $1, updated_at = $2 WHERE id = $3";

/// Bounce comment INSERT (`IssueComment.objects.create`, `:1118-1125`,
/// through the custom `save()` `:598-627`): full column list with the
/// Django-side defaults (no DB default to fall back on). `$1` now
/// (`created_at`), `$2` now (`updated_at`), `$3` id, `$4` project, `$5`
/// workspace, `$6` stripped (`strip_tags` of `$7`), `$7` body, `$8`
/// issue, `$9` actor (the system user; `created_by_id` stays NULL — the
/// bounce path has no crum user, and the fixture pins no audit column).
pub const COMMENT_INSERT_SQL: &str = "INSERT INTO issue_comments (created_at, updated_at, id, project_id, workspace_id, comment_stripped, comment_html, issue_id, actor_id, created_by_id, updated_by_id, deleted_at, comment_json, description_id, attachments, labels, access, external_source, external_id, speaker_type, speaker_label, speaker_agent_run_id, edited_at, parent_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, NULL, NULL, '{}', NULL, '{}', '{}', 'INTERNAL', NULL, NULL, 'agent', '', NULL, NULL, NULL)";

/// Bounce comment `Description` side row (`:623-625`):
/// `Description.objects.create(workspace, project, created_by,
/// updated_by, description_stripped, description_json, description_html)`.
/// `$1` now, `$2` now, `$3` id, `$4` workspace, `$5` project, `$6` body,
/// `$7` stripped.
pub const DESCRIPTION_INSERT_SQL: &str = "INSERT INTO descriptions (created_at, updated_at, id, workspace_id, project_id, description_html, description_stripped, created_by_id, updated_by_id, deleted_at, description_json, description_binary) VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, NULL, NULL, '{}', NULL)";

/// Comment↔description link (`:627`):
/// `save(update_fields=["description_id"])`. `$1` description id, `$2`
/// comment id.
pub const COMMENT_DESCRIPTION_UPDATE_SQL: &str =
    "UPDATE issue_comments SET description_id = $1 WHERE id = $2";

/// Wait activity row (`_record_wait_activity`, `:839-852`): full column
/// list with the Django-side defaults. `$1` now, `$2` now, `$3` id, `$4`
/// project, `$5` workspace, `$6` issue, `$7` pool (`old_value`), `$8`
/// waited (`new_value`), `$9` comment ([`wait_activity_comment`]), `$10`
/// actor (or the system user; `created_by_id` stays NULL — the fixture
/// pins no audit column), `$11` epoch (`time.time()`).
pub const WAIT_ACTIVITY_INSERT_SQL: &str = "INSERT INTO issue_activities (created_at, updated_at, id, project_id, workspace_id, issue_id, old_value, new_value, comment, actor_id, epoch, created_by_id, updated_by_id, deleted_at, verb, field, attachments, issue_comment_id, old_identifier, new_identifier) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULL, NULL, NULL, 'updated', 'agent_wait', '{}', NULL, NULL, NULL)";

/// Wait write without re-arm (`:792-793, :816`):
/// `save(update_fields=["waited", "updated_at"])`. `$1` waited, `$2`
/// now, `$3` ticker id.
pub const WAIT_UPDATE_SQL: &str =
    "UPDATE issue_agent_ticker SET waited = $1, updated_at = $2 WHERE id = $3";

/// Wait write with re-arm (`:810-814, :816`): `["waited",
/// "updated_at", "enabled", "disarm_reason", "next_run_at"]`. `$1`
/// waited, `$2` now, `$3` `next_run_at`, `$4` ticker id.
pub const WAIT_REARM_UPDATE_SQL: &str = "UPDATE issue_agent_ticker SET waited = $1, updated_at = $2, enabled = TRUE, disarm_reason = '', next_run_at = $3 WHERE id = $4";

/// Live-assignee candidates (`:914-918`):
/// `User.objects.filter(pk__in=IssueAssignee.objects.filter(issue=...)
/// .order_by("id")` — `User.objects` is plain (no deleted guard) while
/// `IssueAssignee.objects` inherits the soft-deletion scope. `$1` is the
/// issue id.
pub const LIVE_ASSIGNEE_CANDIDATES_SQL: &str = "SELECT u.id, u.is_active, u.is_bot FROM users u WHERE u.id IN (SELECT ia.assignee_id FROM issue_assignees ia WHERE ia.issue_id = $1 AND ia.deleted_at IS NULL) ORDER BY u.id ASC";

/// Workspace slug for the role check (`issue.workspace.slug`, `:946`).
/// `$1` is the workspace id.
pub const WORKSPACE_SLUG_SQL: &str = "SELECT slug FROM workspaces WHERE id = $1";

/// The three `check_project_role` `EXISTS` queries (`permissions.py:94-117`)
/// as one row: allowed project role (`role IN (20, 15, 5)` =
/// ADMIN/MEMBER/GUEST), any-role project membership, exact-`ADMIN`
/// workspace membership — each under its manager's soft-delete scope.
/// `$1` user id, `$2` workspace slug, `$3` project id.
pub const PROJECT_ROLE_FACTS_SQL: &str = "SELECT EXISTS(SELECT 1 FROM project_members pm INNER JOIN workspaces w ON w.id = pm.workspace_id WHERE pm.member_id = $1 AND w.slug = $2 AND pm.project_id = $3 AND pm.role IN (20, 15, 5) AND pm.is_active AND pm.deleted_at IS NULL), EXISTS(SELECT 1 FROM project_members pm INNER JOIN workspaces w ON w.id = pm.workspace_id WHERE pm.member_id = $1 AND w.slug = $2 AND pm.project_id = $3 AND pm.is_active AND pm.deleted_at IS NULL), EXISTS(SELECT 1 FROM workspace_members wm INNER JOIN workspaces w ON w.id = wm.workspace_id WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = 20 AND wm.is_active AND wm.deleted_at IS NULL)";

// ---------------------------------------------------------------------------
// Log lines
// ---------------------------------------------------------------------------

/// One collected `scheduling`-logger line, in Python order: the level
/// (`INFO` / `WARNING`) plus the rendered message, byte-verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: String,
    pub message: String,
}

fn info(message: String) -> LogLine {
    LogLine {
        level: "INFO".to_owned(),
        message,
    }
}

fn warning(message: String) -> LogLine {
    LogLine {
        level: "WARNING".to_owned(),
        message,
    }
}

/// The bounce line (`:1057-1062`, INFO).
pub fn bounce_log_line(issue_id: Uuid, reason: &str, triggered_by: &str) -> LogLine {
    info(format!(
        "agent_dispatch: bounce issue={issue_id} reason={reason} triggered_by={triggered_by}"
    ))
}

/// The no-safe-target line (`:1100-1106`, WARNING).
pub fn no_backlog_target_log_line(project_id: Uuid, issue_id: Uuid) -> LogLine {
    warning(format!(
        "agent_dispatch: no safe backlog target for project={project_id}; issue={issue_id} stays in current state, ticker disarmed"
    ))
}

/// The explicit-disarm line (`disarm_ticker`, `:630`, INFO).
pub fn disarmed_log_line(issue_id: Uuid, reason: &str) -> LogLine {
    info(format!(
        "agent_ticker: disarmed issue={issue_id} reason={reason}"
    ))
}

/// The skip-dispatch line (`:1146-1151` etc.): INFO for
/// `active-run-exists` / `no-prior-run`, WARNING for `no-creator` /
/// `no-pod`.
pub fn skip_dispatch_log_line(
    issue_id: Uuid,
    reason: &str,
    triggered_by: &str,
    level: &str,
) -> LogLine {
    let message = format!(
        "agent_ticker: skip dispatch issue={issue_id} reason={reason} triggered_by={triggered_by}"
    );
    if level == "WARNING" {
        warning(message)
    } else {
        info(message)
    }
}

/// The applied-pause line (`:1465-1468`, INFO).
pub fn auto_paused_log_line(issue_id: Uuid) -> LogLine {
    info(format!(
        "agent_ticker: auto-paused issue={issue_id} after cap hit"
    ))
}

/// The missing-Paused-state line (`:1427-1430`, WARNING).
pub fn no_paused_state_log_line(issue_id: Uuid) -> LogLine {
    warning(format!(
        "agent_ticker: cannot auto-pause issue={issue_id} — no Paused state in project"
    ))
}

/// The no-auto-pause line (`:1401-1407`, INFO): `%s` is `state.group`.
pub fn cap_hit_leave_log_line(group: &str, issue_id: Uuid) -> LogLine {
    info(format!(
        "agent_ticker: {group} cap hit for issue={issue_id} — leaving it in place, no auto-pause"
    ))
}

/// The agent-wait line (`:819-826`, INFO): `used` and the cap read off
/// the post-increment ticker; a missing run renders `None` (Python
/// `getattr(run, "pk", None)`).
pub fn agent_wait_log_line(
    issue_id: Uuid,
    waited: i32,
    used: i32,
    cap: i32,
    run_id: Option<Uuid>,
) -> LogLine {
    let run = run_id.map_or_else(|| "None".to_owned(), |id| id.to_string());
    info(format!(
        "agent_wait: issue={issue_id} waited={waited} used={used} cap={cap} run={run}"
    ))
}

// ---------------------------------------------------------------------------
// Pure units
// ---------------------------------------------------------------------------

/// Django `strip_tags` (`django.utils.html.strip_tags`, the
/// `tasks_cleanup::versions` algorithm as a per-module copy): scan for
/// `<...>` spans and drop them; an unclosed `<` is kept verbatim.
/// ASCII-only delimiters, so every slice lands on a char boundary.
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        match rest.find('>') {
            Some(rel) => rest = &rest[rel + 1..],
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `triggered_by in _MACHINE_TRIGGERS` (`:868, :895`): the clock's own
/// triggers resolve like a tick.
pub fn is_machine_trigger(triggered_by: &str) -> bool {
    MACHINE_TRIGGERS.contains(&triggered_by)
}

/// `profile.reason_code or ManagedRunnerReason.LLM_CONFIG_MISSING`
/// (`:1010`): an empty reason falls back, mirroring Python's `or`.
pub fn managed_profile_reason_code(profile: &LlmProfile) -> &str {
    if profile.reason_code.is_empty() {
        ManagedRunnerReason::LLM_CONFIG_MISSING
    } else {
        profile.reason_code.as_str()
    }
}

/// The wait activity comment (`:849`): `Waited on a blocker ({waited}
/// of {pool})` plus `; run {run_id}` when the calling run is known.
pub fn wait_activity_comment(pool: i32, waited: i32, run_id: Option<&Uuid>) -> String {
    let base = format!("Waited on a blocker ({waited} of {pool})");
    match run_id {
        Some(id) => format!("{base}; run {id}"),
        None => base,
    }
}

/// The wait re-arm rule (`:802-809`) over the post-increment ticker:
/// the clock stopped *because the pool ran out* (`cap_hit` /
/// `pool_spent`), the raised cap left room, the user switch is on, the
/// project switch is on, and the issue still ticks.
pub fn wait_rearms(
    ticker: &IssueAgentTicker,
    pool: i32,
    ticking_enabled: bool,
    state: Option<&StateRef<'_>>,
) -> bool {
    !ticker.enabled
        && WAIT_REARMABLE_DISARMS.contains(&ticker.disarm_reason.as_str())
        && !ticker.cap_reached(pool)
        && !ticker.user_disabled
        && ticking_enabled
        && is_ticking_state(state)
}

/// One cloud/managed creator candidate (`:921-927` head): dedup by id
/// (the id lands in `seen` even when the candidate fails, so a later
/// dup never re-runs the seams), then the active/non-bot flags. The
/// LLM / enrollment / role legs need the seam and stay in the driver
/// loop, after this predicate — the fixture's `llm_call_order` pins
/// that a skipped candidate never reaches them.
pub fn select_cloud_candidate(candidate: &CandidateUser, seen: &mut HashSet<Uuid>) -> bool {
    if !seen.insert(candidate.id) {
        return false;
    }
    candidate.is_active && !candidate.is_bot
}

/// The deferred-pause pre-lock probe: every value the `:1368-1431`
/// guards read, fetched before the verdict.
#[derive(Debug, Clone, Copy)]
pub struct PauseProbe<'a> {
    /// `run.work_item_id` (`:1368`).
    pub work_item_id: Option<Uuid>,
    /// The unlocked ticker (`:1372-1381`).
    pub ticker: Option<&'a IssueAgentTicker>,
    /// The issue's current state (`:1383-1407`).
    pub state: Option<StateRef<'a>>,
    /// The active run on the issue, if any (`:1413-1415`).
    pub active_run_id: Option<Uuid>,
    /// The run that just terminated.
    pub current_run_id: Uuid,
    /// The project's `Paused` state, if it has one (`:1417-1431`).
    pub paused_state_id: Option<Uuid>,
}

/// Which pre-lock guard stopped the pause (`None` from
/// [`pause_guard_verdict`] means proceed to the lock phase).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseStop {
    NoWorkItem,
    NoTicker,
    TickerEnabled,
    PendingEntry,
    NotCapHit,
    NoState,
    NotTicking,
    NoAutoPause,
    OtherActiveRun,
    NoPausedState,
}

/// The 9-guard pause matrix (`:1368-1431`), in Python order. The driver
/// fetches the probe eagerly (extra SELECTs on early exits are
/// unobservable in row semantics — the L6 `parent_for_next_run`
/// precedent) and calls this once; the lock-phase re-checks stay
/// inline in the driver.
pub fn pause_guard_verdict(probe: &PauseProbe<'_>) -> Option<PauseStop> {
    if probe.work_item_id.is_none() {
        return Some(PauseStop::NoWorkItem);
    }
    let Some(ticker) = probe.ticker else {
        return Some(PauseStop::NoTicker);
    };
    if ticker.enabled {
        return Some(PauseStop::TickerEnabled);
    }
    if ticker.pending_entry {
        return Some(PauseStop::PendingEntry);
    }
    if ticker.disarm_reason != TickerDisarmReason::CapHit.as_str() {
        return Some(PauseStop::NotCapHit);
    }
    let Some(state) = probe.state else {
        return Some(PauseStop::NoState);
    };
    if !is_ticking_state(Some(&state)) {
        return Some(PauseStop::NotTicking);
    }
    if !auto_pauses_on_cap(Some(&state)) {
        return Some(PauseStop::NoAutoPause);
    }
    if probe
        .active_run_id
        .is_some_and(|id| id != probe.current_run_id)
    {
        return Some(PauseStop::OtherActiveRun);
    }
    if probe.paused_state_id.is_none() {
        return Some(PauseStop::NoPausedState);
    }
    None
}

// ---------------------------------------------------------------------------
// Row views
// ---------------------------------------------------------------------------

/// One cloud/managed creator candidate
/// ([`LIVE_ASSIGNEE_CANDIDATES_SQL`], or the head-chain ids with
/// [`CreationSeam::user_flags`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateUser {
    pub id: Uuid,
    pub is_active: bool,
    pub is_bot: bool,
}

/// The [`PROJECT_ROLE_FACTS_SQL`] row: the three `EXISTS` verdicts for
/// [`check_project_role`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleVerdict {
    pub has_allowed_role: bool,
    pub is_project_member: bool,
    pub is_workspace_admin: bool,
}

/// One bounce comment ([`COMMENT_INSERT_SQL`] +
/// [`DESCRIPTION_INSERT_SQL`] + [`COMMENT_DESCRIPTION_UPDATE_SQL`]):
/// the driver builds it, the seam writes all three rows in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIssueComment {
    pub id: Uuid,
    pub description_id: Uuid,
    pub now: DateTime<Utc>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
    pub actor_id: Uuid,
    pub comment_html: String,
    pub comment_stripped: String,
}

/// One wait activity ([`WAIT_ACTIVITY_INSERT_SQL`]).
#[derive(Debug, Clone, PartialEq)]
pub struct NewWaitActivity {
    pub id: Uuid,
    pub now: DateTime<Utc>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
    pub pool: i32,
    pub waited: i32,
    pub run_id: Option<Uuid>,
    pub actor_id: Uuid,
    pub epoch_secs: f64,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure the dispatch drivers report.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DispatchError {
    /// Any database failure (the store stringifies its `sqlx::Error` —
    /// this crate carries no `sqlx`).
    #[error("database error: {0}")]
    Db(String),
    /// A creation-driver failure (guards, builders, pod/creator
    /// resolvers).
    #[error(transparent)]
    Store(#[from] CreationError),
    /// A clock failure (unreachable on the known event kinds — the
    /// ported `:385` `ValueError`).
    #[error("clock error: {0}")]
    Clock(String),
    /// The reserved agent username is held by a human
    /// (`workpad.py:44-71`).
    #[error(transparent)]
    AgentUserCollision(#[from] AgentUserCollisionError),
}

// ---------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------

/// The one-method D-14 seam (`runner/services/matcher.py:375-412`):
/// does a runner registered in `pod_id` accept a run on `issue_id`
/// created by `creator_id`? D-14 (runner, downstream of D-12 — no edge,
/// it would deadlock) implements this later; the D-10 `FireTickSeam`
/// precedent. Boxed future, so the trait stays object-safe for the
/// `&dyn` driver parameter.
pub trait PodRunnerMatcher: Send + Sync {
    fn pod_has_runner_for_issue_principal(
        &self,
        pod_id: Uuid,
        issue_id: Uuid,
        creator_id: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<bool, DispatchError>> + Send + '_>>;
}

/// The dispatch seam: the L6 seams (guards, builders, finalization) plus
/// every read and write the dispatch remainder needs. The jobs-side
/// `LiveCreationStore` implements it over the insertion transaction.
/// Sync methods are the D-11 seams (closures the store carries):
/// `has_usable_llm_config` (`agent_execution.py:78-80`),
/// `managed_llm_profile` (`managed_runner/policy.py:27-36`), and the
/// operator kill switch (`managed_runner_is_enabled`, L3).
#[allow(async_fn_in_trait)]
pub trait DispatchSeam: CreationSeam + FinalizeAgentRunSeam {
    /// The Re-tick issue lock + read ([`RETICK_ISSUE_LOCK_SQL`] then the
    /// unlocked issue row; `all_objects`, no deleted guard).
    async fn lock_issue_for_retick(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueView>, DispatchError>;
    /// The wait ticker lock ([`clock::lock_ticker_sql`]).
    async fn lock_ticker_for_issue(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, DispatchError>;
    /// The unlocked ticker read ([`ticker_select_by_issue_sql`]).
    async fn ticker_for_issue(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, DispatchError>;
    /// The pause ticker re-check ([`ticker_lock_by_id_sql`]).
    async fn lock_ticker_by_id(
        &mut self,
        ticker_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, DispatchError>;
    /// The pause issue lock + read ([`PAUSE_ISSUE_LOCK_SQL`] then the
    /// unlocked issue row).
    async fn lock_issue_by_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueView>, DispatchError>;
    /// `_save_clock` ([`clock::SAVE_CLOCK_SQL`]).
    async fn save_clock(
        &mut self,
        ticker: &IssueAgentTicker,
        now: DateTime<Utc>,
    ) -> Result<(), DispatchError>;
    /// The create-shape INSERT ([`clock::create_ticker_sql`]) of the
    /// final row.
    async fn insert_ticker(&mut self, ticker: &IssueAgentTicker) -> Result<(), DispatchError>;
    /// The wait write ([`WAIT_UPDATE_SQL`], or [`WAIT_REARM_UPDATE_SQL`]
    /// when `rearmed`).
    async fn save_wait(
        &mut self,
        ticker: &IssueAgentTicker,
        now: DateTime<Utc>,
        rearmed: bool,
    ) -> Result<(), DispatchError>;
    /// The bounce Backlog target ([`BACKLOG_TARGET_SQL`]).
    async fn backlog_target_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<StateView>, DispatchError>;
    /// `_in_progress_state_for` ([`IN_PROGRESS_STATE_SQL`]).
    async fn in_progress_state_for_project(
        &mut self,
        project_id: Uuid,
        group: &str,
        state_name: &str,
    ) -> Result<Option<StateView>, DispatchError>;
    /// The pause parking lookup ([`PAUSED_STATE_SQL`]).
    async fn paused_state_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<StateView>, DispatchError>;
    /// The bounce fallback id ([`PROJECT_DEFAULT_STATE_SQL`]).
    async fn default_state_id_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<Uuid>, DispatchError>;
    /// The project clock policy ([`PROJECT_CLOCK_POLICY_SQL`]).
    async fn clock_policy_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<ProjectClockPolicy, DispatchError>;
    /// The state-move write ([`ISSUE_STATE_UPDATE_SQL`]).
    async fn update_issue_state(
        &mut self,
        issue_id: Uuid,
        state_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), DispatchError>;
    /// `get_agent_system_user` (`workpad.py:44-71`): find by reserved
    /// username; create the service bot when missing; refuse when a
    /// human holds the name ([`DispatchError::AgentUserCollision`]).
    async fn agent_system_user_id(&mut self) -> Result<Uuid, DispatchError>;
    /// The three bounce-comment writes, in order.
    async fn insert_bounce_comment(
        &mut self,
        comment: &NewIssueComment,
    ) -> Result<(), DispatchError>;
    /// The wait activity write.
    async fn insert_wait_activity(
        &mut self,
        activity: &NewWaitActivity,
    ) -> Result<(), DispatchError>;
    /// The live-assignee candidates ([`LIVE_ASSIGNEE_CANDIDATES_SQL`]).
    async fn live_assignee_candidates(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Vec<CandidateUser>, DispatchError>;
    /// `issue.workspace.slug` ([`WORKSPACE_SLUG_SQL`]).
    async fn workspace_slug(&mut self, workspace_id: Uuid) -> Result<String, DispatchError>;
    /// The role-check facts ([`PROJECT_ROLE_FACTS_SQL`]).
    async fn project_role_facts(
        &mut self,
        user_id: Uuid,
        workspace_slug: &str,
        project_id: Uuid,
    ) -> Result<RoleVerdict, DispatchError>;
    /// `enrolled_managed_runners(project, user).exists()`
    /// ([`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`][crate::dispatch::ENROLLED_MANAGED_RUNNERS_EXISTS_SQL]).
    async fn enrolled_managed_exists(
        &mut self,
        project_id: Uuid,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<bool, DispatchError>;
    /// The operator kill switch (`managed_runner_is_enabled`, L3).
    fn managed_runner_enabled(&self) -> bool;
    /// `has_usable_llm_config(user)` (`agent_execution.py:78-80`).
    fn has_usable_llm_config(&self, user_id: Uuid) -> bool;
    /// `managed_llm_profile(user)` (`managed_runner/policy.py:27-36`).
    fn llm_profile_for(&self, user_id: Uuid) -> LlmProfile;
}

// ---------------------------------------------------------------------------
// Outcomes
// ---------------------------------------------------------------------------

/// What `preflight_eligibility_or_bounce` decided: `True` in Python
/// becomes `proceed`, with the bounce's lines when it bounced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightOutcome {
    pub proceed: bool,
    pub logs: Vec<LogLine>,
}

/// What `_bounce_issue_no_eligible_runner` did: the move target (`None`
/// when the issue stayed — already Backlog, same-state fallback, or
/// un-bounceable), with the lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BounceOutcome {
    pub moved_to: Option<Uuid>,
    pub logs: Vec<LogLine>,
}

/// What `dispatch_continuation_run` did: the created run, with the
/// guard / bounce lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationDispatchOutcome {
    pub run_id: Option<Uuid>,
    pub logs: Vec<LogLine>,
}

/// What `dispatch_run_ai_run_with_reason` did: the created run plus the
/// machine-readable refusal code (`RUN_AI_*`) when `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunAiOutcome {
    pub run_id: Option<Uuid>,
    pub reason: Option<String>,
    pub logs: Vec<LogLine>,
}

/// What `dispatch_run_ai_run` did: just the run (the thin wrapper
/// drops the reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinRunAiOutcome {
    pub run_id: Option<Uuid>,
    pub logs: Vec<LogLine>,
}

/// What `run_ai_for_human` did: the run, the refusal code when `None`,
/// and whether the jobs driver must roll the re-time back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanRunAiOutcome {
    pub run_id: Option<Uuid>,
    pub reason: Option<String>,
    pub rollback: bool,
    pub logs: Vec<LogLine>,
}

/// What `re_tick_ticker` did: the grant, the reason, the run, the final
/// ticker row, and whether the jobs driver must roll back.
#[derive(Debug, Clone, PartialEq)]
pub struct RetickOutcome {
    pub granted: bool,
    pub reason: String,
    pub ticker: Option<IssueAgentTicker>,
    pub run_id: Option<Uuid>,
    pub rollback: bool,
    pub logs: Vec<LogLine>,
}

/// What `wait_ticker` did: whether the wait applied, the `WAIT_*`
/// reason, the final ticker row.
#[derive(Debug, Clone, PartialEq)]
pub struct WaitOutcome {
    pub applied: bool,
    pub reason: String,
    pub ticker: Option<IssueAgentTicker>,
    pub logs: Vec<LogLine>,
}

/// What `maybe_apply_deferred_pause` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseOutcome {
    pub applied: bool,
    pub logs: Vec<LogLine>,
}

// ---------------------------------------------------------------------------
// Drivers (Python control flow, verbatim order)
// ---------------------------------------------------------------------------

/// Resolve the issue's clock inputs: the state row plus the project
/// policy (a project-less issue answers the `getattr` defaults, like
/// Python's `None.project` reads).
async fn clock_inputs<S: DispatchSeam>(
    seam: &mut S,
    issue: &IssueView,
) -> Result<(Option<StateView>, ProjectClockPolicy), DispatchError> {
    let state = CreationSeam::state(seam, issue.state_id)
        .await
        .map_err(DispatchError::Store)?;
    let policy = match issue.project_id {
        Some(project_id) => seam.clock_policy_for_project(project_id).await?,
        None => ProjectClockPolicy::default(),
    };
    Ok((state, policy))
}

/// Run one [`clock::reconcile`] on the seam: resolve the L2 verdict,
/// dispatch pure, execute the [`ClockWrite`], collect the
/// post-commit-equivalent line. `ticker` is the post-lock row.
#[allow(clippy::too_many_arguments)]
async fn reconcile_on_seam<S: DispatchSeam>(
    seam: &mut S,
    ticker: &mut Option<IssueAgentTicker>,
    issue_id: Uuid,
    state: &Option<StateView>,
    policy: &ProjectClockPolicy,
    event: &TickerEvent,
    now: DateTime<Utc>,
    jitter_secs: f64,
    created_by: Option<Uuid>,
    logs: &mut Vec<LogLine>,
) -> Result<ClockOutcome, DispatchError> {
    let state_ref = state.as_ref().map(|state| StateRef {
        group: state.group.as_str(),
        name: state.name.as_str(),
    });
    let issue = ClockIssue {
        issue_id,
        state: state_ref,
        policy: *policy,
    };
    let has_active_run = CreationSeam::active_run_for(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?
        .is_some();
    let outcome = clock::reconcile(
        ticker,
        &issue,
        event,
        None,
        has_active_run,
        now,
        jitter_secs,
        created_by,
    )
    .map_err(|err| DispatchError::Clock(err.to_string()))?;
    match outcome.write {
        ClockWrite::None => {}
        ClockWrite::Insert => {
            let row = ticker.as_ref().expect("reconcile built the row");
            seam.insert_ticker(row).await?;
        }
        ClockWrite::Update => {
            let row = ticker.as_ref().expect("reconcile kept the row");
            seam.save_clock(row, now).await?;
        }
    }
    logs.push(info(clock::reconcile_log_line(
        &issue_id,
        &event.kind,
        &outcome.decision,
    )));
    Ok(outcome)
}

/// Resolve the effective executor name for the issue (L3): the
/// per-issue override wins, else the project default.
async fn effective_executor<S: DispatchSeam>(
    seam: &mut S,
    issue: &IssueView,
) -> Result<String, DispatchError> {
    // Python's `or` short-circuit (`agent_execution.py:66`): a set
    // override wins without touching the project row at all.
    if let Some(value) = issue
        .agent_executor
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return Ok(value.to_owned());
    }
    let project_id = issue.project_id.ok_or_else(|| {
        DispatchError::Store(CreationError::MissingRow("issue has no project".to_owned()))
    })?;
    let project = CreationSeam::project(seam, project_id)
        .await
        .map_err(DispatchError::Store)?;
    Ok(effective_executor_for_issue(
        issue.agent_executor.as_deref(),
        project.default_agent_executor.as_str(),
    )
    .to_owned())
}

/// A current human execution principal; bots never own tool authority
/// (`_resolve_creator_for_trigger`, `:887-952`).
pub async fn resolve_creator_for_trigger<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    triggered_by: &str,
    actor: Option<Uuid>,
) -> Result<Option<Uuid>, DispatchError> {
    let issue = CreationSeam::issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let effective = effective_executor(seam, &issue).await?;
    if effective == AgentExecutorKind::LocalRunner.value() {
        if let Some(actor) = actor {
            return Ok(Some(actor));
        }
        if is_machine_trigger(triggered_by) {
            return Ok(Some(seam.agent_system_user_id().await?));
        }
        return creation::resolve_fallback_creator(seam, issue_id)
            .await
            .map_err(DispatchError::Store);
    }

    let project_id = issue.project_id.ok_or_else(|| {
        DispatchError::Store(CreationError::MissingRow("issue has no project".to_owned()))
    })?;
    let project = CreationSeam::project(seam, project_id)
        .await
        .map_err(DispatchError::Store)?;
    let managed = effective == AgentExecutorKind::ManagedRunner.value();
    let mut candidates: Vec<CandidateUser> = Vec::new();
    if let Some(id) = actor.filter(|_| !is_machine_trigger(triggered_by)) {
        let flags = CreationSeam::user_flags(seam, id)
            .await
            .map_err(DispatchError::Store)?;
        candidates.push(CandidateUser {
            id,
            is_active: flags.is_active,
            is_bot: flags.is_bot,
        });
    } else {
        for id in [
            issue.created_by_id,
            project.project_lead_id,
            project.default_assignee_id,
        ]
        .into_iter()
        .flatten()
        {
            let flags = CreationSeam::user_flags(seam, id)
                .await
                .map_err(DispatchError::Store)?;
            candidates.push(CandidateUser {
                id,
                is_active: flags.is_active,
                is_bot: flags.is_bot,
            });
        }
        candidates.extend(seam.live_assignee_candidates(issue_id).await?);
    }

    let slug = seam.workspace_slug(project.workspace_id).await?;
    let mut seen = HashSet::new();
    for candidate in &candidates {
        if !select_cloud_candidate(candidate, &mut seen) {
            continue;
        }
        let flags = UserFlags {
            is_active: candidate.is_active,
            is_bot: candidate.is_bot,
        };
        if !user_has_llm_config(Some(&flags), || seam.has_usable_llm_config(candidate.id)) {
            continue;
        }
        if managed
            && !seam
                .enrolled_managed_exists(project_id, candidate.id, project.workspace_id)
                .await?
        {
            continue;
        }
        let verdict = seam
            .project_role_facts(candidate.id, &slug, project_id)
            .await?;
        if check_project_role(
            &ProjectRoleFacts {
                authenticated: true,
                has_allowed_role: verdict.has_allowed_role,
                is_project_member: verdict.is_project_member,
                is_workspace_admin: verdict.is_workspace_admin,
            },
            true,
        ) {
            return Ok(Some(candidate.id));
        }
    }
    Ok(None)
}

/// The project's registered In Progress state, if it has one
/// (`_in_progress_state_for`, `:709-726`): the `started` registry entry
/// (`None` when the registry names none), then the sequence-first row.
pub async fn in_progress_state_for<S: DispatchSeam>(
    seam: &mut S,
    project_id: Uuid,
) -> Result<Option<StateView>, DispatchError> {
    let name = ticking_state_names_by_group()
        .into_iter()
        .find(|(group, _)| *group == "started")
        .map(|(_, name)| name);
    let Some(name) = name else {
        return Ok(None);
    };
    seam.in_progress_state_for_project(project_id, "started", name)
        .await
}

/// Return `true` if dispatch can proceed; `false` if the issue was
/// bounced (`preflight_eligibility_or_bounce`, `:955-1026`). The caller
/// must NOT create the run when this returns `false`.
#[allow(clippy::too_many_arguments)]
pub async fn preflight_eligibility_or_bounce<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    run_creator: Option<Uuid>,
    pod_id: Uuid,
    triggered_by: &str,
    matcher: &dyn PodRunnerMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> Result<PreflightOutcome, DispatchError> {
    let issue = CreationSeam::issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let effective = effective_executor(seam, &issue).await?;
    if effective == AgentExecutorKind::CloudAgent.value() {
        if let Some(creator) = run_creator {
            let flags = CreationSeam::user_flags(seam, creator)
                .await
                .map_err(DispatchError::Store)?;
            if user_has_llm_config(Some(&flags), || seam.has_usable_llm_config(creator)) {
                return Ok(PreflightOutcome {
                    proceed: true,
                    logs: Vec::new(),
                });
            }
        }
        let bounced = bounce_issue_no_eligible_runner(
            seam,
            issue_id,
            triggered_by,
            REASON_NO_LLM_CONFIG,
            now,
            jitter_secs,
        )
        .await?;
        return Ok(PreflightOutcome {
            proceed: false,
            logs: bounced.logs,
        });
    }

    if effective == AgentExecutorKind::ManagedRunner.value() {
        let reason = match run_creator {
            None => Some(REASON_NO_MANAGED_RUNNER.to_owned()),
            Some(_) if !seam.managed_runner_enabled() => {
                Some(ManagedRunnerReason::DISABLED.to_owned())
            }
            Some(creator) => {
                let profile = seam.llm_profile_for(creator);
                if !profile.available {
                    Some(managed_profile_reason_code(&profile).to_owned())
                } else {
                    let project_id = issue.project_id.ok_or_else(|| {
                        DispatchError::Store(CreationError::MissingRow(
                            "issue has no project".to_owned(),
                        ))
                    })?;
                    let project = CreationSeam::project(seam, project_id)
                        .await
                        .map_err(DispatchError::Store)?;
                    if !seam
                        .enrolled_managed_exists(project_id, creator, project.workspace_id)
                        .await?
                    {
                        Some(REASON_NO_MANAGED_RUNNER.to_owned())
                    } else {
                        None
                    }
                }
            }
        };
        match reason {
            None => {
                return Ok(PreflightOutcome {
                    proceed: true,
                    logs: Vec::new(),
                });
            }
            Some(reason) => {
                let bounced = bounce_issue_no_eligible_runner(
                    seam,
                    issue_id,
                    triggered_by,
                    &reason,
                    now,
                    jitter_secs,
                )
                .await?;
                return Ok(PreflightOutcome {
                    proceed: false,
                    logs: bounced.logs,
                });
            }
        }
    }

    if matcher
        .pod_has_runner_for_issue_principal(pod_id, issue_id, run_creator)
        .await?
    {
        return Ok(PreflightOutcome {
            proceed: true,
            logs: Vec::new(),
        });
    }
    let bounced = bounce_issue_no_eligible_runner(
        seam,
        issue_id,
        triggered_by,
        REASON_NO_ELIGIBLE_RUNNER,
        now,
        jitter_secs,
    )
    .await?;
    Ok(PreflightOutcome {
        proceed: false,
        logs: bounced.logs,
    })
}

/// Move `issue` back to Backlog and post the no-eligible-runner notice
/// (`_bounce_issue_no_eligible_runner`, `:1029-1125`).
///
/// Target resolution (design §6.6 step 1): the project's Backlog state
/// (`default` first, then `sequence`); else `project.default_state`
/// only when it is not itself ticking; else the issue stays and the
/// ticker is disarmed explicitly. The state move's signal is an
/// explicit `left_bucket` reconcile on the seam (no L7 edge).
pub async fn bounce_issue_no_eligible_runner<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    triggered_by: &str,
    reason: &str,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> Result<BounceOutcome, DispatchError> {
    let mut logs = Vec::new();
    logs.push(bounce_log_line(issue_id, reason, triggered_by));
    let body = bounce_body(reason);

    let issue = CreationSeam::issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let project_id = issue.project_id.ok_or_else(|| {
        DispatchError::Store(CreationError::MissingRow("issue has no project".to_owned()))
    })?;
    let (mut current_state, policy) = clock_inputs(seam, &issue).await?;
    let current_group = current_state.as_ref().map(|state| state.group.as_str());
    let mut moved_to = None;

    if current_group != Some("backlog") {
        let mut target = seam.backlog_target_for_project(project_id).await?;
        if target.is_none() {
            let fallback_id = seam.default_state_id_for_project(project_id).await?;
            let fallback = CreationSeam::state(seam, fallback_id)
                .await
                .map_err(DispatchError::Store)?;
            let safe = fallback.as_ref().is_some_and(|state| {
                !is_ticking_state(Some(&StateRef {
                    group: state.group.as_str(),
                    name: state.name.as_str(),
                }))
            });
            if safe {
                target = fallback;
            } else {
                logs.push(no_backlog_target_log_line(project_id, issue_id));
            }
        }
        if let Some(target_state) = target {
            if Some(target_state.id) != issue.state_id {
                seam.update_issue_state(issue_id, target_state.id, now)
                    .await?;
                moved_to = Some(target_state.id);
                current_state = Some(target_state);
                // The state-move signal: the clock goes dormant.
                let mut ticker = seam.lock_ticker_for_issue(issue_id).await?;
                let event = TickerEvent::left_bucket();
                reconcile_on_seam(
                    seam,
                    &mut ticker,
                    issue_id,
                    &current_state,
                    &policy,
                    &event,
                    now,
                    jitter_secs,
                    None,
                    &mut logs,
                )
                .await?;
            }
        }
    }

    // Un-moved out of a ticking state: the move signal never disarmed
    // the ticker — do it explicitly so the next tick can't re-enter
    // this bounce endlessly. Silent when no ticker row exists
    // (`disarm_ticker` returns `None` without logging).
    let still_ticking = current_state.as_ref().is_some_and(|state| {
        is_ticking_state(Some(&StateRef {
            group: state.group.as_str(),
            name: state.name.as_str(),
        }))
    });
    if still_ticking {
        let mut ticker = seam.lock_ticker_for_issue(issue_id).await?;
        if let Some(row) = ticker.as_mut() {
            clock::disarm_ticker(Some(row), TickerDisarmReason::LeftTickingState.as_str())
                .map_err(|err| DispatchError::Clock(err.to_string()))?;
            seam.save_clock(row, now).await?;
            logs.push(disarmed_log_line(
                issue_id,
                TickerDisarmReason::LeftTickingState.as_str(),
            ));
        }
    }

    let actor_id = seam.agent_system_user_id().await?;
    seam.insert_bounce_comment(&NewIssueComment {
        id: Uuid::new_v4(),
        description_id: Uuid::new_v4(),
        now,
        project_id,
        workspace_id: issue.workspace_id,
        issue_id,
        actor_id,
        comment_html: body.to_owned(),
        comment_stripped: strip_tags(body),
    })
    .await?;
    Ok(BounceOutcome { moved_to, logs })
}

/// The no-creator bounce shared by the continuation and Run AI paths
/// (`:1173-1185`, `:1283-1290`): cloud / managed bounce loudly, local
/// stays silent.
async fn bounce_no_creator<S: DispatchSeam>(
    seam: &mut S,
    issue: &IssueView,
    issue_id: Uuid,
    triggered_by: &str,
    now: DateTime<Utc>,
    jitter_secs: f64,
    logs: &mut Vec<LogLine>,
) -> Result<(), DispatchError> {
    let effective = effective_executor(seam, issue).await?;
    if effective == AgentExecutorKind::CloudAgent.value() {
        let bounced = bounce_issue_no_eligible_runner(
            seam,
            issue_id,
            triggered_by,
            REASON_NO_LLM_CONFIG,
            now,
            jitter_secs,
        )
        .await?;
        logs.extend(bounced.logs);
    } else if effective == AgentExecutorKind::ManagedRunner.value() {
        let bounced = bounce_issue_no_eligible_runner(
            seam,
            issue_id,
            triggered_by,
            REASON_NO_MANAGED_RUNNER,
            now,
            jitter_secs,
        )
        .await?;
        logs.extend(bounced.logs);
    }
    Ok(())
}

/// Public wrapper for tick / Comment & Run dispatch
/// (`dispatch_continuation_run`, `:1128-1216`).
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_continuation_run<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    triggered_by: &str,
    actor: Option<Uuid>,
    matcher: &dyn PodRunnerMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> Result<ContinuationDispatchOutcome, DispatchError> {
    let mut logs = Vec::new();
    let active = CreationSeam::active_run_for(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    if active.is_some() {
        logs.push(skip_dispatch_log_line(
            issue_id,
            "active-run-exists",
            triggered_by,
            "INFO",
        ));
        return Ok(ContinuationDispatchOutcome { run_id: None, logs });
    }
    let prior = CreationSeam::latest_prior_run(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    if prior.is_none() {
        logs.push(skip_dispatch_log_line(
            issue_id,
            "no-prior-run",
            triggered_by,
            "INFO",
        ));
        return Ok(ContinuationDispatchOutcome { run_id: None, logs });
    }

    let (parent, fresh_session) = creation::parent_for_next_run(seam, issue_id, None, None)
        .await
        .map_err(DispatchError::Store)?;
    let creator = resolve_creator_for_trigger(seam, issue_id, triggered_by, actor).await?;
    let Some(creator) = creator else {
        logs.push(skip_dispatch_log_line(
            issue_id,
            "no-creator",
            triggered_by,
            "WARNING",
        ));
        let issue = CreationSeam::issue(seam, issue_id)
            .await
            .map_err(DispatchError::Store)?;
        bounce_no_creator(
            seam,
            &issue,
            issue_id,
            triggered_by,
            now,
            jitter_secs,
            &mut logs,
        )
        .await?;
        return Ok(ContinuationDispatchOutcome { run_id: None, logs });
    };
    let pod = creation::resolve_pod_for_issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let Some(pod) = pod else {
        logs.push(skip_dispatch_log_line(
            issue_id,
            "no-pod",
            triggered_by,
            "WARNING",
        ));
        return Ok(ContinuationDispatchOutcome { run_id: None, logs });
    };
    let preflight = preflight_eligibility_or_bounce(
        seam,
        issue_id,
        Some(creator),
        pod,
        triggered_by,
        matcher,
        now,
        jitter_secs,
    )
    .await?;
    if !preflight.proceed {
        logs.extend(preflight.logs);
        return Ok(ContinuationDispatchOutcome { run_id: None, logs });
    }
    logs.extend(preflight.logs);

    // Django passes `triggered_by` to the builders unstamped and
    // unvalidated (`scheduling.py:1206/1214`, `trigger: str`): the
    // stored value may sit outside the trigger enum, and the new run
    // carries it verbatim.
    let trigger = triggered_by.to_owned();
    let run_id = match (parent, fresh_session) {
        (Some(parent), false) => {
            creation::create_continuation_run(
                seam,
                &ContinuationRequest {
                    issue_id,
                    parent,
                    creator_id: creator,
                    pod_id: pod,
                    trigger: trigger.clone(),
                    now,
                },
            )
            .await
            .map_err(DispatchError::Store)?
            .created_run
        }
        _ => {
            creation::create_and_dispatch_run(
                seam,
                &CreateDispatchRequest {
                    issue_id,
                    parent: None,
                    creator_id: creator,
                    pod_id: pod,
                    fresh_session: true,
                    trigger: trigger.clone(),
                    now,
                },
            )
            .await
            .map_err(DispatchError::Store)?
            .created_run
        }
    };
    Ok(ContinuationDispatchOutcome { run_id, logs })
}

/// "Run AI" dispatch that also reports *why* nothing was created
/// (`dispatch_run_ai_run_with_reason`, `:1240-1322`).
pub async fn dispatch_run_ai_run_with_reason<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    actor: Option<Uuid>,
    matcher: &dyn PodRunnerMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> Result<RunAiOutcome, DispatchError> {
    let mut logs = Vec::new();
    let active = CreationSeam::active_run_for(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    if active.is_some() {
        logs.push(skip_dispatch_log_line(
            issue_id,
            "active-run-exists",
            TRIGGER_RUN_AI,
            "INFO",
        ));
        return Ok(RunAiOutcome {
            run_id: None,
            reason: Some(RUN_AI_ACTIVE_RUN_EXISTS.to_owned()),
            logs,
        });
    }
    let creator = resolve_creator_for_trigger(seam, issue_id, TRIGGER_RUN_AI, actor).await?;
    let Some(creator) = creator else {
        logs.push(skip_dispatch_log_line(
            issue_id,
            "no-creator",
            TRIGGER_RUN_AI,
            "WARNING",
        ));
        let issue = CreationSeam::issue(seam, issue_id)
            .await
            .map_err(DispatchError::Store)?;
        bounce_no_creator(
            seam,
            &issue,
            issue_id,
            TRIGGER_RUN_AI,
            now,
            jitter_secs,
            &mut logs,
        )
        .await?;
        return Ok(RunAiOutcome {
            run_id: None,
            reason: Some(RUN_AI_NO_ELIGIBLE_RUNNER.to_owned()),
            logs,
        });
    };
    let pod = creation::resolve_pod_for_issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let Some(pod) = pod else {
        logs.push(skip_dispatch_log_line(
            issue_id,
            "no-pod",
            TRIGGER_RUN_AI,
            "WARNING",
        ));
        return Ok(RunAiOutcome {
            run_id: None,
            reason: Some(RUN_AI_NO_POD.to_owned()),
            logs,
        });
    };
    let preflight = preflight_eligibility_or_bounce(
        seam,
        issue_id,
        Some(creator),
        pod,
        TRIGGER_RUN_AI,
        matcher,
        now,
        jitter_secs,
    )
    .await?;
    if !preflight.proceed {
        logs.extend(preflight.logs);
        return Ok(RunAiOutcome {
            run_id: None,
            reason: Some(RUN_AI_NO_ELIGIBLE_RUNNER.to_owned()),
            logs,
        });
    }
    logs.extend(preflight.logs);

    let (parent, fresh_session) = creation::parent_for_next_run(seam, issue_id, None, None)
        .await
        .map_err(DispatchError::Store)?;
    // The prompt-parity contract: a prior run continues (parent linkage
    // + runner pinning); a brand-new issue renders the phase template.
    // Note the create leg passes `fresh_session` through (unlike the
    // continuation path's literal `True`).
    let run_id = match (parent, fresh_session) {
        (Some(parent), false) => {
            creation::create_continuation_run(
                seam,
                &ContinuationRequest {
                    issue_id,
                    parent,
                    creator_id: creator,
                    pod_id: pod,
                    trigger: TRIGGER_RUN_AI.to_owned(),
                    now,
                },
            )
            .await
            .map_err(DispatchError::Store)?
            .created_run
        }
        _ => {
            creation::create_and_dispatch_run(
                seam,
                &CreateDispatchRequest {
                    issue_id,
                    parent: None,
                    creator_id: creator,
                    pod_id: pod,
                    fresh_session,
                    trigger: TRIGGER_RUN_AI.to_owned(),
                    now,
                },
            )
            .await
            .map_err(DispatchError::Store)?
            .created_run
        }
    };
    Ok(RunAiOutcome {
        run_id,
        reason: None,
        logs,
    })
}

/// Public wrapper for the "Run AI" button, returning just the run
/// (`dispatch_run_ai_run`, `:1228-1237`): the thin adapter over
/// [`dispatch_run_ai_run_with_reason`].
pub async fn dispatch_run_ai_run<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    actor: Option<Uuid>,
    matcher: &dyn PodRunnerMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> Result<ThinRunAiOutcome, DispatchError> {
    let outcome =
        dispatch_run_ai_run_with_reason(seam, issue_id, actor, matcher, now, jitter_secs).await?;
    Ok(ThinRunAiOutcome {
        run_id: outcome.run_id,
        logs: outcome.logs,
    })
}

/// Shared body for a human-initiated "Run AI" over the token API
/// (`run_ai_for_human`, `:1325-1340`): re-time the clock for the free
/// human run, dispatch, and roll the re-time back when nothing was
/// created. The jobs driver owns the transaction and reads `rollback`.
pub async fn run_ai_for_human<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    actor: Option<Uuid>,
    matcher: &dyn PodRunnerMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
    created_by: Option<Uuid>,
) -> Result<HumanRunAiOutcome, DispatchError> {
    let mut logs = Vec::new();
    let issue = CreationSeam::issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let (state, policy) = clock_inputs(seam, &issue).await?;
    let mut ticker = seam.lock_ticker_for_issue(issue_id).await?;
    // `reset_ticker_after_comment_and_run`: the same `reconcile` event
    // under its historical name (a missing row is created here too).
    let event = TickerEvent::human_run_requested(false, None, "");
    reconcile_on_seam(
        seam,
        &mut ticker,
        issue_id,
        &state,
        &policy,
        &event,
        now,
        jitter_secs,
        created_by,
        &mut logs,
    )
    .await?;
    let outcome =
        dispatch_run_ai_run_with_reason(seam, issue_id, actor, matcher, now, jitter_secs).await?;
    logs.extend(outcome.logs);
    Ok(HumanRunAiOutcome {
        run_id: outcome.run_id,
        reason: outcome.reason,
        rollback: outcome.run_id.is_none(),
        logs,
    })
}

/// Grant a Re-tick and start a run now (`re_tick_ticker`, `:650-706`,
/// design §5.5). Grant, clock re-time and dispatch share the jobs
/// driver's transaction: a Re-tick that produced no run is reported as
/// not granted with `rollback`, leaving the ticker exactly as it was.
pub async fn re_tick_ticker<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    actor: Option<Uuid>,
    matcher: &dyn PodRunnerMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> Result<RetickOutcome, DispatchError> {
    let mut logs = Vec::new();
    let locked = seam.lock_issue_for_retick(issue_id).await?;
    let Some(locked) = locked else {
        return Ok(RetickOutcome {
            granted: false,
            reason: "no_issue".to_owned(),
            ticker: None,
            run_id: None,
            rollback: false,
            logs,
        });
    };
    let (state, policy) = clock_inputs(seam, &locked).await?;
    let mut ticker = seam.lock_ticker_for_issue(issue_id).await?;
    let event = TickerEvent::retick(true, actor);
    let outcome = reconcile_on_seam(
        seam,
        &mut ticker,
        issue_id,
        &state,
        &policy,
        &event,
        now,
        jitter_secs,
        None,
        &mut logs,
    )
    .await?;
    let decision = outcome.decision;
    if !decision.granted {
        return Ok(RetickOutcome {
            granted: false,
            reason: decision.reason,
            ticker,
            run_id: None,
            rollback: false,
            logs,
        });
    }

    let creator = match actor {
        Some(actor) => Some(actor),
        None => creation::resolve_fallback_creator(seam, issue_id)
            .await
            .map_err(DispatchError::Store)?,
    };
    let mut dispatch_now = decision.dispatch_now;
    if decision.reason == "granted-from-paused" {
        // Bring the issue back into the bucket as a human move (free
        // entry, clock armed on the fresh budget): the state UPDATE
        // plus the explicit `entered_bucket` reconcile with
        // `want_run=false` (the `dispatch_immediate=False` save — the
        // run below carries `actor`, so the signal dispatches nothing).
        let project_id = locked.project_id.ok_or_else(|| {
            DispatchError::Store(CreationError::MissingRow("issue has no project".to_owned()))
        })?;
        let target = in_progress_state_for(seam, project_id).await?;
        let Some(target) = target else {
            return Ok(RetickOutcome {
                granted: false,
                reason: "no_in_progress_state".to_owned(),
                ticker,
                run_id: None,
                rollback: true,
                logs,
            });
        };
        seam.update_issue_state(issue_id, target.id, now).await?;
        let moved_state = Some(target);
        let mut moved_ticker = seam.lock_ticker_for_issue(issue_id).await?;
        let event = TickerEvent::entered_bucket(None, None, false, None);
        reconcile_on_seam(
            seam,
            &mut moved_ticker,
            issue_id,
            &moved_state,
            &policy,
            &event,
            now,
            jitter_secs,
            None,
            &mut logs,
        )
        .await?;
        ticker = moved_ticker;
        dispatch_now = true;
    }

    if dispatch_now {
        let created = match creator {
            Some(creator) => Some(
                dispatch_run_ai_run(seam, issue_id, Some(creator), matcher, now, jitter_secs)
                    .await?,
            ),
            None => None,
        };
        if let Some(created) = &created {
            logs.extend(created.logs.clone());
        }
        let run_id = created.and_then(|created| created.run_id);
        if run_id.is_none() {
            return Ok(RetickOutcome {
                granted: false,
                reason: "dispatch-failed".to_owned(),
                ticker,
                run_id: None,
                rollback: true,
                logs,
            });
        }
        return Ok(RetickOutcome {
            granted: true,
            reason: decision.reason,
            ticker: seam.ticker_for_issue(issue_id).await?,
            run_id,
            rollback: false,
            logs,
        });
    }
    Ok(RetickOutcome {
        granted: true,
        reason: decision.reason,
        ticker,
        run_id: None,
        rollback: false,
        logs,
    })
}

/// Buy back one tick so an agent can yield without spending budget
/// (`wait_ticker`, `:766-827`, PDASHOSS01-204).
#[allow(clippy::too_many_arguments)]
pub async fn wait_ticker<S: DispatchSeam>(
    seam: &mut S,
    issue_id: Uuid,
    run_id: Option<Uuid>,
    actor: Option<Uuid>,
    now: DateTime<Utc>,
    jitter_secs: f64,
    epoch_secs: f64,
) -> Result<WaitOutcome, DispatchError> {
    let mut ticker = seam.lock_ticker_for_issue(issue_id).await?;
    let Some(row) = ticker.as_mut() else {
        return Ok(WaitOutcome {
            applied: false,
            reason: WAIT_NO_TICKER.to_owned(),
            ticker: None,
            logs: Vec::new(),
        });
    };
    let issue = CreationSeam::issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let (state, policy) = clock_inputs(seam, &issue).await?;
    let pool = pool_size_or_default(policy.agent_default_max_ticks);
    if pool == pidash_db::tasks_ticker::INFINITE_MAX_TICKS {
        return Ok(WaitOutcome {
            applied: false,
            reason: WAIT_INFINITE_POOL.to_owned(),
            ticker,
            logs: Vec::new(),
        });
    }
    if row.wait_allowance(pool) <= 0 {
        return Ok(WaitOutcome {
            applied: false,
            reason: WAIT_CAP_REACHED.to_owned(),
            ticker,
            logs: Vec::new(),
        });
    }

    row.waited += 1;
    let state_ref = state.as_ref().map(|state| StateRef {
        group: state.group.as_str(),
        name: state.name.as_str(),
    });
    // The re-arm reads the post-increment cap (`cap_reached` after
    // `waited += 1`); the re-armed `next_run_at` draws the call's
    // jitter, like every other re-time.
    let rearmed = wait_rearms(
        row,
        pool,
        clock::project_ticking_enabled(&policy),
        state_ref.as_ref(),
    );
    if rearmed {
        row.enabled = true;
        row.disarm_reason = TickerDisarmReason::None.as_str().to_owned();
        row.next_run_at = Some(clock::compute_next_run_at(
            clock::effective_interval_seconds(state_ref.as_ref(), &policy),
            now,
            jitter_secs,
        ));
    }
    seam.save_wait(row, now, rearmed).await?;

    let project_id = issue.project_id.ok_or_else(|| {
        DispatchError::Store(CreationError::MissingRow("issue has no project".to_owned()))
    })?;
    let actor_id = match actor {
        Some(actor) => actor,
        None => seam.agent_system_user_id().await?,
    };
    seam.insert_wait_activity(&NewWaitActivity {
        id: Uuid::new_v4(),
        now,
        project_id,
        workspace_id: issue.workspace_id,
        issue_id,
        pool,
        waited: row.waited,
        run_id,
        actor_id,
        epoch_secs,
    })
    .await?;

    let logs = vec![agent_wait_log_line(
        issue_id,
        row.waited,
        row.used,
        row.effective_max_ticks(pool),
        run_id,
    )];
    Ok(WaitOutcome {
        applied: true,
        reason: WAIT_GRANTED.to_owned(),
        ticker,
        logs,
    })
}

/// If the schedule was disarmed by **cap exhaustion**, the issue is
/// still in a ticking state, and no other active runs exist on the
/// issue, transition the issue → Paused (`maybe_apply_deferred_pause`,
/// `:1349-1469`, §4.4.1). Idempotent — only the first concurrent
/// terminate event takes effect.
pub async fn maybe_apply_deferred_pause<S: DispatchSeam>(
    seam: &mut S,
    run_id: Uuid,
    now: DateTime<Utc>,
    jitter_secs: f64,
) -> Result<PauseOutcome, DispatchError> {
    let work_item_id = CreationSeam::work_item_id_for_run(seam, run_id)
        .await
        .map_err(DispatchError::Store)?;
    let Some(issue_id) = work_item_id else {
        return Ok(PauseOutcome {
            applied: false,
            logs: Vec::new(),
        });
    };
    // Eager probe (see `pause_guard_verdict`): the unlocked ticker,
    // the issue + state, the L2 verdict, the parking lookup.
    let ticker = seam.ticker_for_issue(issue_id).await?;
    let issue = CreationSeam::issue(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?;
    let state = CreationSeam::state(seam, issue.state_id)
        .await
        .map_err(DispatchError::Store)?;
    let active_run_id = CreationSeam::active_run_for(seam, issue_id)
        .await
        .map_err(DispatchError::Store)?
        .map(|run| run.id);
    let paused_state: Option<StateView> = match issue.project_id {
        Some(project_id) => seam.paused_state_for_project(project_id).await?,
        None => None,
    };
    let paused_state_id = paused_state.as_ref().map(|state| state.id);
    let state_ref = state.as_ref().map(|state| StateRef {
        group: state.group.as_str(),
        name: state.name.as_str(),
    });
    let probe = PauseProbe {
        work_item_id,
        ticker: ticker.as_ref(),
        state: state_ref,
        active_run_id,
        current_run_id: run_id,
        paused_state_id,
    };
    match pause_guard_verdict(&probe) {
        None => {}
        Some(PauseStop::NoAutoPause) => {
            let group = state.as_ref().map_or("", |state| state.group.as_str());
            return Ok(PauseOutcome {
                applied: false,
                logs: vec![cap_hit_leave_log_line(group, issue_id)],
            });
        }
        Some(PauseStop::NoPausedState) => {
            return Ok(PauseOutcome {
                applied: false,
                logs: vec![no_paused_state_log_line(issue_id)],
            });
        }
        Some(_) => {
            return Ok(PauseOutcome {
                applied: false,
                logs: Vec::new(),
            });
        }
    }
    let paused_id = paused_state_id.expect("verdict passed with a Paused state");
    let from_state_id = state.as_ref().expect("verdict passed with a state").id;
    let policy = match issue.project_id {
        Some(project_id) => seam.clock_policy_for_project(project_id).await?,
        None => ProjectClockPolicy::default(),
    };

    // Re-fetch the schedule under a row lock so the disarmed-check
    // stays valid; re-check the reason under the lock; re-fetch the
    // issue to guard a racing transition. Any miss aborts silently.
    let mut locked_ticker = seam
        .lock_ticker_by_id(ticker.as_ref().expect("verdict passed with a ticker").id)
        .await?;
    let recheck = locked_ticker.as_ref().is_some_and(|row| {
        !row.enabled && row.disarm_reason == TickerDisarmReason::CapHit.as_str()
    });
    if !recheck {
        return Ok(PauseOutcome {
            applied: false,
            logs: Vec::new(),
        });
    }
    let locked_issue = seam.lock_issue_by_id(issue_id).await?;
    let same_state = locked_issue
        .as_ref()
        .is_some_and(|locked| locked.state_id == Some(from_state_id));
    if !same_state {
        return Ok(PauseOutcome {
            applied: false,
            logs: Vec::new(),
        });
    }

    seam.update_issue_state(issue_id, paused_id, now).await?;
    // The state-move signal: the clock goes dormant (the reconcile
    // line precedes the auto-paused line, U11).
    let mut logs = Vec::new();
    let event = TickerEvent::left_bucket();
    reconcile_on_seam(
        seam,
        &mut locked_ticker,
        issue_id,
        &paused_state,
        &policy,
        &event,
        now,
        jitter_secs,
        None,
        &mut logs,
    )
    .await?;
    logs.push(auto_paused_log_line(issue_id));
    Ok(PauseOutcome {
        applied: true,
        logs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::HashMap;

    use pidash_db::dispatch::status::{AgentRunStatus, AgentRunTrigger};
    use pidash_types::orchestration::{TRIGGER_COMMENT_AND_RUN, TRIGGER_TICK, WAIT_ACTIVITY_FIELD};

    use super::super::creation::{
        AdmissionError, ExecutionError, ExecutionFields, ExecutionRequest, LockedIssue,
        NewAgentRun, PodView, RenderBundle, RunView, RunnerView,
    };

    static PREFLIGHT: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/preflight.matrix.json");
    static BOUNCE: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/bounce.matrix.json");
    static CREATOR: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/creator.matrix.json");
    static POD: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/pod_resolve.matrix.json");
    static CONTINUATION: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/continuation.matrix.json");
    static RUN_AI: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/run_ai.matrix.json");
    static RETICK: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/retick.matrix.json");
    static WAIT: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/wait.matrix.json");
    static PAUSE: &str =
        include_str!("../../../../fixtures/orchestration/fx08_dispatch/deferred_pause.matrix.json");

    fn fixture(raw: &str) -> Value {
        serde_json::from_str(raw).expect("fixture parses")
    }

    fn fx_logs(raw: &str, case: &str) -> Vec<LogLine> {
        fixture(raw)["cases"][case]["logs"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|line| LogLine {
                level: line["level"].as_str().expect("level").to_owned(),
                message: line["message"].as_str().expect("message").to_owned(),
            })
            .collect()
    }

    /// Deterministic UUIDs.
    fn uid(tag: u8) -> Uuid {
        Uuid::parse_str(&format!("88888888-aaaa-bbbb-cccc-0000000000{tag:02x}")).expect("uuid")
    }

    /// The fixture generator's frozen clock (`2026-06-01T12:00Z`).
    fn now() -> DateTime<Utc> {
        "2026-06-01T12:00:00Z".parse().expect("frozen now parses")
    }

    /// The fixture generator's jitter draw (`random.seed(52408)` →
    /// `random.uniform(0, 1080)`): R3b/R4/W5 re-time to
    /// `15:14:06.322254`.
    const JITTER: f64 = 846.3222537664747;

    fn fx_time(text: &str) -> DateTime<Utc> {
        text.parse().expect("fixture time parses")
    }

    fn issue_view() -> IssueView {
        IssueView {
            id: uid(0x01),
            workspace_id: uid(0x10),
            project_id: Some(uid(0x20)),
            state_id: Some(uid(0x30)),
            parent_id: None,
            created_by_id: Some(uid(0x41)),
            assigned_pod_id: Some(uid(0x50)),
            agent_executor: None,
            git_work_branch: None,
            workpad: None,
            name: Some("FX8".to_owned()),
            description_stripped: None,
            priority: Some("none".to_owned()),
            sequence_id: 1,
            target_date: None,
        }
    }

    fn project_view() -> creation::ProjectView {
        creation::ProjectView {
            id: uid(0x20),
            workspace_id: uid(0x10),
            identifier: "FX8".to_owned(),
            name: "FX8".to_owned(),
            description: None,
            repo_url: None,
            base_branch: None,
            default_agent_executor: "local_runner".to_owned(),
            project_lead_id: Some(uid(0x42)),
            default_assignee_id: Some(uid(0x43)),
            pool: 10,
            interval_impl: 10800,
            interval_review: 10800,
            interval_test: 10800,
        }
    }

    fn state_view(name: &str, group: &str) -> StateView {
        StateView {
            id: uid(0x30),
            name: name.to_owned(),
            group: group.to_owned(),
        }
    }

    fn run_view(id: Uuid, status: AgentRunStatus, trigger: AgentRunTrigger) -> RunView {
        RunView {
            id,
            workspace_id: uid(0x10),
            created_by_id: uid(0x41),
            pod_id: uid(0x50),
            runner_id: None,
            pinned_runner_id: None,
            parent_run_id: None,
            work_item_id: Some(uid(0x01)),
            status,
            trigger: trigger.value().to_owned(),
            executor_kind: AgentExecutorKind::LocalRunner,
            phase_kind: "coding-task".to_owned(),
            run_config: json!({}),
            tool_plan: json!({}),
            error_code: String::new(),
            error: String::new(),
            prompt: String::new(),
            prompt_manifest: None,
            ended_at: None,
        }
    }

    fn ticker_view() -> IssueAgentTicker {
        IssueAgentTicker {
            id: uid(0x60),
            created_at: now(),
            updated_at: now(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id: uid(0x01),
            used: 3,
            granted: 0,
            waited: 0,
            user_disabled: false,
            next_run_at: Some(now()),
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
            resume_parent_run_id: None,
        }
    }

    fn policy_view() -> ProjectClockPolicy {
        ProjectClockPolicy {
            agent_ticking_enabled: Some(true),
            agent_default_max_ticks: Some(10),
            agent_default_interval_seconds: Some(10800),
            agent_review_default_interval_seconds: Some(10800),
            agent_test_default_interval_seconds: Some(10800),
        }
    }

    /// Scripted [`DispatchSeam`]: every read is a field, every write is
    /// captured. Builder legs (`insert_run`, `execution_fields`,
    /// `render_bundle`) are unreachable — the builder-success paths
    /// replay live in jobs.
    #[derive(Default)]
    struct FakeSeam {
        issue: Option<IssueView>,
        project: Option<creation::ProjectView>,
        states: HashMap<Uuid, StateView>,
        active_run: Option<RunView>,
        prior_run: Option<RunView>,
        runs: HashMap<Uuid, RunView>,
        work_items: HashMap<Uuid, Option<Uuid>>,
        flags: HashMap<Uuid, UserFlags>,
        assigned_pod: Option<PodView>,
        default_pod: Option<PodView>,
        resume_parent: Option<Uuid>,
        ticker: Option<IssueAgentTicker>,
        policy: ProjectClockPolicy,
        backlog_target: Option<StateView>,
        in_progress_state: Option<StateView>,
        paused_state: Option<StateView>,
        default_state_id: Option<Uuid>,
        system_user: Uuid,
        candidates: Vec<CandidateUser>,
        slug: String,
        roles: HashMap<Uuid, RoleVerdict>,
        llm: HashMap<Uuid, bool>,
        llm_calls: RefCell<Vec<Uuid>>,
        enrolled: HashSet<Uuid>,
        managed_enabled: bool,
        profiles: HashMap<Uuid, LlmProfile>,
        lock_ticker_by_id: Option<Option<IssueAgentTicker>>,
        lock_issue_by_id: Option<Option<IssueView>>,
        // Captures.
        issue_states: HashMap<Uuid, Uuid>,
        saved_clocks: Vec<IssueAgentTicker>,
        inserted_tickers: Vec<IssueAgentTicker>,
        saved_waits: Vec<(IssueAgentTicker, bool)>,
        comments: Vec<NewIssueComment>,
        activities: Vec<NewWaitActivity>,
    }

    impl FakeSeam {
        fn stocked() -> Self {
            let issue = issue_view();
            let project = project_view();
            let in_progress = state_view("In Progress", "started");
            FakeSeam {
                issue: Some(issue.clone()),
                project: Some(project),
                states: [(in_progress.id, in_progress.clone())]
                    .into_iter()
                    .collect(),
                system_user: uid(0x99),
                slug: "fx8-ws".to_owned(),
                policy: policy_view(),
                ticker: Some(ticker_view()),
                ..FakeSeam::default()
            }
        }

        fn llm_calls(&self) -> Vec<Uuid> {
            self.llm_calls.borrow().clone()
        }
    }

    impl CreationSeam for FakeSeam {
        async fn issue(&mut self, _issue_id: Uuid) -> Result<IssueView, CreationError> {
            // The state UPDATE is visible to later reads in the same
            // driver (the Python instance is rebound).
            let mut issue = self.issue.clone().expect("issue scripted");
            if let Some(state_id) = self.issue_states.get(&issue.id) {
                issue.state_id = Some(*state_id);
            }
            Ok(issue)
        }

        async fn project(
            &mut self,
            _project_id: Uuid,
        ) -> Result<creation::ProjectView, CreationError> {
            Ok(self.project.clone().expect("project scripted"))
        }

        async fn state(
            &mut self,
            state_id: Option<Uuid>,
        ) -> Result<Option<StateView>, CreationError> {
            match state_id {
                None => Ok(None),
                Some(id) => match self.states.get(&id) {
                    Some(state) => Ok(Some(state.clone())),
                    None => Err(CreationError::MissingRow(format!("no state {id}"))),
                },
            }
        }

        async fn latest_prior_run(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            Ok(self.prior_run.clone())
        }

        async fn active_run_for(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            Ok(self.active_run.clone())
        }

        async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
            Ok(self.runs.get(&run_id).cloned())
        }

        async fn runner(&mut self, _runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
            Ok(None)
        }

        async fn assigned_pod(&mut self, _pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
            Ok(self.assigned_pod.clone())
        }

        async fn default_pod_for_project(
            &mut self,
            _project_id: Uuid,
        ) -> Result<Option<PodView>, CreationError> {
            Ok(self.default_pod.clone())
        }

        async fn resume_parent_run_id(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<Uuid>, CreationError> {
            Ok(self.resume_parent)
        }

        async fn work_item_id_for_run(
            &mut self,
            run_id: Uuid,
        ) -> Result<Option<Uuid>, CreationError> {
            Ok(self.work_items.get(&run_id).cloned().flatten())
        }

        async fn lock_issue_for_handoff(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<LockedIssue>, CreationError> {
            todo!("handoff legs are out of L8 scope")
        }

        async fn lock_run_for_handoff(
            &mut self,
            _run_id: Uuid,
        ) -> Result<Option<RunView>, CreationError> {
            todo!("handoff legs are out of L8 scope")
        }

        async fn user_flags(&mut self, user_id: Uuid) -> Result<UserFlags, CreationError> {
            Ok(self.flags.get(&user_id).copied().unwrap_or(UserFlags {
                is_active: true,
                is_bot: false,
            }))
        }

        async fn insert_run(&mut self, _row: &NewAgentRun) -> Result<RunView, CreationError> {
            todo!("builder-success paths replay live in jobs")
        }

        async fn save_prompt(
            &mut self,
            _run_id: Uuid,
            _prompt: &str,
            _manifest: &Value,
        ) -> Result<(), CreationError> {
            todo!("builder-success paths replay live in jobs")
        }

        async fn save_run_config(
            &mut self,
            _run_id: Uuid,
            _config: &Value,
        ) -> Result<(), CreationError> {
            todo!("builder-success paths replay live in jobs")
        }

        async fn execution_fields(
            &mut self,
            _req: &ExecutionRequest,
        ) -> Result<ExecutionFields, ExecutionError> {
            todo!("builder-success paths replay live in jobs")
        }

        async fn lock_cloud_creation_capacity(
            &mut self,
            _workspace_id: Uuid,
            _executor_kind: AgentExecutorKind,
            _automatic: bool,
        ) -> Result<Option<AdmissionError>, CreationError> {
            todo!("builder-success paths replay live in jobs")
        }

        fn dispatch_after_commit(&mut self, _run_id: Uuid) {
            todo!("builder-success paths replay live in jobs")
        }

        async fn render_bundle(
            &mut self,
            _issue_id: Uuid,
            _run_id: Uuid,
            _parent_run_id: Option<Uuid>,
            _trigger: &str,
            _created_by_id: Uuid,
        ) -> Result<RenderBundle, CreationError> {
            todo!("builder-success paths replay live in jobs")
        }

        fn extra_toolsets_schema_tool(&self) -> String {
            String::new()
        }
    }

    impl FinalizeAgentRunSeam for FakeSeam {
        async fn finalize_failed_run(
            &mut self,
            _run_id: Uuid,
            _error_code: &str,
            _error: &str,
            _now: DateTime<Utc>,
        ) -> Result<RunView, CreationError> {
            todo!("builder-success paths replay live in jobs")
        }
    }

    impl DispatchSeam for FakeSeam {
        async fn lock_issue_for_retick(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<IssueView>, DispatchError> {
            Ok(self.issue.clone())
        }

        async fn lock_ticker_for_issue(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<IssueAgentTicker>, DispatchError> {
            // Later locks see earlier saves in the same driver (one
            // transaction).
            if let Some(saved) = self.saved_clocks.last() {
                return Ok(Some(saved.clone()));
            }
            if let Some(inserted) = self.inserted_tickers.last() {
                return Ok(Some(inserted.clone()));
            }
            if let Some((saved, _)) = self.saved_waits.last() {
                return Ok(Some(saved.clone()));
            }
            Ok(self.ticker.clone())
        }

        async fn ticker_for_issue(
            &mut self,
            issue_id: Uuid,
        ) -> Result<Option<IssueAgentTicker>, DispatchError> {
            self.lock_ticker_for_issue(issue_id).await
        }

        async fn lock_ticker_by_id(
            &mut self,
            _ticker_id: Uuid,
        ) -> Result<Option<IssueAgentTicker>, DispatchError> {
            if let Some(scripted) = &self.lock_ticker_by_id {
                return Ok(scripted.clone());
            }
            self.lock_ticker_for_issue(Uuid::nil()).await
        }

        async fn lock_issue_by_id(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Option<IssueView>, DispatchError> {
            if let Some(scripted) = &self.lock_issue_by_id {
                return Ok(scripted.clone());
            }
            Ok(self.issue.clone())
        }

        async fn save_clock(
            &mut self,
            ticker: &IssueAgentTicker,
            _now: DateTime<Utc>,
        ) -> Result<(), DispatchError> {
            self.saved_clocks.push(ticker.clone());
            Ok(())
        }

        async fn insert_ticker(&mut self, ticker: &IssueAgentTicker) -> Result<(), DispatchError> {
            self.inserted_tickers.push(ticker.clone());
            Ok(())
        }

        async fn save_wait(
            &mut self,
            ticker: &IssueAgentTicker,
            _now: DateTime<Utc>,
            rearmed: bool,
        ) -> Result<(), DispatchError> {
            self.saved_waits.push((ticker.clone(), rearmed));
            Ok(())
        }

        async fn backlog_target_for_project(
            &mut self,
            _project_id: Uuid,
        ) -> Result<Option<StateView>, DispatchError> {
            Ok(self.backlog_target.clone())
        }

        async fn in_progress_state_for_project(
            &mut self,
            _project_id: Uuid,
            _group: &str,
            _state_name: &str,
        ) -> Result<Option<StateView>, DispatchError> {
            Ok(self.in_progress_state.clone())
        }

        async fn paused_state_for_project(
            &mut self,
            _project_id: Uuid,
        ) -> Result<Option<StateView>, DispatchError> {
            Ok(self.paused_state.clone())
        }

        async fn default_state_id_for_project(
            &mut self,
            _project_id: Uuid,
        ) -> Result<Option<Uuid>, DispatchError> {
            Ok(self.default_state_id)
        }

        async fn clock_policy_for_project(
            &mut self,
            _project_id: Uuid,
        ) -> Result<ProjectClockPolicy, DispatchError> {
            Ok(self.policy)
        }

        async fn update_issue_state(
            &mut self,
            issue_id: Uuid,
            state_id: Uuid,
            _now: DateTime<Utc>,
        ) -> Result<(), DispatchError> {
            self.issue_states.insert(issue_id, state_id);
            Ok(())
        }

        async fn agent_system_user_id(&mut self) -> Result<Uuid, DispatchError> {
            Ok(self.system_user)
        }

        async fn insert_bounce_comment(
            &mut self,
            comment: &NewIssueComment,
        ) -> Result<(), DispatchError> {
            self.comments.push(comment.clone());
            Ok(())
        }

        async fn insert_wait_activity(
            &mut self,
            activity: &NewWaitActivity,
        ) -> Result<(), DispatchError> {
            self.activities.push(activity.clone());
            Ok(())
        }

        async fn live_assignee_candidates(
            &mut self,
            _issue_id: Uuid,
        ) -> Result<Vec<CandidateUser>, DispatchError> {
            Ok(self.candidates.clone())
        }

        async fn workspace_slug(&mut self, _workspace_id: Uuid) -> Result<String, DispatchError> {
            Ok(self.slug.clone())
        }

        async fn project_role_facts(
            &mut self,
            user_id: Uuid,
            _workspace_slug: &str,
            _project_id: Uuid,
        ) -> Result<RoleVerdict, DispatchError> {
            Ok(self.roles.get(&user_id).cloned().unwrap_or(RoleVerdict {
                has_allowed_role: false,
                is_project_member: false,
                is_workspace_admin: false,
            }))
        }

        async fn enrolled_managed_exists(
            &mut self,
            _project_id: Uuid,
            user_id: Uuid,
            _workspace_id: Uuid,
        ) -> Result<bool, DispatchError> {
            Ok(self.enrolled.contains(&user_id))
        }

        fn managed_runner_enabled(&self) -> bool {
            self.managed_enabled
        }

        fn has_usable_llm_config(&self, user_id: Uuid) -> bool {
            self.llm_calls.borrow_mut().push(user_id);
            self.llm.get(&user_id).copied().unwrap_or(false)
        }

        fn llm_profile_for(&self, user_id: Uuid) -> LlmProfile {
            self.profiles.get(&user_id).cloned().unwrap_or(LlmProfile {
                available: false,
                reason_code: String::new(),
            })
        }
    }

    struct FakeMatcher {
        answer: bool,
    }

    impl PodRunnerMatcher for FakeMatcher {
        fn pod_has_runner_for_issue_principal(
            &self,
            _pod_id: Uuid,
            _issue_id: Uuid,
            _creator_id: Option<Uuid>,
        ) -> Pin<Box<dyn Future<Output = Result<bool, DispatchError>> + Send + '_>> {
            let answer = self.answer;
            Box::pin(async move { Ok(answer) })
        }
    }

    // -- pure units ----------------------------------------------------------

    #[test]
    fn trigger_consts_and_machine() {
        let triggers = &fixture(CREATOR)["cases"]["triggers"];
        assert_eq!(triggers["TRIGGER_TICK"].as_str().unwrap(), TRIGGER_TICK);
        assert_eq!(
            triggers["TRIGGER_COMMENT_AND_RUN"].as_str().unwrap(),
            TRIGGER_COMMENT_AND_RUN
        );
        assert_eq!(triggers["TRIGGER_RUN_AI"].as_str().unwrap(), TRIGGER_RUN_AI);
        assert_eq!(triggers["MACHINE_TRIGGERS"], json!(["tick"]));
        assert!(is_machine_trigger("tick"));
        assert!(!is_machine_trigger("run_ai"));
        assert!(!is_machine_trigger("comment_and_run"));
        assert!(!is_machine_trigger("state_transition"));
    }

    #[test]
    fn refusal_and_wait_consts() {
        let refusals = &fixture(RUN_AI)["cases"]["refusals"];
        assert_eq!(
            refusals["RUN_AI_ACTIVE_RUN_EXISTS"].as_str().unwrap(),
            RUN_AI_ACTIVE_RUN_EXISTS
        );
        assert_eq!(refusals["RUN_AI_NO_POD"].as_str().unwrap(), RUN_AI_NO_POD);
        assert_eq!(
            refusals["RUN_AI_NO_ELIGIBLE_RUNNER"].as_str().unwrap(),
            RUN_AI_NO_ELIGIBLE_RUNNER
        );
        let consts = &fixture(WAIT)["cases"]["constants"];
        assert_eq!(consts["WAIT_GRANTED"].as_str().unwrap(), WAIT_GRANTED);
        assert_eq!(
            consts["WAIT_CAP_REACHED"].as_str().unwrap(),
            WAIT_CAP_REACHED
        );
        assert_eq!(
            consts["WAIT_INFINITE_POOL"].as_str().unwrap(),
            WAIT_INFINITE_POOL
        );
        assert_eq!(consts["WAIT_NO_TICKER"].as_str().unwrap(), WAIT_NO_TICKER);
        assert_eq!(
            consts["WAIT_ACTIVITY_FIELD"].as_str().unwrap(),
            WAIT_ACTIVITY_FIELD
        );
        assert!(
            WAIT_ACTIVITY_INSERT_SQL.contains("'agent_wait'"),
            "the activity field literal rides in the INSERT"
        );
    }

    #[test]
    fn bounce_bodies_verbatim() {
        let b1 = &fixture(BOUNCE)["cases"]["B1_body_no_llm_config"]["comment_html"][0];
        assert_eq!(b1.as_str().unwrap(), BOUNCE_BODY_NO_LLM_CONFIG);
        let b2 = &fixture(BOUNCE)["cases"]["B2_body_default"]["comment_html"][0];
        assert_eq!(b2.as_str().unwrap(), BOUNCE_BODY_DEFAULT);
        assert_eq!(bounce_body("no-llm-config"), BOUNCE_BODY_NO_LLM_CONFIG);
        // Every other reason — including the managed ones (B2b) — takes
        // the default body.
        for reason in [
            "no-eligible-runner",
            "no-managed-runner",
            "managed_runner_disabled",
            "llm_config_missing",
        ] {
            assert_eq!(bounce_body(reason), BOUNCE_BODY_DEFAULT, "{reason}");
        }
    }

    #[test]
    fn strip_tags_b9_golden() {
        let row = &fixture(BOUNCE)["cases"]["B9_comment_row"]["row"];
        let html = row["comment_html"].as_str().unwrap();
        assert_eq!(html, BOUNCE_BODY_DEFAULT);
        assert_eq!(strip_tags(html), row["comment_stripped"].as_str().unwrap());
        // The Django edge: an unclosed `<` is kept verbatim.
        assert_eq!(strip_tags("a<b"), "a<b");
        assert_eq!(strip_tags(""), "");
    }

    #[test]
    fn wait_activity_comment_shapes() {
        let run = uid(0x71);
        assert_eq!(
            wait_activity_comment(10, 1, Some(&run)),
            format!("Waited on a blocker (1 of 10); run {run}")
        );
        assert_eq!(
            wait_activity_comment(10, 2, Some(&run)),
            format!("Waited on a blocker (2 of 10); run {run}")
        );
        assert_eq!(
            wait_activity_comment(10, 1, None),
            "Waited on a blocker (1 of 10)"
        );
        // Against the fixture activities (W1 with run, W9 without).
        let w1 = &fixture(WAIT)["cases"]["W1_applied"]["activity"];
        assert_eq!(
            wait_activity_comment(
                w1["old_value"].as_str().unwrap().parse().unwrap(),
                w1["new_value"].as_str().unwrap().parse().unwrap(),
                Some(&run),
            ),
            w1["comment"]
                .as_str()
                .unwrap()
                .replace("run-W1", &run.to_string()),
        );
        let w9 = &fixture(WAIT)["cases"]["W9_activity_no_run"]["activity"];
        assert_eq!(
            wait_activity_comment(10, 1, None),
            w9["comment"].as_str().unwrap()
        );
    }

    #[test]
    fn agent_wait_log_renders_none_run() {
        let line = agent_wait_log_line(uid(0x01), 1, 3, 11, None);
        assert_eq!(line.level, "INFO");
        assert!(line.message.ends_with("run=None"), "{}", line.message);
        let line = agent_wait_log_line(uid(0x01), 1, 3, 11, Some(uid(0x71)));
        assert!(
            line.message.ends_with(&format!("run={}", uid(0x71))),
            "{}",
            line.message
        );
    }

    #[test]
    fn managed_profile_reason_falls_back() {
        let available = LlmProfile {
            available: true,
            reason_code: String::new(),
        };
        assert_eq!(
            managed_profile_reason_code(&available),
            ManagedRunnerReason::LLM_CONFIG_MISSING
        );
        let coded = LlmProfile {
            available: false,
            reason_code: "byok_not_supported_on_desktop".to_owned(),
        };
        assert_eq!(
            managed_profile_reason_code(&coded),
            "byok_not_supported_on_desktop"
        );
        let empty = LlmProfile {
            available: false,
            reason_code: String::new(),
        };
        assert_eq!(
            managed_profile_reason_code(&empty),
            ManagedRunnerReason::LLM_CONFIG_MISSING
        );
    }

    #[test]
    fn cloud_candidate_dedups_before_flags() {
        let mut seen = HashSet::new();
        let live = CandidateUser {
            id: uid(0x43),
            is_active: true,
            is_bot: false,
        };
        assert!(select_cloud_candidate(&live, &mut seen));
        // The dup never re-runs the seams, even though the flags pass.
        assert!(!select_cloud_candidate(&live, &mut seen));
        let bot = CandidateUser {
            id: uid(0x44),
            is_active: true,
            is_bot: true,
        };
        assert!(!select_cloud_candidate(&bot, &mut seen));
        // …but its id still lands in `seen`.
        assert!(seen.contains(&uid(0x44)));
        let inactive = CandidateUser {
            id: uid(0x45),
            is_active: false,
            is_bot: false,
        };
        assert!(!select_cloud_candidate(&inactive, &mut seen));
    }

    #[test]
    fn pause_verdict_matrix() {
        let ticker = ticker_view();
        let ticking = StateRef {
            group: "started",
            name: "In Progress",
        };
        let base = PauseProbe {
            work_item_id: Some(uid(0x01)),
            ticker: Some(&ticker),
            state: Some(ticking),
            active_run_id: None,
            current_run_id: uid(0x70),
            paused_state_id: Some(uid(0x31)),
        };
        // U3: enabled.
        assert_eq!(pause_guard_verdict(&base), Some(PauseStop::TickerEnabled));
        let mut disarmed = ticker.clone();
        disarmed.enabled = false;
        disarmed.disarm_reason = "cap_hit".to_owned();
        let disarmed_probe = PauseProbe {
            ticker: Some(&disarmed),
            ..base
        };
        // U11-shape: proceeds.
        assert_eq!(pause_guard_verdict(&disarmed_probe), None);
        // U1/U2/U6 heads.
        assert_eq!(
            pause_guard_verdict(&PauseProbe {
                work_item_id: None,
                ..disarmed_probe
            }),
            Some(PauseStop::NoWorkItem)
        );
        assert_eq!(
            pause_guard_verdict(&PauseProbe {
                ticker: None,
                ..disarmed_probe
            }),
            Some(PauseStop::NoTicker)
        );
        assert_eq!(
            pause_guard_verdict(&PauseProbe {
                state: None,
                ..disarmed_probe
            }),
            Some(PauseStop::NoState)
        );
    }

    #[test]
    fn pause_verdict_stop_heads() {
        let ticking = StateRef {
            group: "started",
            name: "In Progress",
        };
        fn mk<'a>(
            ticker: &'a IssueAgentTicker,
            state: StateRef<'a>,
            active: Option<Uuid>,
            paused: Option<Uuid>,
        ) -> PauseProbe<'a> {
            PauseProbe {
                work_item_id: Some(uid(0x01)),
                ticker: Some(ticker),
                state: Some(state),
                active_run_id: active,
                current_run_id: uid(0x70),
                paused_state_id: paused,
            }
        }
        // U5: pool_spent never auto-pauses.
        let mut ticker = ticker_view();
        ticker.enabled = false;
        ticker.disarm_reason = "pool_spent".to_owned();
        assert_eq!(
            pause_guard_verdict(&mk(&ticker, ticking, None, Some(uid(0x31)))),
            Some(PauseStop::NotCapHit)
        );
        // U4: a queued entry run keeps the issue alive.
        ticker.disarm_reason = "cap_hit".to_owned();
        ticker.pending_entry = true;
        assert_eq!(
            pause_guard_verdict(&mk(&ticker, ticking, None, Some(uid(0x31)))),
            Some(PauseStop::PendingEntry)
        );
        ticker.pending_entry = false;
        // U7: left the bucket already.
        let done = StateRef {
            group: "completed",
            name: "Done",
        };
        assert_eq!(
            pause_guard_verdict(&mk(&ticker, done, None, Some(uid(0x31)))),
            Some(PauseStop::NotTicking)
        );
        // U8: review never auto-pauses.
        let review = StateRef {
            group: "review",
            name: "In Review",
        };
        assert_eq!(
            pause_guard_verdict(&mk(&ticker, review, None, Some(uid(0x31)))),
            Some(PauseStop::NoAutoPause)
        );
        // U9: another run keeps the issue alive …
        assert_eq!(
            pause_guard_verdict(&mk(&ticker, ticking, Some(uid(0x72)), Some(uid(0x31)))),
            Some(PauseStop::OtherActiveRun)
        );
        // … but the terminating run itself is fine (U12-shape).
        assert_eq!(
            pause_guard_verdict(&mk(&ticker, ticking, Some(uid(0x70)), Some(uid(0x31)))),
            None
        );
        // U10: nowhere to park.
        assert_eq!(
            pause_guard_verdict(&mk(&ticker, ticking, None, None)),
            Some(PauseStop::NoPausedState)
        );
    }

    // -- replay scaffolding --------------------------------------------------

    /// The fixture generator's fixed seed ids
    /// (`88888888-aaaa-bbbb-cccc-` + suffix): the fake wears them so
    /// collected log lines compare byte-for-byte.
    fn fxid(suffix: &str) -> Uuid {
        Uuid::parse_str(&format!("88888888-aaaa-bbbb-cccc-00000000{suffix}")).expect("fxid")
    }

    fn stocked_as(issue_suffix: &str) -> FakeSeam {
        let mut seam = FakeSeam::stocked();
        let id = fxid(issue_suffix);
        seam.issue.as_mut().expect("issue").id = id;
        if let Some(ticker) = seam.ticker.as_mut() {
            ticker.issue_id = id;
        }
        seam
    }

    fn backlog_hi() -> StateView {
        StateView {
            id: uid(0x31),
            name: "BacklogHi".to_owned(),
            group: "backlog".to_owned(),
        }
    }

    fn as_executor(seam: &mut FakeSeam, executor: &str) {
        seam.project
            .as_mut()
            .expect("project")
            .default_agent_executor = executor.to_owned();
    }

    fn member_roles() -> RoleVerdict {
        RoleVerdict {
            has_allowed_role: true,
            is_project_member: true,
            is_workspace_admin: false,
        }
    }

    // -- preflight (F) -------------------------------------------------------

    #[tokio::test]
    async fn preflight_f1_cloud_llm_ok() {
        let mut seam = stocked_as("8101");
        as_executor(&mut seam, "cloud_agent");
        let creator = uid(0x41);
        seam.llm.insert(creator, true);
        let matcher = FakeMatcher { answer: false };
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8101"),
            Some(creator),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(out.proceed);
        assert!(out.logs.is_empty());
        assert!(seam.comments.is_empty());
    }

    #[tokio::test]
    async fn preflight_f2_cloud_no_llm() {
        let mut seam = stocked_as("8102");
        as_executor(&mut seam, "cloud_agent");
        let creator = uid(0x41);
        seam.llm.insert(creator, false);
        seam.backlog_target = Some(backlog_hi());
        seam.ticker = None;
        let matcher = FakeMatcher { answer: false };
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8102"),
            Some(creator),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(!out.proceed);
        assert_eq!(out.logs, fx_logs(PREFLIGHT, "F2_cloud_no_llm"));
        assert_eq!(seam.issue_states.get(&fxid("8102")), Some(&uid(0x31)));
        assert_eq!(seam.comments.len(), 1);
        assert_eq!(seam.comments[0].comment_html, BOUNCE_BODY_NO_LLM_CONFIG);
    }

    #[tokio::test]
    async fn preflight_f3_managed_chain() {
        // F3a: no creator at all.
        let mut seam = stocked_as("8103");
        as_executor(&mut seam, "managed_runner");
        seam.backlog_target = Some(backlog_hi());
        seam.ticker = None;
        let matcher = FakeMatcher { answer: false };
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8103"),
            None,
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(!out.proceed);
        assert_eq!(out.logs, fx_logs(PREFLIGHT, "F3a_managed_no_creator"));

        // F3b: the operator kill switch is off.
        let mut seam = stocked_as("8104");
        as_executor(&mut seam, "managed_runner");
        seam.backlog_target = Some(backlog_hi());
        seam.ticker = None;
        seam.managed_enabled = false;
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8104"),
            Some(uid(0x41)),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(!out.proceed);
        assert_eq!(out.logs, fx_logs(PREFLIGHT, "F3b_managed_disabled"));

        // F3c: no usable desktop profile.
        let mut seam = stocked_as("8105");
        as_executor(&mut seam, "managed_runner");
        seam.backlog_target = Some(backlog_hi());
        seam.ticker = None;
        seam.managed_enabled = true;
        seam.profiles.insert(
            uid(0x41),
            LlmProfile {
                available: false,
                reason_code: "llm_config_missing".to_owned(),
            },
        );
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8105"),
            Some(uid(0x41)),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(!out.proceed);
        assert_eq!(out.logs, fx_logs(PREFLIGHT, "F3c_managed_no_profile"));

        // F3d: nothing enrolled for this project.
        let mut seam = stocked_as("8106");
        as_executor(&mut seam, "managed_runner");
        seam.backlog_target = Some(backlog_hi());
        seam.ticker = None;
        seam.managed_enabled = true;
        seam.profiles.insert(
            uid(0x41),
            LlmProfile {
                available: true,
                reason_code: String::new(),
            },
        );
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8106"),
            Some(uid(0x41)),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(!out.proceed);
        assert_eq!(out.logs, fx_logs(PREFLIGHT, "F3d_managed_not_enrolled"));

        // F3e: every structural gate passes — "your laptop is closed"
        // is not one of them.
        let mut seam = stocked_as("8107");
        as_executor(&mut seam, "managed_runner");
        seam.managed_enabled = true;
        seam.profiles.insert(
            uid(0x41),
            LlmProfile {
                available: true,
                reason_code: String::new(),
            },
        );
        seam.enrolled.insert(uid(0x41));
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8107"),
            Some(uid(0x41)),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(out.proceed);
        assert!(out.logs.is_empty());
        assert!(seam.comments.is_empty());
    }

    #[tokio::test]
    async fn preflight_f4_local_matcher() {
        // F4a: a registered runner accepts the principal.
        let mut seam = stocked_as("8108");
        let matcher = FakeMatcher { answer: true };
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8108"),
            Some(uid(0x41)),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(out.proceed);
        assert!(out.logs.is_empty());
        assert!(seam.comments.is_empty());

        // F4b: no runner in the pod, and no Backlog target anywhere on
        // the project — the issue stays, the ticker (absent here)
        // needs no disarm line.
        let mut seam = stocked_as("8109");
        seam.project.as_mut().expect("project").id = fxid("0808");
        seam.issue.as_mut().expect("issue").project_id = Some(fxid("0808"));
        seam.backlog_target = None;
        seam.default_state_id = None;
        seam.ticker = None;
        let matcher = FakeMatcher { answer: false };
        let out = preflight_eligibility_or_bounce(
            &mut seam,
            fxid("8109"),
            Some(uid(0x41)),
            uid(0x50),
            "tick",
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("preflight");
        assert!(!out.proceed);
        assert_eq!(out.logs, fx_logs(PREFLIGHT, "F4b_local_no_runner"));
        assert!(seam.issue_states.is_empty());
        assert_eq!(seam.comments.len(), 1);
    }

    // -- bounce (B) ----------------------------------------------------------

    fn bounce_case(suffix: &str) -> FakeSeam {
        stocked_as(suffix)
    }

    #[tokio::test]
    async fn bounce_b1_move_disarms_through_signal() {
        let mut seam = bounce_case("8110");
        seam.ticker.as_mut().expect("ticker").used = 2;
        seam.backlog_target = Some(backlog_hi());
        let out = bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8110"),
            "tick",
            REASON_NO_LLM_CONFIG,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        assert_eq!(out.moved_to, Some(uid(0x31)));
        assert_eq!(out.logs, fx_logs(BOUNCE, "B1_body_no_llm_config"));
        assert_eq!(seam.comments.len(), 1);
        assert_eq!(seam.comments[0].comment_html, BOUNCE_BODY_NO_LLM_CONFIG);
        // The `left_bucket` signal write: dormant, pool kept.
        let saved = seam.saved_clocks.last().expect("clock saved");
        assert_eq!(saved.used, 2);
        assert!(!saved.enabled);
        assert_eq!(saved.disarm_reason, "left_ticking_state");
        assert_eq!(saved.next_run_at, None);
    }

    #[tokio::test]
    async fn bounce_bodies_and_targets() {
        // B2: the default body on a plain move.
        let mut seam = bounce_case("8111");
        seam.ticker.as_mut().expect("ticker").used = 2;
        seam.backlog_target = Some(backlog_hi());
        let out = bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8111"),
            "tick",
            REASON_NO_ELIGIBLE_RUNNER,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        assert_eq!(out.moved_to, Some(uid(0x31)));
        assert_eq!(out.logs, fx_logs(BOUNCE, "B2_body_default"));
        assert_eq!(seam.comments[0].comment_html, BOUNCE_BODY_DEFAULT);

        // B2b: a managed reason still takes the default body.
        let mut seam = bounce_case("8112");
        seam.ticker = None;
        seam.backlog_target = Some(backlog_hi());
        let out = bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8112"),
            "tick",
            REASON_NO_MANAGED_RUNNER,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        assert_eq!(out.moved_to, Some(uid(0x31)));
        assert_eq!(out.logs, fx_logs(BOUNCE, "B2b_body_managed_reason"));
        assert_eq!(seam.comments[0].comment_html, BOUNCE_BODY_DEFAULT);

        // B3a/B3b: the ordering lives in the SQL (`default` first,
        // then `sequence`) — the driver moves wherever it resolves.
        for (suffix, case) in [
            ("8113", "B3a_default_beats_sequence"),
            ("8114", "B3b_sequence_order"),
        ] {
            let mut seam = bounce_case(suffix);
            seam.ticker = None;
            seam.backlog_target = Some(backlog_hi());
            let out = bounce_issue_no_eligible_runner(
                &mut seam,
                fxid(suffix),
                "tick",
                REASON_NO_ELIGIBLE_RUNNER,
                now(),
                JITTER,
            )
            .await
            .expect("bounce");
            assert_eq!(out.moved_to, Some(uid(0x31)));
            assert_eq!(out.logs, fx_logs(BOUNCE, case));
            assert_eq!(seam.comments.len(), 1);
        }
        // The live suite pins the ordering itself (B3a/B3b live).
    }

    #[tokio::test]
    async fn bounce_b4_default_state_fallback() {
        let mut seam = bounce_case("8115");
        seam.ticker = None;
        seam.backlog_target = None;
        let todo = StateView {
            id: uid(0x32),
            name: "Todo".to_owned(),
            group: "unstarted".to_owned(),
        };
        seam.states.insert(todo.id, todo);
        seam.default_state_id = Some(uid(0x32));
        let out = bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8115"),
            "tick",
            REASON_NO_ELIGIBLE_RUNNER,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        assert_eq!(out.moved_to, Some(uid(0x32)));
        assert_eq!(out.logs, fx_logs(BOUNCE, "B4_default_state_fallback"));
        assert_eq!(seam.comments.len(), 1);
    }

    #[tokio::test]
    async fn bounce_b5_b6_unbounceable_disarms() {
        // B5: the fallback is itself ticking — unsafe, the issue stays.
        let mut seam = bounce_case("8116");
        seam.project.as_mut().expect("project").id = fxid("0808");
        seam.issue.as_mut().expect("issue").project_id = Some(fxid("0808"));
        seam.ticker.as_mut().expect("ticker").used = 2;
        seam.backlog_target = None;
        seam.default_state_id = Some(uid(0x30));
        let out = bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8116"),
            "tick",
            REASON_NO_ELIGIBLE_RUNNER,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        assert_eq!(out.moved_to, None);
        assert_eq!(out.logs, fx_logs(BOUNCE, "B5_ticking_fallback_unbouncable"));
        assert!(seam.issue_states.is_empty());
        assert_eq!(seam.comments.len(), 1);
        let saved = seam.saved_clocks.last().expect("explicit disarm saved");
        assert!(!saved.enabled);
        assert_eq!(saved.disarm_reason, "left_ticking_state");
        assert_eq!(saved.next_run_at, None);

        // B6: no target and no fallback at all.
        let mut seam = bounce_case("8117");
        seam.project.as_mut().expect("project").id = fxid("0808");
        seam.issue.as_mut().expect("issue").project_id = Some(fxid("0808"));
        seam.ticker.as_mut().expect("ticker").used = 2;
        seam.backlog_target = None;
        seam.default_state_id = None;
        let out = bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8117"),
            "tick",
            REASON_NO_ELIGIBLE_RUNNER,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        assert_eq!(out.moved_to, None);
        assert_eq!(out.logs, fx_logs(BOUNCE, "B6_no_target_unbouncable"));
        assert_eq!(seam.comments.len(), 1);
    }

    #[tokio::test]
    async fn bounce_b7_already_backlog() {
        let mut seam = bounce_case("8118");
        let backlog_lo = StateView {
            id: uid(0x33),
            name: "BacklogLo".to_owned(),
            group: "backlog".to_owned(),
        };
        seam.states.insert(backlog_lo.id, backlog_lo);
        seam.issue.as_mut().expect("issue").state_id = Some(uid(0x33));
        seam.ticker = None;
        let out = bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8118"),
            "tick",
            REASON_NO_ELIGIBLE_RUNNER,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        assert_eq!(out.moved_to, None);
        assert_eq!(out.logs, fx_logs(BOUNCE, "B7_already_backlog"));
        assert!(seam.issue_states.is_empty());
        // The comment still posts so the user sees *why*.
        assert_eq!(seam.comments.len(), 1);
    }

    #[tokio::test]
    async fn bounce_b9_comment_shape() {
        let mut seam = stocked_as("8119");
        seam.ticker = None;
        seam.backlog_target = Some(backlog_hi());
        bounce_issue_no_eligible_runner(
            &mut seam,
            fxid("8119"),
            TRIGGER_RUN_AI,
            REASON_NO_ELIGIBLE_RUNNER,
            now(),
            JITTER,
        )
        .await
        .expect("bounce");
        let row = &fixture(BOUNCE)["cases"]["B9_comment_row"]["row"];
        let comment = seam.comments.last().expect("comment written");
        assert_eq!(comment.actor_id, uid(0x99));
        assert_eq!(comment.comment_html, row["comment_html"].as_str().unwrap());
        assert_eq!(
            comment.comment_stripped,
            row["comment_stripped"].as_str().unwrap()
        );
        assert_eq!(comment.project_id, uid(0x20));
        assert_eq!(comment.workspace_id, uid(0x10));
        assert_eq!(comment.issue_id, fxid("8119"));
        assert_eq!(comment.now, now());
    }

    // -- creators (L/C) + pods (D) --------------------------------------------

    fn cloud_executor(seam: &mut FakeSeam) {
        as_executor(seam, "cloud_agent");
    }

    #[tokio::test]
    async fn creator_local_chain() {
        // L1: an explicit actor always wins on a local runner.
        let mut seam = FakeSeam::stocked();
        let actor = uid(0x46);
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "tick", Some(actor))
            .await
            .expect("resolve");
        assert_eq!(out, Some(actor));

        // L2: a tick with no actor resolves the system bot.
        let mut seam = FakeSeam::stocked();
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "tick", None)
            .await
            .expect("resolve");
        assert_eq!(out, Some(uid(0x99)));

        // L3: a Run AI click falls back to the issue creator.
        let mut seam = FakeSeam::stocked();
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", None)
            .await
            .expect("resolve");
        assert_eq!(out, Some(uid(0x41)));

        // L4: the whole chain empty answers None (local, silent).
        let mut seam = FakeSeam::stocked();
        seam.issue.as_mut().expect("issue").created_by_id = None;
        seam.project.as_mut().expect("project").project_lead_id = None;
        seam.project.as_mut().expect("project").default_assignee_id = None;
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", None)
            .await
            .expect("resolve");
        assert_eq!(out, None);
    }

    #[tokio::test]
    async fn executor_override_short_circuits_project_read() {
        // Python's `or` (`agent_execution.py:66`): a set per-issue
        // override wins without touching the project row — even when
        // the issue has no project at all (the fake `project()` would
        // panic on "project scripted" if it were read).
        let mut seam = FakeSeam::stocked();
        seam.issue.as_mut().expect("issue").project_id = None;
        seam.issue.as_mut().expect("issue").agent_executor = Some("local_runner".to_owned());
        seam.project = None;
        let actor = uid(0x46);
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "tick", Some(actor))
            .await
            .expect("resolve");
        assert_eq!(out, Some(actor));
    }

    #[tokio::test]
    async fn creator_cloud_filter() {
        let live = uid(0x43);
        let lead = uid(0x42);
        let creator = uid(0x41);

        // C1: the actor fast path (non-machine trigger).
        let mut seam = FakeSeam::stocked();
        cloud_executor(&mut seam);
        seam.llm.insert(live, true);
        seam.roles.insert(live, member_roles());
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", Some(live))
            .await
            .expect("resolve");
        assert_eq!(out, Some(live));

        // C2: a machine trigger ignores the actor entirely.
        let mut seam = FakeSeam::stocked();
        cloud_executor(&mut seam);
        seam.llm.insert(creator, true);
        seam.roles.insert(creator, member_roles());
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "tick", Some(live))
            .await
            .expect("resolve");
        assert_eq!(out, Some(creator));
        assert!(
            !seam.llm_calls().contains(&live),
            "the actor is never a candidate on a tick"
        );

        // C3a: the bot is skipped, the lead wins, the LLM seam sees
        // only the lead.
        let mut seam = FakeSeam::stocked();
        cloud_executor(&mut seam);
        seam.flags.insert(
            creator,
            UserFlags {
                is_active: true,
                is_bot: true,
            },
        );
        seam.llm.insert(lead, true);
        seam.roles.insert(lead, member_roles());
        seam.candidates = vec![
            CandidateUser {
                id: lead,
                is_active: true,
                is_bot: false,
            },
            CandidateUser {
                id: live,
                is_active: true,
                is_bot: false,
            },
        ];
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", None)
            .await
            .expect("resolve");
        assert_eq!(out, Some(lead));
        assert_eq!(seam.llm_calls(), vec![lead]);

        // C3b: the dup lead row never re-runs the LLM seam; the
        // soft-deleted assignee is never offered (live SQL filters it).
        let mut seam = FakeSeam::stocked();
        cloud_executor(&mut seam);
        seam.project.as_mut().expect("project").default_assignee_id = None;
        seam.flags.insert(
            creator,
            UserFlags {
                is_active: true,
                is_bot: true,
            },
        );
        seam.llm.insert(lead, false);
        seam.llm.insert(live, true);
        seam.roles.insert(live, member_roles());
        seam.candidates = vec![
            CandidateUser {
                id: lead,
                is_active: true,
                is_bot: false,
            },
            CandidateUser {
                id: live,
                is_active: true,
                is_bot: false,
            },
        ];
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", None)
            .await
            .expect("resolve");
        assert_eq!(out, Some(live));
        assert_eq!(seam.llm_calls(), vec![lead, live]);

        // C4: inactive candidates are skipped before the LLM seam.
        let mut seam = FakeSeam::stocked();
        cloud_executor(&mut seam);
        seam.flags.insert(
            creator,
            UserFlags {
                is_active: false,
                is_bot: false,
            },
        );
        seam.llm.insert(lead, false);
        seam.llm.insert(live, true);
        seam.roles.insert(live, member_roles());
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", None)
            .await
            .expect("resolve");
        assert_eq!(out, Some(live));
        assert_eq!(seam.llm_calls(), vec![lead, live]);
    }

    #[tokio::test]
    async fn creator_managed_and_none() {
        // C5: the managed enrollment leg skips the unenrolled.
        let mut seam = FakeSeam::stocked();
        as_executor(&mut seam, "managed_runner");
        for id in [uid(0x41), uid(0x42), uid(0x43)] {
            seam.llm.insert(id, true);
            seam.roles.insert(id, member_roles());
        }
        seam.enrolled.insert(uid(0x43));
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", None)
            .await
            .expect("resolve");
        assert_eq!(out, Some(uid(0x43)));

        // C6: nobody qualifies.
        let mut seam = FakeSeam::stocked();
        cloud_executor(&mut seam);
        let out = resolve_creator_for_trigger(&mut seam, uid(0x01), "run_ai", None)
            .await
            .expect("resolve");
        assert_eq!(out, None);
    }

    #[tokio::test]
    async fn pod_resolve_matrix() {
        assert_eq!(
            fixture(POD)["cases"]
                .as_object()
                .expect("cases")
                .keys()
                .collect::<Vec<_>>(),
            [
                "D1_assigned_pod",
                "D2_dangling_assigned_pod",
                "D3_project_default",
                "D4_no_project"
            ]
            .into_iter()
            .collect::<Vec<_>>(),
        );
        // D1: the live assigned pod wins.
        let mut seam = FakeSeam::stocked();
        seam.assigned_pod = Some(PodView {
            id: uid(0x50),
            project_id: uid(0x20),
        });
        seam.default_pod = Some(PodView {
            id: uid(0x51),
            project_id: uid(0x20),
        });
        let out = creation::resolve_pod_for_issue(&mut seam, uid(0x01))
            .await
            .expect("resolve");
        assert_eq!(out, Some(uid(0x50)));

        // D2: a dangling assigned id falls back to the default.
        let mut seam = FakeSeam::stocked();
        seam.assigned_pod = None;
        seam.default_pod = Some(PodView {
            id: uid(0x51),
            project_id: uid(0x20),
        });
        let out = creation::resolve_pod_for_issue(&mut seam, uid(0x01))
            .await
            .expect("resolve");
        assert_eq!(out, Some(uid(0x51)));

        // D3: unset assignment reads the default.
        let mut seam = FakeSeam::stocked();
        seam.issue.as_mut().expect("issue").assigned_pod_id = None;
        seam.assigned_pod = None;
        seam.default_pod = Some(PodView {
            id: uid(0x51),
            project_id: uid(0x20),
        });
        let out = creation::resolve_pod_for_issue(&mut seam, uid(0x01))
            .await
            .expect("resolve");
        assert_eq!(out, Some(uid(0x51)));

        // D4: no project, no pod.
        let mut seam = FakeSeam::stocked();
        seam.issue.as_mut().expect("issue").assigned_pod_id = None;
        seam.issue.as_mut().expect("issue").project_id = None;
        seam.assigned_pod = None;
        let out = creation::resolve_pod_for_issue(&mut seam, uid(0x01))
            .await
            .expect("resolve");
        assert_eq!(out, None);
    }

    // -- continuation guards (A) ----------------------------------------------

    fn with_prior(seam: &mut FakeSeam) {
        seam.prior_run = Some(run_view(
            uid(0x70),
            AgentRunStatus::Completed,
            AgentRunTrigger::Tick,
        ));
    }

    #[tokio::test]
    async fn continuation_a1_a2_skips() {
        // A1: an active run blocks creation.
        let mut seam = stocked_as("8127");
        seam.active_run = Some(run_view(
            uid(0x71),
            AgentRunStatus::Running,
            AgentRunTrigger::Tick,
        ));
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_continuation_run(
            &mut seam,
            fxid("8127"),
            "tick",
            None,
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.logs, fx_logs(CONTINUATION, "A1_active_run"));

        // A2: nothing to continue from.
        let mut seam = stocked_as("8128");
        let out = dispatch_continuation_run(
            &mut seam,
            fxid("8128"),
            "tick",
            None,
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.logs, fx_logs(CONTINUATION, "A2_no_prior_run"));
    }

    #[tokio::test]
    async fn continuation_a6_system_creator_bounces() {
        // A6: no *human* creator — but a tick resolves the system bot,
        // which the matcher rejects, so the issue bounces.
        let mut seam = stocked_as("8132");
        with_prior(&mut seam);
        seam.issue.as_mut().expect("issue").created_by_id = None;
        seam.project.as_mut().expect("project").project_lead_id = None;
        seam.project.as_mut().expect("project").default_assignee_id = None;
        seam.assigned_pod = Some(PodView {
            id: uid(0x50),
            project_id: uid(0x20),
        });
        seam.backlog_target = Some(backlog_hi());
        seam.ticker = None;
        let matcher = FakeMatcher { answer: false };
        let out = dispatch_continuation_run(
            &mut seam,
            fxid("8132"),
            "tick",
            None,
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.logs, fx_logs(CONTINUATION, "A6_local_no_creator"));
        assert_eq!(seam.comments.len(), 1);
    }

    #[tokio::test]
    async fn continuation_a7_cloud_no_creator_bounces() {
        let mut seam = stocked_as("8133");
        cloud_executor(&mut seam);
        with_prior(&mut seam);
        seam.project.as_mut().expect("project").id = fxid("0809");
        seam.issue.as_mut().expect("issue").project_id = Some(fxid("0809"));
        seam.backlog_target = None;
        seam.default_state_id = None;
        seam.ticker = None;
        let matcher = FakeMatcher { answer: false };
        let out = dispatch_continuation_run(
            &mut seam,
            fxid("8133"),
            "tick",
            None,
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(
            out.logs,
            fx_logs(CONTINUATION, "A7_cloud_no_creator_bounce")
        );
        assert_eq!(seam.comments.len(), 1);
        assert_eq!(seam.comments[0].comment_html, BOUNCE_BODY_NO_LLM_CONFIG);
    }

    #[tokio::test]
    async fn continuation_a8_no_pod() {
        let mut seam = stocked_as("8134");
        with_prior(&mut seam);
        seam.assigned_pod = None;
        seam.default_pod = None;
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_continuation_run(
            &mut seam,
            fxid("8134"),
            "tick",
            None,
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.logs, fx_logs(CONTINUATION, "A8_no_pod"));
        assert!(seam.comments.is_empty());
    }

    // -- run-ai refusals (H) ---------------------------------------------------

    #[tokio::test]
    async fn run_ai_h1_h2_h3_refusals() {
        let matcher = FakeMatcher { answer: true };

        // H1: an active run refuses with `active_run_exists`.
        let mut seam = stocked_as("8136");
        seam.active_run = Some(run_view(
            uid(0x71),
            AgentRunStatus::Running,
            AgentRunTrigger::Tick,
        ));
        let out = dispatch_run_ai_run_with_reason(
            &mut seam,
            fxid("8136"),
            Some(uid(0x41)),
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.reason.as_deref(), Some(RUN_AI_ACTIVE_RUN_EXISTS));
        assert_eq!(out.logs, fx_logs(RUN_AI, "H1_active_refused"));

        // H2: no pod refuses with `no_pod`.
        let mut seam = stocked_as("8137");
        seam.assigned_pod = None;
        seam.default_pod = None;
        let out = dispatch_run_ai_run_with_reason(
            &mut seam,
            fxid("8137"),
            Some(uid(0x41)),
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.reason.as_deref(), Some(RUN_AI_NO_POD));
        assert_eq!(out.logs, fx_logs(RUN_AI, "H2_no_pod"));

        // H3: no creator on a local runner refuses silently.
        let mut seam = stocked_as("8138");
        seam.issue.as_mut().expect("issue").created_by_id = None;
        seam.project.as_mut().expect("project").project_lead_id = None;
        seam.project.as_mut().expect("project").default_assignee_id = None;
        let out =
            dispatch_run_ai_run_with_reason(&mut seam, fxid("8138"), None, &matcher, now(), JITTER)
                .await
                .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.reason.as_deref(), Some(RUN_AI_NO_ELIGIBLE_RUNNER));
        assert_eq!(out.logs, fx_logs(RUN_AI, "H3_no_creator"));
        assert!(seam.comments.is_empty());
    }

    #[tokio::test]
    async fn run_ai_h4_preflight_bounce() {
        let mut seam = stocked_as("8139");
        seam.project.as_mut().expect("project").id = fxid("0808");
        seam.issue.as_mut().expect("issue").project_id = Some(fxid("0808"));
        seam.assigned_pod = Some(PodView {
            id: uid(0x50),
            project_id: fxid("0808"),
        });
        seam.backlog_target = None;
        seam.default_state_id = None;
        seam.ticker = None;
        let matcher = FakeMatcher { answer: false };
        let out = dispatch_run_ai_run_with_reason(
            &mut seam,
            fxid("8139"),
            Some(uid(0x41)),
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.reason.as_deref(), Some(RUN_AI_NO_ELIGIBLE_RUNNER));
        assert_eq!(out.logs, fx_logs(RUN_AI, "H4_preflight_bounce"));
        assert_eq!(seam.comments.len(), 1);
        assert!(seam.issue_states.is_empty());
    }

    #[tokio::test]
    async fn run_ai_h7a_wrapper_refused() {
        let mut seam = stocked_as("8141");
        seam.active_run = Some(run_view(
            uid(0x71),
            AgentRunStatus::Running,
            AgentRunTrigger::Tick,
        ));
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_run_ai_run(
            &mut seam,
            fxid("8141"),
            Some(uid(0x41)),
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.run_id, None);
        assert_eq!(out.logs, fx_logs(RUN_AI, "H7a_wrapper_refused"));
    }

    // -- retick guards (R) + in-progress lookup (Q) -----------------------------

    #[tokio::test]
    async fn retick_r1_no_issue() {
        let mut seam = FakeSeam::stocked();
        seam.issue = None;
        let matcher = FakeMatcher { answer: true };
        let out = re_tick_ticker(
            &mut seam,
            uid(0x01),
            Some(uid(0x41)),
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("retick");
        assert!(!out.granted);
        assert_eq!(out.reason, "no_issue");
        assert_eq!(out.ticker, None);
        assert_eq!(out.run_id, None);
        assert!(!out.rollback);
        assert!(out.logs.is_empty());
    }

    #[tokio::test]
    async fn retick_r2_r7_pool_guards() {
        let matcher = FakeMatcher { answer: true };

        // R2: budget not exhausted — untouched.
        let mut seam = FakeSeam::stocked();
        let out = re_tick_ticker(
            &mut seam,
            uid(0x01),
            Some(uid(0x41)),
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("retick");
        assert!(!out.granted);
        assert_eq!(out.reason, "budget_not_exhausted");
        assert_eq!(out.run_id, None);
        assert!(!out.rollback);
        assert!(seam.saved_clocks.is_empty());
        assert!(seam.inserted_tickers.is_empty());
        let ticker = out.ticker.expect("decision ticker");
        assert_eq!(ticker.used, 3);
        assert_eq!(ticker.granted, 0);

        // R7: an infinite pool never exhausts.
        let mut seam = FakeSeam::stocked();
        seam.policy.agent_default_max_ticks = Some(-1);
        let out = re_tick_ticker(
            &mut seam,
            uid(0x01),
            Some(uid(0x41)),
            &matcher,
            now(),
            JITTER,
        )
        .await
        .expect("retick");
        assert!(!out.granted);
        assert_eq!(out.reason, "budget_not_exhausted");
        assert!(seam.saved_clocks.is_empty());
    }

    #[test]
    fn in_progress_lookup_sql_shape() {
        // Q1/Q2 pin the Django text; the port projects the consumed
        // columns but keeps the predicates, bind order and ordering.
        for case in ["Q1_found", "Q2_none"] {
            let sql = fixture(RETICK)["cases"][case]["sql"][0]
                .as_str()
                .expect("sql")
                .to_lowercase();
            assert!(sql.contains("\"states\".\"deleted_at\" is null"), "{case}");
            assert!(
                sql.contains("order by \"states\".\"sequence\" asc limit 1"),
                "{case}"
            );
        }
        assert!(IN_PROGRESS_STATE_SQL.contains("project_id = $1"));
        assert!(IN_PROGRESS_STATE_SQL.contains("\"group\" = $2"));
        assert!(IN_PROGRESS_STATE_SQL.contains("name = $3"));
        assert!(IN_PROGRESS_STATE_SQL.contains("deleted_at IS NULL"));
        assert!(IN_PROGRESS_STATE_SQL.contains("ORDER BY sequence ASC LIMIT 1"));
        // Q3: soft-deleted rows are skipped by the deleted guard.
    }

    #[tokio::test]
    async fn in_progress_state_none_without_registry() {
        // The registry always names a `started` state today; the driver
        // answers the seam's miss as None (R6-shape, live).
        let mut seam = FakeSeam::stocked();
        seam.in_progress_state = None;
        let out = in_progress_state_for(&mut seam, uid(0x20))
            .await
            .expect("lookup");
        assert_eq!(out, None);
    }

    // -- wait (W) -----------------------------------------------------------------

    const EPOCH: f64 = 1_780_000_000.0;

    fn wait_case() -> FakeSeam {
        FakeSeam::stocked()
    }

    #[tokio::test]
    async fn wait_w2_w3_w4_refusals() {
        // W2: the wait allowance is spent.
        let mut seam = wait_case();
        seam.ticker.as_mut().expect("ticker").waited = 10;
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            None,
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(!out.applied);
        assert_eq!(out.reason, WAIT_CAP_REACHED);
        assert!(seam.saved_waits.is_empty());
        assert!(seam.activities.is_empty());

        // W3: an infinite pool has no budget to buy back.
        let mut seam = wait_case();
        seam.policy.agent_default_max_ticks = Some(-1);
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            None,
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(!out.applied);
        assert_eq!(out.reason, WAIT_INFINITE_POOL);
        assert!(seam.saved_waits.is_empty());

        // W4: no clock, no budget to refund.
        let mut seam = wait_case();
        seam.ticker = None;
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            None,
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(!out.applied);
        assert_eq!(out.reason, WAIT_NO_TICKER);
        assert_eq!(out.ticker, None);
    }

    #[tokio::test]
    async fn wait_w5_rearm_cap_hit() {
        let mut seam = wait_case();
        let ticker = seam.ticker.as_mut().expect("ticker");
        ticker.used = 5;
        ticker.enabled = false;
        ticker.disarm_reason = "cap_hit".to_owned();
        ticker.next_run_at = None;
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            None,
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(out.applied);
        assert_eq!(out.reason, WAIT_GRANTED);
        // The raised cap left room (cap 11 > used 5): re-armed with the
        // call's jitter draw.
        assert_eq!(seam.saved_waits.len(), 1);
        let (saved, rearmed) = seam.saved_waits.last().expect("wait saved");
        assert!(rearmed);
        assert_eq!(saved.waited, 1);
        assert!(saved.enabled);
        assert_eq!(saved.disarm_reason, "");
        assert_eq!(
            saved.next_run_at,
            Some(fx_time("2026-06-01T15:14:06.322254+00:00"))
        );
        let activity = seam.activities.last().expect("activity written");
        let fx = &fixture(WAIT)["cases"]["W5_rearm_cap_hit"]["activity"];
        assert_eq!(activity.pool.to_string(), fx["old_value"].as_str().unwrap());
        assert_eq!(
            activity.waited.to_string(),
            fx["new_value"].as_str().unwrap()
        );
        assert_eq!(
            wait_activity_comment(activity.pool, activity.waited, activity.run_id.as_ref()),
            fx["comment"].as_str().unwrap()
        );
        assert_eq!(activity.actor_id, uid(0x41));
        assert_eq!(activity.epoch_secs, EPOCH);
        assert_eq!(out.logs.len(), 1);
        assert_eq!(out.logs[0].level, "INFO");
        assert!(
            out.logs[0].message.starts_with("agent_wait: "),
            "{}",
            out.logs[0].message
        );
    }

    #[tokio::test]
    async fn wait_w6_w7_w8_no_rearm() {
        // W6: the user switch stays off.
        let mut seam = wait_case();
        let ticker = seam.ticker.as_mut().expect("ticker");
        ticker.used = 5;
        ticker.enabled = false;
        ticker.disarm_reason = "cap_hit".to_owned();
        ticker.next_run_at = None;
        ticker.user_disabled = true;
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            None,
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(out.applied);
        let (saved, rearmed) = seam.saved_waits.last().expect("wait saved");
        assert!(!rearmed);
        assert!(!saved.enabled);
        assert_eq!(saved.disarm_reason, "cap_hit");

        // W7: the raised cap still leaves no room.
        let mut seam = wait_case();
        let ticker = seam.ticker.as_mut().expect("ticker");
        ticker.used = 20;
        ticker.enabled = false;
        ticker.disarm_reason = "cap_hit".to_owned();
        ticker.next_run_at = None;
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            None,
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(out.applied);
        let (saved, rearmed) = seam.saved_waits.last().expect("wait saved");
        assert!(!rearmed);
        assert_eq!(saved.waited, 1);

        // W8: the issue left the bucket (and the reason is not re-armable).
        let mut seam = wait_case();
        let ticker = seam.ticker.as_mut().expect("ticker");
        ticker.used = 5;
        ticker.enabled = false;
        ticker.disarm_reason = "left_ticking_state".to_owned();
        ticker.next_run_at = None;
        let done = StateView {
            id: uid(0x34),
            name: "Done".to_owned(),
            group: "completed".to_owned(),
        };
        seam.states.insert(done.id, done);
        seam.issue.as_mut().expect("issue").state_id = Some(uid(0x34));
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            None,
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(out.applied);
        let (saved, rearmed) = seam.saved_waits.last().expect("wait saved");
        assert!(!rearmed);
        assert_eq!(saved.disarm_reason, "left_ticking_state");
    }

    #[tokio::test]
    async fn wait_w9_w10_activity_rows() {
        // W9: no actor — the system bot owns the activity; no run —
        // no suffix.
        let mut seam = wait_case();
        let out = wait_ticker(&mut seam, uid(0x01), None, None, now(), JITTER, EPOCH)
            .await
            .expect("wait");
        assert!(out.applied);
        let activity = seam.activities.last().expect("activity written");
        assert_eq!(activity.actor_id, uid(0x99));
        assert_eq!(
            wait_activity_comment(activity.pool, activity.waited, activity.run_id.as_ref()),
            fixture(WAIT)["cases"]["W9_activity_no_run"]["activity"]["comment"]
                .as_str()
                .unwrap()
        );

        // W10: a second wait in the same run spends again.
        let mut seam = wait_case();
        seam.ticker.as_mut().expect("ticker").waited = 1;
        let run = uid(0x71);
        let out = wait_ticker(
            &mut seam,
            uid(0x01),
            Some(run),
            Some(uid(0x41)),
            now(),
            JITTER,
            EPOCH,
        )
        .await
        .expect("wait");
        assert!(out.applied);
        let (saved, _) = seam.saved_waits.last().expect("wait saved");
        assert_eq!(saved.waited, 2);
        let activity = seam.activities.last().expect("activity written");
        assert_eq!(
            wait_activity_comment(activity.pool, activity.waited, activity.run_id.as_ref()),
            fixture(WAIT)["cases"]["W10_second_wait"]["activity"]["comment"]
                .as_str()
                .unwrap()
                .replace("run-W1", &run.to_string()),
        );
    }

    // -- deferred pause (U) ---------------------------------------------------------

    fn pause_case(suffix: &str, run_id: Uuid) -> FakeSeam {
        let mut seam = stocked_as(suffix);
        let ticker = seam.ticker.as_mut().expect("ticker");
        ticker.used = 10;
        ticker.enabled = false;
        ticker.disarm_reason = "cap_hit".to_owned();
        ticker.next_run_at = None;
        seam.work_items.insert(run_id, Some(fxid(suffix)));
        seam.paused_state = Some(StateView {
            id: uid(0x35),
            name: "Paused".to_owned(),
            group: "backlog".to_owned(),
        });
        seam
    }

    #[tokio::test]
    async fn pause_u1_u7_silent_stops() {
        // U1: the run is orphaned.
        let mut seam = pause_case("8160", uid(0x70));
        seam.work_items.insert(uid(0x70), None);
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert!(out.logs.is_empty());

        // U2: the issue never armed a clock.
        let mut seam = pause_case("8161", uid(0x70));
        seam.ticker = None;
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert!(out.logs.is_empty());

        // U3: the clock is still live.
        let mut seam = pause_case("8162", uid(0x70));
        let ticker = seam.ticker.as_mut().expect("ticker");
        ticker.enabled = true;
        ticker.disarm_reason = String::new();
        ticker.next_run_at = Some(now());
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert!(seam.issue_states.is_empty());

        // U4: a queued entry will fire — do not pull the issue out.
        let mut seam = pause_case("8163", uid(0x70));
        seam.ticker.as_mut().expect("ticker").pending_entry = true;
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);

        // U5: pool_spent never auto-pauses.
        let mut seam = pause_case("8164", uid(0x70));
        seam.ticker.as_mut().expect("ticker").disarm_reason = "pool_spent".to_owned();
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);

        // U6: stateless.
        let mut seam = pause_case("8165", uid(0x70));
        seam.issue.as_mut().expect("issue").state_id = None;
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);

        // U7: already out of the bucket.
        let mut seam = pause_case("8166", uid(0x70));
        let done = StateView {
            id: uid(0x34),
            name: "Done".to_owned(),
            group: "completed".to_owned(),
        };
        seam.states.insert(done.id, done);
        seam.issue.as_mut().expect("issue").state_id = Some(uid(0x34));
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert!(out.logs.is_empty());
    }

    #[tokio::test]
    async fn pause_u8_u9_u10_loud_and_silent_stops() {
        // U8: review never auto-pauses — the issue stays put for a human.
        let mut seam = pause_case("8170", uid(0x70));
        let review = StateView {
            id: uid(0x36),
            name: "In Review".to_owned(),
            group: "review".to_owned(),
        };
        seam.states.insert(review.id, review);
        seam.issue.as_mut().expect("issue").state_id = Some(uid(0x36));
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert_eq!(out.logs, fx_logs(PAUSE, "U8_review_no_autopause"));

        // U9: another active run keeps the issue alive.
        let mut seam = pause_case("8171", uid(0x70));
        seam.active_run = Some(run_view(
            uid(0x72),
            AgentRunStatus::Running,
            AgentRunTrigger::Tick,
        ));
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert!(out.logs.is_empty());

        // U10: nowhere to park.
        let mut seam = pause_case("8172", uid(0x70));
        seam.paused_state = None;
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert_eq!(out.logs, fx_logs(PAUSE, "U10_no_paused_state"));
    }

    #[tokio::test]
    async fn pause_u12_self_active_applies() {
        // U12: the terminating run itself is active — allowed (the
        // U11-shape, whose live twin runs in jobs).
        let mut seam = pause_case("8174", uid(0x70));
        seam.active_run = Some(run_view(
            uid(0x70),
            AgentRunStatus::Running,
            AgentRunTrigger::Tick,
        ));
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(out.applied);
        assert_eq!(out.logs, fx_logs(PAUSE, "U12_self_active_allowed"));
        assert_eq!(seam.issue_states.get(&fxid("8174")), Some(&uid(0x35)));
        let saved = seam.saved_clocks.last().expect("signal write saved");
        assert!(!saved.enabled);
        assert_eq!(saved.disarm_reason, "left_ticking_state");
    }

    #[tokio::test]
    async fn pause_lock_rechecks_abort_silently() {
        // The concurrency-only branches (noted, not injected, in the
        // fixture method): each lock miss aborts with no lines.
        let mut seam = pause_case("8175", uid(0x70));
        seam.lock_ticker_by_id = Some(None);
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert!(seam.issue_states.is_empty());

        let mut seam = pause_case("8175", uid(0x70));
        let mut rearmed = seam.ticker.clone().expect("ticker");
        rearmed.enabled = true;
        rearmed.disarm_reason = String::new();
        seam.lock_ticker_by_id = Some(Some(rearmed));
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);

        let mut seam = pause_case("8175", uid(0x70));
        let mut flipped = seam.ticker.clone().expect("ticker");
        flipped.disarm_reason = "left_ticking_state".to_owned();
        seam.lock_ticker_by_id = Some(Some(flipped));
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);

        let mut seam = pause_case("8175", uid(0x70));
        seam.lock_issue_by_id = Some(None);
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);

        let mut seam = pause_case("8175", uid(0x70));
        let mut moved = seam.issue.clone().expect("issue");
        moved.state_id = Some(uid(0x36));
        seam.lock_issue_by_id = Some(Some(moved));
        let out = maybe_apply_deferred_pause(&mut seam, uid(0x70), now(), JITTER)
            .await
            .expect("pause");
        assert!(!out.applied);
        assert!(seam.issue_states.is_empty());
    }
}
