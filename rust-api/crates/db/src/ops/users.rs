//! Users + membership ops SQL (D-37, F37-03/F37-04).
//!
//! Row reads and writes for `db/management/commands/{activate_user,
//! reset_password,create_instance_admin,create_project_member,
//! create_dummy_data}.py`, one function per ORM statement. Scopes
//! mirror the managers: every model here except `users` reads through
//! `SoftDeletionManager` (`deleted_at IS NULL`); `users` has no soft
//! delete. Default orderings (`-created_at`) are kept where Django
//! emits them. Management commands run with no request user, so
//! `created_by`/`updated_by` are always `NULL` (crum
//! `get_current_user()` is `None`). Each statement is its own
//! autocommit write — none of the commands wraps a transaction, so
//! partial writes persist exactly like Django.
//!
//! JSON documents come from the model field-default callables; each
//! builder returns a fresh document per call.
//!
//! Every executor is SeaORM over `sea_orm_connection(pool)` with the
//! `sqlx::Error` signatures kept through `orm_tx::into_sqlx`
//! (`orm-migration.md`); row structs are narrow `FromQueryResult`
//! models decoded by name, pinned by statement-shape tests below.

use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, QueryFilter, QueryOrder,
    QuerySelect, Select,
};
use sea_query::{Expr, Query as SeaQuery};
use serde_json::Value;

use crate::entity::{
    instance_admins, instances, project_members, project_user_properties, projects, users,
    workspace_members, workspaces,
};
use crate::orm_tx::into_sqlx;
use crate::pool::sea_orm_connection;

// ---------------------------------------------------------------------------
// JSON model defaults (fresh document per call, like the callables)
// ---------------------------------------------------------------------------

/// `view_props`/`default_props` for `ProjectMember`
/// (`db/models/project.py:43-65`, `get_default_props`): filters +
/// display filters only — no `display_properties` key.
pub fn project_member_props_json() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": ""
        }
    })
}

/// `preferences` for `ProjectMember` / `ProjectUserProperty`
/// (`project.py:get_default_preferences`).
pub fn project_preferences_json() -> Value {
    serde_json::json!({
        "pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}
    })
}

/// `filters` for `ProjectUserProperty`
/// (`db/models/issue.py:50-61`, `get_default_filters`).
pub fn property_filters_json() -> Value {
    serde_json::json!({
        "priority": null, "state": null, "state_group": null,
        "assignees": null, "created_by": null, "labels": null,
        "start_date": null, "target_date": null, "subscriber": null
    })
}

/// `display_filters` for `ProjectUserProperty` (`issue.py:64-73`).
pub fn property_display_filters_json() -> Value {
    serde_json::json!({
        "group_by": null, "order_by": "-created_at", "type": null,
        "sub_issue": true, "show_empty_groups": true,
        "layout": "list", "calendar_date_range": ""
    })
}

/// `display_properties` for `ProjectUserProperty` (`issue.py:76-91`):
/// `sub_issue_count`, exactly as the model default spells it.
pub fn property_display_properties_json() -> Value {
    serde_json::json!({
        "assignee": true, "attachment_count": true, "created_on": true,
        "due_date": true, "estimate": true, "key": true, "labels": true,
        "link": true, "priority": true, "start_date": true, "state": true,
        "sub_issue_count": true, "updated_on": true
    })
}

/// `view_props`/`default_props` for `WorkspaceMember`
/// (`db/models/workspace.py:22-58`, `get_default_props`): filters +
/// display filters + display properties.
pub fn workspace_member_props_json() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": ""
        },
        "display_properties": {
            "assignee": true, "attachment_count": true, "created_on": true,
            "due_date": true, "estimate": true, "key": true, "labels": true,
            "link": true, "priority": true, "start_date": true, "state": true,
            "sub_issue_count": true, "updated_on": true
        }
    })
}

/// `issue_props` for `WorkspaceMember` (`workspace.py:get_issue_props`).
pub fn workspace_issue_props_json() -> Value {
    serde_json::json!({
        "subscribed": true, "assigned": true, "created": true, "all_issues": true
    })
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// Decoded columns the commands read; narrow by design, so e.g. the
/// `password` column never crosses the wire (`orm-migration.md` §3.1).
#[derive(Debug, Clone, FromQueryResult)]
struct UserLookupRow {
    id: uuid::Uuid,
    email: String,
    display_name: String,
    is_staff: bool,
    is_superuser: bool,
    is_active: bool,
    token: String,
    token_updated_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
}

/// A `projects` row reference: the id plus its workspace for the membership guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectRef {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
}

#[derive(Debug, Clone, FromQueryResult)]
struct ProjectLookupRow {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
}

#[derive(Debug, Clone, FromQueryResult)]
struct MinSortRow {
    min_sort_order: Option<f64>,
}

/// The `users` columns the commands read: the `User.save()` inputs
/// plus the flags the branches check.
#[derive(Debug, Clone, PartialEq)]
pub struct UserRow {
    pub id: uuid::Uuid,
    pub email: String,
    pub display_name: String,
    pub is_staff: bool,
    pub is_superuser: bool,
    pub is_active: bool,
    pub token: String,
    pub token_updated_at: Option<DateTime<Utc>>,
}

/// The `User.save()` outputs the commands persist
/// (`db/models/user.py:169-187`), computed by the services kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSaveWrite {
    pub email: String,
    pub display_name: String,
    pub is_staff: bool,
    pub token: String,
    pub token_updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// User reads (`User.objects.filter(email=...).first()`, no soft delete)
// ---------------------------------------------------------------------------

/// `User.objects.filter(email=email).first()` (`activate_user.py:28`,
/// `reset_password.py:35`, `create_instance_admin.py:26`,
/// `create_project_member.py:41`): `Meta.ordering = ("-created_at",)`.
fn user_by_email_query(email: &str) -> Select<users::Entity> {
    users::Entity::find()
        .select_only()
        .columns([
            users::Column::Id,
            users::Column::Email,
            users::Column::DisplayName,
            users::Column::IsStaff,
            users::Column::IsSuperuser,
            users::Column::IsActive,
            users::Column::Token,
            users::Column::TokenUpdatedAt,
        ])
        .filter(users::Column::Email.eq(email.to_owned()))
        .order_by_desc(users::Column::CreatedAt)
}

pub async fn user_by_email(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let row = user_by_email_query(email)
        .into_model::<UserLookupRow>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(row.map(|row| UserRow {
        id: row.id,
        email: row.email,
        display_name: row.display_name,
        is_staff: row.is_staff,
        is_superuser: row.is_superuser,
        is_active: row.is_active,
        token: row.token,
        token_updated_at: row.token_updated_at.map(|stamp| stamp.with_timezone(&Utc)),
    }))
}

/// `User.objects.filter(email=creator).exists()`
/// (`create_dummy_data.py:29`): the explicit existence probe before
/// the `get()` below — ported as its own query, as written.
pub async fn user_exists_by_email(pool: &sqlx::PgPool, email: &str) -> Result<bool, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let hit = users::Entity::find()
        .select_only()
        .column(users::Column::Id)
        .limit(1)
        .filter(users::Column::Email.eq(email.to_owned()))
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(hit.is_some())
}

// NOTE: the `save_user_activation` / `save_user_password` writes lived
// here; they moved to `pidash_services::domain::identity` (PIDASHCONV-1631).

// ---------------------------------------------------------------------------
// Instance admin (`create_instance_admin.py:32-35`)
// ---------------------------------------------------------------------------

/// `Instance.objects.last()` (`create_instance_admin.py:32`):
/// `Meta.ordering = ("-created_at",)` reversed, i.e. the OLDEST live
/// row — `ORDER BY created_at ASC LIMIT 1`.
fn oldest_instance_query() -> Select<instances::Entity> {
    instances::Entity::find()
        .filter(instances::Column::DeletedAt.is_null())
        .order_by_asc(instances::Column::CreatedAt)
}

pub async fn oldest_instance(pool: &sqlx::PgPool) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let id = oldest_instance_query()
        .select_only()
        .column(instances::Column::Id)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

/// The `get()` half of `get_or_create(user=user, instance=instance,
/// role=20)`: all three kwargs are lookup fields.
fn instance_admin_get_query(
    user_id: uuid::Uuid,
    instance_id: uuid::Uuid,
) -> Select<instance_admins::Entity> {
    instance_admins::Entity::find()
        .filter(instance_admins::Column::UserId.eq(user_id))
        .filter(instance_admins::Column::InstanceId.eq(instance_id))
        .filter(instance_admins::Column::Role.eq(20))
        .filter(instance_admins::Column::DeletedAt.is_null())
}

pub async fn instance_admin_get(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    instance_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let id = instance_admin_get_query(user_id, instance_id)
        .select_only()
        .column(instance_admins::Column::Id)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

/// The `create()` half: `role=20`, `is_verified=False`, audit
/// user `NULL`. A missing instance passes `NULL`, and Postgres
/// raises the not-null violation the command prints.
///
/// The insert is a sea-query `InsertStatement`, not an `ActiveModel`:
/// `instance_id` is `NOT NULL` in the entity, so the `None` bind a
/// missing instance relies on cannot be expressed as `Set` — and
/// building that bind is the whole point of the branch.
pub async fn instance_admin_create(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    instance_id: Option<uuid::Uuid>,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let mut stmt = SeaQuery::insert();
    stmt.into_table(instance_admins::Entity);
    stmt.columns([
        instance_admins::Column::Id,
        instance_admins::Column::CreatedAt,
        instance_admins::Column::UpdatedAt,
        instance_admins::Column::CreatedById,
        instance_admins::Column::UpdatedById,
        instance_admins::Column::DeletedAt,
        instance_admins::Column::UserId,
        instance_admins::Column::InstanceId,
        instance_admins::Column::Role,
        instance_admins::Column::IsVerified,
    ]);
    stmt.values_panic([
        id.into(),
        now.into(),
        now.into(),
        Expr::val(None::<uuid::Uuid>).into(),
        Expr::val(None::<uuid::Uuid>).into(),
        Expr::val(None::<sea_orm::prelude::DateTimeWithTimeZone>).into(),
        user_id.into(),
        instance_id
            .map(|id| Expr::val(id).into())
            .unwrap_or_else(|| Expr::val(None::<uuid::Uuid>).into()),
        20.into(),
        false.into(),
    ]);
    let stmt = stmt.to_owned();
    let db = sea_orm_connection(pool);
    db.execute(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Project member (`create_project_member.py:41-65`)
// ---------------------------------------------------------------------------

/// `Project.objects.filter(pk=project_id).first()`
/// (`create_project_member.py:46`).
fn project_lookup_query(project_id: uuid::Uuid) -> Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .columns([projects::Column::Id, projects::Column::WorkspaceId])
        .filter(projects::Column::Id.eq(project_id))
        .filter(projects::Column::DeletedAt.is_null())
        .order_by_desc(projects::Column::CreatedAt)
}

pub async fn project_lookup(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
) -> Result<Option<ProjectRef>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let row = project_lookup_query(project_id)
        .into_model::<ProjectLookupRow>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(row.map(|row| ProjectRef {
        id: row.id,
        workspace_id: row.workspace_id,
    }))
}

/// `WorkspaceMember.objects.filter(workspace=project.workspace,
/// member=user, is_active=True).exists()` (`:51`).
fn workspace_member_active_exists_query(
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Select<workspace_members::Entity> {
    workspace_members::Entity::find()
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .filter(workspace_members::Column::MemberId.eq(user_id))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
}

pub async fn workspace_member_active_exists(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let hit = workspace_member_active_exists_query(workspace_id, user_id)
        .select_only()
        .column(workspace_members::Column::Id)
        .limit(1)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(hit.is_some())
}

/// `ProjectMember.objects.filter(project=project,
/// member=user).exists()` (`:55`).
fn project_member_exists_query(
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Select<project_members::Entity> {
    project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::DeletedAt.is_null())
}

pub async fn project_member_exists(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let hit = project_member_exists_query(project_id, user_id)
        .select_only()
        .column(project_members::Column::Id)
        .limit(1)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(hit.is_some())
}

/// `ProjectUserProperty.objects.filter(workspace_id=..., user=...)
/// .aggregate(Min("sort_order"))` — the `ProjectMember.save()` hook
/// (`db/models/project.py:348-356`): the minimum is per
/// (workspace, member), and `None` means the default `65535`.
fn property_min_sort_query(
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Select<project_user_properties::Entity> {
    project_user_properties::Entity::find()
        .filter(project_user_properties::Column::WorkspaceId.eq(workspace_id))
        .filter(project_user_properties::Column::UserId.eq(user_id))
        .filter(project_user_properties::Column::DeletedAt.is_null())
        .select_only()
        .column_as(
            project_user_properties::Column::SortOrder.min(),
            "min_sort_order",
        )
}

pub async fn property_min_sort(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<f64>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let row = property_min_sort_query(workspace_id, user_id)
        .into_model::<MinSortRow>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(row.and_then(|row| row.min_sort_order))
}

/// The `get()` half of `get_or_create(user=user, project=project)`
/// (`:65`).
fn property_get_query(
    user_id: uuid::Uuid,
    project_id: uuid::Uuid,
) -> Select<project_user_properties::Entity> {
    project_user_properties::Entity::find()
        .filter(project_user_properties::Column::UserId.eq(user_id))
        .filter(project_user_properties::Column::ProjectId.eq(project_id))
        .filter(project_user_properties::Column::DeletedAt.is_null())
}

pub async fn property_get(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    project_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let id = property_get_query(user_id, project_id)
        .select_only()
        .column(project_user_properties::Column::Id)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

// Dummy data scaffolding (`create_dummy_data.py:24-45`)
// ---------------------------------------------------------------------------

/// `Workspace.objects.filter(slug=slug).exists()` (`:24`).
pub async fn workspace_slug_exists(pool: &sqlx::PgPool, slug: &str) -> Result<bool, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let hit = workspaces::Entity::find()
        .filter(workspaces::Column::Slug.eq(slug.to_owned()))
        .filter(workspaces::Column::DeletedAt.is_null())
        .select_only()
        .column(workspaces::Column::Id)
        .limit(1)
        .into_tuple::<uuid::Uuid>()
        .one(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(hit.is_some())
}

/// `User.objects.filter(email__in=members)`
/// (`create_dummy_data.py:40`): ids in the queryset's default
/// ordering; unknown addresses simply resolve to nothing.
pub async fn user_ids_by_emails(
    pool: &sqlx::PgPool,
    emails: &[String],
) -> Result<Vec<uuid::Uuid>, sqlx::Error> {
    if emails.is_empty() {
        return Ok(Vec::new());
    }
    let db = sea_orm_connection(pool);
    let ids = users::Entity::find()
        .filter(users::Column::Email.is_in(emails.to_vec()))
        .order_by_desc(users::Column::CreatedAt)
        .select_only()
        .column(users::Column::Id)
        .into_tuple::<uuid::Uuid>()
        .all(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    fn shape(query: Select<users::Entity>) -> String {
        query.build(DbBackend::Postgres).to_string()
    }

    #[test]
    fn member_json_defaults_match_model_callables() {
        let props = project_member_props_json();
        assert!(props.get("filters").is_some());
        assert!(props.get("display_filters").is_some());
        assert!(props.get("display_properties").is_none());
        assert_eq!(
            props["display_filters"]["order_by"],
            Value::String("-created_at".to_owned())
        );
        assert_eq!(props["filters"]["subscriber"], Value::Null);
        let prefs = project_preferences_json();
        assert_eq!(prefs["pages"]["block_display"], Value::Bool(true));
        assert_eq!(
            prefs["navigation"]["default_tab"],
            Value::String("work_items".to_owned())
        );
    }

    #[test]
    fn property_json_defaults_match_issue_callables() {
        assert_eq!(property_filters_json()["subscriber"], Value::Null);
        assert_eq!(
            property_display_filters_json()["sub_issue"],
            Value::Bool(true)
        );
        let display = property_display_properties_json();
        assert_eq!(display["sub_issue_count"], Value::Bool(true));
        assert!(display.get("sub_issue").is_none());
    }

    #[test]
    fn workspace_member_json_defaults_match_callables() {
        let props = workspace_member_props_json();
        assert_eq!(
            props["display_properties"]["sub_issue_count"],
            Value::Bool(true)
        );
        let issue_props = workspace_issue_props_json();
        for key in ["subscribed", "assigned", "created", "all_issues"] {
            assert_eq!(issue_props[key], Value::Bool(true));
        }
    }

    #[test]
    fn json_defaults_are_fresh_per_call() {
        let mut first = project_member_props_json();
        first["filters"]["priority"] = Value::String("mutated".to_owned());
        assert_eq!(
            project_member_props_json()["filters"]["priority"],
            Value::Null
        );
    }

    #[test]
    fn user_by_email_selects_narrow_and_orders_newest_first() {
        let sql = shape(user_by_email_query("a@example.test"));
        assert!(sql.contains(r#"FROM "users""#), "{sql}");
        assert!(sql.contains(r#""email" = 'a@example.test'"#), "{sql}");
        assert!(sql.contains(r#""created_at" DESC"#), "{sql}");
        // Narrow projection: no `password` column crosses the wire.
        assert!(!sql.contains(r#""password""#), "{sql}");
    }

    #[test]
    fn instance_admin_get_scopes_user_instance_role_and_liveness() {
        let user = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let instance = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        let sql = instance_admin_get_query(user, instance)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""instance_admins""#), "{sql}");
        assert!(sql.contains(&format!(r#""user_id" = '{user}'"#)), "{sql}");
        assert!(
            sql.contains(&format!(r#""instance_id" = '{instance}'"#)),
            "{sql}"
        );
        assert!(sql.contains(r#""role" = 20"#), "{sql}");
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
    }

    #[test]
    fn oldest_instance_orders_oldest_live_first() {
        let sql = oldest_instance_query()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""instances""#), "{sql}");
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""created_at" ASC"#), "{sql}");
    }

    #[test]
    fn property_min_sort_aggregates_live_rows_only() {
        let workspace =
            uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let user = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        let sql = property_min_sort_query(workspace, user)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""project_user_properties""#), "{sql}");
        assert!(
            sql.contains(r#"MIN("project_user_properties"."sort_order")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
    }

    #[test]
    fn workspace_member_exists_scopes_active_flag() {
        let workspace =
            uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let user = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        let sql = workspace_member_active_exists_query(workspace, user)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""is_active""#), "{sql}");
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
    }
}
