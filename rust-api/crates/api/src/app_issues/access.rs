//! The app issue endpoints' tenant and permission reads, through SeaORM.
//!
//! `mod.rs` and `handlers_reads.rs` each carried a hand-written copy of
//! these (project-by-identifier, the project role behind
//! `allow_permission` / `ProjectEntityPermission`, the workspace-admin
//! fallback, the actor's timezone, the project's tenant facts); they now
//! share this one set. Every membership read applies the
//! `SoftDeletionManager` scope (`deleted_at IS NULL`) and `is_active`.

use pidash_db::entity::{
    cycles, issue_types, labels, modules, project_members, projects, states, users,
    workspace_members, workspaces,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, JoinType, QueryFilter, QuerySelect,
    RelationTrait,
};
use uuid::Uuid;

/// `Project.objects.get(workspace__slug=slug, identifier=identifier)`: the
/// live project's id.
pub async fn project_id_by_identifier(
    db: &impl ConnectionTrait,
    slug: &str,
    identifier: &str,
) -> Result<Option<Uuid>, DbErr> {
    projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(projects::Column::Identifier.eq(identifier))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await
}

/// The actor's active, live project membership role in this workspace
/// slug, if any.
pub async fn project_role(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i16>, DbErr> {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Role)
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Workspaces.def(),
        )
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await
}

/// The actor's active, live workspace membership role in this slug, if
/// any (one live row per workspace and member).
pub async fn workspace_role(
    db: &impl ConnectionTrait,
    slug: &str,
    user_id: Uuid,
) -> Result<Option<i16>, DbErr> {
    workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Role)
        .join(
            JoinType::InnerJoin,
            workspace_members::Relation::Workspaces.def(),
        )
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await
}

/// An active, live workspace ADMIN (role 20) membership in this slug.
pub async fn is_workspace_admin(
    db: &impl ConnectionTrait,
    slug: &str,
    user_id: Uuid,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Id)
        .join(
            JoinType::InnerJoin,
            workspace_members::Relation::Workspaces.def(),
        )
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(workspace_members::Column::Role.eq(20i16))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

/// `allow_permission(allowed)` at `PROJECT` level (`app/permissions/base.py`):
/// a listed project role, or any active project membership plus an active
/// workspace ADMIN membership.
pub async fn allows(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
    allowed: &[i32],
) -> Result<bool, DbErr> {
    let role = project_role(db, slug, project_id, user_id).await?;
    if role.is_some_and(|role| allowed.contains(&i32::from(role))) {
        return Ok(true);
    }
    // The fallback's "any active project membership" is the same row the
    // role read just looked for.
    Ok(role.is_some() && is_workspace_admin(db, slug, user_id).await?)
}

/// The actor's `user_timezone`.
pub async fn user_timezone(
    db: &impl ConnectionTrait,
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

/// A live project's `(workspace_id, guest_view_all_features)`.
pub async fn project_tenant(
    db: &impl ConnectionTrait,
    project_id: Uuid,
) -> Result<Option<(Uuid, bool)>, DbErr> {
    projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::WorkspaceId,
            projects::Column::GuestViewAllFeatures,
        ])
        .filter(projects::Column::Id.eq(project_id))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await
}

/// The actor's active, live project role (any workspace), for the guest
/// scoping in the tenant context.
pub async fn project_role_any_slug(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i16>, DbErr> {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Role)
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .into_tuple()
        .one(db)
        .await
}

/// `assigned_runner_detail` by runner id (`issue_assignment.py`,
/// design §14.1): `None` when the issue is pod-assigned.
pub async fn fetch_assigned_runner_detail(
    db: &impl ConnectionTrait,
    runner_id: Option<Uuid>,
) -> Result<Option<serde_json::Value>, DbErr> {
    let Some(runner_id) = runner_id else {
        return Ok(None);
    };
    let Some(runner) = pidash_db::entity::runner::Entity::find_by_id(runner_id)
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    pidash_services::domain::issue_assignment::assigned_runner_detail(db, &runner)
        .await
        .map(Some)
}

/// `IssueType.is_epic` by id.
pub async fn issue_type_is_epic(
    db: &impl ConnectionTrait,
    type_id: Uuid,
) -> Result<Option<bool>, DbErr> {
    issue_types::Entity::find()
        .select_only()
        .column(issue_types::Column::IsEpic)
        .filter(issue_types::Column::Id.eq(type_id))
        .into_tuple()
        .one(db)
        .await
}

/// The `issue_group_values` id lists (`utils/grouper.py`): every live row
/// of the grouped relation in this workspace slug (and project, when the
/// relation is per-project).
pub async fn non_triage_state_ids(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    states::Entity::find()
        .select_only()
        .column(states::Column::Id)
        .join(JoinType::InnerJoin, states::Relation::Workspaces.def())
        .filter(states::Column::IsTriage.eq(false))
        .filter(states::Column::Group.ne("triage"))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(states::Column::ProjectId.eq(project_id))
        .filter(states::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
}

/// See [`non_triage_state_ids`].
pub async fn label_ids(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    labels::Entity::find()
        .select_only()
        .column(labels::Column::Id)
        .join(JoinType::InnerJoin, labels::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(labels::Column::ProjectId.eq(project_id))
        .filter(labels::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
}

/// See [`non_triage_state_ids`]: active, live project members.
pub async fn member_ids(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
) -> Result<Vec<Option<Uuid>>, DbErr> {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::MemberId)
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Workspaces.def(),
        )
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
}

/// See [`non_triage_state_ids`].
pub async fn module_ids(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    modules::Entity::find()
        .select_only()
        .column(modules::Column::Id)
        .join(JoinType::InnerJoin, modules::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(modules::Column::ProjectId.eq(project_id))
        .filter(modules::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
}

/// See [`non_triage_state_ids`].
pub async fn cycle_ids(
    db: &impl ConnectionTrait,
    slug: &str,
    project_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    cycles::Entity::find()
        .select_only()
        .column(cycles::Column::Id)
        .join(JoinType::InnerJoin, cycles::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(cycles::Column::ProjectId.eq(project_id))
        .filter(cycles::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
}

/// See [`non_triage_state_ids`]: every live project in the workspace.
pub async fn workspace_project_ids(
    db: &impl ConnectionTrait,
    slug: &str,
) -> Result<Vec<Uuid>, DbErr> {
    projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The access reads against constraint-free temp tables on a
    /// one-connection pool.
    #[tokio::test]
    async fn live_access_reads() {
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
            "workspaces",
            "projects",
            "project_members",
            "workspace_members",
            "users",
            "states",
            "labels",
            "issue_types",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (ws, project, admin, guest, stranger) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (open_state, triage_state, epic) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        for sql in [
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO projects (id, workspace_id, identifier, guest_view_all_features, deleted_at) \
                 VALUES ('{project}', '{ws}', 'ACM', true, NULL), (gen_random_uuid(), '{ws}', 'OLD', false, now())"
            ),
            format!(
                "INSERT INTO project_members (id, workspace_id, project_id, member_id, role, is_active, deleted_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{project}', '{admin}', 5, true, NULL), \
                 (gen_random_uuid(), '{ws}', '{project}', '{guest}', 5, true, NULL), \
                 (gen_random_uuid(), '{ws}', '{project}', '{stranger}', 20, true, now())"
            ),
            format!(
                "INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, deleted_at) \
                 VALUES (gen_random_uuid(), '{ws}', '{admin}', 20, true, NULL)"
            ),
            format!("INSERT INTO users (id, user_timezone) VALUES ('{guest}', 'Asia/Tokyo')"),
            format!(
                "INSERT INTO states (id, workspace_id, project_id, \"group\", is_triage, deleted_at) VALUES \
                 ('{open_state}', '{ws}', '{project}', 'started', false, NULL), \
                 ('{triage_state}', '{ws}', '{project}', 'triage', false, NULL), \
                 (gen_random_uuid(), '{ws}', '{project}', 'backlog', true, NULL)"
            ),
            format!("INSERT INTO labels (id, workspace_id, project_id) VALUES (gen_random_uuid(), '{ws}', '{project}')"),
            format!("INSERT INTO issue_types (id, is_epic) VALUES ('{epic}', true)"),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let db = pidash_db::pool::sea_orm_connection(&pool);
        assert_eq!(
            project_id_by_identifier(&db, "acme", "ACM")
                .await
                .expect("read"),
            Some(project)
        );
        assert_eq!(
            project_id_by_identifier(&db, "acme", "OLD")
                .await
                .expect("read"),
            None
        );
        assert_eq!(
            project_id_by_identifier(&db, "other", "ACM")
                .await
                .expect("read"),
            None
        );
        // Guest role 5 passes a guest-allowing gate; an ADMIN-only gate needs
        // the workspace-admin fallback.
        assert!(allows(&db, "acme", project, guest, &[20, 15, 5])
            .await
            .expect("gate"));
        assert!(!allows(&db, "acme", project, guest, &[20])
            .await
            .expect("gate"));
        assert!(allows(&db, "acme", project, admin, &[20])
            .await
            .expect("gate"));
        assert_eq!(
            workspace_role(&db, "acme", admin).await.expect("role"),
            Some(20)
        );
        assert_eq!(
            workspace_role(&db, "other", admin).await.expect("role"),
            None
        );
        assert_eq!(
            workspace_role(&db, "acme", guest).await.expect("role"),
            None
        );
        // A soft-deleted membership does not count.
        assert_eq!(
            project_role(&db, "acme", project, stranger)
                .await
                .expect("role"),
            None
        );
        assert_eq!(
            project_tenant(&db, project).await.expect("tenant"),
            Some((ws, true))
        );
        assert_eq!(
            user_timezone(&db, guest).await.expect("tz").as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(
            project_role_any_slug(&db, project, guest)
                .await
                .expect("role"),
            Some(5)
        );
        // StateManager excludes the triage group; is_triage rows are out too.
        assert_eq!(
            non_triage_state_ids(&db, "acme", project)
                .await
                .expect("states"),
            vec![open_state]
        );
        assert_eq!(
            label_ids(&db, "acme", project).await.expect("labels").len(),
            1
        );
        assert_eq!(
            member_ids(&db, "acme", project)
                .await
                .expect("members")
                .len(),
            2
        );
        assert_eq!(
            workspace_project_ids(&db, "acme").await.expect("projects"),
            vec![project]
        );
        assert_eq!(
            issue_type_is_epic(&db, epic).await.expect("epic"),
            Some(true)
        );
    }
}
