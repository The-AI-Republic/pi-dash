#![forbid(unsafe_code)]

//! Jobs-side dispatch seam (D-12 L8, stage 5).
//!
//! The live half of the services [`dispatch`][pidash_services::orchestration::dispatch]
//! drivers:
//!
//! * [`DispatchSeam`][pidash_services::orchestration::dispatch::DispatchSeam]
//!   implemented for the L6 [`LiveCreationStore`], so guards, builders and
//!   clock writes share the insertion transaction (the single-transaction
//!   grant+dispatch with rollback). Reads reuse the L6 SQL consts; only
//!   genuinely new query shapes get new consts (in services, next to the
//!   drivers).
//! * One transaction driver per services driver
//!   ([`dispatch_continuation_run`], [`dispatch_run_ai_run_with_reason`],
//!   [`run_ai_for_human`], [`re_tick_ticker`], [`wait_ticker`],
//!   [`maybe_apply_deferred_pause`], [`bounce_issue_no_eligible_runner`]):
//!   open the transaction, run the services control flow, commit (or roll
//!   back when the outcome says so), then drain post-commit dispatch —
//!   the L6 driver shape, over the same public `dispatch_agent_run` /
//!   `consume_admission_token` collaborators.
//! * [`FireTickShim`][]: the
//!   [`FireTickSeam`][crate::tasks_ticker::FireTickSeam] implementation
//!   over L1 (ticking state, tick interval) + L2 (active/prior runs) +
//!   this unit (dispatch). The two sync trait methods need the State row
//!   but cannot await: the shim resolves it through an injected sync
//!   state resolver (cutover wiring provides a cached one; registration
//!   wiring stays cutover scope, not this issue). Fixtures judge the
//!   values, not the mechanism.
//!
//! The D-14 matcher rides in as an `Arc<dyn PodRunnerMatcher>` per
//! driver call — D-14 implements the trait later; the live tests script
//! it. No new crate edges: jobs already depends on services + db +
//! types.
//!
//! Fixture id replayed by the live suite: FX-ORCH-08
//! (`rust-api/fixtures/orchestration/fx08_dispatch/`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
    QueryTrait, RelationTrait, Set,
};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::comment_hooks::description_stripped_for;
use pidash_db::entity::{
    agent_run, issue_activities, issue_agent_ticker, issue_assignees, issues, project_members,
    projects, states, users, workspace_members, workspaces,
};
use pidash_db::orchestration::runs::ACTIVE_STATUSES;
use pidash_db::orm_tx::{into_sqlx, TxConn};
use pidash_db::pool::sea_orm_connection;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction;
use pidash_services::dispatch::admission::{managed_runner_probe, ManagedRunnerProbe};
use pidash_services::dispatch::{
    consume_admission_token, AdmissionCache, DeferredConsume, LlmProfile,
};
use pidash_services::domain::clock::apply_clock_write;
use pidash_services::domain::comments as comment_domain;
use pidash_services::domain::identity::{
    create_bot_user, find_agent_user, make_unusable_password, stamp_bot_unusable_password,
    AgentUserCollisionError, NewBotUser, AGENT_DISPLAY_NAME, AGENT_USERNAME, AGENT_USER_EMAIL,
    AGENT_USER_FIRST_NAME, AGENT_USER_LAST_NAME,
};
use pidash_services::domain::issues as domain_issues;
use pidash_services::orchestration::clock::ClockWrite;
use pidash_services::orchestration::dispatch::{
    BounceOutcome, CandidateUser, ContinuationDispatchOutcome, DispatchError, DispatchSeam,
    HumanRunAiOutcome, LogLine, NewIssueComment, NewWaitActivity, PauseOutcome, PodRunnerMatcher,
    PreflightOutcome, RetickOutcome, RunAiOutcome, ThinRunAiOutcome, WaitOutcome,
};
use pidash_services::orchestration::entries::clock_policy;
use pidash_types::orchestration::{
    cadence_fields_for, is_ticking_state, StateRef, PAUSED_STATE_NAME,
};

use super::creation_jobs::{LiveCreationStore, StoreDeps};
use crate::dispatch::{dispatch_agent_run, DispatchDecision};
use crate::tasks_ticker::fire_tick::{FireTickSeam, ProjectTickCols};

// ---------------------------------------------------------------------------
// Errors + outcomes
// ---------------------------------------------------------------------------

/// Every failure the dispatch drivers report.
#[derive(Debug, thiserror::Error)]
pub enum DispatchJobsError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Store(#[from] DispatchError),
    #[error("dispatch error: {0}")]
    Drain(#[from] crate::dispatch::ExecutionFieldsError),
}

/// What a dispatch transaction driver did: the services outcome (which
/// carries its [`LogLine`]s) plus the post-commit drain results — the
/// [`CreationOutcome`][super::creation_jobs::CreationOutcome] shape.
#[derive(Debug)]
pub struct DispatchOutcome<T> {
    pub outcome: T,
    pub dispatched: Vec<Uuid>,
    pub dispatch_decisions: Vec<(Uuid, DispatchDecision)>,
    pub terminal_effects: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// DispatchSeam over the L6 store
// ---------------------------------------------------------------------------

fn db(error: sqlx::Error) -> DispatchError {
    DispatchError::Db(error.to_string())
}

impl<C, H, P> DispatchSeam for LiveCreationStore<'_, C, H, P>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    async fn lock_issue_for_retick(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<pidash_services::orchestration::creation::IssueView>, DispatchError> {
        let locked = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            issues::Entity::find_by_id(issue_id)
                .select_only()
                .column(issues::Column::Id)
                .lock_exclusive()
                .into_tuple::<Uuid>()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        match locked {
            None => Ok(None),
            Some(id) => Ok(Some(
                pidash_services::orchestration::creation::CreationSeam::issue(self, id).await?,
            )),
        }
    }

    async fn lock_ticker_for_issue(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, DispatchError> {
        let model = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            issue_agent_ticker::Entity::find()
                .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
                .filter(issue_agent_ticker::Column::DeletedAt.is_null())
                .order_by_asc(issue_agent_ticker::Column::Id)
                .lock_exclusive()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        Ok(model.map(IssueAgentTicker::from))
    }

    async fn ticker_for_issue(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, DispatchError> {
        let model = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            issue_agent_ticker::Entity::find()
                .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
                .filter(issue_agent_ticker::Column::DeletedAt.is_null())
                .order_by_asc(issue_agent_ticker::Column::Id)
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        Ok(model.map(IssueAgentTicker::from))
    }

    async fn lock_ticker_by_id(
        &mut self,
        ticker_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, DispatchError> {
        let model = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            issue_agent_ticker::Entity::find()
                .filter(issue_agent_ticker::Column::Id.eq(ticker_id))
                .filter(issue_agent_ticker::Column::DeletedAt.is_null())
                .order_by_asc(issue_agent_ticker::Column::Id)
                .lock_exclusive()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        Ok(model.map(IssueAgentTicker::from))
    }

    async fn lock_issue_by_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<pidash_services::orchestration::creation::IssueView>, DispatchError> {
        let locked = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            issues::Entity::find_by_id(issue_id)
                .select_only()
                .column(issues::Column::Id)
                .lock_exclusive()
                .into_tuple::<Uuid>()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        match locked {
            None => Ok(None),
            Some(id) => Ok(Some(
                pidash_services::orchestration::creation::CreationSeam::issue(self, id).await?,
            )),
        }
    }

    async fn save_clock(
        &mut self,
        ticker: &IssueAgentTicker,
        now: DateTime<Utc>,
    ) -> Result<(), DispatchError> {
        apply_clock_write(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            &ClockWrite::Update,
            ticker,
            now,
        )
        .await
        .map_err(db)?;
        Ok(())
    }

    async fn insert_ticker(&mut self, ticker: &IssueAgentTicker) -> Result<(), DispatchError> {
        // Insert ignores `now` (it stamps the row's own `created_at`,
        // the pre-consolidation bind); pass the row stamp so the call
        // reads honestly.
        apply_clock_write(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            &ClockWrite::Insert,
            ticker,
            ticker.created_at,
        )
        .await
        .map_err(db)?;
        Ok(())
    }

    async fn save_wait(
        &mut self,
        ticker: &IssueAgentTicker,
        now: DateTime<Utc>,
        rearmed: bool,
    ) -> Result<(), DispatchError> {
        let conn = TxConn::new(self.dispatch_tx().inner());
        if rearmed {
            issue_agent_ticker::Entity::update_many()
                .col_expr(
                    issue_agent_ticker::Column::Waited,
                    Expr::value(ticker.waited),
                )
                .col_expr(issue_agent_ticker::Column::UpdatedAt, Expr::value(now))
                .col_expr(issue_agent_ticker::Column::Enabled, Expr::value(true))
                .col_expr(
                    issue_agent_ticker::Column::DisarmReason,
                    Expr::value(String::new()),
                )
                .col_expr(
                    issue_agent_ticker::Column::NextRunAt,
                    Expr::value(ticker.next_run_at),
                )
                .filter(issue_agent_ticker::Column::Id.eq(ticker.id))
                .exec(&conn)
                .await
                .map(|_| ())
                .map_err(|error| db(into_sqlx(error)))?;
        } else {
            issue_agent_ticker::Entity::update_many()
                .col_expr(
                    issue_agent_ticker::Column::Waited,
                    Expr::value(ticker.waited),
                )
                .col_expr(issue_agent_ticker::Column::UpdatedAt, Expr::value(now))
                .filter(issue_agent_ticker::Column::Id.eq(ticker.id))
                .exec(&conn)
                .await
                .map(|_| ())
                .map_err(|error| db(into_sqlx(error)))?;
        }
        Ok(())
    }

    async fn backlog_target_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<pidash_services::orchestration::creation::StateView>, DispatchError> {
        let model = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            states::Entity::find()
                .select_only()
                .columns([
                    states::Column::Id,
                    states::Column::Name,
                    states::Column::Group,
                ])
                .filter(states::Column::ProjectId.eq(project_id))
                .filter(states::Column::Group.eq("backlog"))
                .filter(states::Column::DeletedAt.is_null())
                .order_by_desc(states::Column::Default)
                .order_by_asc(states::Column::Sequence)
                .limit(1)
                .into_tuple::<(Uuid, String, String)>()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        Ok(model.map(
            |(id, name, group)| pidash_services::orchestration::creation::StateView {
                id,
                name,
                group,
            },
        ))
    }

    async fn in_progress_state_for_project(
        &mut self,
        project_id: Uuid,
        group: &str,
        state_name: &str,
    ) -> Result<Option<pidash_services::orchestration::creation::StateView>, DispatchError> {
        let model = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            states::Entity::find()
                .select_only()
                .columns([
                    states::Column::Id,
                    states::Column::Name,
                    states::Column::Group,
                ])
                .filter(states::Column::ProjectId.eq(project_id))
                .filter(states::Column::Group.eq(group.to_owned()))
                .filter(states::Column::Name.eq(state_name.to_owned()))
                .filter(states::Column::DeletedAt.is_null())
                .order_by_asc(states::Column::Sequence)
                .limit(1)
                .into_tuple::<(Uuid, String, String)>()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        Ok(model.map(
            |(id, name, group)| pidash_services::orchestration::creation::StateView {
                id,
                name,
                group,
            },
        ))
    }

    async fn paused_state_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<pidash_services::orchestration::creation::StateView>, DispatchError> {
        let model = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            states::Entity::find()
                .select_only()
                .columns([
                    states::Column::Id,
                    states::Column::Name,
                    states::Column::Group,
                ])
                .filter(states::Column::ProjectId.eq(project_id))
                .filter(states::Column::Name.eq(PAUSED_STATE_NAME))
                .filter(states::Column::DeletedAt.is_null())
                .order_by_asc(states::Column::Sequence)
                .limit(1)
                .into_tuple::<(Uuid, String, String)>()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        Ok(model.map(
            |(id, name, group)| pidash_services::orchestration::creation::StateView {
                id,
                name,
                group,
            },
        ))
    }

    async fn default_state_id_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<Uuid>, DispatchError> {
        // A missing row AND a NULL column both mean "no fallback"
        // (`scheduling.py:1093-1096` reads `issue.project.default_state`,
        // None-tolerant).
        let row = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            projects::Entity::find_by_id(project_id)
                .select_only()
                .column(projects::Column::DefaultStateId)
                .into_tuple::<Option<Uuid>>()
                .one(&conn)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        Ok(row.flatten())
    }

    async fn clock_policy_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<pidash_services::orchestration::clock::ProjectClockPolicy, DispatchError> {
        let row = {
            let conn = TxConn::new(self.dispatch_tx().inner());
            clock_policy(&conn, project_id)
                .await
                .map_err(|error| db(into_sqlx(error)))?
        };
        let Some((ticking_enabled, max_ticks, interval, interval_review, interval_test)) = row
        else {
            return Err(db(sqlx::Error::RowNotFound));
        };
        Ok(pidash_services::orchestration::clock::ProjectClockPolicy {
            agent_ticking_enabled: Some(ticking_enabled),
            agent_default_max_ticks: Some(max_ticks),
            agent_default_interval_seconds: Some(i64::from(interval)),
            agent_review_default_interval_seconds: Some(i64::from(interval_review)),
            agent_test_default_interval_seconds: Some(i64::from(interval_test)),
        })
    }

    async fn update_issue_state(
        &mut self,
        issue_id: Uuid,
        state_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), DispatchError> {
        domain_issues::update_issue_state_no_audit(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            issue_id,
            state_id,
            now,
        )
        .await
        .map_err(db)?;
        Ok(())
    }

    async fn agent_system_user_id(&mut self) -> Result<Uuid, DispatchError> {
        // `get_agent_system_user` (`workpad.py:44-71`): find by reserved
        // username; create the service bot when missing; refuse when a
        // human holds the name.
        if let Some(found) = find_agent_user(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            AGENT_USERNAME,
        )
        .await
        .map_err(db)?
        {
            if !found.is_bot {
                return Err(AgentUserCollisionError.into());
            }
            return Ok(found.id);
        }
        let at = Utc::now();
        // Values are the Django field defaults (`db/models/user.py`):
        // blank CharFields land `""`, nullable ones NULL, `last_active`
        // evaluates `timezone.now`; the unusable hash lands right after
        // (`workpad.py:62-65`).
        let id = create_bot_user(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            &NewBotUser {
                id: None,
                username: AGENT_USERNAME,
                email: AGENT_USER_EMAIL,
                password_hash: "",
                display_name: AGENT_DISPLAY_NAME,
                first_name: AGENT_USER_FIRST_NAME,
                last_name: AGENT_USER_LAST_NAME,
                is_password_autoset: false,
                bot_type: None,
                now: at,
            },
        )
        .await
        .map_err(db)?;
        stamp_bot_unusable_password(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            id,
            &make_unusable_password(),
        )
        .await
        .map_err(db)?;
        Ok(id)
    }

    async fn insert_bounce_comment(
        &mut self,
        comment: &NewIssueComment,
    ) -> Result<(), DispatchError> {
        // The three writes of `IssueComment.save`, in order (any failure
        // propagates so the transaction rolls the move back). One `now()`
        // feeds all four stamps; the comment column carries the
        // MLStripper recompute the driver computed (`issue.py:606`,
        // PIDASHCONV-1581 item 4), while the description row carries
        // the `Description.save` recompute (Django's own `strip_tags`,
        // entities verbatim, `None`-when-empty — `description.py:23-29`).
        let empty_json = serde_json::json!({});
        let empty_list: Vec<String> = Vec::new();
        let desc_stripped = description_stripped_for(Some(comment.comment_html.as_str()));
        let rows = comment_domain::CommentRows {
            id: comment.id,
            description_id: comment.description_id,
            created_at: comment.now,
            updated_at: comment.now,
            desc_created_at: comment.now,
            desc_updated_at: comment.now,
            workspace_id: comment.workspace_id,
            project_id: comment.project_id,
            issue_id: comment.issue_id,
            actor_id: Some(comment.actor_id),
            created_by: None,
            updated_by: None,
            deleted_at: None,
            html: comment.comment_html.as_str(),
            json: &empty_json,
            stripped: comment.comment_stripped.as_str(),
            desc_stripped: desc_stripped.as_deref(),
            access: "INTERNAL",
            speaker_type: "agent",
            speaker_label: "",
            speaker_agent_run_id: None,
            external_id: None,
            external_source: None,
            parent_id: None,
            attachments: &empty_list,
            labels: &empty_list,
            edited_at: None,
        };
        comment_domain::insert_comment_row(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            &rows,
        )
        .await
        .map_err(db)?;
        comment_domain::insert_description_row(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            &rows,
        )
        .await
        .map_err(db)?;
        comment_domain::link_comment_description(
            &pidash_db::orm_tx::TxConn::new(self.dispatch_tx().inner()),
            rows.id,
            rows.description_id,
        )
        .await
        .map_err(db)?;
        Ok(())
    }

    async fn insert_wait_activity(
        &mut self,
        activity: &NewWaitActivity,
    ) -> Result<(), DispatchError> {
        use pidash_services::orchestration::dispatch::wait_activity_comment;
        let conn = TxConn::new(self.dispatch_tx().inner());
        issue_activities::ActiveModel {
            created_at: Set(activity.now.fixed_offset()),
            updated_at: Set(activity.now.fixed_offset()),
            id: Set(activity.id),
            verb: Set("updated".to_owned()),
            field: Set(Some("agent_wait".to_owned())),
            old_value: Set(Some(activity.pool.to_string())),
            new_value: Set(Some(activity.waited.to_string())),
            comment: Set(wait_activity_comment(
                activity.pool,
                activity.waited,
                activity.run_id.as_ref(),
            )),
            attachments: Set(Vec::new()),
            created_by_id: Set(activity.created_by),
            issue_id: Set(Some(activity.issue_id)),
            issue_comment_id: Set(None),
            project_id: Set(activity.project_id),
            updated_by_id: Set(None),
            workspace_id: Set(activity.workspace_id),
            actor_id: Set(Some(activity.actor_id)),
            new_identifier: Set(None),
            old_identifier: Set(None),
            epoch: Set(Some(activity.epoch_secs)),
            deleted_at: Set(None),
        }
        .insert(&conn)
        .await
        .map(|_| ())
        .map_err(|error| db(into_sqlx(error)))?;
        Ok(())
    }

    async fn live_assignee_candidates(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Vec<CandidateUser>, DispatchError> {
        let conn = TxConn::new(self.dispatch_tx().inner());
        // `users u WHERE u.id IN (SELECT assignee_id FROM
        // issue_assignees WHERE issue_id = $1 AND deleted_at IS NULL)
        // ORDER BY u.id ASC` as one statement: the assignee subselect
        // is the `issue_assignees` projection, the outer read is the
        // users projection ordered by id.
        let subquery = issue_assignees::Entity::find()
            .select_only()
            .column(issue_assignees::Column::AssigneeId)
            .filter(issue_assignees::Column::IssueId.eq(issue_id))
            .filter(issue_assignees::Column::DeletedAt.is_null())
            .into_query();
        let models = users::Entity::find()
            .select_only()
            .columns([
                users::Column::Id,
                users::Column::IsActive,
                users::Column::IsBot,
            ])
            .filter(Expr::col((users::Entity, users::Column::Id)).in_subquery(subquery))
            .order_by_asc(users::Column::Id)
            .into_tuple::<(Uuid, bool, bool)>()
            .all(&conn)
            .await
            .map_err(|error| db(into_sqlx(error)))?;
        Ok(models
            .into_iter()
            .map(|(id, is_active, is_bot)| CandidateUser {
                id,
                is_active,
                is_bot,
            })
            .collect())
    }

    async fn workspace_slug(&mut self, workspace_id: Uuid) -> Result<String, DispatchError> {
        let conn = TxConn::new(self.dispatch_tx().inner());
        let slug = workspaces::Entity::find_by_id(workspace_id)
            .select_only()
            .column(workspaces::Column::Slug)
            .into_tuple::<String>()
            .one(&conn)
            .await
            .map_err(|error| db(into_sqlx(error)))?
            .ok_or_else(|| db(sqlx::Error::RowNotFound))?;
        Ok(slug)
    }

    async fn project_role_facts(
        &mut self,
        user_id: Uuid,
        workspace_slug: &str,
        project_id: Uuid,
    ) -> Result<pidash_services::orchestration::dispatch::RoleVerdict, DispatchError> {
        // `check_project_role` EXISTS trio, each as its own
        // LIMIT-1 existence select over the same predicates as the
        // single combined statement (`pm.role IN (20, 15, 5)` /
        // any-role / `wm.role = 20`, each under the soft-delete
        // scope and the workspace-slug join). Module-private SeaORM
        // so the db `querysets` kernel (ORCH-3) stays untouched.
        let conn = TxConn::new(self.dispatch_tx().inner());
        let has_allowed_role = project_members::Entity::find()
            .join(
                JoinType::InnerJoin,
                project_members::Relation::Workspaces.def(),
            )
            .select_only()
            .column(project_members::Column::Id)
            .filter(project_members::Column::DeletedAt.is_null())
            .filter(project_members::Column::IsActive.eq(true))
            .filter(project_members::Column::MemberId.eq(user_id))
            .filter(project_members::Column::ProjectId.eq(project_id))
            .filter(project_members::Column::Role.is_in([20_i16, 15, 5]))
            .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
            .limit(1)
            .into_tuple::<Uuid>()
            .one(&conn)
            .await
            .map_err(|error| db(into_sqlx(error)))?
            .is_some();
        let conn = TxConn::new(self.dispatch_tx().inner());
        let is_project_member = project_members::Entity::find()
            .join(
                JoinType::InnerJoin,
                project_members::Relation::Workspaces.def(),
            )
            .select_only()
            .column(project_members::Column::Id)
            .filter(project_members::Column::DeletedAt.is_null())
            .filter(project_members::Column::IsActive.eq(true))
            .filter(project_members::Column::MemberId.eq(user_id))
            .filter(project_members::Column::ProjectId.eq(project_id))
            .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
            .limit(1)
            .into_tuple::<Uuid>()
            .one(&conn)
            .await
            .map_err(|error| db(into_sqlx(error)))?
            .is_some();
        let conn = TxConn::new(self.dispatch_tx().inner());
        let is_workspace_admin = workspace_members::Entity::find()
            .join(
                JoinType::InnerJoin,
                workspace_members::Relation::Workspaces.def(),
            )
            .select_only()
            .column(workspace_members::Column::Id)
            .filter(workspace_members::Column::DeletedAt.is_null())
            .filter(workspace_members::Column::IsActive.eq(true))
            .filter(workspace_members::Column::MemberId.eq(user_id))
            .filter(workspace_members::Column::Role.eq(20_i16))
            .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
            .limit(1)
            .into_tuple::<Uuid>()
            .one(&conn)
            .await
            .map_err(|error| db(into_sqlx(error)))?
            .is_some();
        Ok(pidash_services::orchestration::dispatch::RoleVerdict {
            has_allowed_role,
            is_project_member,
            is_workspace_admin,
        })
    }

    async fn enrolled_managed_exists(
        &mut self,
        project_id: Uuid,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<bool, DispatchError> {
        let conn = TxConn::new(self.dispatch_tx().inner());
        managed_runner_probe(
            &conn,
            ManagedRunnerProbe::Enrolled,
            user_id,
            project_id,
            workspace_id,
        )
        .await
        .map(|probe| probe.is_some())
        .map_err(|error| db(into_sqlx(error)))
    }

    fn managed_runner_enabled(&self) -> bool {
        self.dispatch_managed_runner_enabled()
    }

    fn has_usable_llm_config(&self, user_id: Uuid) -> bool {
        self.dispatch_has_usable_llm_config(user_id)
    }

    fn llm_profile_for(&self, user_id: Uuid) -> LlmProfile {
        self.dispatch_llm_profile_for(user_id)
    }
}

// ---------------------------------------------------------------------------
// Transaction drivers
// ---------------------------------------------------------------------------

/// Emit the collected [`LogLine`]s via `tracing` (INFO → `info!`,
/// WARNING → `warn!`), in Python order.
pub(crate) fn emit_logs(logs: &[LogLine]) {
    for line in logs {
        if line.level == "WARNING" {
            tracing::warn!("{}", line.message);
        } else {
            tracing::info!("{}", line.message);
        }
    }
}

/// Drain post-commit dispatch plus the deferred admission consumes —
/// the same collaborators as the L6 `drain` (a private helper this
/// module cannot name), over the same ordering.
pub(crate) async fn drain_dispatch<C: AdmissionCache>(
    pool: &PgPool,
    cloud: &pidash_db::config::CloudAgentSettings,
    cache: &C,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    now: DateTime<Utc>,
) -> Result<Vec<(Uuid, DispatchDecision)>, DispatchJobsError> {
    let mut decisions = Vec::with_capacity(dispatches.len());
    for run_id in dispatches {
        let decision = dispatch_agent_run(pool, cloud, run_id, now).await?;
        decisions.push((run_id, decision));
    }
    for consume in &deferred {
        consume_admission_token(cache, consume);
    }
    Ok(decisions)
}

/// Open the insertion transaction, run one services driver, commit
/// (or roll back when the outcome says so), then drain post-commit
/// dispatch — the L6 driver shape. `$rollback` reads the outcome's
/// rollback flag (`false` for the always-commit drivers).
macro_rules! dispatch_driver {
    ($name:ident, $svc:path, $out:ty, $rollback:expr $(, $arg:ident : $typ:ty)* $(,)?) => {
        #[doc = concat!("Open the insertion transaction, run [`", stringify!($svc), "`], commit (or roll back when the outcome says so), then drain post-commit dispatch. The caller emits the outcome's collected log lines via `tracing` (see [`emit_logs`]).")]
        #[allow(clippy::too_many_arguments)]
        pub async fn $name<C, H, P>(
            pool: &PgPool,
            deps: StoreDeps<C, H, P>,
            matcher: &dyn PodRunnerMatcher,
            now: DateTime<Utc>,
            jitter_secs: f64,
            $($arg : $typ),*
        ) -> Result<DispatchOutcome<$out>, DispatchJobsError>
        where
            C: AdmissionCache,
            H: Fn(Uuid) -> bool + Clone + Send + Sync,
            P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
        {
            let cloud = deps.cloud.clone();
            let tx = Transaction::begin(pool).await?;
            let mut store = LiveCreationStore::new(tx, pool, deps);
            let outcome: $out = $svc(&mut store, $($arg,)* matcher, now, jitter_secs).await?;
            let rollback: bool = $rollback(&outcome);
            let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
            if rollback {
                tx.rollback().await?;
                return Ok(DispatchOutcome {
                    outcome,
                    dispatched: Vec::new(),
                    dispatch_decisions: Vec::new(),
                    terminal_effects: Vec::new(),
                });
            }
            tx.commit().await?;
            let dispatch_decisions =
                drain_dispatch(pool, &cloud, &cache, dispatches, deferred, now).await?;
            let dispatched = dispatch_decisions.iter().map(|(id, _)| *id).collect();
            Ok(DispatchOutcome {
                outcome,
                dispatched,
                dispatch_decisions,
                terminal_effects,
            })
        }
    };
}

dispatch_driver!(
    dispatch_continuation_run,
    pidash_services::orchestration::dispatch::dispatch_continuation_run,
    ContinuationDispatchOutcome,
    |_: &ContinuationDispatchOutcome| false,
    issue_id: Uuid,
    triggered_by: &str,
    actor: Option<Uuid>,
);

dispatch_driver!(
    dispatch_run_ai_run_with_reason,
    pidash_services::orchestration::dispatch::dispatch_run_ai_run_with_reason,
    RunAiOutcome,
    |_: &RunAiOutcome| false,
    issue_id: Uuid,
    actor: Option<Uuid>,
);

dispatch_driver!(
    dispatch_run_ai_run,
    pidash_services::orchestration::dispatch::dispatch_run_ai_run,
    ThinRunAiOutcome,
    |_: &ThinRunAiOutcome| false,
    issue_id: Uuid,
    actor: Option<Uuid>,
);

/// Open the insertion transaction, run
/// [`bounce_issue_no_eligible_runner`][pidash_services::orchestration::dispatch::bounce_issue_no_eligible_runner],
/// commit. (Explicit rather than macro-generated: the bounce takes no
/// matcher.)
pub async fn bounce_issue_no_eligible_runner<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    now: DateTime<Utc>,
    jitter_secs: f64,
    issue_id: Uuid,
    triggered_by: &str,
    reason: &str,
) -> Result<DispatchOutcome<BounceOutcome>, DispatchJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome = pidash_services::orchestration::dispatch::bounce_issue_no_eligible_runner(
        &mut store,
        issue_id,
        triggered_by,
        reason,
        now,
        jitter_secs,
    )
    .await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions =
        drain_dispatch(pool, &cloud, &cache, dispatches, deferred, now).await?;
    let dispatched = dispatch_decisions.iter().map(|(id, _)| *id).collect();
    Ok(DispatchOutcome {
        outcome,
        dispatched,
        dispatch_decisions,
        terminal_effects,
    })
}

dispatch_driver!(
    preflight_eligibility_or_bounce,
    pidash_services::orchestration::dispatch::preflight_eligibility_or_bounce,
    PreflightOutcome,
    |_: &PreflightOutcome| false,
    issue_id: Uuid,
    run_creator: Option<Uuid>,
    pod_id: Uuid,
    triggered_by: &str,
);

dispatch_driver!(
    re_tick_ticker,
    pidash_services::orchestration::dispatch::re_tick_ticker,
    RetickOutcome,
    |outcome: &RetickOutcome| outcome.rollback,
    issue_id: Uuid,
    actor: Option<Uuid>,
);

/// Open the insertion transaction, run
/// [`run_ai_for_human`][pidash_services::orchestration::dispatch::run_ai_for_human],
/// commit — or roll the re-time back when nothing was created — then
/// drain post-commit dispatch. `created_by` is the crum user for the
/// ticker INSERT (the human caller).
#[allow(clippy::too_many_arguments)]
pub async fn run_ai_for_human<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    matcher: &dyn PodRunnerMatcher,
    now: DateTime<Utc>,
    jitter_secs: f64,
    issue_id: Uuid,
    actor: Option<Uuid>,
    created_by: Option<Uuid>,
) -> Result<DispatchOutcome<HumanRunAiOutcome>, DispatchJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome = pidash_services::orchestration::dispatch::run_ai_for_human(
        &mut store,
        issue_id,
        actor,
        matcher,
        now,
        jitter_secs,
        created_by,
    )
    .await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    if outcome.rollback {
        tx.rollback().await?;
        return Ok(DispatchOutcome {
            outcome,
            dispatched: Vec::new(),
            dispatch_decisions: Vec::new(),
            terminal_effects: Vec::new(),
        });
    }
    tx.commit().await?;
    let dispatch_decisions =
        drain_dispatch(pool, &cloud, &cache, dispatches, deferred, now).await?;
    let dispatched = dispatch_decisions.iter().map(|(id, _)| *id).collect();
    Ok(DispatchOutcome {
        outcome,
        dispatched,
        dispatch_decisions,
        terminal_effects,
    })
}

/// Open the insertion transaction, run
/// [`wait_ticker`][pidash_services::orchestration::dispatch::wait_ticker],
/// commit, then emit the agent-wait line. `epoch_secs` is `time.time()`
/// for the activity row; `created_by` is the crum user for it (the HTTP
/// caller).
#[allow(clippy::too_many_arguments)]
pub async fn wait_ticker<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    now: DateTime<Utc>,
    jitter_secs: f64,
    epoch_secs: f64,
    issue_id: Uuid,
    run_id: Option<Uuid>,
    actor: Option<Uuid>,
    created_by: Option<Uuid>,
) -> Result<DispatchOutcome<WaitOutcome>, DispatchJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome = pidash_services::orchestration::dispatch::wait_ticker(
        &mut store,
        issue_id,
        run_id,
        actor,
        now,
        jitter_secs,
        epoch_secs,
        created_by,
    )
    .await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions =
        drain_dispatch(pool, &cloud, &cache, dispatches, deferred, now).await?;
    let dispatched = dispatch_decisions.iter().map(|(id, _)| *id).collect();
    let outcome = DispatchOutcome {
        outcome,
        dispatched,
        dispatch_decisions,
        terminal_effects,
    };
    emit_logs(&outcome.outcome.logs);
    Ok(outcome)
}

/// Open the insertion transaction, run
/// [`maybe_apply_deferred_pause`][pidash_services::orchestration::dispatch::maybe_apply_deferred_pause],
/// commit.
pub async fn maybe_apply_deferred_pause<C, H, P>(
    pool: &PgPool,
    deps: StoreDeps<C, H, P>,
    now: DateTime<Utc>,
    jitter_secs: f64,
    run_id: Uuid,
) -> Result<DispatchOutcome<PauseOutcome>, DispatchJobsError>
where
    C: AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync,
{
    let cloud = deps.cloud.clone();
    let tx = Transaction::begin(pool).await?;
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let outcome = pidash_services::orchestration::dispatch::maybe_apply_deferred_pause(
        &mut store,
        run_id,
        now,
        jitter_secs,
    )
    .await?;
    let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
    tx.commit().await?;
    let dispatch_decisions =
        drain_dispatch(pool, &cloud, &cache, dispatches, deferred, now).await?;
    let dispatched = dispatch_decisions.iter().map(|(id, _)| *id).collect();
    Ok(DispatchOutcome {
        outcome,
        dispatched,
        dispatch_decisions,
        terminal_effects,
    })
}

// ---------------------------------------------------------------------------
// The FireTickSeam shim
// ---------------------------------------------------------------------------

/// The jobs-side [`FireTickSeam`] implementation over L1 (ticking
/// state, tick interval) + L2 (active/prior runs) + this unit
/// (dispatch). Sync trait methods take the state row the caller
/// resolved live in the claim transaction (PIDASHCONV-1218); async
/// methods read live SQL. `new_deps` builds a fresh [`StoreDeps`] per
/// dispatch (caches are call-scoped, like the L6 drivers); `matcher` is
/// the D-14 seam (scripted in tests, implemented by D-14 later).
pub struct FireTickShim<C, H, P> {
    pool: PgPool,
    new_deps: Arc<dyn Fn() -> StoreDeps<C, H, P> + Send + Sync>,
    matcher: Arc<dyn PodRunnerMatcher>,
}

/// The minimal state view the sync shim methods resolve.
#[derive(Debug, Clone)]
pub struct StateView {
    pub id: Uuid,
    pub name: String,
    pub group: String,
}

impl<C, H, P> FireTickShim<C, H, P> {
    pub fn new(
        pool: PgPool,
        new_deps: Arc<dyn Fn() -> StoreDeps<C, H, P> + Send + Sync>,
        matcher: Arc<dyn PodRunnerMatcher>,
    ) -> Self {
        FireTickShim {
            pool,
            new_deps,
            matcher,
        }
    }
}

/// Tick-interval mapping shared by the shim: L1 cadence column → the
/// project row value → [`resolve_project_interval`][pidash_db::tasks_ticker::resolve_project_interval]
/// default. `state` borrows the resolved row.
pub fn shim_tick_interval_seconds(state: Option<&StateView>, project: &ProjectTickCols) -> i64 {
    let state_ref = state.map(|s| StateRef {
        group: s.group.as_str(),
        name: s.name.as_str(),
    });
    let fields = cadence_fields_for(state_ref.as_ref());
    let value: Option<i32> = match fields.project_interval {
        "agent_default_interval_seconds" => project.interval_default,
        "agent_review_default_interval_seconds" => project.interval_review,
        "agent_test_default_interval_seconds" => project.interval_test,
        _ => None,
    };
    // The cadence columns decode at INT4 width; widen to i64 for the
    // pure resolver.
    pidash_db::tasks_ticker::resolve_project_interval(value.map(i64::from))
}

/// Ticking-state verdict shared by the shim.
pub fn shim_is_ticking_state(state: Option<&StateView>) -> bool {
    let state_ref = state.map(|s| StateRef {
        group: s.group.as_str(),
        name: s.name.as_str(),
    });
    is_ticking_state(state_ref.as_ref())
}

impl<C, H, P> FireTickSeam for FireTickShim<C, H, P>
where
    C: AdmissionCache + Send + Sync + 'static,
    H: Fn(Uuid) -> bool + Clone + Send + Sync + 'static,
    P: Fn(Option<Uuid>) -> LlmProfile + Clone + Send + Sync + 'static,
{
    fn is_ticking_state(&self, state: Option<&StateView>) -> bool {
        shim_is_ticking_state(state)
    }

    fn tick_interval_seconds(&self, state: Option<&StateView>, project: &ProjectTickCols) -> i64 {
        shim_tick_interval_seconds(state, project)
    }

    fn has_active_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let conn = sea_orm_connection(&pool);
            let statuses: Vec<String> = ACTIVE_STATUSES
                .iter()
                .map(|status| status.value().to_string())
                .collect();
            let found = agent_run::Entity::find()
                .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
                .filter(agent_run::Column::Status.is_in(statuses))
                .select_only()
                .column(agent_run::Column::Id)
                .limit(1)
                .into_tuple::<Uuid>()
                .one(&conn)
                .await
                .map_err(into_sqlx)?;
            Ok(found.is_some())
        })
    }

    fn has_prior_run(
        &self,
        issue_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        Box::pin(async move {
            let conn = sea_orm_connection(&pool);
            let found = agent_run::Entity::find()
                .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
                .select_only()
                .column(agent_run::Column::Id)
                .limit(1)
                .into_tuple::<Uuid>()
                .one(&conn)
                .await
                .map_err(into_sqlx)?;
            Ok(found.is_some())
        })
    }

    fn dispatch_continuation_run(
        &self,
        issue_id: Uuid,
        triggered_by: &str,
        actor: Option<Uuid>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Uuid>, sqlx::Error>> + Send + '_>> {
        let pool = self.pool.clone();
        let deps = (self.new_deps)();
        let matcher = self.matcher.clone();
        let triggered_by = triggered_by.to_owned();
        Box::pin(async move {
            let now = Utc::now();
            // Continuation never retimes a clock (a bounce only
            // disarms), so the jitter draw is unread — 0.0.
            let outcome = dispatch_continuation_run(
                &pool,
                deps,
                matcher.as_ref(),
                now,
                0.0,
                issue_id,
                &triggered_by,
                actor,
            )
            .await
            .map_err(|err| match err {
                DispatchJobsError::Db(db) => db,
                other => sqlx::Error::Protocol(other.to_string()),
            })?;
            emit_logs(&outcome.outcome.logs);
            Ok(outcome.outcome.run_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
    use pidash_services::extensions::NoExtraToolsets;
    use pidash_services::orchestration::dispatch::BOUNCE_BODY_DEFAULT;
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // -- offline -----------------------------------------------------------

    #[test]
    fn shim_ticking_vectors() {
        let started = StateView {
            id: Uuid::nil(),
            name: "In Progress".to_owned(),
            group: "started".to_owned(),
        };
        assert!(shim_is_ticking_state(Some(&started)));
        let review = StateView {
            id: Uuid::nil(),
            name: "In Review".to_owned(),
            group: "review".to_owned(),
        };
        assert!(shim_is_ticking_state(Some(&review)));
        let done = StateView {
            id: Uuid::nil(),
            name: "Done".to_owned(),
            group: "completed".to_owned(),
        };
        assert!(!shim_is_ticking_state(Some(&done)));
        let paused = StateView {
            id: Uuid::nil(),
            name: "Paused".to_owned(),
            group: "backlog".to_owned(),
        };
        assert!(!shim_is_ticking_state(Some(&paused)));
        assert!(!shim_is_ticking_state(None));
    }

    #[test]
    fn dispatch_statements_pin_shape() {
        use sea_orm::{DbBackend, QueryTrait};
        // The Retick/pause issue locks: pk + FOR UPDATE.
        let issue_id = Uuid::from_u128(1);
        let lock = issues::Entity::find_by_id(issue_id)
            .select_only()
            .column(issues::Column::Id)
            .lock_exclusive()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(lock.contains("\"issues\""), "{lock}");
        assert!(lock.contains("FOR UPDATE"), "{lock}");
        // The ticker lock: soft-delete scope + id asc + FOR UPDATE.
        let ticker_lock = issue_agent_ticker::Entity::find()
            .filter(issue_agent_ticker::Column::IssueId.eq(issue_id))
            .filter(issue_agent_ticker::Column::DeletedAt.is_null())
            .order_by_asc(issue_agent_ticker::Column::Id)
            .lock_exclusive()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            ticker_lock.contains("\"issue_agent_ticker\""),
            "{ticker_lock}"
        );
        assert!(
            ticker_lock.contains("\"deleted_at\" IS NULL"),
            "{ticker_lock}"
        );
        assert!(ticker_lock.contains("ORDER BY"), "{ticker_lock}");
        assert!(ticker_lock.contains("FOR UPDATE"), "{ticker_lock}");
        // The Backlog target: default DESC, sequence ASC, LIMIT 1.
        let backlog = states::Entity::find()
            .select_only()
            .columns([
                states::Column::Id,
                states::Column::Name,
                states::Column::Group,
            ])
            .filter(states::Column::ProjectId.eq(issue_id))
            .filter(states::Column::Group.eq("backlog"))
            .filter(states::Column::DeletedAt.is_null())
            .order_by_desc(states::Column::Default)
            .order_by_asc(states::Column::Sequence)
            .limit(1)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(backlog.contains("\"states\""), "{backlog}");
        assert!(backlog.contains("ORDER BY"), "{backlog}");
        assert!(backlog.contains("LIMIT"), "{backlog}");
        // The live-assignee subquery: IN (SELECT assignee_id ...).
        let assignees = issue_assignees::Entity::find()
            .select_only()
            .column(issue_assignees::Column::AssigneeId)
            .filter(issue_assignees::Column::IssueId.eq(issue_id))
            .filter(issue_assignees::Column::DeletedAt.is_null())
            .into_query();
        let users_sql = users::Entity::find()
            .select_only()
            .columns([
                users::Column::Id,
                users::Column::IsActive,
                users::Column::IsBot,
            ])
            .filter(Expr::col((users::Entity, users::Column::Id)).in_subquery(assignees))
            .order_by_asc(users::Column::Id)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(users_sql.contains("\"users\""), "{users_sql}");
        assert!(users_sql.contains("IN (SELECT"), "{users_sql}");
        assert!(users_sql.contains("\"assignee_id\""), "{users_sql}");
        // The wait update: waited/updated_at by id.
        let wait_update = issue_agent_ticker::Entity::update_many()
            .col_expr(issue_agent_ticker::Column::Waited, Expr::value(1))
            .col_expr(issue_agent_ticker::Column::UpdatedAt, Expr::value(now()))
            .filter(issue_agent_ticker::Column::Id.eq(issue_id))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            wait_update.contains("UPDATE \"issue_agent_ticker\""),
            "{wait_update}"
        );
        assert!(wait_update.contains("\"waited\""), "{wait_update}");
        // The shim exists probes: work_item + status IN, LIMIT 1.
        let statuses: Vec<String> = ACTIVE_STATUSES
            .iter()
            .map(|s| s.value().to_string())
            .collect();
        let active = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
            .filter(agent_run::Column::Status.is_in(statuses.clone()))
            .select_only()
            .column(agent_run::Column::Id)
            .limit(1)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(active.contains("\"agent_run\""), "{active}");
        assert!(active.contains("\"work_item_id\""), "{active}");
        assert!(active.contains("\"status\" IN"), "{active}");
        let prior = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(Some(issue_id)))
            .select_only()
            .column(agent_run::Column::Id)
            .limit(1)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(!prior.contains("\"status\" IN"), "{prior}");
        // The role-facts trio: INNER JOIN workspaces, role IN (20,15,5),
        // is_active, deleted_at scope, slug filter.
        let role = project_members::Entity::find()
            .join(
                sea_orm::JoinType::InnerJoin,
                project_members::Relation::Workspaces.def(),
            )
            .select_only()
            .column(project_members::Column::Id)
            .filter(project_members::Column::DeletedAt.is_null())
            .filter(project_members::Column::IsActive.eq(true))
            .filter(project_members::Column::MemberId.eq(issue_id))
            .filter(project_members::Column::ProjectId.eq(issue_id))
            .filter(project_members::Column::Role.is_in([20_i16, 15, 5]))
            .filter(workspaces::Column::Slug.eq("ws".to_owned()))
            .limit(1)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(role.contains("INNER JOIN"), "{role}");
        assert!(role.contains("\"workspaces\""), "{role}");
        assert!(role.contains("\"role\" IN"), "{role}");
        assert!(role.contains("\"slug\""), "{role}");
        assert!(role.contains("\"is_active\""), "{role}");
        assert!(role.contains("\"deleted_at\" IS NULL"), "{role}");
        assert!(role.contains("LIMIT"), "{role}");
    }

    #[test]
    fn shim_interval_vectors() {
        let project = ProjectTickCols {
            pool: Some(10),
            ticking_enabled: Some(true),
            interval_default: Some(10800),
            interval_review: Some(7200),
            interval_test: Some(3600),
        };
        let started = StateView {
            id: Uuid::nil(),
            name: "In Progress".to_owned(),
            group: "started".to_owned(),
        };
        assert_eq!(shim_tick_interval_seconds(Some(&started), &project), 10800);
        let review = StateView {
            id: Uuid::nil(),
            name: "In Review".to_owned(),
            group: "review".to_owned(),
        };
        assert_eq!(shim_tick_interval_seconds(Some(&review), &project), 7200);
        let test = StateView {
            id: Uuid::nil(),
            name: "In Test".to_owned(),
            group: "test".to_owned(),
        };
        assert_eq!(shim_tick_interval_seconds(Some(&test), &project), 3600);
        // Missing columns fall back to the registry default (L1).
        let empty = ProjectTickCols {
            pool: None,
            ticking_enabled: None,
            interval_default: None,
            interval_review: None,
            interval_test: None,
        };
        assert_eq!(
            shim_tick_interval_seconds(Some(&started), &empty),
            pidash_db::tasks_ticker::DEFAULT_INTERVAL_SECONDS
        );
        assert_eq!(
            shim_tick_interval_seconds(None, &project),
            10800,
            "unknown state reads the implementation column"
        );
    }

    // -- live harness ------------------------------------------------------

    async fn pool() -> PgPool {
        let url =
            std::env::var("DATABASE_URL").expect("export DATABASE_URL for live dispatch tests");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect scratch database");
        let db: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&pool)
            .await
            .expect("current database");
        assert_ne!(
            db, "postgres",
            "never test against the shared postgres database"
        );
        crate::queue::ensure_schema(&pool)
            .await
            .expect("queue schema");
        pool
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
            .expect("frozen clock")
            .with_timezone(&Utc)
    }

    /// The fixture generator's jitter draw (`random.seed(52408)` →
    /// `random.uniform(0, 1080)`): re-times land on `15:14:06.322254`,
    /// like R3b/R4/W5.
    const JITTER: f64 = 846.3222537664747;

    fn django_cloud_settings() -> CloudAgentSettings {
        CloudAgentSettings {
            enabled: false,
            writes_enabled: false,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            reconcile_interval_secs: 30,
            model_request_timeout_secs: 60,
            execution_timeout_secs: 285,
            run_soft_limit_secs: 300,
            run_hard_limit_secs: 330,
            stale_grace_secs: 60,
            dispatch_lease_secs: 60,
            dispatch_backoff_secs: 10,
            dispatch_scan_interval_secs: 10,
            sweep_interval_secs: 30,
            dispatch_scan_batch: 100,
            max_queue_age_secs: 900,
            model_request_limit: 25,
            tool_call_limit: 20,
            write_call_limit: 3,
            input_token_limit: 144_000,
            output_token_limit: 16_000,
            total_token_limit: 160_000,
            max_output_tokens_per_request: 4096,
            max_queued_per_workspace: 20,
            max_running_per_workspace: 2,
            user_creation_rate_per_minute: 6,
            workspace_creation_rate_per_minute: 30,
            tool_timeout_secs: 20,
            max_tool_result_bytes: 65536,
            max_prompt_bytes: 262_144,
            max_final_result_bytes: 65536,
            max_events: 500,
            block_private_urls: true,
        }
    }

    fn django_managed_settings() -> ManagedRunnerSettings {
        ManagedRunnerSettings {
            enabled: false,
            queued_max_age_secs: 43200,
            graceful_stop_secs: 30,
            sweep_interval_secs: 300,
            desktop_min_version: String::new(),
        }
    }

    #[derive(Debug)]
    struct CacheError(String);

    impl std::fmt::Display for CacheError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "cache error: {}", self.0)
        }
    }

    impl std::error::Error for CacheError {}

    /// Send-safe scripted cache (no `RefCell` — the shim's dispatch
    /// future is `Send`-boxed).
    #[derive(Default)]
    struct FakeCache {
        counts: Mutex<HashMap<String, i64>>,
    }

    impl AdmissionCache for FakeCache {
        type Error = CacheError;

        fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
            Ok(self.counts.lock().expect("cache lock").get(key).copied())
        }

        fn add_or_incr(&self, key: &str, _timeout_secs: i64) -> Result<(), Self::Error> {
            *self
                .counts
                .lock()
                .expect("cache lock")
                .entry(key.to_owned())
                .or_insert(0) += 1;
            Ok(())
        }
    }

    fn no_llm(_user: Uuid) -> bool {
        false
    }

    fn llm_ok(_user: Uuid) -> bool {
        true
    }

    fn unavailable_profile(_user: Option<Uuid>) -> LlmProfile {
        LlmProfile {
            available: false,
            reason_code: "fx8-no-profile".to_owned(),
        }
    }

    type TestDeps = StoreDeps<FakeCache, fn(Uuid) -> bool, fn(Option<Uuid>) -> LlmProfile>;

    fn deps() -> TestDeps {
        StoreDeps {
            cloud: django_cloud_settings(),
            managed: django_managed_settings(),
            cache: FakeCache::default(),
            has_usable_llm_config: no_llm,
            toolsets: Arc::new(NoExtraToolsets),
            llm_profile: unavailable_profile,
            now_unix_secs: now().timestamp(),
        }
    }

    /// Scripted [`PodRunnerMatcher`] (D-14 implements the real one later).
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

    struct Graph {
        ws: Uuid,
        user: Uuid,
        project: Uuid,
        pod: Uuid,
        states: HashMap<String, Uuid>,
        issue: Uuid,
    }

    async fn seed_user(pool: &PgPool, tag: &str) -> Uuid {
        let user = Uuid::new_v4();
        let at = now();
        // Random suffix: rerun-safe alongside parallel-safe.
        let tag = format!("{tag}-{}", &Uuid::new_v4().to_string()[..8]);
        sqlx::query(
            "INSERT INTO users (id, username, email, password, first_name, last_name, \
             display_name, avatar, created_location, last_location, last_login_ip, \
             last_login_medium, last_login_uagent, last_logout_ip, user_timezone, token, \
             is_active, is_bot, is_email_valid, is_email_verified, is_managed, \
             is_password_autoset, is_password_expired, is_password_reset_required, \
             is_staff, is_superuser, created_at, updated_at, date_joined) \
             VALUES ($1, $2, $3, '!', '', '', '', '', '', '', '', 'email', '', '', 'UTC', \
             $4, true, false, false, false, false, false, false, false, false, false, \
             $5, $5, $5)",
        )
        .bind(user)
        .bind(format!("fx8-{tag}"))
        .bind(format!("fx8-{tag}@example.com"))
        .bind(Uuid::new_v4().to_string())
        .bind(at)
        .execute(pool)
        .await
        .expect("seed user");
        user
    }

    /// Seed the fx8 graph: workspace + local-runner project (lead +
    /// assignee) + default pod + BacklogHi/BacklogLo/Todo/In
    /// Progress/In Review/In Test/Done/Paused + one `FX8` issue on In
    /// Progress with the pod assigned. Ids are tag-unique
    /// (parallel-safe).
    async fn seed_graph(pool: &PgPool, tag: &str) -> Graph {
        let at = now();
        let user = seed_user(pool, &format!("creator-{tag}")).await;
        let lead = seed_user(pool, &format!("lead-{tag}")).await;
        let ws = Uuid::new_v4();
        let uniq = format!("{tag}-{}", &Uuid::new_v4().to_string()[..8]);
        let ws_slug = format!("fx8-ws-{uniq}");
        let project = Uuid::new_v4();
        let pod = Uuid::new_v4();
        let issue = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, background_color, \
             owner_id, slug, timezone) \
             VALUES ($1, $2, $2, $3, '#f6c8dB', $4, $5, 'UTC')",
        )
        .bind(ws)
        .bind(at)
        .bind("FX8 ws")
        .bind(user)
        .bind(&ws_slug)
        .execute(pool)
        .await
        .expect("seed workspace");
        sqlx::query(
            "INSERT INTO projects (id, created_at, updated_at, name, identifier, description, \
             network, module_view, cycle_view, issue_views_view, page_view, intake_view, \
             is_time_tracking_enabled, is_issue_type_enabled, is_default, \
             guest_view_all_features, members_can_edit_states, archive_in, close_in, \
             logo_props, timezone, repo_url, base_branch, agent_default_interval_seconds, \
             agent_default_max_ticks, agent_review_default_interval_seconds, \
             agent_test_default_interval_seconds, agent_ticking_enabled, \
             default_agent_executor, workspace_id, project_lead_id, default_assignee_id) \
             VALUES ($1, $2, $2, 'FX8', 'FX8', '', 2, false, false, false, true, false, false, \
             false, false, false, true, 0, 0, '{}', 'UTC', 'https://example.com/fx8.git', \
             'main', 10800, 10, 10800, 10800, true, 'local_runner', $3, $4, $5)",
        )
        .bind(project)
        .bind(at)
        .bind(ws)
        .bind(lead)
        .bind(user)
        .execute(pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO pod (id, created_at, updated_at, description, is_default, name, \
             project_id, workspace_id) \
             VALUES ($1, $2, $2, '', true, $3, $4, $5)",
        )
        .bind(pod)
        .bind(at)
        .bind(format!("fx8-pod-{uniq}"))
        .bind(project)
        .bind(ws)
        .execute(pool)
        .await
        .expect("seed pod");
        let mut states = HashMap::new();
        for (index, (name, group, default)) in [
            ("Todo", "unstarted", false),
            ("BacklogLo", "backlog", false),
            ("BacklogHi", "backlog", true),
            ("In Progress", "started", false),
            ("In Review", "review", false),
            ("In Test", "test", false),
            ("Done", "completed", false),
            ("Paused", "backlog", false),
        ]
        .into_iter()
        .enumerate()
        {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
                 project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
                 VALUES ($1, $2, $2, $3, '', '', $4, $5, $6, $7, $8, $9, false)",
            )
            .bind(id)
            .bind(at)
            .bind(name)
            .bind(format!(
                "fx8-{uniq}-{}-{index}",
                name.to_lowercase().replace(' ', "-")
            ))
            .bind(project)
            .bind(ws)
            .bind((index as f64) * 100.0)
            .bind(group)
            .bind(default)
            .execute(pool)
            .await
            .expect("seed state");
            states.insert(name.to_owned(), id);
        }
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority, \
             sequence_id, created_by_id, project_id, state_id, workspace_id, description_html, \
             description_stripped, sort_order, is_draft, git_work_branch, workpad, \
             complexity_score, assigned_pod_id) \
             VALUES ($1, $2, $2, 'FX8 dispatch', '{}', 'none', 1, $3, $4, $5, $6, '', NULL, 0.0, \
             false, '', '', 0, $7)",
        )
        .bind(issue)
        .bind(at)
        .bind(user)
        .bind(project)
        .bind(states["In Progress"])
        .bind(ws)
        .bind(pod)
        .execute(pool)
        .await
        .expect("seed issue");
        Graph {
            ws,
            user,
            project,
            pod,
            states,
            issue,
        }
    }

    #[derive(Debug, Clone)]
    struct TickerSeed {
        used: i32,
        granted: i32,
        waited: i32,
        user_disabled: bool,
        enabled: bool,
        disarm_reason: String,
        next_run_at: Option<DateTime<Utc>>,
        resume_parent_run_id: Option<Uuid>,
    }

    impl Default for TickerSeed {
        fn default() -> Self {
            TickerSeed {
                used: 3,
                granted: 0,
                waited: 0,
                user_disabled: false,
                enabled: true,
                disarm_reason: String::new(),
                next_run_at: Some(now()),
                resume_parent_run_id: None,
            }
        }
    }

    async fn seed_ticker(pool: &PgPool, issue: Uuid, seed: &TickerSeed) -> Uuid {
        let id = Uuid::new_v4();
        let at = now();
        sqlx::query(
            "INSERT INTO issue_agent_ticker (id, created_at, updated_at, issue_id, used, \
             granted, waited, user_disabled, next_run_at, enabled, disarm_reason, \
             pending_entry, pending_entry_free, pending_entry_trigger, resume_parent_run_id) \
             VALUES ($1, $2, $2, $3, $4, $5, $6, $7, $8, $9, $10, false, false, '', $11)",
        )
        .bind(id)
        .bind(at)
        .bind(issue)
        .bind(seed.used)
        .bind(seed.granted)
        .bind(seed.waited)
        .bind(seed.user_disabled)
        .bind(seed.next_run_at)
        .bind(seed.enabled)
        .bind(seed.disarm_reason.as_str())
        .bind(seed.resume_parent_run_id)
        .execute(pool)
        .await
        .expect("seed ticker");
        id
    }

    /// Seed one run with Django-side defaults; the creation-touched
    /// columns vary.
    #[allow(clippy::too_many_arguments)]
    async fn seed_run(
        pool: &PgPool,
        graph: &Graph,
        status: &str,
        trigger: &str,
        executor: &str,
        phase_kind: &str,
        parent: Option<Uuid>,
        run_config: Value,
        created_by: Uuid,
        created_at: DateTime<Utc>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_run (id, workspace_id, created_by_id, pod_id, runner_id, \
             pinned_runner_id, parent_run_id, work_item_id, status, executor_kind, \
             dispatch_attempts, cancel_reason, tool_plan, prompt, trigger, \
             phase_kind, run_config, required_capabilities, thread_id, agent_metadata, \
             error_details, llm_model, usage, created_at) \
             VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, 0, '', '{}', '', $9, \
             $10, $11, '[]', '', '{}', '{}', '', '{}', $12)",
        )
        .bind(id)
        .bind(graph.ws)
        .bind(created_by)
        .bind(graph.pod)
        .bind(parent)
        .bind(graph.issue)
        .bind(status)
        .bind(executor)
        .bind(trigger)
        .bind(phase_kind)
        .bind(run_config)
        .bind(created_at)
        .execute(pool)
        .await
        .expect("seed run");
        id
    }

    async fn ticker_json(pool: &PgPool, issue: Uuid) -> Value {
        sqlx::query_scalar(
            "SELECT to_jsonb(t) - 'created_at' - 'updated_at' - 'id' - 'issue_id' \
             - 'created_by_id' - 'updated_by_id' - 'deleted_at' - 'last_tick_at' \
             - 'pending_entry_actor_id' \
             FROM issue_agent_ticker t WHERE issue_id = $1 AND deleted_at IS NULL",
        )
        .bind(issue)
        .fetch_optional(pool)
        .await
        .expect("read ticker")
        .unwrap_or(Value::Null)
    }

    async fn issue_state(pool: &PgPool, issue: Uuid) -> Uuid {
        sqlx::query_scalar("SELECT state_id FROM issues WHERE id = $1")
            .bind(issue)
            .fetch_one(pool)
            .await
            .expect("read issue state")
    }

    /// Serialize the live dispatch tests: the agent username is
    /// global, and the collision test squats it mid-run — every live
    /// test holds this guard for its whole body (the ignored suite is
    /// slow either way). Callers hold the guard; [`ensure_agent_user`]
    /// assumes it.
    static LIVE_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn ensure_agent_user(pool: &PgPool) -> Uuid {
        let found: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM users WHERE username = 'pi_dash_agent'")
                .fetch_optional(pool)
                .await
                .expect("find agent user");
        if let Some(id) = found {
            return id;
        }
        // Create through the real seam path (empty dispatch would do
        // the same): insert + unusable password via the driver SQL.
        let tx = Transaction::begin(pool).await.expect("begin agent user tx");
        let mut store = LiveCreationStore::new(tx, pool, deps());
        let id = pidash_services::orchestration::dispatch::DispatchSeam::agent_system_user_id(
            &mut store,
        )
        .await
        .expect("create agent user");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.commit().await.expect("commit agent user");
        id
    }

    async fn run_cols(pool: &PgPool, run: Uuid) -> (String, String, Option<Uuid>, Uuid, Uuid) {
        sqlx::query_as(
            "SELECT status, trigger, parent_run_id, created_by_id, pod_id FROM agent_run WHERE id = $1",
        )
        .bind(run)
        .fetch_one(pool)
        .await
        .expect("read run")
    }

    // -- live: continuation (A3/A4/A5/A9) ------------------------------------

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_a3_same_stage_continuation() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "a3").await;
        let system = ensure_agent_user(&pool).await;
        let prior = seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({"tick": 1}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_continuation_run(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            "tick",
            None,
        )
        .await
        .expect("dispatch");
        let run = out.outcome.run_id.expect("created");
        assert!(out.outcome.logs.is_empty());
        assert_eq!(out.dispatched, vec![run]);
        let (status, trigger, parent, created_by, pod) = run_cols(&pool, run).await;
        assert_eq!(status, "queued");
        assert_eq!(trigger, "tick");
        assert_eq!(parent, Some(prior));
        assert_eq!(created_by, system);
        assert_eq!(pod, graph.pod);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_a4_cross_stage_fresh() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "a4").await;
        ensure_agent_user(&pool).await;
        seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({"tick": 1}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        sqlx::query("UPDATE issues SET state_id = $1 WHERE id = $2")
            .bind(graph.states["In Review"])
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("move to review");
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_continuation_run(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            "tick",
            None,
        )
        .await
        .expect("dispatch");
        let run = out.outcome.run_id.expect("created");
        let (status, trigger, parent, _, _) = run_cols(&pool, run).await;
        assert_eq!(status, "queued");
        assert_eq!(trigger, "tick");
        assert_eq!(parent, None);
        assert_eq!(out.dispatched, vec![run]);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_a5_handback_resume_parent() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "a5").await;
        ensure_agent_user(&pool).await;
        let resume = seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(2),
        )
        .await;
        // Latest run is cross-stage (review kind) so the hand-back leg
        // engages; the ticker pins the resume parent.
        seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "review-response",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        seed_ticker(
            &pool,
            graph.issue,
            &TickerSeed {
                resume_parent_run_id: Some(resume),
                ..TickerSeed::default()
            },
        )
        .await;
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_continuation_run(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            "tick",
            None,
        )
        .await
        .expect("dispatch");
        let run = out.outcome.run_id.expect("created");
        let (_, _, parent, _, _) = run_cols(&pool, run).await;
        assert_eq!(parent, Some(resume));
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_a9_preflight_bounce_commits() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "a9").await;
        ensure_agent_user(&pool).await;
        seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        seed_ticker(&pool, graph.issue, &TickerSeed::default()).await;
        let matcher = FakeMatcher { answer: false };
        let out = dispatch_continuation_run(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            "tick",
            None,
        )
        .await
        .expect("dispatch");
        assert_eq!(out.outcome.run_id, None);
        assert!(out.dispatched.is_empty());
        assert_eq!(
            issue_state(&pool, graph.issue).await,
            graph.states["BacklogHi"]
        );
        let comments: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM issue_comments WHERE issue_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count comments");
        assert_eq!(comments, 1);
        assert!(out.outcome.logs[0]
            .message
            .contains("reason=no-eligible-runner"));
    }

    // -- live: run-ai (H5/H6/H7b/H8/H9) --------------------------------------

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_h5_prior_continuation_run_ai() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "h5").await;
        ensure_agent_user(&pool).await;
        let prior = seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({"tick": 1}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_run_ai_run_with_reason(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("dispatch");
        let run = out.outcome.run_id.expect("created");
        assert_eq!(out.outcome.reason, None);
        let (status, trigger, parent, created_by, _) = run_cols(&pool, run).await;
        assert_eq!(status, "queued");
        assert_eq!(trigger, "run_ai");
        assert_eq!(parent, Some(prior));
        assert_eq!(created_by, graph.user);
        assert_eq!(out.dispatched, vec![run]);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_h6_no_prior_fresh_run_ai() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "h6").await;
        ensure_agent_user(&pool).await;
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_run_ai_run_with_reason(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("dispatch");
        let run = out.outcome.run_id.expect("created");
        let (_, trigger, parent, _, _) = run_cols(&pool, run).await;
        assert_eq!(trigger, "run_ai");
        assert_eq!(parent, None);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_h7b_wrapper_created() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "h7b").await;
        ensure_agent_user(&pool).await;
        let matcher = FakeMatcher { answer: true };
        let out = dispatch_run_ai_run(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("dispatch");
        assert!(out.outcome.run_id.is_some());
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_h8_human_created_ticker() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "h8").await;
        ensure_agent_user(&pool).await;
        let prior = seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({"tick": 1}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        let matcher = FakeMatcher { answer: true };
        let out = run_ai_for_human(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
            Some(graph.user),
        )
        .await
        .expect("dispatch");
        let run = out.outcome.run_id.expect("created");
        assert_eq!(out.outcome.reason, None);
        assert!(!out.outcome.rollback);
        let (_, trigger, parent, created_by, _) = run_cols(&pool, run).await;
        assert_eq!(trigger, "run_ai");
        assert_eq!(parent, Some(prior));
        assert_eq!(created_by, graph.user);
        // The ticker was created by the free human run (H8 golden).
        let ticker = ticker_json(&pool, graph.issue).await;
        assert_eq!(ticker["used"], json!(0));
        assert_eq!(ticker["granted"], json!(0));
        assert_eq!(ticker["waited"], json!(0));
        assert_eq!(ticker["enabled"], json!(true));
        assert_eq!(ticker["disarm_reason"], json!(""));
        let expected =
            pidash_services::orchestration::clock::compute_next_run_at(10800, now(), JITTER);
        assert_eq!(
            ticker["next_run_at"],
            json!(expected.to_rfc3339_opts(chrono::SecondsFormat::Micros, false))
        );
        let created_by_ticker: Option<Uuid> =
            sqlx::query_scalar("SELECT created_by_id FROM issue_agent_ticker WHERE issue_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("ticker creator");
        assert_eq!(created_by_ticker, Some(graph.user));
        assert_eq!(out.dispatched, vec![run]);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_h9_human_refused_rollback() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "h9").await;
        ensure_agent_user(&pool).await;
        seed_run(
            &pool,
            &graph,
            "running",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        seed_ticker(&pool, graph.issue, &TickerSeed::default()).await;
        let before = ticker_json(&pool, graph.issue).await;
        let matcher = FakeMatcher { answer: true };
        let out = run_ai_for_human(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
            Some(graph.user),
        )
        .await
        .expect("dispatch");
        assert_eq!(out.outcome.run_id, None);
        assert_eq!(
            out.outcome.reason.as_deref(),
            Some(pidash_types::orchestration::RUN_AI_ACTIVE_RUN_EXISTS)
        );
        assert!(out.outcome.rollback);
        assert!(out.dispatched.is_empty());
        assert_eq!(ticker_json(&pool, graph.issue).await, before);
        let runs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count runs");
        assert_eq!(runs, 1);
    }

    // -- live: retick (R3/R3b/R4/R5/R6) --------------------------------------

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_r3_spent_pool_grant_dispatch() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "r3").await;
        ensure_agent_user(&pool).await;
        seed_ticker(
            &pool,
            graph.issue,
            &TickerSeed {
                used: 20,
                ..TickerSeed::default()
            },
        )
        .await;
        let matcher = FakeMatcher { answer: true };
        let out = re_tick_ticker(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("retick");
        assert!(out.outcome.granted);
        assert_eq!(out.outcome.reason, "granted");
        assert!(!out.outcome.rollback);
        let run = out.outcome.run_id.expect("run");
        // R3 golden: granted +10, then pool-spent disarm (cap still
        // reached) — the run fires anyway. The disarm keeps the old
        // `next_run_at` (only the leave-bucket signal clears it).
        let ticker = ticker_json(&pool, graph.issue).await;
        assert_eq!(ticker["granted"], json!(10));
        assert_eq!(ticker["enabled"], json!(false));
        assert_eq!(ticker["disarm_reason"], json!("pool_spent"));
        assert_eq!(ticker["next_run_at"], json!("2026-06-01T12:00:00+00:00"));
        let (_, trigger, _, created_by, _) = run_cols(&pool, run).await;
        assert_eq!(trigger, "run_ai");
        assert_eq!(created_by, graph.user);
        assert_eq!(out.dispatched, vec![run]);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_r3b_grant_pool_20() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "r3b").await;
        ensure_agent_user(&pool).await;
        sqlx::query("UPDATE projects SET agent_default_max_ticks = 20 WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("pool 20");
        seed_ticker(
            &pool,
            graph.issue,
            &TickerSeed {
                used: 25,
                ..TickerSeed::default()
            },
        )
        .await;
        let matcher = FakeMatcher { answer: true };
        let out = re_tick_ticker(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("retick");
        assert!(out.outcome.granted);
        // Cap 45 > used 25: the clock stays armed and re-timed.
        let ticker = ticker_json(&pool, graph.issue).await;
        assert_eq!(ticker["granted"], json!(20));
        assert_eq!(ticker["enabled"], json!(true));
        assert_eq!(ticker["disarm_reason"], json!(""));
        let expected =
            pidash_services::orchestration::clock::compute_next_run_at(10800, now(), JITTER);
        assert_eq!(
            ticker["next_run_at"],
            json!(expected.to_rfc3339_opts(chrono::SecondsFormat::Micros, false))
        );
        assert!(out.outcome.run_id.is_some());
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_r4_paused_move_dispatch() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "r4").await;
        ensure_agent_user(&pool).await;
        sqlx::query("UPDATE issues SET state_id = $1 WHERE id = $2")
            .bind(graph.states["Paused"])
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("move to paused");
        seed_ticker(
            &pool,
            graph.issue,
            &TickerSeed {
                used: 10,
                enabled: false,
                disarm_reason: "cap_hit".to_owned(),
                next_run_at: None,
                ..TickerSeed::default()
            },
        )
        .await;
        let matcher = FakeMatcher { answer: true };
        let out = re_tick_ticker(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("retick");
        assert!(out.outcome.granted);
        assert_eq!(out.outcome.reason, "granted-from-paused");
        assert!(!out.outcome.rollback);
        // Human move back to In Progress; the clock arms on the fresh
        // budget; the run carries the actor.
        assert_eq!(
            issue_state(&pool, graph.issue).await,
            graph.states["In Progress"]
        );
        let ticker = ticker_json(&pool, graph.issue).await;
        assert_eq!(ticker["granted"], json!(10));
        assert_eq!(ticker["enabled"], json!(true));
        assert_eq!(ticker["disarm_reason"], json!(""));
        assert!(!ticker["next_run_at"].is_null());
        let run = out.outcome.run_id.expect("run");
        let (_, trigger, _, created_by, _) = run_cols(&pool, run).await;
        assert_eq!(trigger, "run_ai");
        assert_eq!(created_by, graph.user);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_r5_dispatch_failed_rollback() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "r5").await;
        ensure_agent_user(&pool).await;
        // No pod anywhere: the dispatch declines after the grant.
        sqlx::query("UPDATE issues SET assigned_pod_id = NULL WHERE id = $1")
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("unassign pod");
        sqlx::query("DELETE FROM pod WHERE id = $1")
            .bind(graph.pod)
            .execute(&pool)
            .await
            .expect("delete pod");
        seed_ticker(
            &pool,
            graph.issue,
            &TickerSeed {
                used: 20,
                ..TickerSeed::default()
            },
        )
        .await;
        let before = ticker_json(&pool, graph.issue).await;
        let matcher = FakeMatcher { answer: true };
        let out = re_tick_ticker(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("retick");
        assert!(!out.outcome.granted);
        assert_eq!(out.outcome.reason, "dispatch-failed");
        assert!(out.outcome.rollback);
        assert!(out.dispatched.is_empty());
        assert_eq!(ticker_json(&pool, graph.issue).await, before);
        let runs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count runs");
        assert_eq!(runs, 0);
        let comments: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM issue_comments WHERE issue_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count comments");
        assert_eq!(comments, 0);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_r6_no_in_progress_rollback() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "r6").await;
        ensure_agent_user(&pool).await;
        sqlx::query("UPDATE issues SET state_id = $1 WHERE id = $2")
            .bind(graph.states["Paused"])
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("move to paused");
        sqlx::query("DELETE FROM states WHERE id = $1")
            .bind(graph.states["In Progress"])
            .execute(&pool)
            .await
            .expect("drop in-progress state");
        seed_ticker(
            &pool,
            graph.issue,
            &TickerSeed {
                used: 10,
                enabled: false,
                disarm_reason: "cap_hit".to_owned(),
                next_run_at: None,
                ..TickerSeed::default()
            },
        )
        .await;
        let before = ticker_json(&pool, graph.issue).await;
        let matcher = FakeMatcher { answer: true };
        let out = re_tick_ticker(
            &pool,
            deps(),
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
        )
        .await
        .expect("retick");
        assert!(!out.outcome.granted);
        assert_eq!(out.outcome.reason, "no_in_progress_state");
        assert!(out.outcome.rollback);
        assert_eq!(ticker_json(&pool, graph.issue).await, before);
        assert_eq!(
            issue_state(&pool, graph.issue).await,
            graph.states["Paused"]
        );
    }

    // -- live: bounce order + rows (B3a/B3b/B8-shape/B9) ----------------------

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_b3_backlog_order() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "b3").await;
        ensure_agent_user(&pool).await;
        // B3a: the default BacklogHi (sequence 200) beats BacklogLo
        // (sequence 100).
        let out = bounce_issue_no_eligible_runner(
            &pool,
            deps(),
            now(),
            JITTER,
            graph.issue,
            "tick",
            "no-eligible-runner",
        )
        .await
        .expect("bounce");
        assert_eq!(out.outcome.moved_to, Some(graph.states["BacklogHi"]));
        // B3b: with no default anywhere, lowest sequence wins.
        let issue2 = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority, \
             sequence_id, created_by_id, project_id, state_id, workspace_id, description_html, \
             description_stripped, sort_order, is_draft, git_work_branch, workpad, \
             complexity_score, assigned_pod_id) \
             VALUES ($1, $2, $2, 'FX8 B3b', '{}', 'none', 2, $3, $4, $5, $6, '', NULL, 0.0, \
             false, '', '', 0, $7)",
        )
        .bind(issue2)
        .bind(now())
        .bind(graph.user)
        .bind(graph.project)
        .bind(graph.states["In Progress"])
        .bind(graph.ws)
        .bind(graph.pod)
        .execute(&pool)
        .await
        .expect("seed issue2");
        sqlx::query("UPDATE states SET \"default\" = false WHERE project_id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("clear defaults");
        let out = bounce_issue_no_eligible_runner(
            &pool,
            deps(),
            now(),
            JITTER,
            issue2,
            "tick",
            "no-eligible-runner",
        )
        .await
        .expect("bounce");
        assert_eq!(out.outcome.moved_to, Some(graph.states["BacklogLo"]));
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_b8_post_move_error_rolls_back() {
        // B8's mechanism (single transaction): a post-move failure —
        // here the reserved-username collision — rolls the move back.
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "b8").await;
        // A human holds the reserved name: flip the shared agent
        // row non-bot (serial lock held) so the bounce's system-user
        // lookup fails AFTER the state UPDATE.
        ensure_agent_user(&pool).await;
        sqlx::query("UPDATE users SET is_bot = false WHERE username = 'pi_dash_agent'")
            .execute(&pool)
            .await
            .expect("squat agent name");
        let err = bounce_issue_no_eligible_runner(
            &pool,
            deps(),
            now(),
            JITTER,
            graph.issue,
            "tick",
            "no-eligible-runner",
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                err,
                DispatchJobsError::Store(DispatchError::AgentUserCollision(_))
            ),
            "unexpected error: {err:?}"
        );
        assert_eq!(
            issue_state(&pool, graph.issue).await,
            graph.states["In Progress"]
        );
        let comments: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM issue_comments WHERE issue_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count comments");
        assert_eq!(comments, 0);
        // Restore the bot flag for the other tests sharing this row.
        sqlx::query("UPDATE users SET is_bot = true WHERE username = 'pi_dash_agent'")
            .execute(&pool)
            .await
            .expect("restore agent bot");
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_b9_comment_row_shape() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "b9").await;
        let system = ensure_agent_user(&pool).await;
        bounce_issue_no_eligible_runner(
            &pool,
            deps(),
            now(),
            JITTER,
            graph.issue,
            "tick",
            "no-eligible-runner",
        )
        .await
        .expect("bounce");
        type CommentRow = (
            String,
            String,
            Uuid,
            String,
            Uuid,
            Uuid,
            Uuid,
            Option<Uuid>,
            Option<chrono::DateTime<Utc>>,
            Option<Uuid>,
            Value,
        );
        let row: CommentRow = sqlx::query_as(
            "SELECT comment_html, comment_stripped, actor_id, speaker_type, project_id, \
             workspace_id, issue_id, description_id, edited_at, parent_id, comment_json \
             FROM issue_comments WHERE issue_id = $1",
        )
        .bind(graph.issue)
        .fetch_one(&pool)
        .await
        .expect("read comment");
        assert_eq!(row.0, BOUNCE_BODY_DEFAULT);
        assert_eq!(
            row.1,
            "Agent run skipped — no eligible runner.No runner is registered in this pod that can \
             serve this issue. Add a runner under your account, or assign this issue to a \
             workspace member whose runner is registered here."
        );
        assert_eq!(row.2, system);
        assert_eq!(row.3, "agent");
        assert_eq!(row.4, graph.project);
        assert_eq!(row.5, graph.ws);
        assert_eq!(row.6, graph.issue);
        let description_id = row.7.expect("description backfill");
        assert_eq!(row.8, None);
        assert_eq!(row.9, None);
        assert_eq!(row.10, json!({}));
        let desc: (String, String, Uuid, Uuid) = sqlx::query_as(
            "SELECT description_html, description_stripped, workspace_id, project_id \
             FROM descriptions WHERE id = $1",
        )
        .bind(description_id)
        .fetch_one(&pool)
        .await
        .expect("read description");
        assert_eq!(desc.0, BOUNCE_BODY_DEFAULT);
        assert_eq!(desc.1, row.1);
        assert_eq!(desc.2, graph.ws);
        assert_eq!(desc.3, graph.project);
    }

    /// The bounce insert strips each column with its own stripper
    /// (PIDASHCONV-1581 item 4): the comment column carries the
    /// driver's MLStripper recompute (`issue.py:606`), the
    /// description mirror the `Description.save` recompute
    /// (`description.py:23-29`, entities verbatim).
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_bounce_insert_splits_strippers_per_column() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "bstrip").await;
        let system = ensure_agent_user(&pool).await;
        let tx = Transaction::begin(&pool).await.expect("begin");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let comment_id = Uuid::new_v4();
        let description_id = Uuid::new_v4();
        // The driver shape: ML-decoded carried text over entity html.
        let carried = NewIssueComment {
            id: comment_id,
            description_id,
            now: now(),
            project_id: graph.project,
            workspace_id: graph.ws,
            issue_id: graph.issue,
            actor_id: system,
            comment_html: "<p>a &amp; b</p>".to_owned(),
            comment_stripped: "a & b".to_owned(),
        };
        pidash_services::orchestration::dispatch::DispatchSeam::insert_bounce_comment(
            &mut store, &carried,
        )
        .await
        .expect("insert bounce comment");
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.commit().await.expect("commit");
        let row: (String, String, Option<Uuid>) = sqlx::query_as(
            "SELECT comment_html, comment_stripped, description_id FROM issue_comments WHERE id = $1",
        )
        .bind(comment_id)
        .fetch_one(&pool)
        .await
        .expect("read back the comment");
        assert_eq!(row.0, "<p>a &amp; b</p>");
        assert_eq!(row.1, "a & b");
        assert_eq!(row.2, Some(description_id));
        let desc: (String, Option<String>) = sqlx::query_as(
            "SELECT description_html, description_stripped FROM descriptions WHERE id = $1",
        )
        .bind(description_id)
        .fetch_one(&pool)
        .await
        .expect("read back the description");
        assert_eq!(desc.0, "<p>a &amp; b</p>");
        assert_eq!(desc.1.as_deref(), Some("a &amp; b"));
    }

    // -- live: null default fallback (PIDASHCONV-1044) ------------------------

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_default_state_id_null_set_missing() {
        // `default_state_id` is nullable: a NULL column reads as "no
        // fallback" (`Ok(None)`), like Django's
        // `issue.project.default_state` (`scheduling.py:1093-1096`),
        // instead of failing the scalar decode.
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "defstate").await;
        let tx = Transaction::begin(&pool).await.expect("begin");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        // `seed_graph` leaves `default_state_id` NULL: no fallback.
        let none =
            pidash_services::orchestration::dispatch::DispatchSeam::default_state_id_for_project(
                &mut store,
                graph.project,
            )
            .await
            .expect("null default reads None");
        assert_eq!(none, None);
        // Missing project row: also no fallback.
        let missing =
            pidash_services::orchestration::dispatch::DispatchSeam::default_state_id_for_project(
                &mut store,
                Uuid::new_v4(),
            )
            .await
            .expect("missing project reads None");
        assert_eq!(missing, None);
        // A set default round-trips.
        sqlx::query("UPDATE projects SET default_state_id = $1 WHERE id = $2")
            .bind(graph.states["Todo"])
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("set default");
        let some =
            pidash_services::orchestration::dispatch::DispatchSeam::default_state_id_for_project(
                &mut store,
                graph.project,
            )
            .await
            .expect("set default reads Some");
        assert_eq!(some, Some(graph.states["Todo"]));
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_bounce_no_backlog_null_default_stays() {
        // The 1044 trigger: no Backlog state + NULL `default_state_id`
        // — Django logs "no safe backlog target" and leaves the issue
        // in place (`scheduling.py:1096-1107`); the bounce declines
        // cleanly instead of 500ing on the fallback decode.
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "bnull").await;
        ensure_agent_user(&pool).await;
        sqlx::query(
            "UPDATE states SET \"group\" = 'unstarted' WHERE project_id = $1 AND \"group\" = 'backlog'",
        )
        .bind(graph.project)
        .execute(&pool)
        .await
        .expect("drop backlog group");
        let out = bounce_issue_no_eligible_runner(
            &pool,
            deps(),
            now(),
            JITTER,
            graph.issue,
            "tick",
            "no-eligible-runner",
        )
        .await
        .expect("bounce");
        assert_eq!(out.outcome.moved_to, None);
        let state: Uuid = sqlx::query_scalar("SELECT state_id FROM issues WHERE id = $1")
            .bind(graph.issue)
            .fetch_one(&pool)
            .await
            .expect("read issue state");
        assert_eq!(state, graph.states["In Progress"]);
    }

    // -- live: wait activity (W1) + pause (U11) -------------------------------

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_w1_activity_row() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "w1").await;
        ensure_agent_user(&pool).await;
        seed_ticker(&pool, graph.issue, &TickerSeed::default()).await;
        let run = seed_run(
            &pool,
            &graph,
            "running",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        let out = wait_ticker(
            &pool,
            deps(),
            now(),
            JITTER,
            1_780_000_000.0,
            graph.issue,
            Some(run),
            Some(graph.user),
            Some(graph.user),
        )
        .await
        .expect("wait");
        assert!(out.outcome.applied);
        assert_eq!(out.outcome.reason, "waited");
        let ticker = ticker_json(&pool, graph.issue).await;
        assert_eq!(ticker["waited"], json!(1));
        let row: (
            String,
            String,
            String,
            String,
            String,
            Uuid,
            f64,
            Option<Uuid>,
            Option<Uuid>,
        ) = sqlx::query_as(
            "SELECT verb, field, old_value, new_value, comment, actor_id, epoch, \
             created_by_id, updated_by_id FROM issue_activities WHERE issue_id = $1",
        )
        .bind(graph.issue)
        .fetch_one(&pool)
        .await
        .expect("read activity");
        assert_eq!(row.0, "updated");
        assert_eq!(row.1, "agent_wait");
        assert_eq!(row.2, "10");
        assert_eq!(row.3, "1");
        assert_eq!(row.4, format!("Waited on a blocker (1 of 10); run {run}"));
        assert_eq!(row.5, graph.user);
        assert_eq!(row.6, 1_780_000_000.0);
        assert_eq!(row.7, Some(graph.user));
        assert_eq!(row.8, None);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_u11_pause_applies() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "u11").await;
        ensure_agent_user(&pool).await;
        let run = seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        seed_ticker(
            &pool,
            graph.issue,
            &TickerSeed {
                used: 10,
                enabled: false,
                disarm_reason: "cap_hit".to_owned(),
                next_run_at: None,
                ..TickerSeed::default()
            },
        )
        .await;
        let out = maybe_apply_deferred_pause(&pool, deps(), now(), JITTER, run)
            .await
            .expect("pause");
        assert!(out.outcome.applied);
        assert_eq!(
            issue_state(&pool, graph.issue).await,
            graph.states["Paused"]
        );
        let ticker = ticker_json(&pool, graph.issue).await;
        assert_eq!(ticker["enabled"], json!(false));
        assert_eq!(ticker["disarm_reason"], json!("left_ticking_state"));
        let updated_by: Option<Uuid> =
            sqlx::query_scalar("SELECT updated_by_id FROM issues WHERE id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("read updated_by");
        assert_eq!(updated_by, None);
        assert_eq!(out.outcome.logs.len(), 2);
        assert!(out.outcome.logs[1].message.contains("auto-paused"));
    }

    // -- live: agent user + cloud creator + shim ------------------------------

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_agent_user_shape() {
        // The shared agent row (created through the seam by whichever
        // test ran first) carries the Django `User` defaults.
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let id = ensure_agent_user(&pool).await;
        let row: (
            String,
            Option<String>,
            String,
            String,
            String,
            bool,
            bool,
            String,
            bool,
        ) = sqlx::query_as(
            "SELECT username, email, display_name, first_name, last_name, is_active, is_bot, \
             user_timezone, is_email_valid FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read agent user");
        assert_eq!(row.0, "pi_dash_agent");
        assert_eq!(row.1.as_deref(), Some("agent@example.com"));
        assert_eq!(row.2, "agent");
        assert_eq!(row.3, "Pi Dash");
        assert_eq!(row.4, "Agent");
        assert!(row.5);
        assert!(row.6);
        assert_eq!(row.7, "UTC");
        assert!(!row.8);
        let password: String = sqlx::query_scalar("SELECT password FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("read password");
        assert!(password.starts_with('!'));
        assert_eq!(password.len(), 41);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_cloud_creator_with_membership() {
        // The role-check SQL leg: a cloud project where the issue
        // creator holds LLM config + a member role resolves, and the
        // cloud preflight proceeds. (Cloud *builders* need cloud
        // admission + drain machinery — D-11 live scope, not this
        // unit.)
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "cc").await;
        ensure_agent_user(&pool).await;
        sqlx::query("UPDATE projects SET default_agent_executor = 'cloud_agent' WHERE id = $1")
            .bind(graph.project)
            .execute(&pool)
            .await
            .expect("cloud executor");
        sqlx::query(
            "INSERT INTO project_members (id, created_at, updated_at, workspace_id, project_id, \
             member_id, role, is_active, view_props, default_props, sort_order, preferences) \
             VALUES ($1, $2, $2, $3, $4, $5, 15, true, '{}', '{}', 0.0, '{}')",
        )
        .bind(Uuid::new_v4())
        .bind(now())
        .bind(graph.ws)
        .bind(graph.project)
        .bind(graph.user)
        .execute(&pool)
        .await
        .expect("seed membership");
        let mut cc_deps = deps();
        cc_deps.has_usable_llm_config = llm_ok;
        let tx = Transaction::begin(&pool).await.expect("begin");
        let mut store = LiveCreationStore::new(tx, &pool, cc_deps);
        let creator = pidash_services::orchestration::dispatch::resolve_creator_for_trigger(
            &mut store,
            graph.issue,
            "run_ai",
            None,
        )
        .await
        .expect("resolve");
        assert_eq!(creator, Some(graph.user));
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
        // And the cloud preflight proceeds (F1 live).
        let mut cc_deps = deps();
        cc_deps.has_usable_llm_config = llm_ok;
        let matcher = FakeMatcher { answer: true };
        let out = preflight_eligibility_or_bounce(
            &pool,
            cc_deps,
            &matcher,
            now(),
            JITTER,
            graph.issue,
            Some(graph.user),
            graph.pod,
            "run_ai",
        )
        .await
        .expect("preflight");
        assert!(out.outcome.proceed);
        assert!(out.outcome.logs.is_empty());
        let comments: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM issue_comments WHERE issue_id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("count comments");
        assert_eq!(comments, 0);
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_shim_vectors() {
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "shim").await;
        ensure_agent_user(&pool).await;
        seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        let shim = FireTickShim::new(
            pool.clone(),
            Arc::new(deps),
            Arc::new(FakeMatcher { answer: true }),
        );
        use crate::tasks_ticker::fire_tick::FireTickSeam as _;
        let in_progress = StateView {
            id: graph.states["In Progress"],
            name: "In Progress".to_owned(),
            group: "started".to_owned(),
        };
        let done = StateView {
            id: graph.states["Done"],
            name: "Done".to_owned(),
            group: "completed".to_owned(),
        };
        let in_review = StateView {
            id: graph.states["In Review"],
            name: "In Review".to_owned(),
            group: "review".to_owned(),
        };
        assert!(shim.is_ticking_state(Some(&in_progress)));
        assert!(!shim.is_ticking_state(Some(&done)));
        assert!(!shim.is_ticking_state(None));
        let project = crate::tasks_ticker::fire_tick::ProjectTickCols {
            pool: Some(10),
            ticking_enabled: Some(true),
            interval_default: Some(10800),
            interval_review: Some(7200),
            interval_test: Some(3600),
        };
        assert_eq!(shim.tick_interval_seconds(Some(&in_review), &project), 7200);
        assert!(!shim.has_active_run(graph.issue).await.expect("active"));
        assert!(shim.has_prior_run(graph.issue).await.expect("prior"));
        let run = shim
            .dispatch_continuation_run(graph.issue, "tick", None)
            .await
            .expect("dispatch");
        assert!(run.is_some());
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_fire_tick_claim_dispatches() {
        // PIDASHCONV-1070: the claim path (`read_claim` →
        // `ProjectTickCols::from_row`) decoded the INT4 project cadence
        // columns as `Option<i64>`, so every live tick died pre-gate
        // with an INT4/INT8 mismatch (PIDASHCONV-1068 fixed the decode;
        // this test keeps it fixed). No earlier live test read the
        // claim — they all dispatch past it — so fire one full tick
        // through the shim and assert the dispatch outcome.
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "tick1070").await;
        ensure_agent_user(&pool).await;
        let prior = seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        let ticker = seed_ticker(&pool, graph.issue, &TickerSeed::default()).await;
        let shim = FireTickShim::new(
            pool.clone(),
            Arc::new(deps),
            Arc::new(FakeMatcher { answer: true }),
        );
        let seam: Arc<dyn FireTickSeam> = Arc::new(shim);
        use rand08::SeedableRng as _;
        let mut rng = rand08::rngs::StdRng::seed_from_u64(1070);
        let fired =
            crate::tasks_ticker::fire_tick::fire_tick(&pool, &seam, ticker, now(), &mut rng)
                .await
                .expect("fire_tick succeeds on live INT4 columns");
        assert!(fired, "a due tick with a prior run dispatches");
        // The claim spent one of the pool and re-armed in the future.
        let (used, next_run_at): (i32, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT used, next_run_at FROM issue_agent_ticker WHERE id = $1")
                .bind(ticker)
                .fetch_one(&pool)
                .await
                .expect("read ticker");
        assert_eq!(used, TickerSeed::default().used + 1);
        assert!(
            next_run_at.is_some_and(|next| next > now()),
            "re-armed past the firing instant"
        );
        // The dispatch continued the prior run (same shape as A3).
        let runs: Vec<(Uuid, String, String, Option<Uuid>)> = sqlx::query_as(
            "SELECT id, status, trigger, parent_run_id FROM agent_run \
             WHERE work_item_id = $1 ORDER BY created_at",
        )
        .bind(graph.issue)
        .fetch_all(&pool)
        .await
        .expect("read runs");
        assert_eq!(runs.len(), 2);
        let (status, trigger, parent) = (&runs[1].1, &runs[1].2, runs[1].3);
        assert_eq!(status, "queued");
        assert_eq!(trigger, "tick");
        assert_eq!(parent, Some(prior));
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_migrated_reads_backlog_slug_policy_assignees() {
        // Direct SeaORM coverage for the migrated seam reads: backlog
        // ordering (default DESC, sequence ASC), workspace slug,
        // clock policy, live-assignee subquery and the role-facts trio.
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "migread").await;
        ensure_agent_user(&pool).await;
        let tx = Transaction::begin(&pool).await.expect("begin");
        let mut store = LiveCreationStore::new(tx, &pool, deps());
        let backlog =
            pidash_services::orchestration::dispatch::DispatchSeam::backlog_target_for_project(
                &mut store,
                graph.project,
            )
            .await
            .expect("backlog");
        assert_eq!(
            backlog.map(|state| state.id),
            Some(graph.states["BacklogHi"])
        );
        let in_progress =
            pidash_services::orchestration::dispatch::DispatchSeam::in_progress_state_for_project(
                &mut store,
                graph.project,
                "started",
                "In Progress",
            )
            .await
            .expect("in progress");
        assert_eq!(
            in_progress.map(|state| state.id),
            Some(graph.states["In Progress"])
        );
        let paused =
            pidash_services::orchestration::dispatch::DispatchSeam::paused_state_for_project(
                &mut store,
                graph.project,
            )
            .await
            .expect("paused");
        assert_eq!(paused.map(|state| state.id), Some(graph.states["Paused"]));
        let slug = pidash_services::orchestration::dispatch::DispatchSeam::workspace_slug(
            &mut store, graph.ws,
        )
        .await
        .expect("slug");
        assert!(slug.starts_with("fx8-ws-"));
        let policy =
            pidash_services::orchestration::dispatch::DispatchSeam::clock_policy_for_project(
                &mut store,
                graph.project,
            )
            .await
            .expect("policy");
        assert_eq!(policy.agent_ticking_enabled, Some(true));
        assert_eq!(policy.agent_default_max_ticks, Some(10));
        assert_eq!(policy.agent_default_interval_seconds, Some(10800));
        // Assignees: seed one live assignment + one tombstoned row.
        sqlx::query(
            "INSERT INTO issue_assignees (id, created_at, updated_at, assignee_id, issue_id, project_id, workspace_id) \
             VALUES ($1, $2, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(now())
        .bind(graph.user)
        .bind(graph.issue)
        .bind(graph.project)
        .bind(graph.ws)
        .execute(&pool)
        .await
        .expect("seed assignee");
        // A tombstoned (soft-deleted) assignment must stay excluded.
        let tombstoned_user = seed_user(&pool, "migread-tomb").await;
        sqlx::query(
            "INSERT INTO issue_assignees (id, created_at, updated_at, assignee_id, issue_id, project_id, workspace_id, deleted_at) \
             VALUES ($1, $2, $2, $3, $4, $5, $6, $2)",
        )
        .bind(Uuid::new_v4())
        .bind(now())
        .bind(tombstoned_user)
        .bind(graph.issue)
        .bind(graph.project)
        .bind(graph.ws)
        .execute(&pool)
        .await
        .expect("seed tombstoned assignee");
        let candidates =
            pidash_services::orchestration::dispatch::DispatchSeam::live_assignee_candidates(
                &mut store,
                graph.issue,
            )
            .await
            .expect("candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, graph.user);
        let facts = pidash_services::orchestration::dispatch::DispatchSeam::project_role_facts(
            &mut store,
            graph.user,
            &slug.clone(),
            graph.project,
        )
        .await
        .expect("role facts");
        assert!(!facts.has_allowed_role);
        assert!(!facts.is_project_member);
        assert!(!facts.is_workspace_admin);
        // True branches: inactive membership stays false, active ADMIN
        // membership and workspace ADMIN answer true.
        sqlx::query(
            "INSERT INTO project_members (id, created_at, updated_at, role, member_id, project_id, workspace_id, view_props, default_props, sort_order, preferences, is_active) \
             VALUES ($1, $2, $2, 20, $3, $4, $5, '{}', '{}', 0, '{}', false)",
        )
        .bind(Uuid::new_v4())
        .bind(now())
        .bind(graph.user)
        .bind(graph.project)
        .bind(graph.ws)
        .execute(&pool)
        .await
        .expect("seed inactive membership");
        let inactive = pidash_services::orchestration::dispatch::DispatchSeam::project_role_facts(
            &mut store,
            graph.user,
            &slug.clone(),
            graph.project,
        )
        .await
        .expect("inactive facts");
        assert!(!inactive.has_allowed_role);
        assert!(!inactive.is_project_member);
        sqlx::query(
            "UPDATE project_members SET is_active = true WHERE member_id = $1 AND project_id = $2",
        )
        .bind(graph.user)
        .bind(graph.project)
        .execute(&pool)
        .await
        .expect("activate membership");
        sqlx::query(
            "INSERT INTO workspace_members (id, created_at, updated_at, role, member_id, workspace_id, view_props, default_props, issue_props, is_active, explored_features, getting_started_checklist, tips) \
             VALUES ($1, $2, $2, 20, $3, $4, '{}', '{}', '{}', true, '{}', '{}', '{}')",
        )
        .bind(Uuid::new_v4())
        .bind(now())
        .bind(graph.user)
        .bind(graph.ws)
        .execute(&pool)
        .await
        .expect("seed workspace admin");
        let active = pidash_services::orchestration::dispatch::DispatchSeam::project_role_facts(
            &mut store,
            graph.user,
            &slug.clone(),
            graph.project,
        )
        .await
        .expect("active facts");
        assert!(active.has_allowed_role);
        assert!(active.is_project_member);
        assert!(active.is_workspace_admin);
        let (tx, _, _, _, _, _) = store.into_parts();
        tx.rollback().await.expect("rollback");
    }

    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    #[tokio::test]
    async fn live_unknown_triggered_by_stamps_raw() {
        // Django passes `triggered_by` to the builders unstamped and
        // unvalidated (`scheduling.py:1206/1214`, `trigger: str`): the
        // fire-tick path forwards the ticker's stored
        // `pending_entry_trigger`, which may sit outside the trigger
        // enum, and the new run carries it verbatim.
        let _serial = LIVE_SERIAL.lock().await;
        let pool = pool().await;
        let graph = seed_graph(&pool, "trigraw").await;
        ensure_agent_user(&pool).await;
        seed_run(
            &pool,
            &graph,
            "completed",
            "tick",
            "local_runner",
            "coding-task",
            None,
            json!({}),
            graph.user,
            now() - chrono::Duration::hours(1),
        )
        .await;
        let shim = FireTickShim::new(
            pool.clone(),
            Arc::new(deps),
            Arc::new(FakeMatcher { answer: true }),
        );
        use crate::tasks_ticker::fire_tick::FireTickSeam as _;
        let run = shim
            .dispatch_continuation_run(graph.issue, "human", None)
            .await
            .expect("dispatch tolerates unknown triggered_by");
        let run = run.expect("run created");
        let trigger: String = sqlx::query_scalar("SELECT trigger FROM agent_run WHERE id = $1")
            .bind(run)
            .fetch_one(&pool)
            .await
            .expect("read stamped trigger");
        assert_eq!(trigger, "human");
    }
}
