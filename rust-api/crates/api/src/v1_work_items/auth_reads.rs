//! The v1 work-item request preamble reads on SeaORM, shared by every
//! handler file in this module.
//!
//! Each handler file keeps its own preamble (`actor`, `preamble`, the
//! gates) so its `Denial` mapping stays local; the reads behind them are
//! here, once. Every function returns the raw [`DbErr`]; the callers map
//! it through their own `db_error`.
//!
//! * [`live_api_token`][]: `APIToken.objects.filter(token=...)`
//!   (`api/middleware/api_authentication.py:19-46`); the
//!   `SoftDeletionManager` scope (`deleted_at IS NULL`) hides tombstones.
//! * [`machine_token_with_device`][]: the `machine_token` row by HMAC hash,
//!   plus whether its `dev_machine` (LEFT JOIN) is revoked.
//! * [`is_active_workspace_member`][]: `is_workspace_member`
//!   (`core/permissions.py:28-34`).
//! * [`user_timezone`][]: `request.user.user_timezone` (`TimezoneMixin`).
//! * [`workspace_id_by_slug`][]: the slug lookup the gates scope on (no
//!   liveness predicate, as before).
//! * [`project_member_roles`], [`project_membership_exists`],
//!   [`identifier_membership`][], [`workspace_role_exists`]: the
//!   `app/permissions/project.py` membership facts.
//! * [`run_facts_by_id`], [`newest_run_facts_on_issue`],
//!   [`yield_run_by_id`]: the `AgentRun.objects.select_related("runner")`
//!   rows `perms::resolve_moved_by_run` judges.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait,
};
use uuid::Uuid;

use pidash_db::entity::{
    agent_run, api_tokens, dev_machine, machine_token, project_members, projects, runner, users,
    workspace_members, workspaces,
};
use pidash_types::runner_runs::AgentRunStatus;

use super::perms::RunFacts;

/// The `api_tokens` columns the validator reads (`db/models/api.py:35-57`).
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct ApiTokenLookup {
    pub token: String,
    pub is_active: bool,
    pub expired_at: Option<DateTime<Utc>>,
    pub user_id: Uuid,
}

/// The live `api_tokens` row with exactly this token (`deleted_at IS
/// NULL`, the manager scope). `is_active` / `expired_at` are judged by
/// `pidash_auth::token::validate_api_token`.
pub async fn live_api_token<C: ConnectionTrait>(
    db: &C,
    presented: &str,
) -> Result<Option<ApiTokenLookup>, DbErr> {
    api_tokens::Entity::find()
        .select_only()
        .columns([
            api_tokens::Column::Token,
            api_tokens::Column::IsActive,
            api_tokens::Column::ExpiredAt,
            api_tokens::Column::UserId,
        ])
        .filter(api_tokens::Column::Token.eq(presented))
        .filter(api_tokens::Column::DeletedAt.is_null())
        .into_model::<ApiTokenLookup>()
        .one(db)
        .await
}

/// A `machine_token` row by hash plus the revocation bit of its device.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct MachineTokenLookup {
    pub token_hash: String,
    pub user_id: Uuid,
    pub dev_machine_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub revoked_at: Option<DateTime<Utc>>,
    /// 60-day idle expiry (`NEWPIDASH-50`); judged by the token kernel.
    pub expires_at: Option<DateTime<Utc>>,
    /// `dev_machine.revoked_at IS NOT NULL` over the LEFT JOIN: `false`
    /// when the token has no device.
    pub dev_revoked: Option<bool>,
}

/// `MachineToken` by `token_hash`, LEFT JOIN `dev_machine` for its
/// revocation (`api_authentication.py:47-70`). No liveness scope: the
/// table has no soft delete.
pub async fn machine_token_with_device<C: ConnectionTrait>(
    db: &C,
    token_hash: &str,
) -> Result<Option<MachineTokenLookup>, DbErr> {
    machine_token::Entity::find()
        .select_only()
        .columns([
            machine_token::Column::TokenHash,
            machine_token::Column::UserId,
            machine_token::Column::DevMachineId,
            machine_token::Column::WorkspaceId,
            machine_token::Column::RevokedAt,
            machine_token::Column::ExpiresAt,
        ])
        .column_as(
            Expr::col((dev_machine::Entity, dev_machine::Column::RevokedAt)).is_not_null(),
            "dev_revoked",
        )
        .join(
            JoinType::LeftJoin,
            machine_token::Entity::belongs_to(dev_machine::Entity)
                .from(machine_token::Column::DevMachineId)
                .to(dev_machine::Column::Id)
                .into(),
        )
        .filter(machine_token::Column::TokenHash.eq(token_hash))
        .into_model::<MachineTokenLookup>()
        .one(db)
        .await
}

/// `is_workspace_member(user, workspace_id)`: an active, live
/// `workspace_members` row.
pub async fn is_active_workspace_member<C: ConnectionTrait>(
    db: &C,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<bool, DbErr> {
    workspace_role_exists(db, workspace_id, user_id, &[]).await
}

/// An active, live `workspace_members` row for the user whose role is in
/// `roles` (any role when `roles` is empty).
pub async fn workspace_role_exists<C: ConnectionTrait>(
    db: &C,
    workspace_id: Uuid,
    user_id: Uuid,
    roles: &[i16],
) -> Result<bool, DbErr> {
    let mut query = workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Id)
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .filter(workspace_members::Column::MemberId.eq(user_id));
    if !roles.is_empty() {
        query = query.filter(workspace_members::Column::Role.is_in(roles.iter().copied()));
    }
    let hit: Option<Uuid> = query
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .limit(1)
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// The stored `users.user_timezone` name, unparsed (`None` when the user
/// row is missing).
pub async fn user_timezone<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
) -> Result<Option<String>, DbErr> {
    users::Entity::find()
        .select_only()
        .column(users::Column::UserTimezone)
        .filter(users::Column::Id.eq(user_id))
        .into_tuple()
        .one(db)
        .await
}

/// The workspace id for a slug. No liveness predicate (the old read had
/// none either): an unknown slug is `None`, and every gate then denies.
pub async fn workspace_id_by_slug<C: ConnectionTrait>(
    db: &C,
    slug: &str,
) -> Result<Option<Uuid>, DbErr> {
    workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .filter(workspaces::Column::Slug.eq(slug))
        .into_tuple()
        .one(db)
        .await
}

/// The roles of the user's active, live `project_members` rows on the
/// project, workspace-scoped (`project.py:56-116`).
pub async fn project_member_roles<C: ConnectionTrait>(
    db: &C,
    workspace_id: Uuid,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<i16>, DbErr> {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Role)
        .filter(project_members::Column::WorkspaceId.eq(workspace_id))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
}

/// Which `project_members` rows [`project_membership_exists`] matches.
#[derive(Debug, Clone, Copy, Default)]
pub struct MembershipScope<'a> {
    pub workspace_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    /// Allowed roles; empty means any role.
    pub roles: &'a [i16],
}

/// An active, live `project_members` row for the user within `scope`.
pub async fn project_membership_exists<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    scope: MembershipScope<'_>,
) -> Result<bool, DbErr> {
    let mut query = project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Id)
        .filter(project_members::Column::MemberId.eq(user_id));
    if let Some(workspace_id) = scope.workspace_id {
        query = query.filter(project_members::Column::WorkspaceId.eq(workspace_id));
    }
    if let Some(project_id) = scope.project_id {
        query = query.filter(project_members::Column::ProjectId.eq(project_id));
    }
    if !scope.roles.is_empty() {
        query = query.filter(project_members::Column::Role.is_in(scope.roles.iter().copied()));
    }
    let hit: Option<Uuid> = query
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .limit(1)
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// The by-identifier `project_identifier` arm (`project.py:91-98`): an
/// active, live membership on the project carrying the identifier, any
/// role. The joined `projects` row carries no liveness scope (Django never
/// scopes joined tables).
pub async fn identifier_membership<C: ConnectionTrait>(
    db: &C,
    workspace_id: Uuid,
    user_id: Uuid,
    project_identifier: &str,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Id)
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Projects.def(),
        )
        .filter(project_members::Column::WorkspaceId.eq(workspace_id))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(projects::Column::Identifier.eq(project_identifier))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .limit(1)
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// The `agent_run` columns [`RunFacts`] carries, plus the runner owner
/// over the `select_related("runner")` LEFT JOIN.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct RunFactsRow {
    pub id: Uuid,
    pub created_by_id: Uuid,
    pub owner_id: Option<Uuid>,
    pub runner_id: Option<Uuid>,
    pub runner_owner_id: Option<Uuid>,
    pub work_item_id: Option<Uuid>,
    pub status: String,
}

impl RunFactsRow {
    /// An unknown stored status maps to a non-active member, as the guards
    /// document (`perms::RunFacts::status`).
    pub fn into_facts(self) -> RunFacts {
        RunFacts {
            id: self.id,
            created_by_id: self.created_by_id,
            owner_id: self.owner_id,
            runner_id: self.runner_id,
            runner_owner_id: self.runner_owner_id,
            work_item_id: self.work_item_id,
            status: AgentRunStatus::from_value(&self.status).unwrap_or(AgentRunStatus::Cancelled),
        }
    }
}

/// `AgentRun.objects.select_related("runner")` narrowed to the
/// [`RunFactsRow`] columns.
fn run_facts_select() -> sea_orm::Select<agent_run::Entity> {
    agent_run::Entity::find()
        .select_only()
        .columns([
            agent_run::Column::Id,
            agent_run::Column::CreatedById,
            agent_run::Column::OwnerId,
            agent_run::Column::RunnerId,
        ])
        .column_as(runner::Column::OwnerId, "runner_owner_id")
        .columns([agent_run::Column::WorkItemId, agent_run::Column::Status])
        .join(JoinType::LeftJoin, agent_run::Relation::Runner.def())
}

/// `AgentRun.objects.select_related("runner").filter(pk=run_id).first()`.
pub async fn run_facts_by_id<C: ConnectionTrait>(
    db: &C,
    run_id: Uuid,
) -> Result<Option<RunFactsRow>, DbErr> {
    run_facts_select()
        .filter(agent_run::Column::Id.eq(run_id))
        .into_model::<RunFactsRow>()
        .one(db)
        .await
}

/// `AgentRun.objects.select_related("runner").filter(work_item_id=pk)
/// .order_by("-created_at")[:5]`: the no-header inference rows.
pub async fn newest_run_facts_on_issue<C: ConnectionTrait>(
    db: &C,
    issue_id: Uuid,
) -> Result<Vec<RunFactsRow>, DbErr> {
    run_facts_select()
        .filter(agent_run::Column::WorkItemId.eq(issue_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .limit(5)
        .into_model::<RunFactsRow>()
        .all(db)
        .await
}

/// A yield run row: [`RunFactsRow`] plus the run's workspace and
/// `done_payload`.
#[derive(Debug, Clone, PartialEq, FromQueryResult)]
pub struct YieldRunRow {
    pub id: Uuid,
    pub created_by_id: Uuid,
    pub owner_id: Option<Uuid>,
    pub runner_id: Option<Uuid>,
    pub runner_owner_id: Option<Uuid>,
    pub work_item_id: Option<Uuid>,
    pub status: String,
    pub workspace_id: Uuid,
    pub done_payload: Option<serde_json::Value>,
}

impl YieldRunRow {
    /// The [`RunFacts`] half, mapped as [`RunFactsRow::into_facts`].
    pub fn facts(&self) -> RunFacts {
        RunFactsRow {
            id: self.id,
            created_by_id: self.created_by_id,
            owner_id: self.owner_id,
            runner_id: self.runner_id,
            runner_owner_id: self.runner_owner_id,
            work_item_id: self.work_item_id,
            status: self.status.clone(),
        }
        .into_facts()
    }
}

/// `AgentRun.objects.select_related("workspace", "work_item", "runner")
/// .filter(pk=run_id, workspace__slug=slug).first()`: INNER JOIN
/// `workspaces` for the slug, LEFT JOIN `runner` for the owner.
pub async fn yield_run_by_id<C: ConnectionTrait>(
    db: &C,
    slug: &str,
    run_id: Uuid,
) -> Result<Option<YieldRunRow>, DbErr> {
    run_facts_select()
        .columns([
            agent_run::Column::WorkspaceId,
            agent_run::Column::DonePayload,
        ])
        .join(
            JoinType::InnerJoin,
            agent_run::Entity::belongs_to(workspaces::Entity)
                .from(agent_run::Column::WorkspaceId)
                .to(workspaces::Column::Id)
                .into(),
        )
        .filter(agent_run::Column::Id.eq(run_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .into_model::<YieldRunRow>()
        .one(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    #[test]
    fn machine_token_statement_left_joins_the_device() {
        let sql = machine_token::Entity::find()
            .select_only()
            .column(machine_token::Column::TokenHash)
            .column_as(
                Expr::col((dev_machine::Entity, dev_machine::Column::RevokedAt)).is_not_null(),
                "dev_revoked",
            )
            .join(
                JoinType::LeftJoin,
                machine_token::Entity::belongs_to(dev_machine::Entity)
                    .from(machine_token::Column::DevMachineId)
                    .to(dev_machine::Column::Id)
                    .into(),
            )
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains(r#"LEFT JOIN "dev_machine" ON "machine_token"."dev_machine_id" = "dev_machine"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""dev_machine"."revoked_at" IS NOT NULL AS "dev_revoked""#),
            "{sql}"
        );
    }

    /// Every read against constraint-free temp copies on one connection.
    #[tokio::test]
    async fn live_auth_reads() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "api_tokens",
            "machine_token",
            "dev_machine",
            "workspace_members",
            "users",
            "workspaces",
            "project_members",
            "projects",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (ws, other_ws, user, stranger) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (project, other_project, device, dead_device) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for sql in [
            format!(
                "INSERT INTO api_tokens (id, token, is_active, expired_at, user_id, deleted_at) VALUES \
                 (gen_random_uuid(), 'tok-live', true, NULL, '{user}', NULL), \
                 (gen_random_uuid(), 'tok-dead', true, NULL, '{user}', now()), \
                 (gen_random_uuid(), 'tok-exp', false, '2020-01-02T03:04:05Z', '{stranger}', NULL)"
            ),
            format!(
                "INSERT INTO dev_machine (id, revoked_at) VALUES ('{device}', NULL), ('{dead_device}', now())"
            ),
            format!(
                "INSERT INTO machine_token (id, token_hash, user_id, dev_machine_id, workspace_id, revoked_at) VALUES \
                 (gen_random_uuid(), 'h-bare', '{user}', NULL, '{ws}', NULL), \
                 (gen_random_uuid(), 'h-dev', '{user}', '{device}', '{ws}', now()), \
                 (gen_random_uuid(), 'h-dead', '{user}', '{dead_device}', '{ws}', NULL)"
            ),
            format!(
                "INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, deleted_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{user}', 15, true, NULL), \
                 (gen_random_uuid(), '{ws}', '{stranger}', 20, false, NULL), \
                 (gen_random_uuid(), '{other_ws}', '{stranger}', 20, true, now())"
            ),
            format!("INSERT INTO users (id, user_timezone) VALUES ('{user}', 'Asia/Tokyo')"),
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO projects (id, identifier) VALUES ('{project}', 'ACME'), ('{other_project}', 'OTH')"
            ),
            format!(
                "INSERT INTO project_members (id, workspace_id, project_id, member_id, role, is_active, deleted_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{project}', '{user}', 20, true, NULL), \
                 (gen_random_uuid(), '{ws}', '{project}', '{user}', 5, true, now()), \
                 (gen_random_uuid(), '{ws}', '{other_project}', '{user}', 5, false, NULL), \
                 (gen_random_uuid(), '{other_ws}', '{other_project}', '{stranger}', 15, true, NULL)"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let db = pidash_db::pool::sea_orm_connection(&pool);

        let live = live_api_token(&db, "tok-live").await.expect("token");
        assert_eq!(
            live,
            Some(ApiTokenLookup {
                token: "tok-live".into(),
                is_active: true,
                expired_at: None,
                user_id: user,
            })
        );
        assert_eq!(live_api_token(&db, "tok-dead").await.expect("token"), None);
        let expired = live_api_token(&db, "tok-exp")
            .await
            .expect("token")
            .expect("found");
        assert!(!expired.is_active);
        assert_eq!(
            expired.expired_at.map(|dt| dt.timestamp()),
            Some(1_577_934_245)
        );

        let bare = machine_token_with_device(&db, "h-bare")
            .await
            .expect("mt")
            .expect("found");
        assert_eq!(
            (bare.dev_machine_id, bare.workspace_id, bare.dev_revoked),
            (None, ws, Some(false))
        );
        let dev = machine_token_with_device(&db, "h-dev")
            .await
            .expect("mt")
            .expect("found");
        assert!(dev.revoked_at.is_some());
        assert_eq!(dev.dev_revoked, Some(false));
        let dead = machine_token_with_device(&db, "h-dead")
            .await
            .expect("mt")
            .expect("found");
        assert_eq!(dead.dev_revoked, Some(true));
        assert!(machine_token_with_device(&db, "nope")
            .await
            .expect("mt")
            .is_none());

        assert!(is_active_workspace_member(&db, ws, user).await.expect("m"));
        assert!(!is_active_workspace_member(&db, ws, stranger)
            .await
            .expect("m"));
        assert!(!is_active_workspace_member(&db, other_ws, stranger)
            .await
            .expect("m"));
        assert!(workspace_role_exists(&db, ws, user, &[20, 15])
            .await
            .expect("r"));
        assert!(!workspace_role_exists(&db, ws, user, &[20])
            .await
            .expect("r"));

        assert_eq!(
            user_timezone(&db, user).await.expect("tz").as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(user_timezone(&db, stranger).await.expect("tz"), None);
        assert_eq!(
            workspace_id_by_slug(&db, "acme").await.expect("ws"),
            Some(ws)
        );
        assert_eq!(workspace_id_by_slug(&db, "nope").await.expect("ws"), None);

        assert_eq!(
            project_member_roles(&db, ws, user, project)
                .await
                .expect("roles"),
            vec![20]
        );
        assert!(project_member_roles(&db, ws, user, other_project)
            .await
            .expect("roles")
            .is_empty());
        let any = MembershipScope {
            project_id: Some(project),
            ..Default::default()
        };
        assert!(project_membership_exists(&db, user, any).await.expect("e"));
        let admin_only = MembershipScope {
            workspace_id: Some(ws),
            project_id: Some(project),
            roles: &[20],
        };
        assert!(project_membership_exists(&db, user, admin_only)
            .await
            .expect("e"));
        let guest_only = MembershipScope {
            roles: &[5],
            ..admin_only
        };
        assert!(!project_membership_exists(&db, user, guest_only)
            .await
            .expect("e"));
        let workspace_wide = MembershipScope {
            workspace_id: Some(other_ws),
            ..Default::default()
        };
        assert!(project_membership_exists(&db, stranger, workspace_wide)
            .await
            .expect("e"));
        assert!(!project_membership_exists(&db, user, workspace_wide)
            .await
            .expect("e"));

        assert!(identifier_membership(&db, ws, user, "ACME")
            .await
            .expect("i"));
        assert!(!identifier_membership(&db, ws, user, "OTH")
            .await
            .expect("i"));
        assert!(!identifier_membership(&db, ws, user, "acme")
            .await
            .expect("i"));
    }
}
