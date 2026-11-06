//! Machine merge + replace candidates (`machine_identity.py:170-405`,
//! design §4.10 + §13).
//!
//! [`replace_candidates`]: the caller's other machines with the same
//! non-empty `host_label`, not revoked/merged and with no live session
//! (`revoked_at IS NULL` + `last_seen_at >= now - 90s`), ordered
//! `-last_seen_at, -created_at` (Postgres `DESC` puts NULLS first).
//!
//! [`merge_dev_machine`]: one transaction that moves the source's live
//! runners to the target with the §13 collision rule: when both have a
//! live engine runner for one project, the target's survives, the
//! source's queued work is handed over (re-pinned in the survivor's
//! pod, released (`local_runner`) or cancelled (`managed_runner`)
//! elsewhere) and the loser is revoked with `dev_machine_merged`.
//! Every other runner moves (engine renamed to the target scheme, other
//! uniquified). The source's tokens/sessions die, the source is
//! revoked with `merged_into` + stored `merge_result`, and its engine
//! credential is revoked. No run's `pod` is rewritten. Idempotent: a
//! retry for the same target returns the stored result.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::entity::{
    agent_run, agent_run_event, dev_machine, machine_session, pod as pod_entity,
    projects as project_entity, runner as runner_entity, workspaces as workspace_entity,
};

use crate::domain::engine_credential::{self, revoke_reason};
use crate::domain::tokens;
use crate::runner_enroll::engine_naming;
use crate::runner_enroll::queries::enroll_reads;
use crate::runner_enroll::revoke as revoke_kernels;
use crate::runner_runs::finalization;
use crate::runner_runs::SetValue;

/// Live-session window (`LIVE_SESSION_WINDOW`, 90s).
pub const LIVE_SESSION_WINDOW_SECS: i64 = 90;

/// One replace-candidate row: the machine model (the payload keys
/// include the live-runner count, fetched per row by the handler).
pub type ReplaceCandidate = dev_machine::Model;

/// `replace_candidates` (`machine_identity.py:193-205`): same owner,
/// same non-empty `host_label`, not revoked/merged, `pk != machine.pk`,
/// no live session; `-last_seen_at, -created_at` (NULLS FIRST).
///
/// Returns an empty vec when the machine itself has no `host_label`
/// (mirrors `DevMachine.objects.none()`).
pub async fn replace_candidates(
    db: &impl ConnectionTrait,
    machine: &dev_machine::Model,
    now: DateTime<Utc>,
) -> Result<Vec<ReplaceCandidate>, DbErr> {
    if machine.host_label.is_empty() {
        return Ok(Vec::new());
    }
    let cutoff = now - Duration::seconds(LIVE_SESSION_WINDOW_SECS);
    // Live-session subquery: a machine is live when a session row with
    // the same `dev_machine_id`, `revoked_at IS NULL` and
    // `last_seen_at >= cutoff` exists. The NOT EXISTS is rendered as a
    // filtered `all()` + in-memory skip (the candidate set is tiny);
    // Postgres NULLS FIRST comes from the `desc()` ordering.
    let rows = dev_machine::Entity::find()
        .filter(dev_machine::Column::OwnerId.eq(machine.owner_id))
        .filter(dev_machine::Column::HostLabel.eq(machine.host_label.clone()))
        .filter(dev_machine::Column::RevokedAt.is_null())
        .filter(dev_machine::Column::MergedIntoId.is_null())
        .filter(dev_machine::Column::Id.ne(machine.id))
        .order_by_desc(dev_machine::Column::LastSeenAt)
        .order_by_desc(dev_machine::Column::CreatedAt)
        .all(db)
        .await?;
    let mut out = Vec::new();
    for row in rows {
        let sessions = machine_session::Entity::find()
            .filter(machine_session::Column::DevMachineId.eq(row.id))
            .filter(machine_session::Column::RevokedAt.is_null())
            .filter(machine_session::Column::LastSeenAt.gte(cutoff))
            .limit(1u64)
            .all(db)
            .await?;
        if sessions.is_empty() {
            out.push(row);
        }
    }
    Ok(out)
}

/// Whether the machine has a live session (`has_live_session`).
pub async fn has_live_session(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<bool, DbErr> {
    let cutoff = now - Duration::seconds(LIVE_SESSION_WINDOW_SECS);
    let rows = machine_session::Entity::find()
        .filter(machine_session::Column::DevMachineId.eq(machine_id))
        .filter(machine_session::Column::RevokedAt.is_null())
        .filter(machine_session::Column::LastSeenAt.gte(cutoff))
        .limit(1u64)
        .all(db)
        .await?;
    Ok(!rows.is_empty())
}

/// Bump helper used only by unit tests.
#[cfg(test)]
fn merged_at_iso(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_at_iso_is_drf_time() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-06T12:00:00.123456Z")
            .expect("ts")
            .with_timezone(&Utc);
        // `isoformat()` in Django is RFC 3339 with offset; rendering
        // here only pins that the helper is stable (wire bytes tested
        // by contract tests, not by this pin).
        assert!(merged_at_iso(now).contains("2026-11-06T12:00:00"));
    }
}

/// Merge refusal (`MergeError` in `machine_identity.py`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    /// `cannot_merge_into_self` — 400.
    #[error("cannot_merge_into_self")]
    CannotMergeIntoSelf,
    /// `dev_machine_not_found` (either missing or not the caller's) — 404.
    #[error("dev_machine_not_found")]
    NotFound,
    /// `already_merged_into_another_machine` — 409.
    #[error("already_merged_into_another_machine")]
    AlreadyMerged,
    /// `source_machine_revoked` — 409.
    #[error("source_machine_revoked")]
    SourceRevoked,
    /// `target_machine_not_live` — 409.
    #[error("target_machine_not_live")]
    TargetNotLive,
    /// `source_machine_has_live_session` — 409.
    #[error("source_machine_has_live_session")]
    LiveSession,
    /// Storage failure — 500.
    #[error("merge db error: {0}")]
    Db(String),
}

impl MergeError {
    /// The `{"error": code}` code (`MergeError.code`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::CannotMergeIntoSelf => "cannot_merge_into_self",
            Self::NotFound => "dev_machine_not_found",
            Self::AlreadyMerged => "already_merged_into_another_machine",
            Self::SourceRevoked => "source_machine_revoked",
            Self::TargetNotLive => "target_machine_not_live",
            Self::LiveSession => "source_machine_has_live_session",
            Self::Db(_) => "server_error",
        }
    }

    /// The HTTP status (`MergeError.http_status`).
    pub fn http_status(&self) -> u16 {
        match self {
            Self::CannotMergeIntoSelf => 400,
            Self::NotFound => 404,
            Self::AlreadyMerged | Self::SourceRevoked | Self::TargetNotLive | Self::LiveSession => {
                409
            }
            Self::Db(_) => 500,
        }
    }
}

impl From<DbErr> for MergeError {
    fn from(err: DbErr) -> Self {
        Self::Db(err.to_string())
    }
}

/// The merge outcome: the stored `merge_result` document (the same
/// object the response answers) plus the post-commit effects in
/// Django registration order — publishes (per finalized run), handoffs
/// (per revoke-cancelled run), pod drains, stream cleanup (per
/// revoked loser). The caller fires them after commit.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeOutcome {
    pub result: Value,
    pub drain_pod_ids: Vec<Uuid>,
    pub publish_run_ids: Vec<Uuid>,
    pub handoff_pairs: Vec<(Uuid, Uuid)>,
    pub cleanup_runner_ids: Vec<Uuid>,
}

/// `finalize_agent_run` (`agent_run_finalization.py:50-96`) for the
/// merge cancels, over SeaORM: row-lock the run, merge
/// `error_details` under the lock, write the terminal values
/// (`status`, `ended_at`, both markers cleared, `cancel_reason` when
/// the caller passes one), append the cloud-only terminal event, and
/// report whether the row finalized (the caller then collects the
/// terminal-effects publication). First-writer-wins: a terminal row
/// or a failed expected filter is `Ok(false)`.
///
/// `expected_queued`: the handover cancel's `expected_status=QUEUED`
/// (no `cancel_reason`). `expected_runner`: the revoke cancel's
/// `expected_runner_id` (`cancel_reason` set).
async fn finalize_merge_cancel(
    db: &impl ConnectionTrait,
    run_id: Uuid,
    now: DateTime<Utc>,
    expected_queued: bool,
    expected_runner: Option<Uuid>,
    cancel_reason: Option<&str>,
) -> Result<bool, DbErr> {
    use pidash_types::runner_runs::AgentRunStatus;

    let mut extras: Vec<(&'static str, SetValue)> = vec![
        (
            "error",
            SetValue::Text(revoke_kernels::FINALIZE_ERROR.to_owned()),
        ),
        (
            "error_code",
            SetValue::Text(revoke_kernels::FINALIZE_ERROR_CODE.to_owned()),
        ),
    ];
    if let Some(reason) = cancel_reason {
        extras.push(("cancel_reason", SetValue::Text(reason.to_owned())));
    }
    let values = finalization::plan_finalize_values(AgentRunStatus::Cancelled, &extras)
        .map_err(|err| DbErr::RecordNotFound(err.to_string()))?;
    let locked = agent_run::Entity::find_by_id(run_id)
        .lock_exclusive()
        .one(db)
        .await?;
    let Some(run) = locked else {
        return Ok(false);
    };
    let row_status = AgentRunStatus::from_value(&run.status).unwrap_or(AgentRunStatus::Queued);
    if !finalization::finalize_lock_passes(
        row_status,
        run.runner_id,
        expected_runner,
        expected_queued.then_some(AgentRunStatus::Queued),
    ) {
        return Ok(false);
    }
    // Folded keys (`FOLDED_KEYS`: `error` → `message`,
    // `error_code` → `code`): the bag carries code/message.
    use pidash_db::runner_runs::agent_run::{ERROR_DETAILS_CODE, ERROR_DETAILS_MESSAGE};
    let mut bag = match run.error_details.clone() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    bag.insert(
        ERROR_DETAILS_MESSAGE.to_owned(),
        Value::String(revoke_kernels::FINALIZE_ERROR.to_owned()),
    );
    bag.insert(
        ERROR_DETAILS_CODE.to_owned(),
        Value::String(revoke_kernels::FINALIZE_ERROR_CODE.to_owned()),
    );
    let mut update = agent_run::Entity::update_many();
    update = update
        .col_expr(agent_run::Column::Status, Expr::value("cancelled"))
        .col_expr(agent_run::Column::EndedAt, Expr::value(now))
        .col_expr(
            agent_run::Column::TerminalHooksAppliedAt,
            Expr::value(Option::<DateTime<Utc>>::None),
        )
        .col_expr(
            agent_run::Column::TerminalCapacityReleasedAt,
            Expr::value(Option::<DateTime<Utc>>::None),
        )
        .col_expr(
            agent_run::Column::ErrorDetails,
            Expr::value(Value::Object(bag)),
        );
    if let Some(reason) = cancel_reason {
        update = update.col_expr(
            agent_run::Column::CancelReason,
            Expr::value(reason.to_owned()),
        );
    }
    update
        .filter(agent_run::Column::Id.eq(run_id))
        .exec(db)
        .await
        .map(drop)?;
    // Cloud-only terminal event (`:82-94`).
    if run.executor_kind == "cloud_agent" {
        let exists = agent_run_event::Entity::find()
            .filter(agent_run_event::Column::AgentRunId.eq(run_id))
            .filter(agent_run_event::Column::Kind.eq("terminal"))
            .limit(1u64)
            .all(db)
            .await?;
        if exists.is_empty() {
            let max_seq = agent_run_event::Entity::find()
                .filter(agent_run_event::Column::AgentRunId.eq(run_id))
                .order_by_desc(agent_run_event::Column::Seq)
                .limit(1u64)
                .all(db)
                .await?
                .first()
                .map(|row| row.seq);
            let plan = finalization::plan_terminal_event(
                max_seq,
                AgentRunStatus::Cancelled,
                &finalization::finalize_error_code(&values),
            );
            use sea_orm::{ActiveValue, Set};
            agent_run_event::Entity::insert(agent_run_event::ActiveModel {
                id: ActiveValue::NotSet,
                seq: Set(plan.seq),
                kind: Set("terminal".to_owned()),
                payload: Set(plan.payload),
                created_at: Set(now.into()),
                agent_run_id: Set(run_id),
            })
            .exec_without_returning(db)
            .await?;
        }
    }
    Ok(true)
}

/// Hand `loser`'s queued runs to `survivor` (`_hand_over_to_survivor`,
/// revision 4.0). Returns the pods to drain (`{survivor.pod} | loose`)
/// and the finalized run ids (terminal-effects publication each).
async fn hand_over_to_survivor(
    db: &impl ConnectionTrait,
    loser: &runner_entity::Model,
    survivor: &runner_entity::Model,
    now: DateTime<Utc>,
) -> Result<(HashSet<Uuid>, Vec<Uuid>), DbErr> {
    // Queued runs pinned to the loser, oldest first.
    let queued = agent_run::Entity::find()
        .filter(agent_run::Column::PinnedRunnerId.eq(loser.id))
        .filter(agent_run::Column::Status.eq("queued"))
        .order_by_asc(agent_run::Column::CreatedAt)
        .order_by_asc(agent_run::Column::Id)
        .all(db)
        .await?;
    let mut loose_pods = HashSet::new();
    let mut published = Vec::new();
    for run in &queued {
        if run.pod_id == survivor.pod_id {
            // Re-pin to the survivor (same pod).
            agent_run::Entity::update_many()
                .col_expr(
                    agent_run::Column::PinnedRunnerId,
                    Expr::value(Some(survivor.id)),
                )
                .filter(agent_run::Column::Id.eq(run.id))
                .filter(agent_run::Column::Status.eq("queued"))
                .exec(db)
                .await
                .map(drop)?;
        } else if run.executor_kind == "local_runner" {
            loose_pods.insert(run.pod_id);
            agent_run::Entity::update_many()
                .col_expr(agent_run::Column::PinnedRunnerId, Expr::value(None::<Uuid>))
                .filter(agent_run::Column::Id.eq(run.id))
                .filter(agent_run::Column::Status.eq("queued"))
                .exec(db)
                .await
                .map(drop)?;
        } else if run.executor_kind == "managed_runner"
            && finalize_merge_cancel(db, run.id, now, true, None, None).await?
        {
            published.push(run.id);
        }
        // Any other executor kind keeps its pin (Django ignores it the
        // same way: only `local_runner` is released, only
        // `managed_runner` is cancelled).
    }
    let mut drains = loose_pods;
    drains.insert(survivor.pod_id);
    Ok((drains, published))
}

/// The loser revoke (`Runner.revoke` with `dev_machine_merged`,
/// S1-S4 via the `revoke` performers): stamp (`stamp_runner_revoked`),
/// revoke sessions, cancel non-terminal runs through
/// [`finalize_merge_cancel`] (markers, `cancel_reason`, cloud event),
/// unpin queued follow-ups. Returns the post-commit descriptors —
/// drain pods (S3+S4, first-seen), published runs, handoff pairs —
/// plus the cleanup runner id (always, per `revoke`).
#[allow(clippy::type_complexity)]
async fn revoke_loser(
    db: &impl ConnectionTrait,
    loser_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(Vec<Uuid>, Vec<Uuid>, Vec<(Uuid, Uuid)>), DbErr> {
    revoke_kernels::stamp_runner_revoked(db, loser_id, now, "dev_machine_merged").await?;
    // S2: revoke sessions (domain writer).
    tokens::revoke_runner_sessions_for_runner(db, loser_id, now, "dev_machine_merged")
        .await
        .map_err(|err| DbErr::RecordNotFound(err.to_string()))?;
    // S3: locked non-terminal runs, newest first
    // (`revoke::lock_active_runs` carries the full 8-status set).
    let active_runs = revoke_kernels::lock_active_runs(db, loser_id).await?;
    let mut cancelled_run_ids = Vec::with_capacity(active_runs.len());
    let mut published = Vec::new();
    for (run_id, _) in &active_runs {
        if finalize_merge_cancel(
            db,
            *run_id,
            now,
            false,
            Some(loser_id),
            Some("dev_machine_merged"),
        )
        .await?
        {
            cancelled_run_ids.push(*run_id);
            published.push(*run_id);
        }
    }
    // S4: unpin queued follow-ups still pinned here.
    let pinned_pod_ids = revoke_kernels::pinned_queued_pod_ids(db, loser_id).await?;
    if !pinned_pod_ids.is_empty() {
        revoke_kernels::unpin_queued_runs_for_runner(db, loser_id).await?;
    }
    let mut drained_pod_ids: Vec<Uuid> = Vec::new();
    for (_, pod_id) in &active_runs {
        if let Some(pod_id) = pod_id {
            if !drained_pod_ids.contains(pod_id) {
                drained_pod_ids.push(*pod_id);
            }
        }
    }
    for pod_id in pinned_pod_ids.iter().flatten() {
        if !drained_pod_ids.contains(pod_id) {
            drained_pod_ids.push(*pod_id);
        }
    }
    let handoff_pairs = cancelled_run_ids
        .iter()
        .map(|run_id| (loser_id, *run_id))
        .collect();
    Ok((drained_pod_ids, published, handoff_pairs))
}

/// `merge_dev_machine` (`machine_identity.py:286-405`): one
/// transaction. The caller passes the transaction connection
/// (`TxConn` over a begun sqlx transaction) and commits on `Ok`.
/// Locks run `ORDER BY pk` (both machines, then the source's runners
/// in `created_at, id` order); the stored result is returned verbatim
/// on a same-target retry.
pub async fn merge_dev_machine(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    source_id: Uuid,
    target_id: Uuid,
    now: DateTime<Utc>,
) -> Result<MergeOutcome, MergeError> {
    if source_id == target_id {
        return Err(MergeError::CannotMergeIntoSelf);
    }
    // Lock both rows ordered by pk (the `pk__in=... .order_by("pk")`
    // read). SeaORM `lock_exclusive` renders `FOR UPDATE`; the lock
    // order is the caller's `ORDER BY`.
    let first_id = source_id.min(target_id);
    let second_id = source_id.max(target_id);
    let first = dev_machine::Entity::find_by_id(first_id)
        .lock_exclusive()
        .one(db)
        .await?;
    let second = dev_machine::Entity::find_by_id(second_id)
        .lock_exclusive()
        .one(db)
        .await?;
    let (mut source, mut target) = if first_id == source_id {
        (first, second)
    } else {
        (second, first)
    };
    let source = source.take().ok_or(MergeError::NotFound)?;
    let target = target.take().ok_or(MergeError::NotFound)?;
    if source.owner_id != user_id || target.owner_id != user_id {
        return Err(MergeError::NotFound);
    }
    if let Some(prev) = source.merged_into_id {
        if prev == target.id && !source.merge_result.is_null() {
            // Same-target retry: the stored document verbatim.
            return Ok(MergeOutcome {
                result: source.merge_result.clone(),
                drain_pod_ids: Vec::new(),
                publish_run_ids: Vec::new(),
                handoff_pairs: Vec::new(),
                cleanup_runner_ids: Vec::new(),
            });
        }
        return Err(MergeError::AlreadyMerged);
    }
    if source.revoked_at.is_some() {
        return Err(MergeError::SourceRevoked);
    }
    if target.revoked_at.is_some() || target.merged_into_id.is_some() {
        return Err(MergeError::TargetNotLive);
    }
    if has_live_session(db, source.id, now).await? {
        return Err(MergeError::LiveSession);
    }

    // Survivors: the target's live engine runners keyed by project,
    // oldest first (later same-project rows never overwrite).
    let survivor_rows = runner_entity::Entity::find()
        .filter(runner_entity::Column::DevMachineId.eq(Some(target.id)))
        .filter(runner_entity::Column::Provisioning.eq("desktop_bundled"))
        .filter(runner_entity::Column::RevokedAt.is_null())
        .order_by_asc(runner_entity::Column::CreatedAt)
        .all(db)
        .await?;
    let mut survivors: HashMap<Uuid, runner_entity::Model> = HashMap::new();
    for row in survivor_rows {
        let pod = pod_entity::Entity::find_by_id(row.pod_id).one(db).await?;
        if let Some(pod) = pod {
            survivors.entry(pod.project_id).or_insert(row);
        }
    }

    // The source's live runners, oldest first (locked).
    let runners = runner_entity::Entity::find()
        .filter(runner_entity::Column::DevMachineId.eq(Some(source.id)))
        .filter(runner_entity::Column::RevokedAt.is_null())
        .order_by_asc(runner_entity::Column::CreatedAt)
        .order_by_asc(runner_entity::Column::Id)
        .all(db)
        .await?;
    let mut moved: Vec<String> = Vec::new();
    let mut collisions: Vec<Value> = Vec::new();
    let mut drain_pods: HashSet<Uuid> = HashSet::new();
    let mut publish_run_ids: Vec<Uuid> = Vec::new();
    let mut handoff_pairs: Vec<(Uuid, Uuid)> = Vec::new();
    let mut cleanup_runner_ids: Vec<Uuid> = Vec::new();
    for runner in &runners {
        let pod = pod_entity::Entity::find_by_id(runner.pod_id)
            .one(db)
            .await?
            .ok_or_else(|| MergeError::Db("merge pod vanished".to_owned()))?;
        let project_id = pod.project_id;
        let is_engine = runner.provisioning == "desktop_bundled";
        if let Some(survivor) = survivors.get(&project_id).filter(|_| is_engine) {
            let survivor = survivor.clone();
            let (drains, published) = hand_over_to_survivor(db, runner, &survivor, now).await?;
            drain_pods.extend(drains);
            publish_run_ids.extend(published);
            let (loser_drains, loser_published, loser_handoffs) =
                revoke_loser(db, runner.id, now).await?;
            drain_pods.extend(loser_drains);
            publish_run_ids.extend(loser_published);
            handoff_pairs.extend(loser_handoffs);
            cleanup_runner_ids.push(runner.id);
            collisions.push(serde_json::json!({
                "revoked_runner_id": runner.id.to_string(),
                "survivor_runner_id": survivor.id.to_string(),
            }));
            continue;
        }
        // Move: engine renamed to the target scheme, other uniquified.
        let name = if is_engine {
            let workspace = workspace_entity::Entity::find_by_id(runner.workspace_id)
                .one(db)
                .await?
                .ok_or_else(|| MergeError::Db("merge workspace vanished".to_owned()))?;
            let project = project_entity::Entity::find_by_id(pod.project_id)
                .one(db)
                .await?
                .ok_or_else(|| MergeError::Db("merge project vanished".to_owned()))?;
            let base = engine_naming::engine_runner_base_name(
                target.id,
                &target.host_label,
                &target.label,
                &workspace.slug,
                project.id,
                &project.identifier,
            );
            let taken = enroll_reads::taken_names(db, target.id, pod.id, Some(runner.id)).await?;
            enroll_reads::engine_runner_name_from_base(&base, &taken)
        } else {
            let taken = enroll_reads::taken_names(db, target.id, pod.id, Some(runner.id)).await?;
            enroll_reads::unique_runner_name(&runner.name, &taken)
        };
        tokens::revoke_runner_sessions_for_runner(db, runner.id, now, "dev_machine_merged")
            .await
            .map_err(|err| MergeError::Db(err.to_string()))?;
        let host_label = if !target.host_label.is_empty() {
            target.host_label.clone()
        } else {
            runner.host_label.clone()
        };
        runner_entity::Entity::update_many()
            .col_expr(
                runner_entity::Column::DevMachineId,
                Expr::value(Some(target.id)),
            )
            .col_expr(runner_entity::Column::HostLabel, Expr::value(host_label))
            .col_expr(runner_entity::Column::Name, Expr::value(name))
            .col_expr(runner_entity::Column::Status, Expr::value("offline"))
            .col_expr(runner_entity::Column::UpdatedAt, Expr::value(now))
            .filter(runner_entity::Column::Id.eq(runner.id))
            .exec(db)
            .await
            .map(drop)?;
        if is_engine {
            // The moved engine becomes the survivor for its project.
            let moved_row = runner_entity::Entity::find_by_id(runner.id)
                .one(db)
                .await?
                .ok_or_else(|| MergeError::Db("merge moved runner vanished".to_owned()))?;
            survivors.insert(project_id, moved_row);
        }
        moved.push(runner.id.to_string());
    }

    tokens::revoke_tokens_for_dev_machine(db, source.id, now)
        .await
        .map_err(|err| MergeError::Db(err.to_string()))?;
    machine_session::Entity::update_many()
        .col_expr(machine_session::Column::RevokedAt, Expr::value(now))
        .col_expr(
            machine_session::Column::RevokedReason,
            Expr::value("dev_machine_merged"),
        )
        .filter(machine_session::Column::DevMachineId.eq(Some(source.id)))
        .filter(machine_session::Column::RevokedAt.is_null())
        .exec(db)
        .await
        .map(drop)?;

    let result = serde_json::json!({
        "source_dev_machine_id": source.id.to_string(),
        "target_dev_machine_id": target.id.to_string(),
        "merged_at": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        "moved_runner_ids": moved,
        "collisions": collisions,
    });
    // Stamp the source: revoked + merged_into + stored result +
    // `updated_at` (one save).
    dev_machine::Entity::update_many()
        .col_expr(dev_machine::Column::RevokedAt, Expr::value(now))
        .col_expr(
            dev_machine::Column::MergedIntoId,
            Expr::value(Some(target.id)),
        )
        .col_expr(
            dev_machine::Column::MergeResult,
            Expr::value(result.clone()),
        )
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now))
        .filter(dev_machine::Column::Id.eq(source.id))
        .exec(db)
        .await
        .map(drop)?;
    engine_credential::revoke_engine_credential(db, source.id, revoke_reason::MACHINE_MERGED, now)
        .await
        .map_err(|err| MergeError::Db(err.to_string()))?;
    let mut drain_pod_ids: Vec<Uuid> = drain_pods.into_iter().collect();
    drain_pod_ids.sort();
    Ok(MergeOutcome {
        result,
        drain_pod_ids,
        publish_run_ids,
        handoff_pairs,
        cleanup_runner_ids,
    })
}
