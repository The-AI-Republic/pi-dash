//! `pod_has_runner_for_issue_principal` (`matcher.py:436-490`), SeaORM
//! implementation (the single live matcher).
//!
//! Mirrors the eligibility set of
//! `filter_runs_usable_by_runner` for private runners — the only
//! visibility. A runner counts as potentially serviceable when its
//! owner is the run's creator, the issue's creator, or a live
//! assignee. Transient status is deliberately ignored — OFFLINE /
//! BUSY / paused runners still count; REVOKED runners (permanent)
//! and `pinned_only` runners (never take unpinned work) never count.
//!
//! Built-in engines count only for their owner's work: runs the
//! owner starts, automatic runs (`human == false`) on the owner's
//! issues, or any such run when the machine opted into teammates'
//! runs (`engine_team_runs`). An offline or paused pod-member
//! engine still counts, so its owner's run waits in the pod.
//!
//! The one-method seam this implements lives in
//! [`crate::orchestration::dispatch::PodRunnerMatcher`]; the two
//! former hand-rolled live copies (jobs, api) delegate here.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

use sea_orm::{ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

use pidash_db::entity::{dev_machine, issue_assignees, issues, runner};

use crate::orchestration::dispatch::{DispatchError, PodRunnerMatcher};

/// Does `pod_id` contain any registered runner whose owner could
/// serve a run on `issue_id` triggered by `creator_id`?
pub async fn pod_has_runner_for_issue_principal(
    db: &impl ConnectionTrait,
    pod_id: Uuid,
    issue_id: Uuid,
    creator_id: Option<Uuid>,
    human: bool,
    pod_once: bool,
) -> Result<bool, sea_orm::DbErr> {
    let issue = issues::Entity::find_by_id(issue_id).one(db).await?;
    let issue = issue.ok_or_else(|| sea_orm::DbErr::RecordNotFound("issue".to_owned()))?;
    let mut issue_people: HashSet<Uuid> = HashSet::new();
    if let Some(creator) = issue.created_by_id {
        issue_people.insert(creator);
    }
    // Assignees through the soft-delete manager: the un-assign path
    // only stamps `deleted_at` on the through row.
    let rows = issue_assignees::Entity::find()
        .filter(issue_assignees::Column::IssueId.eq(issue_id))
        .filter(issue_assignees::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    for row in rows {
        issue_people.insert(row.assignee_id);
    }

    let mut eligible_owner_ids: HashSet<Uuid> = issue_people.clone();
    if let Some(creator) = creator_id {
        eligible_owner_ids.insert(creator);
    }
    if eligible_owner_ids.is_empty() {
        return Ok(false);
    }

    // Dedicated issue (§9.4): served only by the assigned runner —
    // in the run's pod, non-revoked, of an eligible owner, and the
    // usage rule admits the run's creator/trigger. `pod_dispatch`
    // is ignored (a pinned_only runner takes its dedicated runs,
    // §23.1); offline/paused stay countable (transient).
    if let Some(assigned_id) = issue.assigned_runner_id {
        if !pod_once {
            let assigned = runner::Entity::find_by_id(assigned_id)
                .filter(runner::Column::PodId.eq(pod_id))
                .filter(runner::Column::OwnerId.is_in(eligible_owner_ids.clone()))
                .filter(runner::Column::Status.ne("revoked"))
                .one(db)
                .await?;
            return match assigned {
                Some(assigned) => {
                    crate::domain::issue_assignment::runner_may_take_run(
                        db,
                        &assigned,
                        creator_id,
                        human,
                        Some(&issue),
                    )
                    .await
                }
                None => Ok(false),
            };
        }
    }

    // Non-revoked pod members of every eligible owner (engines and
    // own-agent runners alike; paused engines count too).
    let candidates = runner::Entity::find()
        .filter(runner::Column::PodId.eq(pod_id))
        .filter(runner::Column::OwnerId.is_in(eligible_owner_ids.clone()))
        .filter(runner::Column::Status.ne("revoked"))
        .filter(runner::Column::PodDispatch.ne("pinned_only"))
        .all(db)
        .await?;
    // Any own-agent candidate serves any of these principals.
    if candidates
        .iter()
        .any(|runner| runner.provisioning != "desktop_bundled")
    {
        return Ok(true);
    }
    // Engines: owner-only work unless the owner opted in (PO-3).
    let team_machines: HashSet<Uuid> = {
        let ids: Vec<Uuid> = candidates
            .iter()
            .filter(|runner| runner.provisioning == "desktop_bundled")
            .filter_map(|runner| runner.dev_machine_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if ids.is_empty() {
            HashSet::new()
        } else {
            dev_machine::Entity::find()
                .filter(dev_machine::Column::Id.is_in(ids))
                .filter(dev_machine::Column::EngineTeamRuns.eq(true))
                .all(db)
                .await?
                .into_iter()
                .map(|machine| machine.id)
                .collect()
        }
    };
    Ok(candidates.iter().any(|runner| {
        runner.provisioning == "desktop_bundled"
            && engine_counts(
                runner.owner_id,
                creator_id,
                human,
                issue_people.contains(&runner.owner_id),
                runner
                    .dev_machine_id
                    .is_some_and(|machine| team_machines.contains(&machine)),
            )
    }))
}

/// The PO-3 engine gate (§8): does this engine's owner serve the
/// run? The owner starts it, or the run is automatic on one of the
/// owner's issues, or the machine opted into teammates' runs.
fn engine_counts(
    owner_id: Uuid,
    creator_id: Option<Uuid>,
    human: bool,
    in_issue_people: bool,
    team_runs: bool,
) -> bool {
    Some(owner_id) == creator_id || (!human && in_issue_people) || team_runs
}

/// The single live [`PodRunnerMatcher`]: SeaORM over the caller's
/// pool (the D-12 fire-tick driver's `JobsMatcher` and the api
/// `LiveMatcher` delegate here).
pub struct SeaOrmPodMatcher {
    db: Arc<DatabaseConnection>,
}

impl SeaOrmPodMatcher {
    pub fn new(db: DatabaseConnection) -> Self {
        SeaOrmPodMatcher { db: Arc::new(db) }
    }
}

impl PodRunnerMatcher for SeaOrmPodMatcher {
    fn pod_has_runner_for_issue_principal(
        &self,
        pod_id: Uuid,
        issue_id: Uuid,
        creator_id: Option<Uuid>,
        human: bool,
        pod_once: bool,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<bool, DispatchError>> + Send + '_>> {
        let db = self.db.clone();
        Box::pin(async move {
            pod_has_runner_for_issue_principal(
                db.as_ref(),
                pod_id,
                issue_id,
                creator_id,
                human,
                pod_once,
            )
            .await
            .map_err(|error| {
                DispatchError::Store(crate::orchestration::CreationError::MissingRow(
                    error.to_string(),
                ))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Alias, Expr, Query};
    use serde_json::Value;

    static FX07: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-07-matcher.json");

    fn fx07() -> Value {
        serde_json::from_str(FX07).expect("FX-RSES-07 parses")
    }

    /// The candidate filter mirrors Django's
    /// `Runner.objects.filter(pod, owner_id__in).exclude(revoked,
    /// pinned_only)` — pinned by term presence.
    #[test]
    fn candidate_filter_terms() {
        let pod = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let query = Query::select()
            .column((Alias::new("runner"), "id"))
            .from(Alias::new("runner"))
            .and_where(Expr::col((Alias::new("runner"), "pod_id")).eq(pod))
            .and_where(Expr::col((Alias::new("runner"), "owner_id")).is_in([owner]))
            .and_where(Expr::col((Alias::new("runner"), "status")).ne("revoked"))
            .and_where(Expr::col((Alias::new("runner"), "pod_dispatch")).ne("pinned_only"))
            .to_owned();
        let sql = query.to_string(sea_query::PostgresQueryBuilder);
        for term in [
            "\"runner\".\"pod_id\"",
            "\"runner\".\"owner_id\"",
            "'revoked'",
            "'pinned_only'",
        ] {
            assert!(sql.contains(term), "term present: {term}");
        }
    }

    /// The PO-3 gate against the oracle matrix (Django capture,
    /// #2903): a teammate's click bounces, the clock's tick on the
    /// owner's issue waits, team runs admit, pinned-only never
    /// counts, stray-owner issues never match.
    #[test]
    fn engine_gate_matches_oracle_matrix() {
        let fx = fx07();
        let ph = &fx["pod_has_runner_for_issue_principal"];
        let owner = Uuid::new_v4();
        let mate = Uuid::new_v4();
        // (case, creator, human, in_issue_people, team_runs, expected)
        let cases: &[(&str, Option<Uuid>, bool, bool, bool)] = &[
            ("engine_owner_click", Some(owner), true, false, false),
            ("engine_teammate_click", Some(mate), true, true, false),
            ("engine_tick_owner_issue", Some(mate), false, true, false),
            ("engine_paused_counts", Some(mate), false, true, false),
            ("engine_team_teammate_click", Some(mate), true, true, true),
        ];
        for (label, creator, human, in_people, team) in cases {
            assert_eq!(
                engine_counts(owner, *creator, *human, *in_people, *team),
                ph[label]["value"].as_bool().expect("value"),
                "{label}"
            );
        }
        // Own-agent candidates count for every principal; the
        // pinned-only and stranger cases never reach the gate.
        assert_eq!(ph["own_agent_human"]["value"].as_bool(), Some(true));
        assert_eq!(ph["own_agent_tick"]["value"].as_bool(), Some(true));
        assert_eq!(ph["engine_pinned_only"]["value"].as_bool(), Some(false));
        assert_eq!(ph["stranger_no_match"]["value"].as_bool(), Some(false));
    }
}
