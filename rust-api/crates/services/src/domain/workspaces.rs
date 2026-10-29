//! OSS workspace writes: `workspaces`, `workspace_members` (ORM-07, PIDASHCONV-1525).
//!
//! Every OSS write to the two workspace tables lives in this module; the
//! workspace-create, invite/join, signup-join, member-admin and ops
//! call sites call these functions instead of executing SQL text. Reads
//! stay where they are (design D5).
//!
//! Execution shapes, by writer kind:
//!
//! * [`create`] runs on `&PgPool`: the workspace `INSERT` (via
//!   [`insert`]) plus the `post_save` seeding effect
//!   (`scheduler/signals.py:28-39`) as a direct same-crate call to
//!   [`seed_builtin_schedulers`](crate::scheduler::builtins::seed_builtin_schedulers).
//!   All three create entry points (app, license-config, dummy-data)
//!   are pool-autocommit, so no caller needs a transaction here; the
//!   direct call preserves Django's in-save order (seed runs after the
//!   row lands, before the member row) and the receiver's swallow-all-
//!   errors semantics. There is deliberately no SeaORM `after_save`
//!   hook on the `workspaces` entity: the seeding implementation lives
//!   in this crate and `pidash-db` cannot depend on it (unlike the
//!   users prefs insert, which is plain SQL the hook owns). A future
//!   `WriteTxn` flow would hang the effect on the hook registry
//!   ([`super::HookRegistry`], which names this exact case); until a
//!   caller needs that shape, the direct call is the whole mechanism.
//! * Everything else is generic over `sqlx::Executor`, like
//!   [`super::tokens`]: the row-only workspace insert, the member
//!   insert, the bulk insert, the role / reactivate / deactivate
//!   updates, the platform insert / overwrite / sync /
//!   no-stamp-deactivate shapes, the bulk workspace deactivations,
//!   the slug rename and the name sync all run on the caller's pool
//!   or on the caller's existing `sqlx` transaction (the
//!   join-request approve path holds one; the overlay platform
//!   ensure holds one) with no restructuring.
//!
//! Per-caller deltas (design D6) ride on the input structs. Two Rust-side
//! divergences the survey found are preserved verbatim:
//!
//! * the license-config create stamps `updated_at` from a second
//!   `Utc::now()` where every other path reuses the `created_at` stamp
//!   (separate `created_at`/`updated_at` inputs).
//! * the project-invite join member insert uses DB-side `now()` stamps
//!   where Django (and every other path) binds the request clock
//!   (`db_now`; same failure-arm-only window as the Part-1 stamps).

use chrono::{DateTime, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// insert (executor-generic; row only) + create (pool; INSERT + post_save seeding)
// ---------------------------------------------------------------------------

/// One workspace row to insert: every column without a DB default.
/// `created_at`/`updated_at` are separate inputs because the
/// license-config path stamps them from two `Utc::now()` calls.
pub struct NewWorkspace<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub slug: &'a str,
    pub owner_id: Uuid,
    /// App/license stamp the request user (`BaseModel.save` via crum);
    /// the dummy-data command stamps `NULL` (no request user).
    pub created_by: Option<Uuid>,
    pub logo: Option<&'a str>,
    pub logo_asset_id: Option<Uuid>,
    pub organization_size: Option<&'a str>,
    pub timezone: &'a str,
    pub background_color: &'a str,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The seeding `created_by`: `Some` request user on the HTTP paths
    /// (`BaseModel.save` stamps the crum user on the scheduler rows
    /// too), `None` on crum-less entry points (Django stamps `NULL`).
    /// Only [`create`] reads this; [`insert`] leaves seeding to the
    /// caller and ignores it.
    pub seed_created_by: Option<Uuid>,
}

/// The `RETURNING` stamps of the workspace `INSERT`.
#[derive(Debug)]
pub struct WorkspaceStamps {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const WORKSPACE_INSERT_SQL: &str = "INSERT INTO \"workspaces\" \
    (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
    \"name\", \"logo\", \"logo_asset_id\", \"owner_id\", \"slug\", \
    \"organization_size\", \"timezone\", \"background_color\") \
    VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
    RETURNING \"created_at\", \"updated_at\"";

/// `Workspace.objects.create(...)`: the row, and nothing else. The
/// overlay platform ensure
/// (`_get_or_create_platform_workspace`, `service.py:335-339`) runs
/// this on its own `sqlx` transaction next to the unique-slug probe
/// and the link insert, then seeds the builtins itself after commit;
/// adopting [`create`] would split that transaction and move the
/// seeding. Unique violations propagate as `sqlx::Error` for the
/// caller's 409 arm. The `None` arm preserves the app path's
/// `fetch_optional` shape.
pub async fn insert<'e, E>(
    ex: E,
    ws: &NewWorkspace<'_>,
) -> Result<Option<WorkspaceStamps>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<(DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(WORKSPACE_INSERT_SQL)
        .bind(ws.id)
        .bind(ws.created_at)
        .bind(ws.updated_at)
        .bind(ws.created_by)
        .bind(ws.deleted_at)
        .bind(ws.name)
        .bind(ws.logo)
        .bind(ws.logo_asset_id)
        .bind(ws.owner_id)
        .bind(ws.slug)
        .bind(ws.organization_size)
        .bind(ws.timezone)
        .bind(ws.background_color)
        .fetch_optional(ex)
        .await?;
    Ok(row.map(|(created_at, updated_at)| WorkspaceStamps {
        id: ws.id,
        created_at,
        updated_at,
    }))
}

/// `Workspace.objects.create(...)`: the row plus the `post_save`
/// seeding effect. Unique violations propagate as `sqlx::Error` so
/// each caller keeps its own 409 arm (app JSON vs license `Conflict`).
/// The `None` arm preserves the app path's `fetch_optional` shape.
pub async fn create(
    pool: &sqlx::PgPool,
    ws: &NewWorkspace<'_>,
) -> Result<Option<WorkspaceStamps>, sqlx::Error> {
    let stamps = insert(pool, ws).await?;
    // `post_save` on `Workspace` (`scheduler/signals.py:28-39`) fires
    // during the save above, before the member row below. The receiver
    // swallows every failure, so this never fails the request.
    crate::scheduler::builtins::seed_builtin_schedulers(pool, ws.id, ws.seed_created_by).await;
    Ok(stamps)
}

// ---------------------------------------------------------------------------
// rename (executor-generic; the slug repair write)
// ---------------------------------------------------------------------------

/// `save(update_fields=["slug"])`
/// (`update_deleted_workspace_slug.py:63-64`): the `slug` column only
/// (verified — `updated_at` untouched). Moved from
/// `db::ops::repair::UPDATE_WORKSPACE_SLUG_SQL` (D8: private now).
const RENAME_SQL: &str = "UPDATE workspaces SET slug = $1 WHERE id = $2";

/// Rename one workspace slug, nothing else.
pub async fn rename<'e, E>(ex: E, workspace_id: Uuid, new_slug: &str) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(RENAME_SQL)
        .bind(new_slug)
        .bind(workspace_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// set_name (executor-generic; the platform name sync)
// ---------------------------------------------------------------------------

/// `workspace.save(update_fields=["name", "updated_at"])`
/// (`platform_federation/service.py:332` in the ensure,
/// `platform_federation/service.py:381` in the metadata sync):
/// Django's `_meta` order runs `updated_at` first, so the `SET`
/// lists it before `name`. Replaces the two overlay
/// `sync_workspace_name` helpers.
const SET_NAME_SQL: &str = "UPDATE workspaces SET updated_at = $1, name = $2 WHERE id = $3";

/// Sync one workspace name plus the `updated_at` stamp, nothing else.
pub async fn set_name<'e, E>(
    ex: E,
    workspace_id: Uuid,
    name: &str,
    updated_at: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(SET_NAME_SQL)
        .bind(updated_at)
        .bind(name)
        .bind(workspace_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// members
// ---------------------------------------------------------------------------

/// OSS `workspace_members` writes: add / bulk-add / role / reactivate /
/// deactivate, plus the platform insert / overwrite / sync /
/// no-stamp-deactivate shapes (D6 gap PIDASHCONV-1613) and the bulk
/// workspace deactivations (D6 gap PIDASHCONV-1617). Reads (the
/// pre-write existence probes, the role guards) stay at the call
/// sites.
pub mod members {
    use super::*;

    /// One member row to insert. `updated_by`/`deleted_at` are always
    /// `NULL` on insert on every path; `is_active` is always `TRUE`
    /// and the checklist triple always `'{}'`.
    pub struct NewMember<'a> {
        pub workspace_id: Uuid,
        /// The project-invite join flows a missing user into the
        /// insert as `NULL` and 400s there; every other path passes
        /// `Some`.
        pub user_id: Option<Uuid>,
        pub role: i16,
        /// `None` on invite/signup joins; the create paths echo the
        /// input (`app`) or the validated value (`license-config`).
        pub company_role: Option<&'a str>,
        /// `None` on signup joins (no actor) and the ops bulk rows;
        /// the responder/approver/actor elsewhere.
        pub created_by: Option<Uuid>,
        /// Caller-supplied model defaults (`get_default_props` /
        /// `get_issue_props`); each caller module keeps its own copy.
        pub view_props: &'a serde_json::Value,
        pub default_props: &'a serde_json::Value,
        pub issue_props: &'a serde_json::Value,
        pub now: DateTime<Utc>,
        /// Per-row `ON CONFLICT DO NOTHING` (the signup-join loops
        /// replay Django's bulk `ignore_conflicts` one row at a time).
        pub ignore_conflicts: bool,
        /// DB-side `now()` stamps (the project-invite join only).
        pub db_now: bool,
    }

    pub(crate) const MEMBER_INSERT_SQL: &str = "INSERT INTO \"workspace_members\" \
        (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
        \"workspace_id\", \"member_id\", \"role\", \"company_role\", \
        \"view_props\", \"default_props\", \"issue_props\", \
        \"getting_started_checklist\", \"tips\", \"explored_features\", \"is_active\") \
        VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, \
        '{}', '{}', '{}', TRUE)";

    pub(crate) const MEMBER_INSERT_IGNORE_SQL: &str = "INSERT INTO \"workspace_members\" \
        (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
        \"workspace_id\", \"member_id\", \"role\", \"company_role\", \
        \"view_props\", \"default_props\", \"issue_props\", \
        \"getting_started_checklist\", \"tips\", \"explored_features\", \"is_active\") \
        VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, \
        '{}', '{}', '{}', TRUE) \
        ON CONFLICT DO NOTHING";

    pub(crate) const MEMBER_INSERT_DB_NOW_SQL: &str = "INSERT INTO \"workspace_members\" \
        (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
        \"workspace_id\", \"member_id\", \"role\", \"company_role\", \
        \"view_props\", \"default_props\", \"issue_props\", \
        \"getting_started_checklist\", \"tips\", \"explored_features\", \"is_active\") \
        VALUES ($1, now(), now(), $2, NULL, NULL, $3, $4, $5, $6, $7, $8, $9, \
        '{}', '{}', '{}', TRUE)";

    /// `WorkspaceMember.objects.create(...)` (and the per-row signup
    /// replays of `bulk_create(..., ignore_conflicts=True)`). Returns
    /// the new row id. Integrity errors propagate for the caller's
    /// 400/409/500 arm.
    pub async fn add<'e, E>(ex: E, member: &NewMember<'_>) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        if member.db_now {
            sqlx::query(MEMBER_INSERT_DB_NOW_SQL)
                .bind(id)
                .bind(member.created_by)
                .bind(member.workspace_id)
                .bind(member.user_id)
                .bind(member.role)
                .bind(member.company_role)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.default_props))
                .bind(sqlx::types::Json(member.issue_props))
                .execute(ex)
                .await?;
        } else if member.ignore_conflicts {
            sqlx::query(MEMBER_INSERT_IGNORE_SQL)
                .bind(id)
                .bind(member.now)
                .bind(member.now)
                .bind(member.created_by)
                .bind(member.workspace_id)
                .bind(member.user_id)
                .bind(member.role)
                .bind(member.company_role)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.default_props))
                .bind(sqlx::types::Json(member.issue_props))
                .execute(ex)
                .await?;
        } else {
            sqlx::query(MEMBER_INSERT_SQL)
                .bind(id)
                .bind(member.now)
                .bind(member.now)
                .bind(member.created_by)
                .bind(member.workspace_id)
                .bind(member.user_id)
                .bind(member.role)
                .bind(member.company_role)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.default_props))
                .bind(sqlx::types::Json(member.issue_props))
                .execute(ex)
                .await?;
        }
        Ok(id)
    }

    /// One bulk row: the per-row columns of the multi-row insert.
    /// Rows may span workspaces (the my-invites accept bulk carries
    /// one row per invite); the dummy-data bulk repeats one.
    pub struct BulkMember {
        pub workspace_id: Uuid,
        pub user_id: Uuid,
        pub role: i16,
        /// `None` on the dummy-data rows; the bulk inviter on the
        /// my-invites accept path.
        pub created_by: Option<Uuid>,
    }

    /// Render the multi-row `INSERT ... ON CONFLICT DO NOTHING` for
    /// `nrows` rows (7 binds per row: stamps, audit, membership,
    /// role, shared props). Pure; unit-tested.
    pub(crate) fn bulk_add_sql(nrows: usize) -> String {
        let mut sql = String::from(
            "INSERT INTO \"workspace_members\" \
            (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
            \"workspace_id\", \"member_id\", \"role\", \"company_role\", \
            \"view_props\", \"default_props\", \"issue_props\", \
            \"getting_started_checklist\", \"tips\", \"explored_features\", \"is_active\") \
            VALUES ",
        );
        for index in 0..nrows {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * 7 + 1;
            sql.push_str(&format!(
                "(gen_random_uuid(), ${b}, ${b}, ${c}, NULL, NULL, ${w}, ${m}, ${r}, NULL, \
                 ${p}, ${p}, ${i}, '{{}}', '{{}}', '{{}}', TRUE)",
                b = base,
                c = base + 1,
                w = base + 2,
                m = base + 3,
                r = base + 4,
                p = base + 5,
                i = base + 6,
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        sql
    }

    /// `WorkspaceMember.objects.bulk_create([...],
    /// ignore_conflicts=True)`: one multi-row `INSERT ...
    /// ON CONFLICT DO NOTHING`. An empty list emits no statement at
    /// all, like `bulk_create([])`. `props` serves both `view_props`
    /// and `default_props`: they share `get_default_props`
    /// (`workspace.py:207-208`), and both callers bind identical
    /// content for the two columns.
    pub async fn bulk_add<'e, E>(
        ex: E,
        rows: &[BulkMember],
        props: &serde_json::Value,
        issue_props: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if rows.is_empty() {
            return Ok(());
        }
        // The shared props bind per row: Django's `get_default_props`
        // callables mint a fresh (identical-content) dict per
        // instance, so shared binds commit identical bytes.
        let sql = bulk_add_sql(rows.len());
        let mut query = sqlx::query(&sql);
        for row in rows {
            query = query
                .bind(now)
                .bind(row.created_by)
                .bind(row.workspace_id)
                .bind(row.user_id)
                .bind(row.role)
                .bind(sqlx::types::Json(props))
                .bind(sqlx::types::Json(issue_props));
        }
        query.execute(ex).await?;
        Ok(())
    }

    /// A role change plus the reactivate stamps. `created_by`
    /// `None` leaves the column untouched (the join-request approve
    /// path); `Some` overwrites it (the invite-accept path stamps
    /// `caller.and(current)`).
    pub struct RoleUpdate {
        pub id: Uuid,
        pub role: i16,
        pub updated_at: DateTime<Utc>,
        pub updated_by: Option<Uuid>,
        pub created_by: Option<Option<Uuid>>,
    }

    pub(crate) const UPDATE_ROLE_SQL: &str = "UPDATE workspace_members \
        SET is_active = TRUE, role = $1, updated_at = $2, updated_by_id = $3 \
        WHERE id = $4";

    pub(crate) const UPDATE_ROLE_CREATED_BY_SQL: &str = "UPDATE workspace_members \
        SET is_active = TRUE, role = $1, updated_at = $2, updated_by_id = $3, created_by_id = $4 \
        WHERE id = $5";

    /// The invite-accept / join-approve reactivate `.save()`: active
    /// plus role plus the audit stamps.
    pub async fn update_role<'e, E>(ex: E, update: &RoleUpdate) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        match update.created_by {
            None => {
                sqlx::query(UPDATE_ROLE_SQL)
                    .bind(update.role)
                    .bind(update.updated_at)
                    .bind(update.updated_by)
                    .bind(update.id)
                    .execute(ex)
                    .await?;
            }
            Some(created_by) => {
                sqlx::query(UPDATE_ROLE_CREATED_BY_SQL)
                    .bind(update.role)
                    .bind(update.updated_at)
                    .bind(update.updated_by)
                    .bind(created_by)
                    .bind(update.id)
                    .execute(ex)
                    .await?;
            }
        }
        Ok(())
    }

    pub(crate) const ACTIVATE_BY_MEMBERSHIP_SQL: &str = "UPDATE workspace_members \
        SET is_active = TRUE, role = $1 \
        WHERE workspace_id = $2 AND member_id = $3 AND deleted_at IS NULL";

    /// The my-invites per-invite reactivate: a queryset `update()`,
    /// so `SET` only — no `updated_at` stamp.
    pub async fn activate_by_membership<'e, E>(
        ex: E,
        workspace_id: Uuid,
        user_id: Uuid,
        role: i16,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(ACTIVATE_BY_MEMBERSHIP_SQL)
            .bind(role)
            .bind(workspace_id)
            .bind(user_id)
            .execute(ex)
            .await
            .map(|_| ())
    }

    pub(crate) const ACTIVATE_BY_ID_SQL: &str = "UPDATE workspace_members \
        SET is_active = true, updated_at = now(), updated_by_id = $1 \
        WHERE id = $2";

    /// The project-invite join reactivate `.save()`: active plus
    /// stamps, no role change. DB-side `now()` preserved verbatim.
    pub async fn activate_by_id<'e, E>(
        ex: E,
        id: Uuid,
        updated_by: Option<Uuid>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(ACTIVATE_BY_ID_SQL)
            .bind(updated_by)
            .bind(id)
            .execute(ex)
            .await
            .map(|_| ())
    }

    pub(crate) const DEACTIVATE_SQL: &str = "UPDATE workspace_members \
        SET is_active = FALSE, updated_at = $2, updated_by_id = $3 \
        WHERE id = $1";

    /// The member deactivation `.save()`: inactive plus the stamps
    /// (`updated_by` from the request user, `db/models/base.py:23-44`).
    pub async fn deactivate<'e, E>(
        ex: E,
        id: Uuid,
        updated_at: DateTime<Utc>,
        updated_by: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(DEACTIVATE_SQL)
            .bind(id)
            .bind(updated_at)
            .bind(updated_by)
            .execute(ex)
            .await
            .map(|_| ())
    }

    /// One platform member row to insert: the overlay's
    /// `insert_workspace_member` shape (the owner
    /// `WorkspaceMember.objects.create(...)` and the member-event
    /// create, `ee-overlay` `service.py:415-419,598-603`). Unlike
    /// [`add`], `is_active` is a caller param — the member-event
    /// applier computes it from the event/status, reachable as
    /// `FALSE`. `created_by`/`updated_by`/`company_role` bind `NULL`
    /// (anonymous webhook crum); `default_props` rides the
    /// `view_props` value and the checklist triple is always `'{}'`.
    pub struct NewMemberWithActive<'a> {
        pub workspace_id: Uuid,
        pub user_id: Uuid,
        pub role: i16,
        pub is_active: bool,
        pub view_props: &'a serde_json::Value,
        pub issue_props: &'a serde_json::Value,
        pub now: DateTime<Utc>,
    }

    /// The overlay `INSERT` text verbatim (unquoted identifiers kept;
    /// D6 gap PIDASHCONV-1613).
    pub(crate) const MEMBER_INSERT_ACTIVE_SQL: &str = "INSERT INTO workspace_members \
        (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, \
        member_id, role, company_role, view_props, default_props, issue_props, is_active, \
        getting_started_checklist, tips, explored_features) \
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)";

    /// The platform member insert. Returns the new row id. Integrity
    /// errors propagate for the caller's savepoint arm.
    pub async fn add_with_active<'e, E>(
        ex: E,
        member: &NewMemberWithActive<'_>,
    ) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        let empty = serde_json::Value::Object(serde_json::Map::new());
        sqlx::query(MEMBER_INSERT_ACTIVE_SQL)
            .bind(id)
            .bind(member.now)
            .bind(member.now)
            .bind(None::<Uuid>)
            .bind(None::<Uuid>)
            .bind(None::<DateTime<Utc>>)
            .bind(member.workspace_id)
            .bind(member.user_id)
            .bind(member.role)
            .bind(None::<String>)
            .bind(sqlx::types::Json(member.view_props))
            .bind(sqlx::types::Json(member.view_props))
            .bind(sqlx::types::Json(member.issue_props))
            .bind(member.is_active)
            .bind(sqlx::types::Json(&empty))
            .bind(sqlx::types::Json(&empty))
            .bind(sqlx::types::Json(&empty))
            .execute(ex)
            .await?;
        Ok(id)
    }

    /// An unconditional member overwrite: the overlay's
    /// `overwrite_workspace_member` shape (the member-event
    /// `role` + `is_active` write, `ee-overlay`
    /// `service.py:605-607`). Always executed, even when nothing
    /// changed; no `updated_by` stamp.
    pub struct MemberOverwrite {
        pub id: Uuid,
        pub role: i16,
        pub is_active: bool,
        pub updated_at: DateTime<Utc>,
    }

    /// The overlay overwrite text verbatim (D6 gap PIDASHCONV-1613).
    pub(crate) const OVERWRITE_SQL: &str =
        "UPDATE workspace_members SET updated_at = $1, role = $2, is_active = $3 WHERE id = $4";

    /// Overwrite one member's role and active flag plus `updated_at`.
    pub async fn overwrite<'e, E>(ex: E, overwrite: &MemberOverwrite) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(OVERWRITE_SQL)
            .bind(overwrite.updated_at)
            .bind(overwrite.role)
            .bind(overwrite.is_active)
            .bind(overwrite.id)
            .execute(ex)
            .await
            .map(|_| ())
    }

    /// A dirty-column member sync: the overlay's `sync_owner_member`
    /// shape (the owner reactivate, `ee-overlay`
    /// `service.py:420-429`). Only dirty columns (`role` and/or
    /// `is_active`) plus `updated_at`; no write when already
    /// converged. Activation only ever sets `TRUE` (the sync runs
    /// right after a get-or-create).
    pub struct MemberSync {
        pub id: Uuid,
        pub current_role: i16,
        pub current_is_active: bool,
        pub role: i16,
        pub updated_at: DateTime<Utc>,
    }

    /// The three overlay sync statements verbatim (D6 gap
    /// PIDASHCONV-1613).
    pub(crate) const SYNC_ROLE_SQL: &str =
        "UPDATE workspace_members SET updated_at = $1, role = $2 WHERE id = $3";
    pub(crate) const SYNC_ACTIVE_SQL: &str =
        "UPDATE workspace_members SET updated_at = $1, is_active = $2 WHERE id = $3";
    pub(crate) const SYNC_ROLE_ACTIVE_SQL: &str =
        "UPDATE workspace_members SET updated_at = $1, role = $2, is_active = $3 WHERE id = $4";

    /// Sync one member toward `role`/active, writing only dirty
    /// columns; a converged member emits no statement.
    pub async fn sync<'e, E>(ex: E, sync: &MemberSync) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let role_dirty = sync.current_role != sync.role;
        let active_dirty = !sync.current_is_active;
        if !role_dirty && !active_dirty {
            return Ok(());
        }
        if role_dirty && active_dirty {
            sqlx::query(SYNC_ROLE_ACTIVE_SQL)
                .bind(sync.updated_at)
                .bind(sync.role)
                .bind(true)
                .bind(sync.id)
                .execute(ex)
                .await?;
        } else if role_dirty {
            sqlx::query(SYNC_ROLE_SQL)
                .bind(sync.updated_at)
                .bind(sync.role)
                .bind(sync.id)
                .execute(ex)
                .await?;
        } else {
            sqlx::query(SYNC_ACTIVE_SQL)
                .bind(sync.updated_at)
                .bind(true)
                .bind(sync.id)
                .execute(ex)
                .await?;
        }
        Ok(())
    }

    /// The overlay sweep text verbatim (D6 gap PIDASHCONV-1613).
    pub(crate) const DEACTIVATE_NO_STAMP_SQL: &str =
        "UPDATE workspace_members SET updated_at = $1, is_active = $2 WHERE id = $3";

    /// The sweep member deactivation (`ee-overlay`
    /// `service.py:847-849`): a queryset-style
    /// `save(update_fields=["is_active", "updated_at"])` with no
    /// `updated_by` stamp.
    pub async fn deactivate_without_updated_by<'e, E>(
        ex: E,
        id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(DEACTIVATE_NO_STAMP_SQL)
            .bind(updated_at)
            .bind(false)
            .bind(id)
            .execute(ex)
            .await
            .map(|_| ())
    }

    /// The overlay bulk-deactivate texts verbatim (D6 gap
    /// PIDASHCONV-1617): the workspace scope and the
    /// workspace+member scope of `_deactivate_local_access`.
    pub(crate) const DEACTIVATE_BY_WORKSPACE_SQL: &str = "UPDATE workspace_members \
        SET is_active = FALSE \
        WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE";

    pub(crate) const DEACTIVATE_BY_WORKSPACE_MEMBER_SQL: &str = "UPDATE workspace_members \
        SET is_active = FALSE \
        WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE AND member_id = $2";

    /// The platform bulk deactivation (`pi_dash_cloud` `service.py:480-489`):
    /// `WorkspaceMember.objects.filter(workspace, is_active=True)[.filter(member)]
    /// .update(is_active=False)`. A queryset `update()`, so `SET` only — no
    /// `updated_at` stamp and no `updated_by` (unlike [`deactivate`]); the
    /// soft-delete scope comes from the default manager. Runs on the
    /// caller's executor (the overlay holds its own transaction).
    pub async fn deactivate_by_workspace<'e, E>(
        ex: E,
        workspace_id: Uuid,
        member_id: Option<Uuid>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if let Some(user) = member_id {
            sqlx::query(DEACTIVATE_BY_WORKSPACE_MEMBER_SQL)
                .bind(workspace_id)
                .bind(user)
                .execute(ex)
                .await
                .map(|_| ())
        } else {
            sqlx::query(DEACTIVATE_BY_WORKSPACE_SQL)
                .bind(workspace_id)
                .execute(ex)
                .await
                .map(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::members::*;
    use super::*;

    #[test]
    fn workspace_insert_lists_every_column() {
        for column in [
            "\"id\"",
            "\"created_at\"",
            "\"updated_at\"",
            "\"created_by_id\"",
            "\"name\"",
            "\"logo\"",
            "\"logo_asset_id\"",
            "\"owner_id\"",
            "\"slug\"",
            "\"organization_size\"",
            "\"timezone\"",
            "\"background_color\"",
        ] {
            assert!(WORKSPACE_INSERT_SQL.contains(column), "{column} listed");
        }
        assert!(
            WORKSPACE_INSERT_SQL.contains("RETURNING \"created_at\", \"updated_at\""),
            "stamps returned"
        );
        for bind in 1..=13 {
            assert!(
                WORKSPACE_INSERT_SQL.contains(&format!("${bind}")),
                "bind ${bind} present"
            );
        }
    }

    #[test]
    fn member_inserts_share_columns_and_differ_only_in_tail() {
        for column in [
            "\"workspace_id\"",
            "\"member_id\"",
            "\"role\"",
            "\"company_role\"",
            "\"view_props\"",
            "\"default_props\"",
            "\"issue_props\"",
        ] {
            assert!(MEMBER_INSERT_SQL.contains(column), "{column} listed");
            assert!(MEMBER_INSERT_IGNORE_SQL.contains(column), "{column} listed");
            assert!(MEMBER_INSERT_DB_NOW_SQL.contains(column), "{column} listed");
        }
        assert!(!MEMBER_INSERT_SQL.contains("ON CONFLICT"));
        assert!(MEMBER_INSERT_IGNORE_SQL.ends_with("ON CONFLICT DO NOTHING"));
        assert!(MEMBER_INSERT_DB_NOW_SQL.contains("now(), now()"));
        assert!(!MEMBER_INSERT_DB_NOW_SQL.contains("ON CONFLICT"));
        assert_eq!(RENAME_SQL, "UPDATE workspaces SET slug = $1 WHERE id = $2");
        assert!(!RENAME_SQL.contains("updated_at"));
    }

    #[test]
    fn set_name_sql_pins_platform_sync_shape() {
        // Byte-identical to the overlay `sync_workspace_name` render
        // (`UPDATE {TABLE} SET updated_at = $1, name = $2 WHERE id =
        // $3` with `TABLE = "workspaces"`); binds stay (now, name,
        // id).
        assert_eq!(
            SET_NAME_SQL,
            "UPDATE workspaces SET updated_at = $1, name = $2 WHERE id = $3"
        );
    }

    #[test]
    fn bulk_sql_counts_placeholders_per_row() {
        let one = bulk_add_sql(1);
        assert!(one.ends_with(" ON CONFLICT DO NOTHING"));
        for bind in 1..=7 {
            assert!(one.contains(&format!("${bind}")), "bind ${bind} present");
        }
        assert!(!one.contains("$8"));
        let two = bulk_add_sql(2);
        assert!(two.contains("$14"), "second row placeholders");
        assert!(!two.contains("$15"));
        // Shared props bind: view and default ride the same placeholder.
        assert_eq!(two.matches("$6").count() + two.matches("$13").count(), 4);
    }

    #[test]
    fn platform_shapes_match_overlay_statements() {
        assert_eq!(
            MEMBER_INSERT_ACTIVE_SQL,
            "INSERT INTO workspace_members \
            (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
            workspace_id, member_id, role, company_role, view_props, default_props, \
            issue_props, is_active, getting_started_checklist, tips, explored_features) \
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)"
        );
        assert_eq!(
            OVERWRITE_SQL,
            "UPDATE workspace_members SET updated_at = $1, role = $2, is_active = $3 WHERE id = $4"
        );
        assert_eq!(
            SYNC_ROLE_SQL,
            "UPDATE workspace_members SET updated_at = $1, role = $2 WHERE id = $3"
        );
        assert_eq!(
            SYNC_ACTIVE_SQL,
            "UPDATE workspace_members SET updated_at = $1, is_active = $2 WHERE id = $3"
        );
        assert_eq!(
            SYNC_ROLE_ACTIVE_SQL,
            "UPDATE workspace_members SET updated_at = $1, role = $2, is_active = $3 WHERE id = $4"
        );
        assert_eq!(
            DEACTIVATE_NO_STAMP_SQL,
            "UPDATE workspace_members SET updated_at = $1, is_active = $2 WHERE id = $3"
        );
        // The overlay shapes coincide: both-dirty sync is the overwrite
        // text, and the sweep deactivate is the active-only sync text.
        assert_eq!(SYNC_ROLE_ACTIVE_SQL, OVERWRITE_SQL);
        assert_eq!(DEACTIVATE_NO_STAMP_SQL, SYNC_ACTIVE_SQL);
    }

    #[test]
    fn bulk_deactivate_statements_match_overlay() {
        assert_eq!(
            DEACTIVATE_BY_WORKSPACE_SQL,
            "UPDATE workspace_members SET is_active = FALSE \
            WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE"
        );
        assert_eq!(
            DEACTIVATE_BY_WORKSPACE_MEMBER_SQL,
            "UPDATE workspace_members SET is_active = FALSE \
            WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE AND member_id = $2"
        );
        assert!(!DEACTIVATE_BY_WORKSPACE_SQL.contains("updated_at"));
        assert!(!DEACTIVATE_BY_WORKSPACE_MEMBER_SQL.contains("updated_at"));
    }

    /// One-connection scratch pool: temp tables are
    /// connection-scoped, so a single connection keeps every statement
    /// in the test on the temp tables (zero writes to real tables).
    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    /// Temp `workspaces` mirroring production nullability (slug unique:
    /// the create 409 arm keys on it).
    const WORKSPACES_DDL: &str = "CREATE TEMPORARY TABLE workspaces (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL,
        created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ,
        name TEXT NOT NULL, logo TEXT, logo_asset_id UUID, owner_id UUID NOT NULL,
        slug TEXT NOT NULL UNIQUE, organization_size TEXT,
        timezone TEXT NOT NULL, background_color TEXT NOT NULL)";

    /// Temp `workspace_members` plus the partial unique index the
    /// `ON CONFLICT DO NOTHING` arms key on.
    const MEMBERS_DDL: &str = "CREATE TEMPORARY TABLE workspace_members (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL,
        created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ,
        workspace_id UUID NOT NULL, member_id UUID, role SMALLINT NOT NULL,
        company_role TEXT, view_props JSONB NOT NULL, default_props JSONB NOT NULL,
        issue_props JSONB NOT NULL, is_active BOOLEAN NOT NULL,
        getting_started_checklist JSONB NOT NULL, tips JSONB NOT NULL,
        explored_features JSONB NOT NULL)";

    const MEMBERS_UNIQUE_DDL: &str = "CREATE UNIQUE INDEX members_ws_member_uidx \
        ON workspace_members (workspace_id, member_id) WHERE deleted_at IS NULL";

    /// Temp `schedulers` with the columns the seeding select/insert
    /// touch (loose nullability: the seeding path is the assertion).
    const SCHEDULERS_DDL: &str = "CREATE TEMPORARY TABLE schedulers (
        created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, created_by_id UUID,
        updated_by_id UUID, deleted_at TIMESTAMPTZ, id UUID,
        workspace_id UUID, slug TEXT, name TEXT, description TEXT,
        prompt TEXT, source TEXT, is_enabled BOOLEAN, color TEXT)";

    fn props() -> serde_json::Value {
        serde_json::json!({"a": 1})
    }

    fn issue_props() -> serde_json::Value {
        serde_json::json!({"b": 2})
    }

    /// `create` inserts the row and seeds the builtins (the `post_save`
    /// effect); a second create on the same slug 23505s for the
    /// caller's 409 arm.
    #[tokio::test]
    async fn live_create_inserts_and_seeds() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(WORKSPACES_DDL)
            .execute(&pool)
            .await
            .expect("temp workspaces");
        sqlx::query(SCHEDULERS_DDL)
            .execute(&pool)
            .await
            .expect("temp schedulers");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let owner = Uuid::new_v4();
        let ws = NewWorkspace {
            id: Uuid::new_v4(),
            name: "Acme",
            slug: "acme",
            owner_id: owner,
            created_by: Some(owner),
            logo: None,
            logo_asset_id: None,
            organization_size: None,
            timezone: "UTC",
            background_color: "#fff",
            deleted_at: None,
            created_at: now,
            updated_at: now,
            seed_created_by: Some(owner),
        };
        let stamps = create(&pool, &ws).await.expect("create").expect("stamps");
        assert_eq!(stamps.id, ws.id);
        assert_eq!(stamps.created_at, now);
        let name: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
            .bind(ws.id)
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert_eq!(name, "Acme");
        let seeded: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM schedulers WHERE workspace_id = $1")
                .bind(ws.id)
                .fetch_one(&pool)
                .await
                .expect("seeded count");
        assert!(seeded > 0, "builtins seeded, got {seeded}");
        // Slug conflict propagates as 23505 for the caller's 409 arm.
        let clash = NewWorkspace {
            id: Uuid::new_v4(),
            ..ws_for_clash(&ws)
        };
        let err = create(&pool, &clash).await.expect_err("slug clash 23505");
        assert_eq!(
            err.as_database_error().and_then(|db| db.code()).as_deref(),
            Some("23505")
        );
    }

    /// `add` inserts the full row; the ignore arm replays Django's
    /// per-row `ON CONFLICT DO NOTHING` (second add is a silent
    /// no-op); `db_now` stamps from the database clock.
    #[tokio::test]
    async fn live_add_plain_ignore_and_db_now() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        sqlx::query(MEMBERS_UNIQUE_DDL)
            .execute(&pool)
            .await
            .expect("temp unique index");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let view = props();
        let issue = issue_props();
        let ws = Uuid::new_v4();
        let user = Uuid::new_v4();
        let id = add(
            &pool,
            &NewMember {
                workspace_id: ws,
                user_id: Some(user),
                role: 20,
                company_role: None,
                created_by: Some(user),
                view_props: &view,
                default_props: &view,
                issue_props: &issue,
                now,
                ignore_conflicts: false,
                db_now: false,
            },
        )
        .await
        .expect("plain add");
        let stored: (i16, bool) =
            sqlx::query_as("SELECT role, is_active FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stored, (20, true));
        // Plain double-add violates the unique index (caller's arm).
        let clash = add(
            &pool,
            &NewMember {
                workspace_id: ws,
                user_id: Some(user),
                role: 15,
                company_role: None,
                created_by: None,
                view_props: &view,
                default_props: &view,
                issue_props: &issue,
                now,
                ignore_conflicts: false,
                db_now: false,
            },
        )
        .await
        .expect_err("plain double-add 23505");
        assert_eq!(
            clash
                .as_database_error()
                .and_then(|db| db.code())
                .as_deref(),
            Some("23505")
        );
        // Ignore arm: silent no-op, original row (role 20) untouched.
        add(
            &pool,
            &NewMember {
                workspace_id: ws,
                user_id: Some(user),
                role: 15,
                company_role: None,
                created_by: None,
                view_props: &view,
                default_props: &view,
                issue_props: &issue,
                now,
                ignore_conflicts: true,
                db_now: false,
            },
        )
        .await
        .expect("ignore double-add");
        let role: i16 = sqlx::query_scalar(
            "SELECT role FROM workspace_members WHERE workspace_id = $1 AND member_id = $2",
        )
        .bind(ws)
        .bind(user)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(role, 20);
        // DB-side stamps differ from the passed request clock.
        let user2 = Uuid::new_v4();
        let id2 = add(
            &pool,
            &NewMember {
                workspace_id: ws,
                user_id: Some(user2),
                role: 15,
                company_role: None,
                created_by: None,
                view_props: &view,
                default_props: &view,
                issue_props: &issue,
                now,
                ignore_conflicts: false,
                db_now: true,
            },
        )
        .await
        .expect("db_now add");
        let stamped: DateTime<Utc> =
            sqlx::query_scalar("SELECT created_at FROM workspace_members WHERE id = $1")
                .bind(id2)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(stamped != now, "db clock stamps, got {stamped}");
    }

    /// `bulk_add` lands every row in one statement, skips conflicts,
    /// and emits nothing for an empty list.
    #[tokio::test]
    async fn live_bulk_add_rows_conflicts_and_empty() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        sqlx::query(MEMBERS_UNIQUE_DDL)
            .execute(&pool)
            .await
            .expect("temp unique index");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let view = props();
        let issue = issue_props();
        let ws = Uuid::new_v4();
        bulk_add(&pool, &[], &view, &issue, now)
            .await
            .expect("empty no-op");
        let users = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let rows: Vec<BulkMember> = users
            .iter()
            .map(|user| BulkMember {
                workspace_id: ws,
                user_id: *user,
                role: 20,
                created_by: None,
            })
            .collect();
        bulk_add(&pool, &rows, &view, &issue, now)
            .await
            .expect("bulk add");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_members")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 3);
        // Re-run: conflicts skipped, still 3 rows.
        bulk_add(&pool, &rows, &view, &issue, now)
            .await
            .expect("bulk re-add");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_members")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 3);
    }

    /// `update_role` reactivates with role plus stamps; `created_by`
    /// `None` leaves the column, `Some` overwrites it.
    #[tokio::test]
    async fn live_update_role_arms() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let view = props();
        let issue = issue_props();
        let ws = Uuid::new_v4();
        let creator = Uuid::new_v4();
        let id = add(
            &pool,
            &NewMember {
                workspace_id: ws,
                user_id: Some(Uuid::new_v4()),
                role: 5,
                company_role: None,
                created_by: Some(creator),
                view_props: &view,
                default_props: &view,
                issue_props: &issue,
                now,
                ignore_conflicts: false,
                db_now: false,
            },
        )
        .await
        .expect("add");
        sqlx::query("UPDATE workspace_members SET is_active = FALSE WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("deactivate fixture");
        let approver = Uuid::new_v4();
        update_role(
            &pool,
            &RoleUpdate {
                id,
                role: 15,
                updated_at: later,
                updated_by: Some(approver),
                created_by: None,
            },
        )
        .await
        .expect("update_role leave");
        let row: (bool, i16, Option<Uuid>) = sqlx::query_as(
            "SELECT is_active, role, created_by_id FROM workspace_members WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(row, (true, 15, Some(creator)));
        let overwriter = Uuid::new_v4();
        update_role(
            &pool,
            &RoleUpdate {
                id,
                role: 20,
                updated_at: later,
                updated_by: None,
                created_by: Some(Some(overwriter)),
            },
        )
        .await
        .expect("update_role overwrite");
        let stamped: Option<Uuid> =
            sqlx::query_scalar("SELECT created_by_id FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stamped, Some(overwriter));
    }

    /// The activate/deactivate round trip across both lookup shapes.
    #[tokio::test]
    async fn live_activate_and_deactivate() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let view = props();
        let issue = issue_props();
        let ws = Uuid::new_v4();
        let user = Uuid::new_v4();
        let id = add(
            &pool,
            &NewMember {
                workspace_id: ws,
                user_id: Some(user),
                role: 5,
                company_role: None,
                created_by: None,
                view_props: &view,
                default_props: &view,
                issue_props: &issue,
                now,
                ignore_conflicts: false,
                db_now: false,
            },
        )
        .await
        .expect("add");
        let actor = Uuid::new_v4();
        deactivate(&pool, id, now, actor).await.expect("deactivate");
        let active: bool =
            sqlx::query_scalar("SELECT is_active FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(!active);
        activate_by_id(&pool, id, Some(actor))
            .await
            .expect("activate_by_id");
        let row: (bool, i16) =
            sqlx::query_as("SELECT is_active, role FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(row, (true, 5));
        deactivate(&pool, id, now, actor).await.expect("deactivate");
        activate_by_membership(&pool, ws, user, 15)
            .await
            .expect("activate_by_membership");
        let row: (bool, i16) =
            sqlx::query_as("SELECT is_active, role FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(row, (true, 15));
    }

    /// `add_with_active` inserts the full row with the caller flag
    /// (both arms), NULL audit/company columns, `default_props`
    /// riding `view_props`, and an empty checklist triple.
    #[tokio::test]
    async fn live_add_with_active_both_arms() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let view = props();
        let issue = issue_props();
        let ws = Uuid::new_v4();
        for is_active in [true, false] {
            let id = add_with_active(
                &pool,
                &NewMemberWithActive {
                    workspace_id: ws,
                    user_id: Uuid::new_v4(),
                    role: 20,
                    is_active,
                    view_props: &view,
                    issue_props: &issue,
                    now,
                },
            )
            .await
            .expect("add_with_active");
            let row: (i16, bool, Option<Uuid>, Option<Uuid>, Option<String>) = sqlx::query_as(
                "SELECT role, is_active, created_by_id, updated_by_id, company_role \
                    FROM workspace_members WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("read back");
            assert_eq!(row, (20, is_active, None, None, None));
            let stored: (serde_json::Value, serde_json::Value, serde_json::Value) = sqlx::query_as(
                "SELECT view_props, default_props, tips FROM workspace_members WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("read back");
            assert_eq!(stored.0, view);
            assert_eq!(stored.1, view);
            assert_eq!(stored.2, serde_json::json!({}));
        }
    }

    /// `overwrite` unconditionally sets role + active + `updated_at` by
    /// id on the caller's transaction, leaves `updated_by` NULL, and
    /// rewrites even when the values already match.
    #[tokio::test]
    async fn live_overwrite_unconditional_on_tx() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let again: DateTime<Utc> = "2026-10-05T12:00:00+00:00".parse().expect("T2");
        let view = props();
        let issue = issue_props();
        let id = add_with_active(
            &pool,
            &NewMemberWithActive {
                workspace_id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                role: 5,
                is_active: true,
                view_props: &view,
                issue_props: &issue,
                now,
            },
        )
        .await
        .expect("add");
        let mut tx = pool.begin().await.expect("tx");
        overwrite(
            &mut *tx,
            &MemberOverwrite {
                id,
                role: 15,
                is_active: false,
                updated_at: later,
            },
        )
        .await
        .expect("overwrite on tx");
        tx.commit().await.expect("commit");
        let row: (i16, bool, DateTime<Utc>, Option<Uuid>) = sqlx::query_as(
            "SELECT role, is_active, updated_at, updated_by_id \
            FROM workspace_members WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(row, (15, false, later, None));
        // Same values again: still executes (the stamp moves).
        overwrite(
            &pool,
            &MemberOverwrite {
                id,
                role: 15,
                is_active: false,
                updated_at: again,
            },
        )
        .await
        .expect("overwrite again");
        let stamped: DateTime<Utc> =
            sqlx::query_scalar("SELECT updated_at FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stamped, again);
    }

    /// `sync` writes only dirty columns (role-only, active-only,
    /// both) and emits no statement when converged — the `updated_at`
    /// stamp proves each arm.
    #[tokio::test]
    async fn live_sync_dirty_only_and_converged_silent() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let view = props();
        let issue = issue_props();
        let ws = Uuid::new_v4();
        async fn seed(
            pool: &sqlx::PgPool,
            ws: Uuid,
            view: &serde_json::Value,
            issue: &serde_json::Value,
            now: DateTime<Utc>,
            is_active: bool,
        ) -> Uuid {
            add_with_active(
                pool,
                &NewMemberWithActive {
                    workspace_id: ws,
                    user_id: Uuid::new_v4(),
                    role: 5,
                    is_active,
                    view_props: view,
                    issue_props: issue,
                    now,
                },
            )
            .await
            .expect("seed")
        }
        async fn read_back(pool: &sqlx::PgPool, id: Uuid) -> (i16, bool, DateTime<Utc>) {
            sqlx::query_as(
                "SELECT role, is_active, updated_at FROM workspace_members WHERE id = $1",
            )
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("read back")
        }
        // Converged: no statement, stamp untouched.
        let id = seed(&pool, ws, &view, &issue, now, true).await;
        sync(
            &pool,
            &MemberSync {
                id,
                current_role: 5,
                current_is_active: true,
                role: 5,
                updated_at: later,
            },
        )
        .await
        .expect("sync converged");
        assert_eq!(read_back(&pool, id).await, (5, true, now));
        // Role-only.
        let id = seed(&pool, ws, &view, &issue, now, true).await;
        sync(
            &pool,
            &MemberSync {
                id,
                current_role: 5,
                current_is_active: true,
                role: 15,
                updated_at: later,
            },
        )
        .await
        .expect("sync role");
        assert_eq!(read_back(&pool, id).await, (15, true, later));
        // Active-only: reactivates, role untouched.
        let id = seed(&pool, ws, &view, &issue, now, false).await;
        sync(
            &pool,
            &MemberSync {
                id,
                current_role: 5,
                current_is_active: false,
                role: 5,
                updated_at: later,
            },
        )
        .await
        .expect("sync active");
        assert_eq!(read_back(&pool, id).await, (5, true, later));
        // Both dirty.
        let id = seed(&pool, ws, &view, &issue, now, false).await;
        sync(
            &pool,
            &MemberSync {
                id,
                current_role: 5,
                current_is_active: false,
                role: 15,
                updated_at: later,
            },
        )
        .await
        .expect("sync both");
        assert_eq!(read_back(&pool, id).await, (15, true, later));
    }

    /// `deactivate_without_updated_by` flips inactive plus
    /// `updated_at`, never stamps `updated_by`.
    #[tokio::test]
    async fn live_deactivate_without_updated_by() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let view = props();
        let issue = issue_props();
        let id = add_with_active(
            &pool,
            &NewMemberWithActive {
                workspace_id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                role: 20,
                is_active: true,
                view_props: &view,
                issue_props: &issue,
                now,
            },
        )
        .await
        .expect("add");
        deactivate_without_updated_by(&pool, id, later)
            .await
            .expect("deactivate");
        let row: (bool, DateTime<Utc>, Option<Uuid>) = sqlx::query_as(
            "SELECT is_active, updated_at, updated_by_id FROM workspace_members WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(row, (false, later, None));
    }

    /// `deactivate_by_workspace` flips only live active rows in the
    /// workspace (both scopes), leaves `updated_at` untouched, and
    /// skips inactive, soft-deleted and foreign-workspace rows.
    #[tokio::test]
    async fn live_deactivate_by_workspace_scopes() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(MEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let view = props();
        let issue = issue_props();
        let ws_a = Uuid::new_v4();
        let ws_b = Uuid::new_v4();
        let user_1 = Uuid::new_v4();
        let user_2 = Uuid::new_v4();
        let user_3 = Uuid::new_v4();
        let user_4 = Uuid::new_v4();
        for (ws, user) in [
            (ws_a, user_1),
            (ws_a, user_2),
            (ws_a, user_3),
            (ws_a, user_4),
            (ws_b, user_1),
        ] {
            add(
                &pool,
                &NewMember {
                    workspace_id: ws,
                    user_id: Some(user),
                    role: 5,
                    company_role: None,
                    created_by: None,
                    view_props: &view,
                    default_props: &view,
                    issue_props: &issue,
                    now,
                    ignore_conflicts: false,
                    db_now: false,
                },
            )
            .await
            .expect("seed");
        }
        // Already-inactive and soft-deleted (but active-flagged) rows.
        sqlx::query("UPDATE workspace_members SET is_active = FALSE WHERE member_id = $1")
            .bind(user_3)
            .execute(&pool)
            .await
            .expect("inactive seed");
        sqlx::query("UPDATE workspace_members SET deleted_at = $1 WHERE member_id = $2")
            .bind(now)
            .bind(user_4)
            .execute(&pool)
            .await
            .expect("deleted seed");
        async fn read_back(pool: &sqlx::PgPool, ws: Uuid, user: Uuid) -> (bool, DateTime<Utc>) {
            sqlx::query_as(
                "SELECT is_active, updated_at FROM workspace_members \
                WHERE workspace_id = $1 AND member_id = $2",
            )
            .bind(ws)
            .bind(user)
            .fetch_one(pool)
            .await
            .expect("read back")
        }
        // Member scope: only that user's live row flips.
        deactivate_by_workspace(&pool, ws_a, Some(user_1))
            .await
            .expect("member scope");
        assert_eq!(read_back(&pool, ws_a, user_1).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_2).await, (true, now));
        assert_eq!(read_back(&pool, ws_b, user_1).await, (true, now));
        // Workspace scope: every live active row in the workspace
        // flips; inactive, deleted and foreign rows are untouched.
        sqlx::query("UPDATE workspace_members SET is_active = TRUE WHERE member_id = $1")
            .bind(user_1)
            .execute(&pool)
            .await
            .expect("restore");
        deactivate_by_workspace(&pool, ws_a, None)
            .await
            .expect("workspace scope");
        assert_eq!(read_back(&pool, ws_a, user_1).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_2).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_3).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_4).await, (true, now));
        assert_eq!(read_back(&pool, ws_b, user_1).await, (true, now));
    }

    /// `rename` rewrites the slug and touches nothing else.
    #[tokio::test]
    async fn live_rename_rewrites_slug_only() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(WORKSPACES_DDL)
            .execute(&pool)
            .await
            .expect("temp workspaces");
        sqlx::query(SCHEDULERS_DDL)
            .execute(&pool)
            .await
            .expect("temp schedulers");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let id = Uuid::new_v4();
        create(
            &pool,
            &NewWorkspace {
                id,
                name: "Acme",
                slug: "acme",
                owner_id: Uuid::new_v4(),
                created_by: None,
                logo: None,
                logo_asset_id: None,
                organization_size: None,
                timezone: "UTC",
                background_color: "#fff",
                deleted_at: None,
                created_at: now,
                updated_at: now,
                seed_created_by: None,
            },
        )
        .await
        .expect("create");
        rename(&pool, id, "acme-2").await.expect("rename");
        let row: (String, DateTime<Utc>) =
            sqlx::query_as("SELECT slug, updated_at FROM workspaces WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(row, ("acme-2".to_owned(), now));
    }

    /// `insert` lands the full row on the pool and on the caller's
    /// transaction alike, and seeds nothing (the overlay ensure seeds
    /// itself after commit).
    #[tokio::test]
    async fn live_insert_row_only_no_seed() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(WORKSPACES_DDL)
            .execute(&pool)
            .await
            .expect("temp workspaces");
        sqlx::query(SCHEDULERS_DDL)
            .execute(&pool)
            .await
            .expect("temp schedulers");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let owner = Uuid::new_v4();
        let id = Uuid::new_v4();
        let stamps = insert(
            &pool,
            &NewWorkspace {
                id,
                name: "Acme",
                slug: "acme",
                owner_id: owner,
                created_by: None,
                logo: None,
                logo_asset_id: None,
                organization_size: None,
                timezone: "UTC",
                background_color: "#a1b2c3",
                deleted_at: None,
                created_at: now,
                updated_at: now,
                seed_created_by: Some(owner),
            },
        )
        .await
        .expect("insert")
        .expect("stamps");
        assert_eq!(stamps.id, id);
        assert_eq!(stamps.created_at, now);
        assert_eq!(stamps.updated_at, now);
        let row: (String, String, Uuid, String, String) = sqlx::query_as(
            "SELECT name, slug, owner_id, timezone, background_color \
             FROM workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(
            row,
            (
                "Acme".to_owned(),
                "acme".to_owned(),
                owner,
                "UTC".to_owned(),
                "#a1b2c3".to_owned(),
            )
        );
        let audit: (Option<Uuid>, Option<Uuid>) =
            sqlx::query_as("SELECT created_by_id, updated_by_id FROM workspaces WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(audit, (None, None));
        let deleted: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT deleted_at FROM workspaces WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(deleted, None);
        let opts: (Option<String>, Option<Uuid>, Option<String>) = sqlx::query_as(
            "SELECT logo, logo_asset_id, organization_size FROM workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(opts, (None, None, None));
        // The seeding input is ignored: no scheduler rows appear.
        let seeded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schedulers")
            .fetch_one(&pool)
            .await
            .expect("seeded count");
        assert_eq!(seeded, 0);
        // The executor-generic shape: the same insert on the caller's
        // transaction lands on commit.
        let tx_id = Uuid::new_v4();
        let mut tx = pool.begin().await.expect("tx");
        insert(
            &mut *tx,
            &NewWorkspace {
                id: tx_id,
                name: "Tx",
                slug: "tx",
                owner_id: owner,
                created_by: None,
                logo: None,
                logo_asset_id: None,
                organization_size: None,
                timezone: "UTC",
                background_color: "#000000",
                deleted_at: None,
                created_at: now,
                updated_at: now,
                seed_created_by: None,
            },
        )
        .await
        .expect("insert on tx");
        tx.commit().await.expect("commit");
        let name: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
            .bind(tx_id)
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert_eq!(name, "Tx");
    }

    /// `set_name` rewrites the name plus `updated_at` and touches
    /// nothing else.
    #[tokio::test]
    async fn live_set_name_updates_name_and_stamp() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(WORKSPACES_DDL)
            .execute(&pool)
            .await
            .expect("temp workspaces");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let id = Uuid::new_v4();
        insert(
            &pool,
            &NewWorkspace {
                id,
                name: "Acme",
                slug: "acme",
                owner_id: Uuid::new_v4(),
                created_by: None,
                logo: None,
                logo_asset_id: None,
                organization_size: None,
                timezone: "UTC",
                background_color: "#fff",
                deleted_at: None,
                created_at: now,
                updated_at: now,
                seed_created_by: None,
            },
        )
        .await
        .expect("insert");
        set_name(&pool, id, "Acme Org", later)
            .await
            .expect("set_name");
        let row: (String, DateTime<Utc>, String, DateTime<Utc>) = sqlx::query_as(
            "SELECT name, updated_at, slug, created_at FROM workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(row, ("Acme Org".to_owned(), later, "acme".to_owned(), now));
    }

    /// Field-level copy for the clash probe (`NewWorkspace` borrows;
    /// the clash row reuses every value but the id).
    fn ws_for_clash<'a>(ws: &'a NewWorkspace<'a>) -> NewWorkspace<'a> {
        NewWorkspace {
            id: Uuid::new_v4(),
            name: ws.name,
            slug: ws.slug,
            owner_id: ws.owner_id,
            created_by: ws.created_by,
            logo: ws.logo,
            logo_asset_id: ws.logo_asset_id,
            organization_size: ws.organization_size,
            timezone: ws.timezone,
            background_color: ws.background_color,
            deleted_at: ws.deleted_at,
            created_at: ws.created_at,
            updated_at: ws.updated_at,
            seed_created_by: ws.seed_created_by,
        }
    }
}
