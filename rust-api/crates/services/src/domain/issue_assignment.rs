//! Dedicated-runner shared rules (PDASHOSS01-297,
//! `.ai_design/dedicated_runner_assignment/design.md` §6-§14).
//!
//! Mirrors `apps/api/pi_dash/runner/services/issue_assignment.py`: read-side
//! helpers (`queue_depth`, `is_busy`, `assignable`, `assigned_runner_detail`,
//! `dispatch_wait`) used by the web-app and v1 serializers and the runner
//! list; the shared E1-E8 (sans E6) validation; the pin resolver; the
//! run-time refusal rules (H-11/H-10); and the lifecycle writes
//! (`apply_execution_target`, `clear_dedicated_runner`, the lost-runner
//! policy, the 12-hour bound and the stranded sweep).
//!
//! `local_runner` is the only executor an issue can have (design rev 2),
//! so there is no executor arm here; the E6/`executor_not_local` code is
//! gone with I2. The engine rules this module composes (`runner_may_take`,
//! `locality_pin_allowed`, `engine_takes_pod_work`, `can_use_runner`) live
//! in `pidash_auth::permissions::runner` (NEWPIDASH-49 port of #2903) and
//! are reused, never re-implemented.

use std::borrow::Cow;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
};
use sea_query::LockType;
use serde_json::{json, Value};
use uuid::Uuid;

use pidash_auth::permissions::runner::{
    can_use_runner as auth_can_use_runner, locality_pin_allowed as auth_locality_pin_allowed,
    runner_may_take as auth_runner_may_take, RunnerFacts,
};
use pidash_db::entity::{
    agent_run, dev_machine, issue_activities, issue_assignees, issue_comments, issues, pod,
    projects, runner, users,
};
use pidash_types::runner_runs::{AgentRunStatus, AgentRunTrigger, HUMAN_TRIGGERS};

use crate::runner_sessions::guards::{alive_threshold, BUSY_STATUSES};

/// Unwrap `db.transaction(..)` (both arms are `DbErr`).
fn unmap(error: sea_orm::TransactionError<sea_orm::DbErr>) -> sea_orm::DbErr {
    match error {
        sea_orm::TransactionError::Connection(e) => e,
        sea_orm::TransactionError::Transaction(e) => e,
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `PinKind.LOCALITY` (`runner/models.py`).
pub const PIN_LOCALITY: &str = "locality";
/// `PinKind.DEDICATED` (`runner/models.py`).
pub const PIN_DEDICATED: &str = "dedicated";

/// The engine provisioning value (`RunnerProvisioning.DESKTOP_BUNDLED`).
pub const PROVISIONING_DESKTOP_BUNDLED: &str = "desktop_bundled";
/// `RunnerPodDispatch.PINNED_ONLY`.
pub const POD_DISPATCH_PINNED_ONLY: &str = "pinned_only";
/// `RunnerPodDispatch.POD_MEMBER`.
pub const POD_DISPATCH_POD_MEMBER: &str = "pod_member";
/// `RunnerStatus.REVOKED` / `.OFFLINE` / `.ONLINE`.
pub const RUNNER_STATUS_REVOKED: &str = "revoked";
pub const RUNNER_STATUS_OFFLINE: &str = "offline";
pub const RUNNER_STATUS_ONLINE: &str = "online";

/// `DEDICATED_RUN_QUEUED_MAX_AGE_SECS` (12 h, design §11.3): the bound an
/// automatic dedicated run waits at most. The Django config registry
/// default is 43200; the Rust server reads the same env with the same
/// default (the `tasks_cleanup` precedent), and `Settings` gains the
/// field in a later slice.
pub fn dedicated_run_queued_max_age_secs() -> i64 {
    std::env::var("DEDICATED_RUN_QUEUED_MAX_AGE_SECS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(43200)
}

/// `settings.DEDICATED_RUN_QUEUED_MAX_AGE_SECS` for the `dispatch_wait`
/// `expires_at` and the 12-hour bound.
pub fn dedicated_max_age_secs() -> i64 {
    dedicated_run_queued_max_age_secs()
}

// ---------------------------------------------------------------------------
// Read-side helpers (design §7.4, §10.1, §11.1, §14.1, §14.3)
// ---------------------------------------------------------------------------

/// Number of QUEUED runs pinned to `runner_id` (design §10.1).
pub async fn queue_depth(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
) -> Result<u64, sea_orm::DbErr> {
    agent_run::Entity::find()
        .filter(agent_run::Column::PinnedRunnerId.eq(runner_id))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .count(db)
        .await
}

/// True when the runner has a run in BUSY_STATUSES (design §10.1).
pub async fn is_busy(db: &impl ConnectionTrait, runner_id: Uuid) -> Result<bool, sea_orm::DbErr> {
    let statuses: Vec<String> = BUSY_STATUSES.iter().map(|s| s.value().to_owned()).collect();
    let found = agent_run::Entity::find()
        .filter(agent_run::Column::RunnerId.eq(runner_id))
        .filter(agent_run::Column::Status.is_in(statuses))
        .one(db)
        .await?;
    Ok(found.is_some())
}

/// Offline per design §11.1 (status or heartbeat freshness).
pub fn runner_is_offline(runner: &runner::Model, now: DateTime<Utc>) -> bool {
    if runner.status != RUNNER_STATUS_ONLINE {
        return true;
    }
    match runner.last_heartbeat_at {
        None => true,
        Some(last) => last < alive_threshold(now),
    }
}

/// Why `runner` is paused, or `None` when it is not paused (design §11.1,
/// §23.5). Paused wins over offline in `dispatch_wait`/`dedicated_run_refusal`
/// because a person must act on it.
pub fn runner_paused_reason<'a>(
    runner: &'a runner::Model,
    machine: Option<&'a dev_machine::Model>,
) -> Option<Cow<'a, str>> {
    if runner.paused_at.is_some() {
        return Some(Cow::Borrowed("dispatch_paused"));
    }
    if runner.provisioning == PROVISIONING_DESKTOP_BUNDLED {
        if let Some(machine) = machine {
            if machine.engine_paused_by_owner {
                return Some(Cow::Borrowed("owner_paused"));
            }
            let reason = machine.engine_auto_pause_reason.as_str();
            if !reason.is_empty() {
                let reason = match reason {
                    "engine_sign_in_required" => "engine_sign_in_required",
                    "gateway_session_revoked" => "gateway_session_revoked",
                    "wallet_empty" => "wallet_empty",
                    "profile_unavailable" => "profile_unavailable",
                    "engine_disabled" => "engine_disabled",
                    other => return Some(Cow::Borrowed(other)),
                };
                return Some(Cow::Borrowed(reason));
            }
        }
    }
    None
}

/// House style for ISO timestamps in serialised payloads.
fn iso(value: Option<chrono::DateTime<chrono::FixedOffset>>) -> Option<String> {
    value.map(|v| v.to_rfc3339())
}

/// The flat inputs of the `assigned_runner_detail` object (design §14.1).
/// Split from the fetch so sqlx-only render paths (the api crate's
/// `render_detail_map`) can fill it with one row + counts and share the
/// exact JSON shape.
#[derive(Clone, Debug)]
pub struct RunnerDetailRow {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub pod_id: Uuid,
    pub owner_id: Uuid,
    pub owner_display_name: String,
    pub host_label: String,
    pub last_heartbeat_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub is_busy: bool,
    pub queue_depth: u64,
}

/// The `assigned_runner_detail` object (design §14.1) from flat inputs.
pub fn runner_detail_value(row: &RunnerDetailRow) -> Value {
    json!({
        "id": row.id.to_string(),
        "name": row.name,
        "status": row.status,
        "pod": row.pod_id.to_string(),
        "owner": row.owner_id.to_string(),
        "owner_display_name": row.owner_display_name,
        "host_label": row.host_label,
        "last_heartbeat_at": iso(row.last_heartbeat_at),
        "is_busy": row.is_busy,
        "queue_depth": row.queue_depth,
    })
}

/// The `assigned_runner_detail` object (design §14.1) or `Null`.
///
/// Shown to everyone who can see the issue (H-6), even though the runner
/// list itself is owner-only.
pub async fn assigned_runner_detail(
    db: &impl ConnectionTrait,
    runner: &runner::Model,
) -> Result<Value, sea_orm::DbErr> {
    let owner_display_name = match users::Entity::find_by_id(runner.owner_id).one(db).await? {
        Some(owner) => owner.display_name,
        None => String::new(),
    };
    Ok(runner_detail_value(&RunnerDetailRow {
        id: runner.id,
        name: runner.name.clone(),
        status: runner.status.clone(),
        pod_id: runner.pod_id,
        owner_id: runner.owner_id,
        owner_display_name,
        host_label: runner.host_label.clone(),
        last_heartbeat_at: runner.last_heartbeat_at,
        is_busy: is_busy(db, runner.id).await?,
        queue_depth: queue_depth(db, runner.id).await?,
    }))
}

// ---------------------------------------------------------------------------
// E1-E8 validation (design §7.1, sans E6)
// ---------------------------------------------------------------------------

/// The E1-E8 evaluation inputs for one dedicated choice. `issue_id` `None`
/// means "new" (E7 satisfied by construction, E2 reads `project_id`).
/// `pod_id` is the requested execution pod, if the request names one.
#[derive(Clone, Copy, Debug, Default)]
pub struct AssignmentCheck<'a> {
    pub user_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub issue: Option<&'a issues::Model>,
}

/// Live assignees of the issue (soft-delete manager: the un-assign path
/// only stamps `deleted_at` on the through row).
pub async fn issue_people(
    db: &impl ConnectionTrait,
    issue_id: Uuid,
) -> Result<HashSet<Uuid>, sea_orm::DbErr> {
    let mut people = HashSet::new();
    let rows = issue_assignees::Entity::find()
        .filter(issue_assignees::Column::IssueId.eq(issue_id))
        .filter(issue_assignees::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    for row in rows {
        people.insert(row.assignee_id);
    }
    Ok(people)
}

/// Shared E1-E8 (sans E6) evaluation for one dedicated choice (design §7.1).
///
/// Returns `(code, detail)` — `code` is `""` when the choice is assignable,
/// else the first failing E-code in E1-E8 order, with the human-readable
/// `detail` for the refusal.
pub async fn check_assignment_code(
    db: &impl ConnectionTrait,
    runner_row: &runner::Model,
    check: &AssignmentCheck<'_>,
) -> Result<(&'static str, &'static str), sea_orm::DbErr> {
    // E1: the requester can use the runner (today: owns it).
    let facts = RunnerFacts {
        workspace: pidash_types::WorkspaceId(runner_row.workspace_id.to_string()),
        authenticated: check.user_id.is_some(),
        visibility: i32::from(runner_row.visibility),
        owned_by_requester: check.user_id == Some(runner_row.owner_id),
    };
    if !auth_can_use_runner(&facts) {
        return Ok(("runner_not_found", "runner not found"));
    }
    // E2: the runner's pod is in the issue's project.
    let runner_pod = pod::Entity::find_by_id(runner_row.pod_id).one(db).await?;
    let runner_project = runner_pod.as_ref().map(|p| p.project_id);
    if let Some(issue) = check.issue {
        if runner_project != Some(issue.project_id) {
            return Ok(("runner_wrong_project", "runner is in a different project"));
        }
    } else if let Some(project_id) = check.project_id {
        if runner_project != Some(project_id) {
            return Ok(("runner_wrong_project", "runner is in a different project"));
        }
    } else if let Some(pod_id) = check.pod_id {
        let requested = pod::Entity::find_by_id(pod_id).one(db).await?;
        if requested.as_ref().map(|p| p.project_id) != runner_project {
            return Ok(("runner_wrong_project", "runner is in a different project"));
        }
    }
    // E3: a request that also names a pod must name the runner's pod.
    if let Some(pod_id) = check.pod_id {
        if pod_id != runner_row.pod_id {
            return Ok(("runner_pod_mismatch", "runner is in a different pod"));
        }
    }
    // E4: not revoked.
    if runner_row.status == RUNNER_STATUS_REVOKED || runner_row.revoked_at.is_some() {
        return Ok(("runner_revoked", "runner has been revoked"));
    }
    // E5: lifted for engine runners (§23.1) — a desktop_bundled runner is
    // now choosable, so no check here. E6 is removed (rev 2).
    // E7: the runner's owner is the issue's creator or a current assignee.
    if let Some(issue) = check.issue {
        let mut people = issue_people(db, issue.id).await?;
        if let Some(created_by) = issue.created_by_id {
            people.insert(created_by);
        }
        if !people.contains(&runner_row.owner_id) {
            return Ok((
                "runner_owner_not_on_issue",
                "the runner's owner is not on this issue",
            ));
        }
    }
    // E8: no run the runner holds; QUEUED and PAUSED_AWAITING_INPUT pass.
    if let Some(issue) = check.issue {
        if has_blocking_run(db, issue.id).await? {
            return Ok((
                "active_run_in_progress",
                "the issue has an active run; changes take effect on the next run",
            ));
        }
    }
    Ok(("", ""))
}

/// True when the issue has a run in a non-terminal status other than
/// QUEUED / PAUSED_AWAITING_INPUT (the E8 blocking set). `paused_awaiting_input`
/// is not an `AgentRunStatus` member in Rust types; it is read by value.
pub async fn has_blocking_run(
    db: &impl ConnectionTrait,
    issue_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let blocking = crate::runner_sessions::guards::NON_TERMINAL_STATUSES
        .iter()
        .filter(|s| **s != AgentRunStatus::Queued)
        .map(|s| s.value().to_owned())
        .collect::<Vec<_>>();
    let found = agent_run::Entity::find()
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
        .filter(agent_run::Column::Status.is_in(blocking))
        .one(db)
        .await?;
    Ok(found.is_some())
}

/// Evaluate E1-E8 (sans E6) for `runner` as a dedicated choice (design §7.1).
///
/// Returns `(assignable, code)` where `code` is `""` when assignable, else
/// the first failing E-code in E1-E8 order. Read-side only: the write path
/// (`validate_execution_target`) shares these rules but raises.
pub async fn assignable_for_runner(
    db: &impl ConnectionTrait,
    runner_row: &runner::Model,
    check: &AssignmentCheck<'_>,
) -> Result<(bool, &'static str), sea_orm::DbErr> {
    let (code, _) = check_assignment_code(db, runner_row, check).await?;
    Ok((code.is_empty(), code))
}

/// One field validation failure, keyed on the field the caller posted
/// (mirroring DRF's `ValidationError({field: ErrorDetail(detail, code)})`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub code: &'static str,
    pub detail: &'static str,
}

/// The write-path request for `validate_execution_target`. `runner_present`
/// distinguishes "key absent" (pod-only change) from "explicit clear".
/// `runner_id` `None` with `runner_present` is a clear.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutionTargetWrite {
    pub issue_id: Option<Uuid>,
    pub runner_present: bool,
    pub runner_id: Option<Uuid>,
    pub pod_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

/// Why `validate_execution_target` failed: a malformed assignment choice,
/// or a database failure. Callers render [`FieldError`] as the API's
/// `{field: detail}` validation body with `code` on the error detail;
/// database errors follow the usual 500 path.
#[derive(Debug)]
pub enum ValidateError {
    Field(FieldError),
    Db(sea_orm::DbErr),
}

impl From<sea_orm::DbErr> for ValidateError {
    fn from(error: sea_orm::DbErr) -> Self {
        ValidateError::Db(error)
    }
}

/// Validate the requested execution target (design §7.1).
///
/// When a runner is named, E1-E8 are evaluated and the first failure is
/// returned as [`ValidateError::Field`] keyed on `assigned_runner` (the
/// attrs/source key both Django serializers raise on — the app surface
/// posts `assigned_runner_id`, but Django's `validate()` keys the error
/// on `assigned_runner`). Skips runner checks when the request names no
/// runner — except E8: clearing the runner under an ASSIGNED/RUNNING
/// run is refused with `active_run_in_progress` (§13.2). `project_id`
/// carries E2 on create (when there is no issue yet). A no-op resend of
/// the current runner is never rejected (§7.2).
pub async fn validate_execution_target(
    db: &impl ConnectionTrait,
    write: &ExecutionTargetWrite,
) -> Result<(), ValidateError> {
    if !write.runner_present {
        return Ok(());
    }
    // Both Django serializers carry the runner in attrs under the
    // source/model key `assigned_runner`, so the error keys there.
    let field = "assigned_runner";
    match write.runner_id {
        None => {
            // A clear is a runner change: E8 applies (§13.2). The no-op
            // case (already unassigned) passes by construction.
            if let Some(issue_id) = write.issue_id {
                if let Some(issue) = issues::Entity::find_by_id(issue_id).one(db).await? {
                    if has_blocking_run(db, issue.id).await? {
                        return Err(ValidateError::Field(FieldError {
                            field,
                            code: "active_run_in_progress",
                            detail:
                                "the issue has an active run; changes take effect on the next run",
                        }));
                    }
                }
            }
            Ok(())
        }
        Some(runner_id) => {
            // No-op resend (§7.2): re-sending the current runner is never
            // rejected, even after eligibility drifted.
            if let Some(issue_id) = write.issue_id {
                if let Some(issue) = issues::Entity::find_by_id(issue_id).one(db).await? {
                    if issue.assigned_runner_id == Some(runner_id) {
                        return Ok(());
                    }
                }
            }
            let Some(runner_row) = runner::Entity::find_by_id(runner_id).one(db).await? else {
                return Err(ValidateError::Field(FieldError {
                    field,
                    code: "runner_not_found",
                    detail: "runner not found",
                }));
            };
            let issue = match write.issue_id {
                Some(issue_id) => issues::Entity::find_by_id(issue_id).one(db).await?,
                None => None,
            };
            let check = AssignmentCheck {
                user_id: write.user_id,
                issue_id: write.issue_id,
                pod_id: write.pod_id,
                project_id: write.project_id,
                issue: issue.as_ref(),
            };
            let (code, detail) = check_assignment_code(db, &runner_row, &check).await?;
            if !code.is_empty() {
                return Err(ValidateError::Field(FieldError {
                    field,
                    code,
                    detail,
                }));
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Engine usage rule (Python form, NEWPIDASH-49 kernel)
// ---------------------------------------------------------------------------

/// `runner_may_take` for one run against `runner` (Python form of the usage
/// rule, `dispatch_rules.py`). A run its owner created is always takable;
/// otherwise the owner must be involved (on the issue, or the binding's
/// author); for an engine, an involved run is takable only when automatic
/// or `team_runs` is on.
pub async fn runner_may_take_run(
    db: &impl ConnectionTrait,
    runner_row: &runner::Model,
    created_by_id: Option<Uuid>,
    human: bool,
    issue: Option<&issues::Model>,
) -> Result<bool, sea_orm::DbErr> {
    let owner = runner_row.owner_id;
    let created_by_owner = created_by_id == Some(owner);
    let is_engine = runner_row.provisioning == PROVISIONING_DESKTOP_BUNDLED;
    let team_runs = if is_engine {
        match runner_row.dev_machine_id {
            Some(machine_id) => dev_machine::Entity::find_by_id(machine_id)
                .one(db)
                .await?
                .map(|m| m.engine_team_runs)
                .unwrap_or(false),
            None => false,
        }
    } else {
        true
    };
    let issue_involves_owner = match issue {
        Some(issue) => {
            if issue.created_by_id == Some(owner) {
                true
            } else {
                let people = issue_people(db, issue.id).await?;
                people.contains(&owner)
            }
        }
        None => false,
    };
    // `scheduler_authored_by_owner`: the run's scheduler binding was
    // authored by the owner. Callers that know the binding pass it through
    // [`runner_may_take_binding`]; this entry has no binding facts.
    Ok(auth_runner_may_take(
        is_engine,
        team_runs,
        created_by_owner,
        human,
        issue_involves_owner,
        false,
    ))
}

/// The pinned result of `resolve_run_pin` / `locality_runner_for`:
/// the runner (if pinned) and why.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPin {
    pub runner: Option<runner::Model>,
    pub pin_kind: &'static str,
}

impl ResolvedPin {
    fn unpinned_locality() -> Self {
        ResolvedPin {
            runner: None,
            pin_kind: PIN_LOCALITY,
        }
    }
}

/// Locality pin: the parent's runner while it takes pod work now.
///
/// Keeps every rule `_pinned_runner_for` had on main: online, heartbeat-
/// fresh, not paused, same pod, `locality_pin_allowed` (an engine only
/// while `engine_takes_pod_work`), and for an engine `runner_may_take`
/// for the new run's creator and trigger.
pub async fn locality_runner_for(
    db: &impl ConnectionTrait,
    parent: Option<&agent_run::Model>,
    target_pod_id: Option<Uuid>,
    creator_id: Option<Uuid>,
    trigger: AgentRunTrigger,
    issue: Option<&issues::Model>,
) -> Result<Option<runner::Model>, sea_orm::DbErr> {
    let Some(parent) = parent else {
        return Ok(None);
    };
    let Some(parent_runner_id) = parent.runner_id else {
        return Ok(None);
    };
    let Some(candidate) = runner::Entity::find_by_id(parent_runner_id).one(db).await? else {
        return Ok(None);
    };
    if candidate.status == RUNNER_STATUS_REVOKED || candidate.status == RUNNER_STATUS_OFFLINE {
        return Ok(None);
    }
    if candidate.paused_at.is_some() {
        return Ok(None);
    }
    if !is_alive(&candidate, Utc::now()) {
        return Ok(None);
    }
    if let Some(pod_id) = target_pod_id {
        if candidate.pod_id != pod_id {
            return Ok(None);
        }
    }
    let is_engine = candidate.provisioning == PROVISIONING_DESKTOP_BUNDLED;
    let machine = match candidate.dev_machine_id {
        Some(machine_id) => dev_machine::Entity::find_by_id(machine_id).one(db).await?,
        None => None,
    };
    let pod_member = candidate.pod_dispatch == POD_DISPATCH_POD_MEMBER;
    let engine_paused = machine
        .as_ref()
        .map(|m| m.engine_paused_by_owner || !m.engine_auto_pause_reason.is_empty())
        .unwrap_or(false);
    if !auth_locality_pin_allowed(true, is_engine, pod_member, engine_paused) {
        return Ok(None);
    }
    if is_engine {
        let human = HUMAN_TRIGGERS.contains(&trigger);
        if !runner_may_take_run(db, &candidate, creator_id, human, issue).await? {
            return Ok(None);
        }
    }
    Ok(Some(candidate))
}

/// Heartbeat-fresh check (the shared `alive_threshold` rule).
fn is_alive(runner_row: &runner::Model, now: DateTime<Utc>) -> bool {
    runner_row
        .last_heartbeat_at
        .map(|last| last >= alive_threshold(now))
        .unwrap_or(false)
}

/// `resolve_run_pin` inputs (design §9.1). `actor_id` is the comment
/// author for the stale clear (Django's `get_agent_system_user`
/// equivalent at the caller's seam).
#[derive(Clone, Copy, Debug)]
pub struct ResolvePinArgs<'a> {
    pub issue: &'a issues::Model,
    pub parent: Option<&'a agent_run::Model>,
    pub pod_id: Uuid,
    pub fresh_session: bool,
    pub creator_id: Option<Uuid>,
    pub trigger: AgentRunTrigger,
    pub pod_once: bool,
    pub actor_id: Uuid,
}

/// Return the `(runner, pin_kind)` for a run created on `issue` (design §9.1).
///
/// Dedicated-first: `issue.assigned_runner` wins unless `pod_once` (§23.3)
/// deliberately bypasses it. A revoked or pod-mismatched dedicated runner
/// is self-healed (cleared with `stale_at_dispatch`) and dispatch falls
/// through to locality. Offline/paused/busy dedicated runners stay pinned —
/// a person's run is refused before it reaches here (§11), so only
/// automatic runs arrive pinned to a runner that is not taking work.
#[allow(clippy::too_many_arguments)]
pub async fn resolve_run_pin(
    db: &impl ConnectionTrait,
    args: &ResolvePinArgs<'_>,
) -> Result<ResolvedPin, sea_orm::DbErr> {
    let issue = args.issue;
    let parent = args.parent;
    let pod_id = args.pod_id;
    let fresh_session = args.fresh_session;
    let creator_id = args.creator_id;
    let trigger = args.trigger;
    let pod_once = args.pod_once;
    let actor_id = args.actor_id;
    if let Some(assigned_id) = issue.assigned_runner_id {
        if !pod_once {
            let assigned = runner::Entity::find_by_id(assigned_id).one(db).await?;
            if let Some(assigned) = assigned {
                if assigned.status == RUNNER_STATUS_REVOKED || assigned.pod_id != pod_id {
                    // Stale: clear with an activity/comment record. The
                    // caller owns the transaction; the clear does its own
                    // entity writes on this connection (no commit here).
                    clear_dedicated_runner(db, issue.id, "stale_at_dispatch", actor_id).await?;
                } else {
                    return Ok(ResolvedPin {
                        runner: Some(assigned),
                        pin_kind: PIN_DEDICATED,
                    });
                }
            }
        }
    }
    if fresh_session || parent.is_none() || pod_once {
        return Ok(ResolvedPin::unpinned_locality());
    }
    let locality =
        locality_runner_for(db, parent, Some(pod_id), creator_id, trigger, Some(issue)).await?;
    Ok(ResolvedPin {
        runner: locality,
        pin_kind: PIN_LOCALITY,
    })
}

// ---------------------------------------------------------------------------
// dispatch_wait (design §14.3)
// ---------------------------------------------------------------------------

/// The flat inputs `build_dispatch_wait` / `runs_ahead` need (design
/// §14.3), so both the SeaORM row and the L2 run row feed the same
/// builder without a variant per caller.
#[derive(Clone, Debug)]
pub struct RunWaitInputs {
    pub id: Uuid,
    pub status: String,
    pub pinned_runner_id: Option<Uuid>,
    pub pin_kind: String,
    pub work_item_id: Option<Uuid>,
    pub pod_id: Uuid,
    pub created_by_id: Uuid,
    pub trigger: String,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
}

impl From<&agent_run::Model> for RunWaitInputs {
    fn from(run: &agent_run::Model) -> Self {
        RunWaitInputs {
            id: run.id,
            status: run.status.clone(),
            pinned_runner_id: run.pinned_runner_id,
            pin_kind: run.pin_kind.clone(),
            work_item_id: run.work_item_id,
            pod_id: run.pod_id,
            created_by_id: run.created_by_id,
            trigger: run.trigger.clone(),
            created_at: run.created_at,
        }
    }
}

/// The `dispatch_wait` object for a QUEUED pinned run (design §14.3), else
/// `Null`. Dedicated pins are always `releasable: false` (H-13);
/// `pause_reason` is set only with `runner_paused`; `expires_at` is
/// `queued_at + DEDICATED_RUN_QUEUED_MAX_AGE_SECS` for dedicated pins,
/// else `null`.
pub async fn build_dispatch_wait(
    db: &impl ConnectionTrait,
    run: &RunWaitInputs,
    viewer_id: Option<Uuid>,
) -> Result<Value, sea_orm::DbErr> {
    if run.status != AgentRunStatus::Queued.value() || run.pinned_runner_id.is_none() {
        return Ok(Value::Null);
    }
    let pinned = run.pinned_runner_id;
    let pinned_runner = match pinned {
        Some(id) => runner::Entity::find_by_id(id).one(db).await?,
        None => None,
    };
    let (runner_name, reason, pause_reason, ahead_count, runner_last_seen) = match pinned_runner {
        None => (None, "dispatching".to_owned(), None, 0_u64, None),
        Some(pinned_runner) => {
            let machine = match pinned_runner.dev_machine_id {
                Some(machine_id) => dev_machine::Entity::find_by_id(machine_id).one(db).await?,
                None => None,
            };
            let name = pinned_runner.name.clone();
            let pause = runner_paused_reason(&pinned_runner, machine.as_ref())
                .map(|reason| reason.into_owned());
            let ahead = runs_ahead(db, run).await?;
            let last_seen = pinned_runner.last_heartbeat_at;
            let reason = if pause.is_some() {
                "runner_paused".to_owned()
            } else if runner_is_offline(&pinned_runner, Utc::now()) {
                "runner_offline".to_owned()
            } else if is_busy(db, pinned_runner.id).await? || ahead > 0 {
                "runner_busy".to_owned()
            } else {
                // Online, not paused, not busy: dispatching unless the
                // usage rule excludes this run (stranded, §9.4/§12.1).
                let issue = match run.work_item_id {
                    Some(issue_id) => issues::Entity::find_by_id(issue_id).one(db).await?,
                    None => None,
                };
                let human = AgentRunTrigger::from_value(run.trigger.as_str())
                    .map(|trigger| HUMAN_TRIGGERS.contains(&trigger))
                    .unwrap_or(true);
                let excluded = match issue {
                    Some(issue) => {
                        !runner_may_take_run(
                            db,
                            &pinned_runner,
                            Some(run.created_by_id),
                            human,
                            Some(&issue),
                        )
                        .await?
                    }
                    None => false,
                };
                if excluded {
                    "runner_not_permitted".to_owned()
                } else {
                    "dispatching".to_owned()
                }
            };
            (Some(name), reason, pause, ahead, last_seen)
        }
    };
    let dedicated = run.pin_kind == PIN_DEDICATED;
    let releasable = if dedicated {
        false
    } else {
        // Locality pin: follows the release endpoint's permission check
        // (issue creator / assignees or runner owner). The dedicated
        // refusal (H-13) is handled above; this is the pre-H-13 rule.
        match (viewer_id, run.work_item_id) {
            (Some(viewer), Some(issue_id)) => can_view_run(db, viewer, run, issue_id).await?,
            _ => false,
        }
    };
    let expires_at = if dedicated {
        run.created_at + chrono::Duration::seconds(dedicated_max_age_secs())
    } else {
        return Ok(json!({
            "reason": reason,
            "pin_kind": run.pin_kind,
            "runner": pinned.map(|id| id.to_string()),
            "runner_name": runner_name,
            "queued_at": iso(Some(run.created_at)),
            "runner_last_seen_at": iso(runner_last_seen),
            "runs_ahead": ahead_count,
            "pause_reason": pause_reason.filter(|_| reason == "runner_paused"),
            "expires_at": Value::Null,
            "releasable": releasable,
        }));
    };
    Ok(json!({
        "reason": reason,
        "pin_kind": run.pin_kind,
        "runner": pinned.map(|id| id.to_string()),
        "runner_name": runner_name,
        "queued_at": iso(Some(run.created_at)),
        "runner_last_seen_at": iso(runner_last_seen),
        "runs_ahead": ahead_count,
        "pause_reason": pause_reason.filter(|_| reason == "runner_paused"),
        "expires_at": iso(Some(expires_at)),
        "releasable": releasable,
    }))
}

/// Release permission for a locality pin (issue creator / assignees or
/// runner owner), mirroring `_can_cancel_run`'s locality arm.
async fn can_view_run(
    db: &impl ConnectionTrait,
    viewer_id: Uuid,
    run: &RunWaitInputs,
    issue_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    if let Some(issue) = issues::Entity::find_by_id(issue_id).one(db).await? {
        if issue.created_by_id == Some(viewer_id) {
            return Ok(true);
        }
        let people = issue_people(db, issue_id).await?;
        if people.contains(&viewer_id) {
            return Ok(true);
        }
    }
    if let Some(pinned_id) = run.pinned_runner_id {
        if let Some(pinned) = runner::Entity::find_by_id(pinned_id).one(db).await? {
            if pinned.owner_id == viewer_id {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Older QUEUED runs pinned to the same runner (read-side `runs_ahead`).
///
/// Under the §10.2 alternation a dedicated run may also wait behind one
/// shared run (rank 1 beats rank 2 when the last run was dedicated) and
/// behind newer locality pins (rank 0 always); those count too.
pub async fn runs_ahead(
    db: &impl ConnectionTrait,
    run: &RunWaitInputs,
) -> Result<u64, sea_orm::DbErr> {
    let Some(pinned_id) = run.pinned_runner_id else {
        return Ok(0);
    };
    let mut ahead = agent_run::Entity::find()
        .filter(agent_run::Column::PinnedRunnerId.eq(pinned_id))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .filter(agent_run::Column::CreatedAt.lt(run.created_at))
        .filter(agent_run::Column::Id.ne(run.id))
        .count(db)
        .await?;
    if run.pin_kind == PIN_DEDICATED && last_served_dedicated(db, pinned_id).await? {
        // Locality pins outrank a dedicated run whatever their age.
        ahead += agent_run::Entity::find()
            .filter(agent_run::Column::PinnedRunnerId.eq(pinned_id))
            .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
            .filter(agent_run::Column::PinKind.eq(PIN_LOCALITY))
            .filter(agent_run::Column::CreatedAt.gte(run.created_at))
            .filter(agent_run::Column::Id.ne(run.id))
            .count(db)
            .await?;
        // One shared run is served before the next dedicated (§10.2).
        let shared = agent_run::Entity::find()
            .filter(agent_run::Column::PodId.eq(run.pod_id))
            .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
            .filter(agent_run::Column::PinnedRunnerId.is_null())
            .one(db)
            .await?;
        if shared.is_some() {
            ahead += 1;
        }
    }
    Ok(ahead)
}

/// True when the runner's most recent assigned run was dedicated (design
/// §10.2). Derived, not stored: the latest run by `assigned_at` (NULLs
/// excluded) with a `pinned_runner` and `pin_kind = dedicated`.
pub async fn last_served_dedicated(
    db: &impl ConnectionTrait,
    runner_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let last = agent_run::Entity::find()
        .filter(agent_run::Column::RunnerId.eq(runner_id))
        .filter(agent_run::Column::AssignedAt.is_not_null())
        .order_by_desc(agent_run::Column::AssignedAt)
        .one(db)
        .await?;
    Ok(last
        .map(|run| run.pinned_runner_id.is_some() && run.pin_kind == PIN_DEDICATED)
        .unwrap_or(false))
}

// ---------------------------------------------------------------------------
// Run-time refusal (design §11, §23.3)
// ---------------------------------------------------------------------------

/// Pre-creation refusal for a person's run (§11, §23.3).
///
/// Returns the refusal code — `assigned_runner_not_owner`,
/// `assigned_runner_offline` or `assigned_runner_paused` — or `None` when
/// the run may proceed. Only a person's run is refused; an automatic run
/// is created and waits (§11.3). Busy is never a refusal. Skipped
/// entirely on a `pod_once` request (§23.3).
///
/// Order (§11.2): the H-10 usage-rule check first, so a teammate excluded
/// by an engine's usage rule gets `assigned_runner_not_owner` whatever
/// the engine's state; the availability check second.
pub async fn dedicated_run_refusal(
    db: &impl ConnectionTrait,
    issue: &issues::Model,
    human: bool,
    user_id: Option<Uuid>,
    pod_once: bool,
) -> Result<Option<&'static str>, sea_orm::DbErr> {
    if !human || pod_once {
        return Ok(None);
    }
    let Some(assigned_id) = issue.assigned_runner_id else {
        return Ok(None);
    };
    let Some(assigned) = runner::Entity::find_by_id(assigned_id).one(db).await? else {
        return Ok(None);
    };
    if assigned.revoked_at.is_some() {
        return Ok(None); // E4 already failed at assignment
    }
    if !runner_may_take_run(db, &assigned, user_id, true, Some(issue)).await? {
        return Ok(Some("assigned_runner_not_owner"));
    }
    let machine = match assigned.dev_machine_id {
        Some(machine_id) => dev_machine::Entity::find_by_id(machine_id).one(db).await?,
        None => None,
    };
    if runner_paused_reason(&assigned, machine.as_ref()).is_some() {
        return Ok(Some("assigned_runner_paused"));
    }
    if runner_is_offline(&assigned, Utc::now()) {
        return Ok(Some("assigned_runner_offline"));
    }
    Ok(None)
}

/// Plain-words pause phrase for refusal `error` text (§11.2).
pub fn refusal_pause_phrase(pause_reason: &str) -> &'static str {
    match pause_reason {
        "dispatch_paused" => "is paused by its owner",
        "owner_paused" => "is paused by its owner",
        "wallet_empty" => "is paused: the wallet is empty",
        "engine_sign_in_required" => "is paused: the engine's sign-in expired",
        "gateway_session_revoked" => "is paused: the gateway session was revoked",
        "profile_unavailable" => "is paused: the engine profile is unavailable",
        "engine_disabled" => "is paused: the engine is disabled",
        _ => "is paused",
    }
}

/// The full §11.2 refusal body for `code` on `issue`.
///
/// Shape: `{error, reason, pause_reason, runner, runner_name,
/// runner_last_seen_at, pod, pod_name, options}`.
pub async fn dedicated_refusal_body(
    db: &impl ConnectionTrait,
    issue: &issues::Model,
    code: &'static str,
) -> Result<Value, sea_orm::DbErr> {
    let assigned = match issue.assigned_runner_id {
        Some(id) => runner::Entity::find_by_id(id).one(db).await?,
        None => None,
    };
    let pod_row = match issue.assigned_pod_id {
        Some(id) => pod::Entity::find_by_id(id).one(db).await?,
        None => None,
    };
    let pause_reason: Option<String> = if code == "assigned_runner_paused" {
        match assigned.as_ref() {
            Some(assigned) => {
                let machine = match assigned.dev_machine_id {
                    Some(machine_id) => dev_machine::Entity::find_by_id(machine_id).one(db).await?,
                    None => None,
                };
                Some(
                    runner_paused_reason(assigned, machine.as_ref())
                        .map(|r| r.into_owned())
                        .unwrap_or_else(|| "dispatch_paused".to_owned()),
                )
            }
            None => None,
        }
    } else {
        None
    };
    let options: Vec<&str> = if code == "assigned_runner_not_owner" {
        vec!["run_on_my_runner", "pod_once"]
    } else {
        vec!["pod_once", "change_runner"]
    };
    let error = match assigned.as_ref() {
        None => "the dedicated runner is unavailable".to_owned(),
        Some(assigned) => match code {
            "assigned_runner_not_owner" => {
                format!(
                    "{} cannot take this run (not the runner's owner)",
                    assigned.name
                )
            }
            "assigned_runner_paused" => {
                let phrase =
                    refusal_pause_phrase(pause_reason.as_deref().unwrap_or("dispatch_paused"));
                format!("{} {}", assigned.name, phrase)
            }
            "assigned_runner_offline" => {
                let seen = assigned
                    .last_heartbeat_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "unknown".to_owned());
                format!("{} is offline (last seen {})", assigned.name, seen)
            }
            _ => "the dedicated runner is unavailable".to_owned(),
        },
    };
    Ok(json!({
        "error": error,
        "reason": code,
        "pause_reason": pause_reason,
        "runner": assigned.as_ref().map(|r| r.id.to_string()),
        "runner_name": assigned.as_ref().map(|r| r.name.clone()),
        "runner_last_seen_at": assigned.as_ref().and_then(|r| iso(r.last_heartbeat_at)),
        "pod": pod_row.as_ref().map(|p| p.id.to_string()),
        "pod_name": pod_row.as_ref().map(|p| p.name.clone()),
        "options": options,
    }))
}

/// The `pod_once` authorization refusal for `user` on `issue` (§23.3).
pub async fn pod_once_auth_code(
    db: &impl ConnectionTrait,
    issue: &issues::Model,
    user_id: Option<Uuid>,
) -> Result<Option<&'static str>, sea_orm::DbErr> {
    if issue.assigned_runner_id.is_none() {
        return Ok(Some("pod_once_not_applicable"));
    }
    let Some(user_id) = user_id else {
        return Ok(Some("pod_once_not_allowed"));
    };
    if issue.created_by_id == Some(user_id) {
        return Ok(None);
    }
    let people = issue_people(db, issue.id).await?;
    if people.contains(&user_id) {
        return Ok(None);
    }
    Ok(Some("pod_once_not_allowed"))
}

// ---------------------------------------------------------------------------
// Lifecycle writes (design §13)
// ---------------------------------------------------------------------------

/// Clear `Issue.assigned_runner` and record why (design §13).
///
/// Always clears; every clear writes an activity entry naming the runner
/// and one system comment naming the runner and the reason (§13.1/§14.7),
/// attributed to `actor_id`. `assigned_pod` is left alone.
pub async fn clear_dedicated_runner(
    db: &impl ConnectionTrait,
    issue_id: Uuid,
    reason: &str,
    actor_id: Uuid,
) -> Result<(), sea_orm::DbErr> {
    let Some(issue) = issues::Entity::find_by_id(issue_id).one(db).await? else {
        return Ok(());
    };
    let assigned = match issue.assigned_runner_id {
        Some(id) => runner::Entity::find_by_id(id).one(db).await?,
        None => None,
    };
    let runner_name = assigned
        .as_ref()
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "the dedicated runner".to_owned());
    let runner_id = assigned.as_ref().map(|r| r.id);
    let now = Utc::now();
    let model = issues::ActiveModel {
        id: sea_orm::Unchanged(issue.id),
        assigned_runner_id: sea_orm::Set(None),
        updated_at: sea_orm::Set(now.into()),
        ..Default::default()
    };
    model.update(db).await?;
    let body = format!(
        "<p>The dedicated runner assignment to <strong>{runner_name}</strong> was cleared: <strong>{reason}</strong>.</p>"
    );
    let comment = issue_comments::ActiveModel {
        id: sea_orm::Set(Uuid::new_v4()),
        comment_stripped: sea_orm::Set(body.clone()),
        attachments: sea_orm::Set(Vec::new()),
        created_by_id: sea_orm::Set(Some(actor_id)),
        issue_id: sea_orm::Set(issue.id),
        project_id: sea_orm::Set(issue.project_id),
        updated_by_id: sea_orm::Set(Some(actor_id)),
        workspace_id: sea_orm::Set(issue.workspace_id),
        actor_id: sea_orm::Set(Some(actor_id)),
        comment_html: sea_orm::Set(body),
        comment_json: sea_orm::Set(json!({})),
        access: sea_orm::Set("full".to_owned()),
        external_id: sea_orm::Set(None),
        external_source: sea_orm::Set(None),
        deleted_at: sea_orm::Set(None),
        edited_at: sea_orm::Set(None),
        description_id: sea_orm::Set(None),
        parent_id: sea_orm::Set(None),
        speaker_type: sea_orm::Set("system".to_owned()),
        speaker_label: sea_orm::Set(String::new()),
        speaker_agent_run_id: sea_orm::Set(None),
        labels: sea_orm::Set(Vec::new()),
        created_at: sea_orm::Set(now.into()),
        updated_at: sea_orm::Set(now.into()),
    };
    comment.insert(db).await?;
    let activity = issue_activities::ActiveModel {
        id: sea_orm::Set(Uuid::new_v4()),
        verb: sea_orm::Set("updated".to_owned()),
        field: sea_orm::Set(Some("assigned_runner".to_owned())),
        old_value: sea_orm::Set(Some(runner_name.clone())),
        new_value: sea_orm::Set(None),
        comment: sea_orm::Set(format!(
            "cleared the dedicated runner {runner_name}: {reason}"
        )),
        attachments: sea_orm::Set(Vec::new()),
        created_by_id: sea_orm::Set(Some(actor_id)),
        issue_id: sea_orm::Set(Some(issue.id)),
        issue_comment_id: sea_orm::Set(None),
        project_id: sea_orm::Set(issue.project_id),
        updated_by_id: sea_orm::Set(Some(actor_id)),
        workspace_id: sea_orm::Set(issue.workspace_id),
        actor_id: sea_orm::Set(Some(actor_id)),
        new_identifier: sea_orm::Set(None),
        old_identifier: sea_orm::Set(runner_id),
        epoch: sea_orm::Set(None),
        deleted_at: sea_orm::Set(None),
        created_at: sea_orm::Set(now.into()),
        updated_at: sea_orm::Set(now.into()),
    };
    activity.insert(db).await?;
    Ok(())
}

/// Persist the chosen execution target (design §13.2).
///
/// Saves `assigned_runner` (and `assigned_pod` from the runner when
/// setting), re-points the issue's QUEUED run (if any), and writes an
/// activity row. Call only after `validate_execution_target` has passed.
///
/// One transaction: the issue row is locked first, then the run row (the
/// issue → run order `complete_project_move_handoff` uses). The caller
/// owns the transaction connection; this function runs its writes on `db`
/// (a `DatabaseConnection` transaction from the caller).
pub async fn apply_execution_target(
    db: &sea_orm::DatabaseConnection,
    issue_id: Uuid,
    new_runner_id: Option<Uuid>,
    actor_id: Uuid,
) -> Result<ApplyOutcome, sea_orm::DbErr> {
    let outcome = db
        .transaction(|txn| {
            Box::pin(async move {
                // Lock the issue row first (issue -> run order, §13.2).
                let issue = issues::Entity::find_by_id(issue_id)
                    .lock(LockType::Update)
                    .one(txn)
                    .await?
                    .ok_or_else(|| sea_orm::DbErr::RecordNotFound("issue".to_owned()))?;
                let prev_runner_id = issue.assigned_runner_id;
                let now = Utc::now();
                let mut model = issues::ActiveModel {
                    id: sea_orm::Unchanged(issue.id),
                    updated_at: sea_orm::Set(now.into()),
                    ..Default::default()
                };
                match new_runner_id {
                    Some(new_runner_id) => {
                        model.assigned_runner_id = sea_orm::Set(Some(new_runner_id));
                        let new_runner = runner::Entity::find_by_id(new_runner_id).one(txn).await?;
                        if let Some(new_runner) = new_runner {
                            model.assigned_pod_id = sea_orm::Set(Some(new_runner.pod_id));
                        }
                    }
                    None => {
                        model.assigned_runner_id = sea_orm::Set(None);
                    }
                }
                model.update(txn).await?;
                let queued = agent_run::Entity::find()
                    .filter(agent_run::Column::WorkItemId.eq(issue.id))
                    .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
                    .order_by_asc(agent_run::Column::CreatedAt)
                    .one(txn)
                    .await?;
                let mut drain_pod: Option<Uuid> = None;
                if let Some(queued) = queued {
                    repoint_queued_run(txn, &queued, new_runner_id, actor_id).await?;
                    drain_pod = Some(queued.pod_id);
                }
                let old_name = match prev_runner_id {
                    Some(id) => runner::Entity::find_by_id(id)
                        .one(txn)
                        .await?
                        .map(|r| r.name)
                        .unwrap_or_default(),
                    None => String::new(),
                };
                let new_name = match new_runner_id {
                    Some(id) => runner::Entity::find_by_id(id)
                        .one(txn)
                        .await?
                        .map(|r| r.name)
                        .unwrap_or_default(),
                    None => String::new(),
                };
                let activity = issue_activities::ActiveModel {
                    id: sea_orm::Set(Uuid::new_v4()),
                    verb: sea_orm::Set("updated".to_owned()),
                    field: sea_orm::Set(Some("assigned_runner".to_owned())),
                    old_value: sea_orm::Set(Some(old_name)),
                    new_value: sea_orm::Set(Some(new_name)),
                    comment: sea_orm::Set("updated the dedicated runner to".to_owned()),
                    attachments: sea_orm::Set(Vec::new()),
                    created_by_id: sea_orm::Set(Some(actor_id)),
                    issue_id: sea_orm::Set(Some(issue.id)),
                    issue_comment_id: sea_orm::Set(None),
                    project_id: sea_orm::Set(issue.project_id),
                    updated_by_id: sea_orm::Set(Some(actor_id)),
                    workspace_id: sea_orm::Set(issue.workspace_id),
                    actor_id: sea_orm::Set(Some(actor_id)),
                    new_identifier: sea_orm::Set(new_runner_id),
                    old_identifier: sea_orm::Set(prev_runner_id),
                    epoch: sea_orm::Set(None),
                    deleted_at: sea_orm::Set(None),
                    created_at: sea_orm::Set(now.into()),
                    updated_at: sea_orm::Set(now.into()),
                };
                activity.insert(txn).await?;
                Ok(ApplyOutcome { drain_pod })
            })
        })
        .await
        .map_err(unmap)?;
    Ok(outcome)
}

/// What `apply_execution_target` did: the pod whose queue to drain after
/// commit (Django's `transaction.on_commit(_drain)`), if any.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApplyOutcome {
    pub drain_pod: Option<Uuid>,
}

/// Re-point one QUEUED run at its new dedicated pin (§13.2).
async fn repoint_queued_run(
    db: &impl ConnectionTrait,
    run: &agent_run::Model,
    new_runner_id: Option<Uuid>,
    actor_id: Uuid,
) -> Result<(), sea_orm::DbErr> {
    let old = match run.pinned_runner_id {
        Some(id) => runner::Entity::find_by_id(id).one(db).await?,
        None => None,
    };
    let old_name = old
        .map(|r| r.name)
        .unwrap_or_else(|| "the pod queue".to_owned());
    let new_kind = match new_runner_id {
        Some(_) => PIN_DEDICATED,
        None => PIN_LOCALITY,
    };
    let model = agent_run::ActiveModel {
        id: sea_orm::Unchanged(run.id),
        pinned_runner_id: sea_orm::Set(new_runner_id),
        pin_kind: sea_orm::Set(new_kind.to_owned()),
        ..Default::default()
    };
    model.update(db).await?;
    // §13.2: a re-pointed run starts a fresh session on its new target —
    // the parent's thread id is stale.
    if let Some(parent_id) = run.parent_run_id {
        if let Some(parent) = agent_run::Entity::find_by_id(parent_id).one(db).await? {
            if !parent.thread_id.is_empty() {
                let cleared = agent_run::ActiveModel {
                    id: sea_orm::Unchanged(parent.id),
                    thread_id: sea_orm::Set(String::new()),
                    ..Default::default()
                };
                cleared.update(db).await?;
            }
        }
    }
    if let Some(issue_id) = run.work_item_id {
        if let Some(issue) = issues::Entity::find_by_id(issue_id).one(db).await? {
            let new_name = match new_runner_id {
                Some(id) => runner::Entity::find_by_id(id)
                    .one(db)
                    .await?
                    .map(|r| r.name)
                    .unwrap_or_else(|| "the pod queue".to_owned()),
                None => "the pod queue".to_owned(),
            };
            let body = format!(
                "<p>Your run was re-pointed from <strong>{old_name}</strong> to <strong>{new_name}</strong>.</p>"
            );
            let now = Utc::now();
            let comment = issue_comments::ActiveModel {
                id: sea_orm::Set(Uuid::new_v4()),
                comment_stripped: sea_orm::Set(body.clone()),
                attachments: sea_orm::Set(Vec::new()),
                created_by_id: sea_orm::Set(Some(actor_id)),
                issue_id: sea_orm::Set(issue.id),
                project_id: sea_orm::Set(issue.project_id),
                updated_by_id: sea_orm::Set(Some(actor_id)),
                workspace_id: sea_orm::Set(issue.workspace_id),
                actor_id: sea_orm::Set(Some(actor_id)),
                comment_html: sea_orm::Set(body),
                comment_json: sea_orm::Set(json!({})),
                access: sea_orm::Set("full".to_owned()),
                external_id: sea_orm::Set(None),
                external_source: sea_orm::Set(None),
                deleted_at: sea_orm::Set(None),
                edited_at: sea_orm::Set(None),
                description_id: sea_orm::Set(None),
                parent_id: sea_orm::Set(None),
                speaker_type: sea_orm::Set("system".to_owned()),
                speaker_label: sea_orm::Set(String::new()),
                speaker_agent_run_id: sea_orm::Set(None),
                labels: sea_orm::Set(Vec::new()),
                created_at: sea_orm::Set(now.into()),
                updated_at: sea_orm::Set(now.into()),
            };
            comment.insert(db).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Lost-runner policy, 12h bound, stranded sweep (design §11.3, §12.1, §13.1)
// ---------------------------------------------------------------------------

/// Run `error` text for a queued dedicated run lost with its runner.
pub fn lost_runner_revoke_error(runner_name: &str) -> String {
    format!(
        "Runner {runner_name} was revoked or deleted, so this run cannot start. \
        The issue is back in the pod's routing; choose a new runner or keep the pod."
    )
}

/// The §11.3 failure text for a dedicated 12h bound expiry.
pub fn dedicated_unavailable_error(runner_name: &str, detail: &str, pod_name: &str) -> String {
    format!(
        "Runner {runner_name} did not take this run within 12 hours: {detail}. \
        Automatic runs on this issue are stopped. \
        The issue stays assigned to {runner_name}. \
        When the runner is back, use Run or Re-tick. \
        To run it elsewhere, use Run on any runner in {pod_name} once, \
        or change the issue's runner."
    )
}

/// Fail one QUEUED run with `error_code`/`error` (+ optional done payload).
/// Returns `true` when the row was still QUEUED under the lock.
async fn fail_queued_run(
    db: &impl ConnectionTrait,
    run_id: Uuid,
    error_code: &str,
    error: &str,
    done_payload: Option<serde_json::Value>,
) -> Result<bool, sea_orm::DbErr> {
    let run = agent_run::Entity::find_by_id(run_id)
        .lock(LockType::Update)
        .one(db)
        .await?;
    let Some(run) = run else { return Ok(false) };
    if run.status != AgentRunStatus::Queued.value() {
        return Ok(false);
    }
    let now = Utc::now();
    let mut details = run.error_details.clone();
    if !details.is_object() {
        details = json!({});
    }
    details["code"] = json!(error_code);
    details["message"] = json!(error);
    let mut model = agent_run::ActiveModel {
        id: sea_orm::Unchanged(run.id),
        status: sea_orm::Set(AgentRunStatus::Failed.value().to_owned()),
        error_details: sea_orm::Set(details),
        ended_at: sea_orm::Set(Some(now.into())),
        ..Default::default()
    };
    if let Some(payload) = done_payload {
        model.done_payload = sea_orm::Set(Some(payload));
    }
    model.update(db).await?;
    Ok(true)
}

/// Fail dedicated runs whose runner is still unavailable after 12h (§11.3).
///
/// Every dedicated QUEUED run older than
/// `DEDICATED_RUN_QUEUED_MAX_AGE_SECS` whose pinned runner is unavailable
/// now (§11.1) fails with `dedicated_runner_unavailable` and
/// `done_payload.stop_ticking` so the clock stops (§11.3). The
/// unavailability is re-checked under the row lock so a runner that came
/// back in the meantime keeps its run. A run behind a busy online runner
/// is not failed: it is moving.
pub async fn fail_dedicated_runs(db: &impl ConnectionTrait) -> Result<u64, sea_orm::DbErr> {
    let cutoff = Utc::now() - chrono::Duration::seconds(dedicated_max_age_secs());
    let stale = agent_run::Entity::find()
        .filter(agent_run::Column::PinKind.eq(PIN_DEDICATED))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .filter(agent_run::Column::CreatedAt.lt(cutoff))
        .filter(agent_run::Column::PinnedRunnerId.is_not_null())
        .order_by_asc(agent_run::Column::CreatedAt)
        .limit(500)
        .all(db)
        .await?;
    let mut failed = 0_u64;
    for candidate in stale {
        let run = agent_run::Entity::find_by_id(candidate.id)
            .lock(LockType::Update)
            .one(db)
            .await?;
        let Some(run) = run else { continue };
        if run.status != AgentRunStatus::Queued.value() || run.pin_kind != PIN_DEDICATED {
            continue;
        }
        let Some(pinned_id) = run.pinned_runner_id else {
            continue;
        };
        let Some(pinned) = runner::Entity::find_by_id(pinned_id).one(db).await? else {
            continue;
        };
        // Locked re-check: a runner that came back keeps its run.
        let machine = match pinned.dev_machine_id {
            Some(machine_id) => dev_machine::Entity::find_by_id(machine_id).one(db).await?,
            None => None,
        };
        if runner_paused_reason(&pinned, machine.as_ref()).is_none()
            && !runner_is_offline(&pinned, Utc::now())
        {
            continue;
        }
        let pause = runner_paused_reason(&pinned, machine.as_ref());
        let detail = match pause {
            Some(reason) => format!("it {}", refusal_pause_phrase(&reason)),
            None => {
                let seen = pinned
                    .last_heartbeat_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "unknown".to_owned());
                format!("it is offline (last seen {seen})")
            }
        };
        let pod_name = match pod::Entity::find_by_id(run.pod_id).one(db).await? {
            Some(pod_row) => pod_row.name,
            None => "the pod".to_owned(),
        };
        let error = dedicated_unavailable_error(&pinned.name, &detail, &pod_name);
        if fail_queued_run(
            db,
            run.id,
            "dedicated_runner_unavailable",
            &error,
            Some(json!({"stop_ticking": true})),
        )
        .await?
        {
            failed += 1;
        }
    }
    Ok(failed)
}

/// True when a dedicated run's runner excludes it under the usage rule (§12.1).
pub async fn stranded_queued(
    db: &impl ConnectionTrait,
    run: &agent_run::Model,
    issue: Option<&issues::Model>,
) -> Result<bool, sea_orm::DbErr> {
    let Some(pinned_id) = run.pinned_runner_id else {
        return Ok(false);
    };
    let Some(pinned) = runner::Entity::find_by_id(pinned_id).one(db).await? else {
        return Ok(false);
    };
    let human = AgentRunTrigger::from_value(run.trigger.as_str())
        .map(|trigger| HUMAN_TRIGGERS.contains(&trigger))
        .unwrap_or(true);
    Ok(!runner_may_take_run(db, &pinned, Some(run.created_by_id), human, issue).await?)
}

/// Stranded sweep for dedicated pins (§12.1, §9.4).
///
/// When the pinned runner is online, heartbeat-fresh, not paused and
/// idle, the run is older than one sweep interval, and
/// `filter_runs_usable_by_runner` excludes it (the owner was un-assigned,
/// the engine opt-in turned off, …), fail the run with
/// `dedicated_runner_not_permitted`. The runner is there and will never
/// take the run, so waiting cannot help. Runs whose runner is unavailable
/// (offline/paused) are the 12h bound's (§11.3), not this sweep's, and
/// runs behind a busy runner are moving.
pub async fn sweep_stranded_dedicated_runs(
    db: &impl ConnectionTrait,
    sweep_interval_secs: i64,
) -> Result<u64, sea_orm::DbErr> {
    let cutoff = Utc::now() - chrono::Duration::seconds(sweep_interval_secs.max(1));
    let candidates = agent_run::Entity::find()
        .filter(agent_run::Column::PinKind.eq(PIN_DEDICATED))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
        .filter(agent_run::Column::CreatedAt.lt(cutoff))
        .filter(agent_run::Column::PinnedRunnerId.is_not_null())
        .order_by_asc(agent_run::Column::CreatedAt)
        .limit(500)
        .all(db)
        .await?;
    let mut handled = 0_u64;
    for candidate in candidates {
        let run = agent_run::Entity::find_by_id(candidate.id)
            .lock(LockType::Update)
            .one(db)
            .await?;
        let Some(run) = run else { continue };
        if run.status != AgentRunStatus::Queued.value() || run.pin_kind != PIN_DEDICATED {
            continue;
        }
        let Some(pinned_id) = run.pinned_runner_id else {
            continue;
        };
        let Some(pinned) = runner::Entity::find_by_id(pinned_id).one(db).await? else {
            continue;
        };
        // Gating (§12.1): online, heartbeat-fresh, not paused, idle.
        let machine = match pinned.dev_machine_id {
            Some(machine_id) => dev_machine::Entity::find_by_id(machine_id).one(db).await?,
            None => None,
        };
        if runner_paused_reason(&pinned, machine.as_ref()).is_some() {
            continue; // unavailable: the 12h bound owns it (§11.3)
        }
        if runner_is_offline(&pinned, Utc::now()) {
            continue;
        }
        if is_busy(db, pinned_id).await? {
            continue; // moving: queue depth shows the wait (§10.1)
        }
        let issue = match run.work_item_id {
            Some(issue_id) => issues::Entity::find_by_id(issue_id).one(db).await?,
            None => None,
        };
        if !stranded_queued(db, &run, issue.as_ref()).await? {
            continue;
        }
        let error = format!(
            "Runner {} can no longer serve this run: its usage rule excludes it.",
            pinned.name
        );
        if fail_queued_run(db, run.id, "dedicated_runner_not_permitted", &error, None).await? {
            handled += 1;
        }
    }
    Ok(handled)
}

/// What `apply_lost_runner_policy` decided, so the caller can finish the
/// issue-side work (bounce, recreate) that needs its own seams.
#[derive(Clone, Debug, Default)]
pub struct LostRunnerOutcome {
    /// The issue needs the no-eligible-runner bounce (ticking state).
    pub bounce: bool,
    /// The 3-step `fall_back` outcome: the pod the issue now uses.
    pub fell_back_pod: Option<Uuid>,
    /// `fall_back` step 2: cancel+recreate descriptor for the default pod.
    pub recreate_in_pod: Option<Uuid>,
}

/// Apply the project's lost-runner handling to one issue (§13.1).
///
/// Called inside the revoking transaction, after the assignment has been
/// read; clears the assignment itself. Applies whether or not the issue
/// has a queued dedicated run.
///
/// `fail_to_backlog` (default): the queued dedicated runs fail with
/// `dedicated_runner_revoked`; the assignment is cleared; a ticking
/// issue is reported for the caller's no-eligible-runner bounce (reason
/// `dedicated-runner-revoked`).
///
/// `fall_back`: routed to a squad — its own pod when that pod has a
/// runner that could serve the issue, else the project's default pod
/// (reported for the caller's cancel+recreate in the default pod), else
/// the bounce. The could-serve check reuses the pod branch
/// (`pod_has_runner_for_issue_principal` with `pod_once`), so steps 2
/// and 3 cannot disagree.
pub async fn apply_lost_runner_policy(
    db: &sea_orm::DatabaseConnection,
    issue_id: Uuid,
    reason: &str,
    actor_id: Uuid,
) -> Result<LostRunnerOutcome, sea_orm::DbErr> {
    db.transaction(|txn| {
        Box::pin(async move {
            let issue = issues::Entity::find_by_id(issue_id)
                .lock(LockType::Update)
                .one(txn)
                .await?
                .ok_or_else(|| sea_orm::DbErr::RecordNotFound("issue".to_owned()))?;
            let runner_name = match issue.assigned_runner_id {
                Some(id) => runner::Entity::find_by_id(id)
                    .one(txn)
                    .await?
                    .map(|r| r.name)
                    .unwrap_or_else(|| "the dedicated runner".to_owned()),
                None => "the dedicated runner".to_owned(),
            };
            let queued = agent_run::Entity::find()
                .filter(agent_run::Column::WorkItemId.eq(issue.id))
                .filter(agent_run::Column::PinKind.eq(PIN_DEDICATED))
                .filter(agent_run::Column::Status.eq(AgentRunStatus::Queued.value()))
                .all(txn)
                .await?;
            let project = projects::Entity::find_by_id(issue.project_id)
                .one(txn)
                .await?;
            let policy = project
                .as_ref()
                .map(|p| p.dedicated_runner_lost_action.as_str())
                .unwrap_or("fail_to_backlog");
            if policy == "fall_back" {
                return apply_fall_back(txn, &issue, &queued, &runner_name, actor_id).await;
            }
            for run in &queued {
                let error = lost_runner_revoke_error(&runner_name);
                fail_queued_run(txn, run.id, "dedicated_runner_revoked", &error, None).await?;
            }
            // The ticking check needs the issue state; the caller finishes
            // it (the bounce seam lives outside domain).
            let bounce = is_ticking_issue(txn, &issue).await?;
            clear_dedicated_runner(
                txn,
                issue.id,
                "runner lost — run failed, issue moved to backlog",
                actor_id,
            )
            .await?;
            let _ = reason;
            Ok(LostRunnerOutcome {
                bounce,
                ..Default::default()
            })
        })
    })
    .await
    .map_err(unmap)
}

/// `fall_back` arm of [`apply_lost_runner_policy`] (§13.1).
async fn apply_fall_back(
    db: &impl ConnectionTrait,
    issue: &issues::Model,
    queued: &[agent_run::Model],
    runner_name: &str,
    actor_id: Uuid,
) -> Result<LostRunnerOutcome, sea_orm::DbErr> {
    let mut outcome = LostRunnerOutcome::default();
    let run = queued.first();
    let (creator, human) = match run {
        Some(run) => (
            Some(run.created_by_id),
            AgentRunTrigger::from_value(run.trigger.as_str())
                .map(|trigger| HUMAN_TRIGGERS.contains(&trigger))
                .unwrap_or(true),
        ),
        None => (None, false),
    };
    let could_serve = |pod_id: Option<Uuid>| async move {
        let Some(pod_id) = pod_id else {
            return Ok(false);
        };
        crate::runner_sessions::pod_match::pod_has_runner_for_issue_principal(
            db, pod_id, issue.id, creator, human,
        )
        .await
    };
    let own_pod_id = issue.assigned_pod_id;
    if could_serve(own_pod_id).await? {
        // Step 1: own pod has a runner that could serve the issue.
        for run in queued {
            let model = agent_run::ActiveModel {
                id: sea_orm::Unchanged(run.id),
                pinned_runner_id: sea_orm::Set(None),
                pin_kind: sea_orm::Set(PIN_LOCALITY.to_owned()),
                ..Default::default()
            };
            model.update(db).await?;
        }
        clear_dedicated_runner(db, issue.id, "runner lost — fell back to the pod", actor_id)
            .await?;
        post_fall_back_comment(db, issue, runner_name, own_pod_id, actor_id).await?;
        outcome.fell_back_pod = own_pod_id;
        return Ok(outcome);
    }
    let default_pod =
        crate::runner_enroll::queries::enroll_reads::default_pod_for_project(db, issue.project_id)
            .await?
            .map(|(id, _, _)| id);
    let other_pod = default_pod.is_some() && default_pod != own_pod_id;
    if other_pod && could_serve(default_pod).await? {
        // Step 2: the project's default pod has one.
        let model = issues::ActiveModel {
            id: sea_orm::Unchanged(issue.id),
            assigned_pod_id: sea_orm::Set(default_pod),
            updated_at: sea_orm::Set(Utc::now().into()),
            ..Default::default()
        };
        model.update(db).await?;
        for run in queued {
            fail_queued_run(
                db,
                run.id,
                "runner_revoked",
                "runner revoked; replaced in the default pod",
                None,
            )
            .await?;
        }
        outcome.recreate_in_pod = default_pod;
        clear_dedicated_runner(
            db,
            issue.id,
            "runner lost — fell back to the default pod",
            actor_id,
        )
        .await?;
        post_fall_back_comment(db, issue, runner_name, default_pod, actor_id).await?;
        outcome.fell_back_pod = default_pod;
        return Ok(outcome);
    }
    // Step 3: no squad can serve the issue — the existing bounce applies.
    for run in queued {
        let error = lost_runner_revoke_error(runner_name);
        fail_queued_run(db, run.id, "dedicated_runner_revoked", &error, None).await?;
    }
    clear_dedicated_runner(
        db,
        issue.id,
        "runner lost — no runner left in the pod",
        actor_id,
    )
    .await?;
    outcome.bounce = is_ticking_issue(db, issue).await?;
    Ok(outcome)
}

/// System comment for `fall_back` naming runner, reason and pod (§13.1).
async fn post_fall_back_comment(
    db: &impl ConnectionTrait,
    issue: &issues::Model,
    runner_name: &str,
    pod_id: Option<Uuid>,
    actor_id: Uuid,
) -> Result<(), sea_orm::DbErr> {
    let pod_name = match pod_id {
        Some(id) => pod::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(|p| p.name)
            .unwrap_or_else(|| "the pod".to_owned()),
        None => "the pod".to_owned(),
    };
    let body = format!(
        "<p>Runner <strong>{runner_name}</strong> is gone. The issue now uses pod <strong>{pod_name}</strong>.</p>"
    );
    let now = Utc::now();
    let comment = issue_comments::ActiveModel {
        id: sea_orm::Set(Uuid::new_v4()),
        comment_stripped: sea_orm::Set(body.clone()),
        attachments: sea_orm::Set(Vec::new()),
        created_by_id: sea_orm::Set(Some(actor_id)),
        issue_id: sea_orm::Set(issue.id),
        project_id: sea_orm::Set(issue.project_id),
        updated_by_id: sea_orm::Set(Some(actor_id)),
        workspace_id: sea_orm::Set(issue.workspace_id),
        actor_id: sea_orm::Set(Some(actor_id)),
        comment_html: sea_orm::Set(body),
        comment_json: sea_orm::Set(json!({})),
        access: sea_orm::Set("full".to_owned()),
        external_id: sea_orm::Set(None),
        external_source: sea_orm::Set(None),
        deleted_at: sea_orm::Set(None),
        edited_at: sea_orm::Set(None),
        description_id: sea_orm::Set(None),
        parent_id: sea_orm::Set(None),
        speaker_type: sea_orm::Set("system".to_owned()),
        speaker_label: sea_orm::Set(String::new()),
        speaker_agent_run_id: sea_orm::Set(None),
        labels: sea_orm::Set(Vec::new()),
        created_at: sea_orm::Set(now.into()),
        updated_at: sea_orm::Set(now.into()),
    };
    comment.insert(db).await?;
    Ok(())
}

/// Ticking-state check for the bounce arms (`is_ticking_state`,
/// `agent_phases.py:149-162`).
async fn is_ticking_issue(
    db: &impl ConnectionTrait,
    issue: &issues::Model,
) -> Result<bool, sea_orm::DbErr> {
    let Some(state_id) = issue.state_id else {
        return Ok(false);
    };
    let Some(state) = pidash_db::entity::states::Entity::find_by_id(state_id)
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    Ok(crate::assistant::tools_runs::is_ticking_state(
        Some(state.group.as_str()),
        Some(state.name.as_str()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner_fixture() -> runner::Model {
        runner::Model {
            id: Uuid::new_v4(),
            name: "mac-mini".to_owned(),
            capabilities: serde_json::json!({}),
            status: RUNNER_STATUS_ONLINE.to_owned(),
            os: "macos".to_owned(),
            arch: "arm64".to_owned(),
            runner_version: "1.0".to_owned(),
            protocol_version: 1,
            last_heartbeat_at: Some(Utc::now().into()),
            created_at: Utc::now().into(),
            updated_at: Utc::now().into(),
            revoked_at: None,
            owner_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            pod_id: Uuid::new_v4(),
            host_label: String::new(),
            refresh_token_hash: String::new(),
            refresh_token_fingerprint: String::new(),
            refresh_token_generation: 0,
            previous_refresh_token_hash: String::new(),
            access_token_signing_key_version: 0,
            enrollment_token_hash: String::new(),
            enrollment_token_fingerprint: String::new(),
            enrolled_at: None,
            revoked_reason: String::new(),
            visibility: 0,
            dev_machine_id: None,
            dev_metadata: serde_json::json!({}),
            provisioning: "own_agent".to_owned(),
            pod_dispatch: POD_DISPATCH_POD_MEMBER.to_owned(),
            paused_at: None,
            paused_by_id: None,
        }
    }

    fn machine_fixture() -> dev_machine::Model {
        dev_machine::Model {
            id: Uuid::new_v4(),
            host_label: "mini".to_owned(),
            label: "mini".to_owned(),
            visibility: 0,
            last_seen_at: None,
            revoked_at: None,
            created_at: Utc::now().into(),
            updated_at: Utc::now().into(),
            owner_id: Uuid::new_v4(),
            provisioning: "desktop_bundled".to_owned(),
            engine_paused_by_owner: false,
            engine_auto_pause_reason: String::new(),
            engine_team_runs: false,
        }
    }

    #[test]
    fn paused_reason_operator_pause() {
        let mut runner = runner_fixture();
        runner.paused_at = Some(Utc::now().into());
        let reason = runner_paused_reason(&runner, None).unwrap();
        assert_eq!(reason, "dispatch_paused");
    }

    #[test]
    fn paused_reason_owner_pause_and_auto() {
        let mut runner = runner_fixture();
        runner.provisioning = PROVISIONING_DESKTOP_BUNDLED.to_owned();
        let mut machine = machine_fixture();
        machine.engine_paused_by_owner = true;
        assert_eq!(
            runner_paused_reason(&runner, Some(&machine)).unwrap(),
            "owner_paused"
        );
        machine.engine_paused_by_owner = false;
        machine.engine_auto_pause_reason = "wallet_empty".to_owned();
        assert_eq!(
            runner_paused_reason(&runner, Some(&machine)).unwrap(),
            "wallet_empty"
        );
    }

    #[test]
    fn paused_reason_none_for_own_agent() {
        let runner = runner_fixture();
        assert!(runner_paused_reason(&runner, None).is_none());
        let machine = machine_fixture();
        // An own-agent runner's machine pause does not apply (§23.5).
        assert!(runner_paused_reason(&runner, Some(&machine)).is_none());
    }

    #[test]
    fn pause_phrases_match_design() {
        assert_eq!(
            refusal_pause_phrase("dispatch_paused"),
            "is paused by its owner"
        );
        assert_eq!(
            refusal_pause_phrase("owner_paused"),
            "is paused by its owner"
        );
        assert_eq!(
            refusal_pause_phrase("wallet_empty"),
            "is paused: the wallet is empty"
        );
        assert_eq!(
            refusal_pause_phrase("engine_sign_in_required"),
            "is paused: the engine's sign-in expired"
        );
        assert_eq!(
            refusal_pause_phrase("gateway_session_revoked"),
            "is paused: the gateway session was revoked"
        );
        assert_eq!(
            refusal_pause_phrase("profile_unavailable"),
            "is paused: the engine profile is unavailable"
        );
        assert_eq!(
            refusal_pause_phrase("engine_disabled"),
            "is paused: the engine is disabled"
        );
        assert_eq!(refusal_pause_phrase("unknown_future"), "is paused");
    }

    #[test]
    fn lost_runner_error_text_pinned() {
        let text = lost_runner_revoke_error("mac-mini");
        assert!(
            text.starts_with("Runner mac-mini was revoked or deleted, so this run cannot start."),
            "got: {text}"
        );
        assert!(text.contains("choose a new runner"));
    }

    #[test]
    fn unavailable_error_text_pinned() {
        let text =
            dedicated_unavailable_error("mac-mini", "it is offline (last seen unknown)", "Squad");
        assert!(text.contains("within 12 hours"), "got: {text}");
        assert!(text.contains("Automatic runs on this issue are stopped."));
        assert!(text.contains("Run on any runner in Squad once"));
    }

    #[test]
    fn offline_checks_status_then_heartbeat() {
        let mut runner = runner_fixture();
        runner.status = RUNNER_STATUS_OFFLINE.to_owned();
        assert!(runner_is_offline(&runner, Utc::now()));
        runner.status = RUNNER_STATUS_ONLINE.to_owned();
        runner.last_heartbeat_at = None;
        assert!(runner_is_offline(&runner, Utc::now()));
    }

    #[test]
    fn max_age_default_is_12_hours() {
        // The Django registry default (43200) unless the env overrides.
        let saved = std::env::var("DEDICATED_RUN_QUEUED_MAX_AGE_SECS").ok();
        std::env::remove_var("DEDICATED_RUN_QUEUED_MAX_AGE_SECS");
        assert_eq!(dedicated_run_queued_max_age_secs(), 43200);
        std::env::set_var("DEDICATED_RUN_QUEUED_MAX_AGE_SECS", "60");
        assert_eq!(dedicated_run_queued_max_age_secs(), 60);
        match saved {
            Some(value) => std::env::set_var("DEDICATED_RUN_QUEUED_MAX_AGE_SECS", value),
            None => std::env::remove_var("DEDICATED_RUN_QUEUED_MAX_AGE_SECS"),
        }
    }
}
