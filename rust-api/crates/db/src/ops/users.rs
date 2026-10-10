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

use serde_json::Value;

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

/// The raw `users` columns one lookup decodes.
type UserRowTuple = (
    uuid::Uuid,
    String,
    String,
    bool,
    bool,
    bool,
    String,
    Option<chrono::DateTime<chrono::Utc>>,
);

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
    pub token_updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The `User.save()` outputs the commands persist
/// (`db/models/user.py:169-187`), computed by the services kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSaveWrite {
    pub email: String,
    pub display_name: String,
    pub is_staff: bool,
    pub token: String,
    pub token_updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A `projects` row reference: the id plus its workspace for the
/// membership guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectRef {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
}

// ---------------------------------------------------------------------------
// User reads (`User.objects.filter(email=...).first()`, no soft delete)
// ---------------------------------------------------------------------------

/// `User.objects.filter(email=email).first()` (`activate_user.py:28`,
/// `reset_password.py:35`, `create_instance_admin.py:26`,
/// `create_project_member.py:41`): `Meta.ordering = ("-created_at",)`.
pub async fn user_by_email(
    pool: &sqlx::PgPool,
    email: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    let row: Option<UserRowTuple> = sqlx::query_as(
        r#"SELECT "id", "email", "display_name", "is_staff", "is_superuser",
                  "is_active", "token", "token_updated_at"
           FROM "users" WHERE "email" = $1
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(id, email, display_name, is_staff, is_superuser, is_active, token, token_updated_at)| {
            UserRow {
                id,
                email,
                display_name,
                is_staff,
                is_superuser,
                is_active,
                token,
                token_updated_at,
            }
        },
    ))
}

/// `User.objects.filter(email=creator).exists()`
/// (`create_dummy_data.py:29`): the explicit existence probe before
/// the `get()` below — ported as its own query, as written.
pub async fn user_exists_by_email(pool: &sqlx::PgPool, email: &str) -> Result<bool, sqlx::Error> {
    let exists: bool =
        sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM "users" WHERE "email" = $1)"#)
            .bind(email)
            .fetch_one(pool)
            .await?;
    Ok(exists)
}

// ---------------------------------------------------------------------------
// User writes (`user.is_active = True; user.save()` / `set_password`)
// ---------------------------------------------------------------------------

/// `user.is_active = True; user.save()` (`activate_user.py:35-36`):
/// `save()` rewrites the row with the normalized email, filled
/// display name, staff escalation, possible token rotation, and the
/// `auto_now` `updated_at`.
pub async fn save_user_activation(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    write: &UserSaveWrite,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now();
    sqlx::query(
        r#"UPDATE "users" SET "is_active" = TRUE, "email" = $2, "display_name" = $3,
                  "is_staff" = $4, "token" = $5, "token_updated_at" = $6, "updated_at" = $7
           WHERE "id" = $1"#,
    )
    .bind(id)
    .bind(&write.email)
    .bind(&write.display_name)
    .bind(write.is_staff)
    .bind(&write.token)
    .bind(write.token_updated_at)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// `user.set_password(password); user.is_password_autoset = False;
/// user.save()` (`reset_password.py:62-64`).
pub async fn save_user_password(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    encoded_password: &str,
    write: &UserSaveWrite,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now();
    sqlx::query(
        r#"UPDATE "users" SET "password" = $2, "is_password_autoset" = FALSE,
                  "email" = $3, "display_name" = $4, "is_staff" = $5,
                  "token" = $6, "token_updated_at" = $7, "updated_at" = $8
           WHERE "id" = $1"#,
    )
    .bind(id)
    .bind(encoded_password)
    .bind(&write.email)
    .bind(&write.display_name)
    .bind(write.is_staff)
    .bind(&write.token)
    .bind(write.token_updated_at)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Instance admin (`create_instance_admin.py:32-35`)
// ---------------------------------------------------------------------------

/// `Instance.objects.last()` (`create_instance_admin.py:32`):
/// `Meta.ordering = ("-created_at",)` reversed, i.e. the OLDEST live
/// row — `ORDER BY created_at ASC LIMIT 1`.
pub async fn oldest_instance(pool: &sqlx::PgPool) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let id: Option<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT "id" FROM "instances" WHERE "deleted_at" IS NULL
           ORDER BY "created_at" ASC LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?
    .flatten();
    Ok(id)
}

/// The `get()` half of `get_or_create(user=user, instance=instance,
/// role=20)`: all three kwargs are lookup fields.
pub async fn instance_admin_get(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    instance_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let id: Option<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT "id" FROM "instance_admins"
           WHERE "user_id" = $1 AND "instance_id" = $2 AND "role" = 20
             AND "deleted_at" IS NULL LIMIT 1"#,
    )
    .bind(user_id)
    .bind(instance_id)
    .fetch_optional(pool)
    .await?
    .flatten();
    Ok(id)
}

/// The `create()` half: `role=20`, `is_verified=False`, audit
/// user `NULL`. A missing instance passes `NULL`, and Postgres
/// raises the not-null violation the command prints.
pub async fn instance_admin_create(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    instance_id: Option<uuid::Uuid>,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    sqlx::query(
        r#"INSERT INTO "instance_admins"
           ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
            "user_id", "instance_id", "role", "is_verified")
           VALUES ($1,$2,$2,NULL,NULL,NULL, $3,$4,20,FALSE)"#,
    )
    .bind(id)
    .bind(now)
    .bind(user_id)
    .bind(instance_id)
    .execute(pool)
    .await?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Project member (`create_project_member.py:41-65`)
// ---------------------------------------------------------------------------

/// `Project.objects.filter(pk=project_id).first()`
/// (`create_project_member.py:46`).
pub async fn project_lookup(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
) -> Result<Option<ProjectRef>, sqlx::Error> {
    let row: Option<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        r#"SELECT "id", "workspace_id" FROM "projects"
           WHERE "id" = $1 AND "deleted_at" IS NULL
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, workspace_id)| ProjectRef { id, workspace_id }))
}

/// `WorkspaceMember.objects.filter(workspace=project.workspace,
/// member=user, is_active=True).exists()` (`:51`).
pub async fn workspace_member_active_exists(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "workspace_members"
           WHERE "workspace_id" = $1 AND "member_id" = $2 AND "is_active" = TRUE
             AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// `ProjectMember.objects.filter(project=project,
/// member=user).exists()` (`:55`).
pub async fn project_member_exists(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "project_members"
           WHERE "project_id" = $1 AND "member_id" = $2 AND "deleted_at" IS NULL)"#,
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// `ProjectMember.objects.filter(project=project,
/// member=user).update(is_active=True, role=role)` (`:57-59`):
/// queryset `update()` touches ONLY the two columns — no `save()`,
/// no `auto_now` bump. `None` binds `NULL` (the BUG-2 write, which
/// Postgres rejects). Bound as `int8`: in-range values assign to
/// the `smallint` column, out-of-range ones raise the server's
/// `smallint out of range`, exactly as the psycopg-bound values do.
pub async fn project_member_update(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
    role: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE "project_members" SET "is_active" = TRUE, "role" = $3
           WHERE "project_id" = $1 AND "member_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(project_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

/// `ProjectMember.objects.create(project=project, member=user,
/// role=role)` (`:62`): model-default JSON (project-flavored
/// `view_props`/`default_props`), `comment NULL`, `sort_order
/// 65535`, `is_active TRUE`. The `save()` hook's property row is a
/// separate call the `bin` layer runs first, in the same order.
#[allow(clippy::too_many_arguments)]
pub async fn project_member_create(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
    role: Option<i64>,
    props: &Value,
    preferences: &Value,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    sqlx::query(
        r#"INSERT INTO "project_members"
           ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
            "project_id", "workspace_id", "member_id", "comment", "role",
            "view_props", "default_props", "preferences", "sort_order", "is_active")
           VALUES ($1,$2,$2,NULL,NULL,NULL, $3,$4,$5,NULL,$6, $7,$7,$8,65535,TRUE)"#,
    )
    .bind(id)
    .bind(now)
    .bind(project_id)
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .bind(sqlx::types::Json(props))
    .bind(sqlx::types::Json(preferences))
    .execute(pool)
    .await?;
    Ok(id)
}

/// `ProjectUserProperty.objects.filter(workspace_id=..., user=...)
/// .aggregate(Min("sort_order"))` — the `ProjectMember.save()` hook
/// (`db/models/project.py:348-356`): the minimum is per
/// (workspace, member), and `None` means the default `65535`.
pub async fn property_min_sort(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Option<f64>, sqlx::Error> {
    let min: Option<f64> = sqlx::query_scalar(
        r#"SELECT MIN("sort_order") FROM "project_user_properties"
           WHERE "workspace_id" = $1 AND "user_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .flatten();
    Ok(min)
}

/// The `get()` half of `get_or_create(user=user, project=project)`
/// (`:65`).
pub async fn property_get(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    project_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, sqlx::Error> {
    let id: Option<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT "id" FROM "project_user_properties"
           WHERE "user_id" = $1 AND "project_id" = $2 AND "deleted_at" IS NULL LIMIT 1"#,
    )
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await?
    .flatten();
    Ok(id)
}

/// The hook's `ProjectUserProperty.objects.create(workspace_id=...,
/// project=..., user=..., sort_order=...)` and the `get_or_create`
/// fallback insert: identical shapes, so one function serves both.
/// `rich_filters` is `{}`, `sort_order` is the hook value (or the
/// `65535` model default on the fallback path).
#[allow(clippy::too_many_arguments)]
pub async fn property_create(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
    sort_order: f64,
    filters: &Value,
    display_filters: &Value,
    display_properties: &Value,
    preferences: &Value,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    sqlx::query(
        r#"INSERT INTO "project_user_properties"
           ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
            "workspace_id", "project_id", "user_id",
            "filters", "display_filters", "display_properties", "rich_filters",
            "preferences", "sort_order")
           VALUES ($1,$2,$2,NULL,NULL,NULL, $3,$4,$5, $6,$7,$8,'{}', $9,$10)"#,
    )
    .bind(id)
    .bind(now)
    .bind(workspace_id)
    .bind(project_id)
    .bind(user_id)
    .bind(sqlx::types::Json(filters))
    .bind(sqlx::types::Json(display_filters))
    .bind(sqlx::types::Json(display_properties))
    .bind(sqlx::types::Json(preferences))
    .bind(sort_order)
    .execute(pool)
    .await?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Dummy data scaffolding (`create_dummy_data.py:24-45`)
// ---------------------------------------------------------------------------

/// `Workspace.objects.filter(slug=slug).exists()` (`:24`).
pub async fn workspace_slug_exists(pool: &sqlx::PgPool, slug: &str) -> Result<bool, sqlx::Error> {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "workspaces" WHERE "slug" = $1 AND "deleted_at" IS NULL)"#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// `User.objects.filter(email__in=members)`
/// (`create_dummy_data.py:40`): ids in the queryset's default
/// ordering; unknown addresses simply resolve to nothing.
pub async fn user_ids_by_emails(
    pool: &sqlx::PgPool,
    emails: &[String],
) -> Result<Vec<uuid::Uuid>, sqlx::Error> {
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT "id" FROM "users" WHERE "email" = ANY($1) ORDER BY "created_at" DESC"#,
    )
    .bind(emails)
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

/// `Workspace.objects.create(slug=..., name=..., owner=user)`
/// (`:37`): `timezone UTC`, random `background_color`, `logo` /
/// `organization_size` `NULL`.
pub async fn workspace_create(
    pool: &sqlx::PgPool,
    name: &str,
    slug: &str,
    owner_id: uuid::Uuid,
    background_color: &str,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    sqlx::query(
        r#"INSERT INTO "workspaces"
           ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
            "name", "logo", "logo_asset_id", "owner_id", "slug",
            "organization_size", "timezone", "background_color")
           VALUES ($1,$2,$2,NULL,NULL,NULL, $3,NULL,NULL,$4,$5, NULL,'UTC',$6)"#,
    )
    .bind(id)
    .bind(now)
    .bind(name)
    .bind(owner_id)
    .bind(slug)
    .bind(background_color)
    .execute(pool)
    .await?;
    Ok(id)
}

/// `WorkspaceMember.objects.create(workspace=..., role=20,
/// member=user)` (`:39`) and the `bulk_create` rows (`:42-45`):
/// workspace-flavored `view_props`/`default_props`, `issue_props`,
/// `company_role NULL`, checklists `{}`. Single-row shape shared by
/// both; the bulk path below repeats it per member.
#[allow(clippy::too_many_arguments)]
pub async fn workspace_member_create(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
    role: i32,
    props: &Value,
    issue_props: &Value,
) -> Result<uuid::Uuid, sqlx::Error> {
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    sqlx::query(
        r#"INSERT INTO "workspace_members"
           ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
            "workspace_id", "member_id", "role", "company_role",
            "view_props", "default_props", "issue_props", "is_active",
            "getting_started_checklist", "tips", "explored_features")
           VALUES ($1,$2,$2,NULL,NULL,NULL, $3,$4,$5,NULL, $6,$6,$7,TRUE, '{}','{}','{}')"#,
    )
    .bind(id)
    .bind(now)
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .bind(sqlx::types::Json(props))
    .bind(sqlx::types::Json(issue_props))
    .execute(pool)
    .await?;
    Ok(id)
}

/// `WorkspaceMember.objects.bulk_create([...], ignore_conflicts=True)`
/// (`:42-45`): one multi-row `INSERT ... ON CONFLICT DO NOTHING`.
/// An empty list emits no statement at all, like `bulk_create([])`.
pub async fn workspace_members_bulk_create(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
    user_ids: &[uuid::Uuid],
    props: &Value,
    issue_props: &Value,
) -> Result<(), sqlx::Error> {
    if user_ids.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now();
    // One multi-row INSERT with positional parameters, like
    // `bulk_create` emits (batch size unbounded here).
    let mut sql = String::from(
        r#"INSERT INTO "workspace_members"
           ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at",
            "workspace_id", "member_id", "role", "company_role",
            "view_props", "default_props", "issue_props", "is_active",
            "getting_started_checklist", "tips", "explored_features")
           VALUES "#,
    );
    for (index, _) in user_ids.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        let base = index * 5 + 1;
        sql.push_str(&format!(
            "(gen_random_uuid(),${b},${b},NULL,NULL,NULL, ${w},${m},20,NULL, ${p},${p},${i},TRUE, '{{}}','{{}}','{{}}')",
            b = base,
            w = base + 1,
            m = base + 2,
            p = base + 3,
            i = base + 4,
        ));
    }
    sql.push_str(" ON CONFLICT DO NOTHING");
    let mut query = sqlx::query(&sql);
    for user_id in user_ids {
        query = query
            .bind(now)
            .bind(workspace_id)
            .bind(user_id)
            .bind(sqlx::types::Json(props))
            .bind(sqlx::types::Json(issue_props));
    }
    query.execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
