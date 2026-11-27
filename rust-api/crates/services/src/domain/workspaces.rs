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
//! * Everything else takes a SeaORM [`ConnectionTrait`], like
//!   [`super::tokens`]: the row-only workspace insert, the member
//!   insert, the bulk insert, the role / reactivate / deactivate
//!   updates, the platform insert / overwrite / sync /
//!   no-stamp-deactivate shapes, the bulk workspace deactivations,
//!   the slug rename and the name sync all run on the caller's pool
//!   (`pidash_db::pool::sea_orm_connection`) or on the caller's
//!   existing `sqlx` transaction (`pidash_db::orm_tx::TxConn`; the
//!   join-request approve path holds one, the overlay platform
//!   ensure holds one) with no restructuring. Errors stay
//!   [`sqlx::Error`] through [`pidash_db::orm_tx::into_sqlx`], so the
//!   callers' 23505 / 400 / 409 arms classify as before.
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
use pidash_db::entity::{workspace_members, workspaces};
use pidash_db::orm_tx::into_sqlx;
use sea_orm::sea_query::{Alias, Expr, Func, InsertStatement, OnConflict, Query, SimpleExpr};
use sea_orm::{ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, UpdateMany};
use uuid::Uuid;

/// Run one built insert statement.
async fn exec_insert(db: &impl ConnectionTrait, stmt: &InsertStatement) -> Result<(), sqlx::Error> {
    db.execute(DbBackend::Postgres.build(stmt))
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// Run one bulk update.
async fn exec_update<E: EntityTrait>(
    db: &impl ConnectionTrait,
    stmt: UpdateMany<E>,
) -> Result<(), sqlx::Error> {
    stmt.exec(db).await.map(|_| ()).map_err(into_sqlx)
}

/// A JSON column value (`jsonb` bind).
fn json(value: &serde_json::Value) -> SimpleExpr {
    Expr::value(value.clone())
}

/// The `'{}'` checklist / tips / features literal.
fn empty_object() -> SimpleExpr {
    Expr::value(serde_json::json!({}))
}

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

/// The workspace `INSERT`: every column (`updated_by_id` NULL), the
/// two stamps returned.
fn workspace_insert(ws: &NewWorkspace<'_>) -> InsertStatement {
    use workspaces::Column as C;
    Query::insert()
        .into_table(workspaces::Entity)
        .columns([
            C::Id,
            C::CreatedAt,
            C::UpdatedAt,
            C::CreatedById,
            C::UpdatedById,
            C::DeletedAt,
            C::Name,
            C::Logo,
            C::LogoAssetId,
            C::OwnerId,
            C::Slug,
            C::OrganizationSize,
            C::Timezone,
            C::BackgroundColor,
        ])
        .values_panic([
            Expr::value(ws.id),
            Expr::value(ws.created_at),
            Expr::value(ws.updated_at),
            Expr::value(ws.created_by),
            Expr::value(None::<Uuid>),
            Expr::value(ws.deleted_at),
            Expr::value(ws.name),
            Expr::value(ws.logo),
            Expr::value(ws.logo_asset_id),
            Expr::value(ws.owner_id),
            Expr::value(ws.slug),
            Expr::value(ws.organization_size),
            Expr::value(ws.timezone),
            Expr::value(ws.background_color),
        ])
        .returning(Query::returning().columns([C::CreatedAt, C::UpdatedAt]))
        .to_owned()
}

/// `Workspace.objects.create(...)`: the row, and nothing else. The
/// overlay platform ensure
/// (`_get_or_create_platform_workspace`, `service.py:335-339`) runs
/// this on its own `sqlx` transaction (`TxConn`) next to the
/// unique-slug probe and the link insert, then seeds the builtins
/// itself after commit; adopting [`create`] would split that
/// transaction and move the seeding. Unique violations propagate as
/// `sqlx::Error` for the caller's 409 arm. The `None` arm preserves the
/// app path's `fetch_optional` shape.
pub async fn insert(
    db: &impl ConnectionTrait,
    ws: &NewWorkspace<'_>,
) -> Result<Option<WorkspaceStamps>, sqlx::Error> {
    let row = db
        .query_one(DbBackend::Postgres.build(&workspace_insert(ws)))
        .await
        .map_err(into_sqlx)?;
    row.map(|row| {
        Ok(WorkspaceStamps {
            id: ws.id,
            created_at: row
                .try_get::<DateTime<Utc>>("", "created_at")
                .map_err(into_sqlx)?,
            updated_at: row
                .try_get::<DateTime<Utc>>("", "updated_at")
                .map_err(into_sqlx)?,
        })
    })
    .transpose()
}

/// `Workspace.objects.create(...)`: the row plus the `post_save`
/// seeding effect. Unique violations propagate as `sqlx::Error` so
/// each caller keeps its own 409 arm (app JSON vs license `Conflict`).
/// The `None` arm preserves the app path's `fetch_optional` shape.
pub async fn create(
    pool: &sqlx::PgPool,
    ws: &NewWorkspace<'_>,
) -> Result<Option<WorkspaceStamps>, sqlx::Error> {
    let stamps = insert(&pidash_db::pool::sea_orm_connection(pool), ws).await?;
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
fn rename_stmt(workspace_id: Uuid, new_slug: &str) -> UpdateMany<workspaces::Entity> {
    workspaces::Entity::update_many()
        .col_expr(workspaces::Column::Slug, Expr::value(new_slug))
        .filter(workspaces::Column::Id.eq(workspace_id))
}

/// Rename one workspace slug, nothing else.
pub async fn rename(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    new_slug: &str,
) -> Result<(), sqlx::Error> {
    exec_update(db, rename_stmt(workspace_id, new_slug)).await
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
fn set_name_stmt(
    workspace_id: Uuid,
    name: &str,
    updated_at: DateTime<Utc>,
) -> UpdateMany<workspaces::Entity> {
    workspaces::Entity::update_many()
        .col_expr(workspaces::Column::UpdatedAt, Expr::value(updated_at))
        .col_expr(workspaces::Column::Name, Expr::value(name))
        .filter(workspaces::Column::Id.eq(workspace_id))
}

/// Sync one workspace name plus the `updated_at` stamp, nothing else.
pub async fn set_name(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    name: &str,
    updated_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    exec_update(db, set_name_stmt(workspace_id, name, updated_at)).await
}

// patch + soft_delete (PIDASHCONV-1628; executor-generic)
// ---------------------------------------------------------------------------

/// The validated `PATCH workspaces/<slug>/` columns: every updatable
/// column the serializer admits, resolved from the merged row. The
/// `sets` order (validation order) drives the `SET` order.
pub struct WorkspacePatch<'a> {
    pub deleted_at: Option<DateTime<Utc>>,
    pub name: &'a str,
    pub logo: Option<&'a str>,
    pub slug: &'a str,
    pub organization_size: Option<&'a str>,
    pub timezone: &'a str,
    pub background_color: &'a str,
    pub logo_asset_id: Option<Uuid>,
}

/// The stock `update()` statement for one `sets` combination: the
/// validated columns in order plus the `updated_at`/`updated_by`
/// re-stamp. An unknown column (unreachable: validation only emits
/// known columns) binds NULL under its own name, as before. Pure.
fn patch_stmt(
    workspace_id: Uuid,
    sets: &[&str],
    patch: &WorkspacePatch<'_>,
    updated_by: &Uuid,
    now: DateTime<Utc>,
) -> sea_orm::sea_query::UpdateStatement {
    use workspaces::Column as C;
    let mut stmt = Query::update();
    stmt.table(workspaces::Entity);
    for column in sets {
        match *column {
            "deleted_at" => stmt.value(C::DeletedAt, Expr::value(patch.deleted_at)),
            "name" => stmt.value(C::Name, Expr::value(patch.name)),
            "logo" => stmt.value(C::Logo, Expr::value(patch.logo)),
            "slug" => stmt.value(C::Slug, Expr::value(patch.slug)),
            "organization_size" => {
                stmt.value(C::OrganizationSize, Expr::value(patch.organization_size))
            }
            "timezone" => stmt.value(C::Timezone, Expr::value(patch.timezone)),
            "background_color" => {
                stmt.value(C::BackgroundColor, Expr::value(patch.background_color))
            }
            "logo_asset_id" => stmt.value(C::LogoAssetId, Expr::value(patch.logo_asset_id)),
            other => stmt.value(Alias::new(other), Expr::value(Option::<String>::None)),
        };
    }
    stmt.value(C::UpdatedAt, Expr::value(now))
        .value(C::UpdatedById, Expr::value(*updated_by))
        .and_where(Expr::col(C::Id).eq(workspace_id))
        .to_owned()
}

/// The stock `update()` write: validated columns plus the
/// `updated_at`/`updated_by` re-stamp. Moved from
/// `api::app_workspace::handlers_workspace` (PIDASHCONV-1628) — the
/// column order and the per-column values are unchanged. Runs on the
/// caller's connection.
pub async fn patch(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    sets: &[&str],
    patch: &WorkspacePatch<'_>,
    updated_by: &Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    db.execute(DbBackend::Postgres.build(&patch_stmt(workspace_id, sets, patch, updated_by, now)))
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `super().destroy()` (`mixins.py:72-78` + `workspace.py:107-118`):
/// soft delete, slug tombstone, `updated_by` re-stamp. The
/// profile-pointer cleanup and the fan-out enqueues stay at the call
/// site (not a workspaces write).
fn soft_delete_stmt(
    workspace_id: Uuid,
    deleted_at: DateTime<Utc>,
    updated_by: Uuid,
    tombstone_slug: &str,
) -> UpdateMany<workspaces::Entity> {
    workspaces::Entity::update_many()
        .col_expr(workspaces::Column::DeletedAt, Expr::value(deleted_at))
        .col_expr(workspaces::Column::UpdatedAt, Expr::value(deleted_at))
        .col_expr(workspaces::Column::UpdatedById, Expr::value(updated_by))
        .col_expr(workspaces::Column::Slug, Expr::value(tombstone_slug))
        .filter(workspaces::Column::Id.eq(workspace_id))
}

/// Soft-delete one workspace with its slug tombstone. Runs on the
/// caller's connection.
pub async fn soft_delete(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    deleted_at: DateTime<Utc>,
    updated_by: Uuid,
    tombstone_slug: &str,
) -> Result<(), sqlx::Error> {
    exec_update(
        db,
        soft_delete_stmt(workspace_id, deleted_at, updated_by, tombstone_slug),
    )
    .await
}

// ---------------------------------------------------------------------------
// logo asset link (executor-generic; the v2 file-asset writes)
// ---------------------------------------------------------------------------

/// `WorkspaceFileAssetEndpoint.entity_asset_save`
/// (`v2.py:247-284`): the logo link plus the full
/// `workspace.save()` stamps (`auto_now` `updated_at`, crum
/// `updated_by`). The previous-asset delete stays at the caller.
fn logo_link_stmt(
    workspace_id: Uuid,
    asset_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> UpdateMany<workspaces::Entity> {
    workspaces::Entity::update_many()
        .col_expr(workspaces::Column::Logo, Expr::value(""))
        .col_expr(workspaces::Column::LogoAssetId, Expr::value(asset_id))
        .col_expr(workspaces::Column::UpdatedAt, Expr::value(now))
        .col_expr(workspaces::Column::UpdatedById, Expr::value(*actor_id))
        .filter(workspaces::Column::Id.eq(workspace_id))
}

/// Link a logo asset onto one workspace row
/// (`logo = ""`, the ported empty-string write).
pub async fn stamp_logo(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    asset_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> Result<(), sqlx::Error> {
    exec_update(db, logo_link_stmt(workspace_id, asset_id, now, actor_id)).await
}

/// `WorkspaceFileAssetEndpoint.entity_asset_delete`
/// (`v2.py:286-312`): clear the logo link FK plus the full
/// `workspace.save()` stamps.
fn logo_unlink_stmt(
    workspace_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> UpdateMany<workspaces::Entity> {
    workspaces::Entity::update_many()
        .col_expr(workspaces::Column::LogoAssetId, Expr::value(None::<Uuid>))
        .col_expr(workspaces::Column::UpdatedAt, Expr::value(now))
        .col_expr(workspaces::Column::UpdatedById, Expr::value(*actor_id))
        .filter(workspaces::Column::Id.eq(workspace_id))
}

/// Clear one workspace row's logo link FK.
pub async fn clear_logo_asset(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> Result<(), sqlx::Error> {
    exec_update(db, logo_unlink_stmt(workspace_id, now, actor_id)).await
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
    use workspace_members::Column as M;

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

    /// The full member column list, in the insert order every
    /// `add`/`bulk_add` row follows.
    const MEMBER_COLUMNS: [M; 17] = [
        M::Id,
        M::CreatedAt,
        M::UpdatedAt,
        M::CreatedById,
        M::UpdatedById,
        M::DeletedAt,
        M::WorkspaceId,
        M::MemberId,
        M::Role,
        M::CompanyRole,
        M::ViewProps,
        M::DefaultProps,
        M::IssueProps,
        M::GettingStartedChecklist,
        M::Tips,
        M::ExploredFeatures,
        M::IsActive,
    ];

    /// The [`add`] insert: the request clock (or `CURRENT_TIMESTAMP`
    /// under `db_now`), the optional `ON CONFLICT DO NOTHING`.
    pub(crate) fn member_insert(id: Uuid, member: &NewMember<'_>) -> InsertStatement {
        let stamp = || {
            if member.db_now {
                Expr::current_timestamp().into()
            } else {
                Expr::value(member.now)
            }
        };
        let mut stmt = Query::insert();
        stmt.into_table(workspace_members::Entity)
            .columns(MEMBER_COLUMNS)
            .values_panic([
                Expr::value(id),
                stamp(),
                stamp(),
                Expr::value(member.created_by),
                Expr::value(None::<Uuid>),
                Expr::value(None::<DateTime<Utc>>),
                Expr::value(member.workspace_id),
                Expr::value(member.user_id),
                Expr::value(member.role),
                Expr::value(member.company_role),
                json(member.view_props),
                json(member.default_props),
                json(member.issue_props),
                empty_object(),
                empty_object(),
                empty_object(),
                Expr::value(true),
            ]);
        if member.ignore_conflicts && !member.db_now {
            stmt.on_conflict(OnConflict::new().do_nothing().to_owned());
        }
        stmt.to_owned()
    }

    /// `WorkspaceMember.objects.create(...)` (and the per-row signup
    /// replays of `bulk_create(..., ignore_conflicts=True)`). Returns
    /// the new row id. Integrity errors propagate for the caller's
    /// 400/409/500 arm.
    pub async fn add(
        db: &impl ConnectionTrait,
        member: &NewMember<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        exec_insert(db, &member_insert(id, member)).await?;
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

    /// The multi-row `INSERT ... ON CONFLICT DO NOTHING`: one
    /// `gen_random_uuid()` row per member (stamps, audit, membership,
    /// role, shared props). `None` for an empty list. Pure.
    pub(crate) fn bulk_add_insert(
        rows: &[BulkMember],
        props: &serde_json::Value,
        issue_props: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Option<InsertStatement> {
        if rows.is_empty() {
            return None;
        }
        let mut stmt = Query::insert();
        stmt.into_table(workspace_members::Entity)
            .columns(MEMBER_COLUMNS);
        // The shared props bind per row: Django's `get_default_props`
        // callables mint a fresh (identical-content) dict per
        // instance, so shared binds commit identical bytes.
        for row in rows {
            stmt.values_panic([
                Func::cust(Alias::new("gen_random_uuid")).into(),
                Expr::value(now),
                Expr::value(now),
                Expr::value(row.created_by),
                Expr::value(None::<Uuid>),
                Expr::value(None::<DateTime<Utc>>),
                Expr::value(row.workspace_id),
                Expr::value(row.user_id),
                Expr::value(row.role),
                Expr::value(None::<String>),
                json(props),
                json(props),
                json(issue_props),
                empty_object(),
                empty_object(),
                empty_object(),
                Expr::value(true),
            ]);
        }
        stmt.on_conflict(OnConflict::new().do_nothing().to_owned());
        Some(stmt.to_owned())
    }

    /// `WorkspaceMember.objects.bulk_create([...],
    /// ignore_conflicts=True)`: one multi-row `INSERT ...
    /// ON CONFLICT DO NOTHING`. An empty list emits no statement at
    /// all, like `bulk_create([])`. `props` serves both `view_props`
    /// and `default_props`: they share `get_default_props`
    /// (`workspace.py:207-208`), and both callers bind identical
    /// content for the two columns.
    pub async fn bulk_add(
        db: &impl ConnectionTrait,
        rows: &[BulkMember],
        props: &serde_json::Value,
        issue_props: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        match bulk_add_insert(rows, props, issue_props, now) {
            Some(stmt) => exec_insert(db, &stmt).await,
            None => Ok(()),
        }
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

    pub(crate) fn update_role_stmt(update: &RoleUpdate) -> UpdateMany<workspace_members::Entity> {
        let mut stmt = workspace_members::Entity::update_many()
            .col_expr(M::IsActive, Expr::value(true))
            .col_expr(M::Role, Expr::value(update.role))
            .col_expr(M::UpdatedAt, Expr::value(update.updated_at))
            .col_expr(M::UpdatedById, Expr::value(update.updated_by));
        if let Some(created_by) = update.created_by {
            stmt = stmt.col_expr(M::CreatedById, Expr::value(created_by));
        }
        stmt.filter(M::Id.eq(update.id))
    }

    /// The invite-accept / join-approve reactivate `.save()`: active
    /// plus role plus the audit stamps.
    pub async fn update_role(
        db: &impl ConnectionTrait,
        update: &RoleUpdate,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, update_role_stmt(update)).await
    }

    pub(crate) fn activate_by_membership_stmt(
        workspace_id: Uuid,
        user_id: Uuid,
        role: i16,
    ) -> UpdateMany<workspace_members::Entity> {
        workspace_members::Entity::update_many()
            .col_expr(M::IsActive, Expr::value(true))
            .col_expr(M::Role, Expr::value(role))
            .filter(M::WorkspaceId.eq(workspace_id))
            .filter(M::MemberId.eq(user_id))
            .filter(M::DeletedAt.is_null())
    }

    /// The my-invites per-invite reactivate: a queryset `update()`,
    /// so `SET` only — no `updated_at` stamp.
    pub async fn activate_by_membership(
        db: &impl ConnectionTrait,
        workspace_id: Uuid,
        user_id: Uuid,
        role: i16,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, activate_by_membership_stmt(workspace_id, user_id, role)).await
    }

    pub(crate) fn activate_by_id_stmt(
        id: Uuid,
        updated_by: Option<Uuid>,
    ) -> UpdateMany<workspace_members::Entity> {
        workspace_members::Entity::update_many()
            .col_expr(M::IsActive, Expr::value(true))
            .col_expr(M::UpdatedAt, Expr::current_timestamp().into())
            .col_expr(M::UpdatedById, Expr::value(updated_by))
            .filter(M::Id.eq(id))
    }

    /// The project-invite join reactivate `.save()`: active plus
    /// stamps, no role change. The DB clock preserved.
    pub async fn activate_by_id(
        db: &impl ConnectionTrait,
        id: Uuid,
        updated_by: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, activate_by_id_stmt(id, updated_by)).await
    }

    pub(crate) fn deactivate_stmt(
        id: Uuid,
        updated_at: DateTime<Utc>,
        updated_by: Uuid,
    ) -> UpdateMany<workspace_members::Entity> {
        workspace_members::Entity::update_many()
            .col_expr(M::IsActive, Expr::value(false))
            .col_expr(M::UpdatedAt, Expr::value(updated_at))
            .col_expr(M::UpdatedById, Expr::value(updated_by))
            .filter(M::Id.eq(id))
    }

    /// The member deactivation `.save()`: inactive plus the stamps
    /// (`updated_by` from the request user, `db/models/base.py:23-44`).
    pub async fn deactivate(
        db: &impl ConnectionTrait,
        id: Uuid,
        updated_at: DateTime<Utc>,
        updated_by: Uuid,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, deactivate_stmt(id, updated_at, updated_by)).await
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

    /// One workspace-seed member row (`WorkspaceMember.objects.create
    /// :535-540` in `bgtasks/workspace_seed_task.py`: role 20,
    /// `company_role=""`, no `created_by` so NULL audit). Both props
    /// columns bind the one `get_default_props` value
    /// (`db/models/workspace.py:22`); `issue_props` is `get_issue_props`
    /// (`db/models/workspace.py:110-111`). Moved verbatim from the
    /// seed store (PIDASHCONV-1632); the caller keeps its props copy.
    pub struct SeedMember<'a> {
        pub workspace_id: Uuid,
        pub member_id: Uuid,
        pub role: i16,
        pub company_role: &'a str,
        pub view_props: &'a serde_json::Value,
        pub issue_props: &'a serde_json::Value,
        pub now: DateTime<Utc>,
    }

    /// The overlay member insert (D6 gap PIDASHCONV-1613): the full
    /// row with NULL audit and `company_role`.
    pub(crate) fn member_insert_active(
        id: Uuid,
        member: &NewMemberWithActive<'_>,
    ) -> InsertStatement {
        Query::insert()
            .into_table(workspace_members::Entity)
            .columns([
                M::Id,
                M::CreatedAt,
                M::UpdatedAt,
                M::CreatedById,
                M::UpdatedById,
                M::DeletedAt,
                M::WorkspaceId,
                M::MemberId,
                M::Role,
                M::CompanyRole,
                M::ViewProps,
                M::DefaultProps,
                M::IssueProps,
                M::IsActive,
                M::GettingStartedChecklist,
                M::Tips,
                M::ExploredFeatures,
            ])
            .values_panic([
                Expr::value(id),
                Expr::value(member.now),
                Expr::value(member.now),
                Expr::value(None::<Uuid>),
                Expr::value(None::<Uuid>),
                Expr::value(None::<DateTime<Utc>>),
                Expr::value(member.workspace_id),
                Expr::value(member.user_id),
                Expr::value(member.role),
                Expr::value(None::<String>),
                json(member.view_props),
                json(member.view_props),
                json(member.issue_props),
                Expr::value(member.is_active),
                empty_object(),
                empty_object(),
                empty_object(),
            ])
            .to_owned()
    }

    /// The platform member insert. Returns the new row id. Integrity
    /// errors propagate for the caller's savepoint arm.
    pub async fn add_with_active(
        db: &impl ConnectionTrait,
        member: &NewMemberWithActive<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        exec_insert(db, &member_insert_active(id, member)).await?;
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

    /// `updated_at` first, then the dirty `role` / `is_active`
    /// columns (the overlay's overwrite, sync and sweep shapes).
    pub(crate) fn stamp_role_active_stmt(
        id: Uuid,
        updated_at: DateTime<Utc>,
        role: Option<i16>,
        is_active: Option<bool>,
    ) -> UpdateMany<workspace_members::Entity> {
        let mut stmt = workspace_members::Entity::update_many()
            .col_expr(M::UpdatedAt, Expr::value(updated_at));
        if let Some(role) = role {
            stmt = stmt.col_expr(M::Role, Expr::value(role));
        }
        if let Some(is_active) = is_active {
            stmt = stmt.col_expr(M::IsActive, Expr::value(is_active));
        }
        stmt.filter(M::Id.eq(id))
    }

    /// Overwrite one member's role and active flag plus `updated_at`.
    pub async fn overwrite(
        db: &impl ConnectionTrait,
        overwrite: &MemberOverwrite,
    ) -> Result<(), sqlx::Error> {
        exec_update(
            db,
            stamp_role_active_stmt(
                overwrite.id,
                overwrite.updated_at,
                Some(overwrite.role),
                Some(overwrite.is_active),
            ),
        )
        .await
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

    /// Sync one member toward `role`/active, writing only dirty
    /// columns; a converged member emits no statement.
    pub async fn sync(db: &impl ConnectionTrait, sync: &MemberSync) -> Result<(), sqlx::Error> {
        let role_dirty = sync.current_role != sync.role;
        let active_dirty = !sync.current_is_active;
        if !role_dirty && !active_dirty {
            return Ok(());
        }
        exec_update(
            db,
            stamp_role_active_stmt(
                sync.id,
                sync.updated_at,
                role_dirty.then_some(sync.role),
                active_dirty.then_some(true),
            ),
        )
        .await
    }

    /// The sweep member deactivation (`ee-overlay`
    /// `service.py:847-849`): a queryset-style
    /// `save(update_fields=["is_active", "updated_at"])` with no
    /// `updated_by` stamp.
    pub async fn deactivate_without_updated_by(
        db: &impl ConnectionTrait,
        id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        exec_update(
            db,
            stamp_role_active_stmt(id, updated_at, None, Some(false)),
        )
        .await
    }

    /// The validated `PATCH members/<pk>/` columns: every settable
    /// column the serializer admits, resolved from the validated
    /// patch. No `updated_by` field: the validated value is dropped
    /// at the call site — `BaseModel.save` overwrites an explicit
    /// `updated_by` input with the request user, so the stamp wins
    /// unconditionally.
    pub struct MemberPatch {
        pub role: Option<i32>,
        pub company_role: Option<Option<String>>,
        pub view_props: Option<serde_json::Value>,
        pub default_props: Option<serde_json::Value>,
        pub issue_props: Option<serde_json::Value>,
        pub is_active: Option<bool>,
        pub getting_started_checklist: Option<serde_json::Value>,
        pub tips: Option<serde_json::Value>,
        pub explored_features: Option<serde_json::Value>,
        pub workspace: Option<Uuid>,
        pub created_by: Option<Option<Uuid>>,
        pub deleted_at: Option<Option<DateTime<Utc>>>,
    }

    /// The PATCH statement: the set columns in the fixed order, then
    /// the `updated_at`/`updated_by` stamps. Pure.
    pub(crate) fn patch_stmt(
        target_id: &Uuid,
        actor_id: &Uuid,
        patch: &MemberPatch,
        now: DateTime<Utc>,
    ) -> UpdateMany<workspace_members::Entity> {
        let mut stmt = workspace_members::Entity::update_many();
        if let Some(role) = patch.role {
            stmt = stmt.col_expr(
                M::Role,
                Expr::value(i16::try_from(role).expect("role fits")),
            );
        }
        if let Some(company_role) = &patch.company_role {
            stmt = stmt.col_expr(M::CompanyRole, Expr::value(company_role.clone()));
        }
        if let Some(view_props) = &patch.view_props {
            stmt = stmt.col_expr(M::ViewProps, json(view_props));
        }
        if let Some(default_props) = &patch.default_props {
            stmt = stmt.col_expr(M::DefaultProps, json(default_props));
        }
        if let Some(issue_props) = &patch.issue_props {
            stmt = stmt.col_expr(M::IssueProps, json(issue_props));
        }
        if let Some(is_active) = patch.is_active {
            stmt = stmt.col_expr(M::IsActive, Expr::value(is_active));
        }
        if let Some(checklist) = &patch.getting_started_checklist {
            stmt = stmt.col_expr(M::GettingStartedChecklist, json(checklist));
        }
        if let Some(tips) = &patch.tips {
            stmt = stmt.col_expr(M::Tips, json(tips));
        }
        if let Some(features) = &patch.explored_features {
            stmt = stmt.col_expr(M::ExploredFeatures, json(features));
        }
        if let Some(created_by) = &patch.created_by {
            stmt = stmt.col_expr(M::CreatedById, Expr::value(*created_by));
        }
        if let Some(workspace) = &patch.workspace {
            stmt = stmt.col_expr(M::WorkspaceId, Expr::value(*workspace));
        }
        if let Some(deleted_at) = &patch.deleted_at {
            stmt = stmt.col_expr(M::DeletedAt, Expr::value(*deleted_at));
        }
        stmt.col_expr(M::UpdatedAt, Expr::value(now))
            .col_expr(M::UpdatedById, Expr::value(*actor_id))
            .filter(M::Id.eq(*target_id))
    }

    /// Apply validated PATCH assignments: the dynamic `SET` plus the
    /// `updated_at`/`updated_by` stamps. Moved from
    /// `api::app_workspace::handlers_members` (PIDASHCONV-1628) — the
    /// column order is unchanged. Runs on the caller's connection.
    pub async fn patch(
        db: &impl ConnectionTrait,
        target_id: &Uuid,
        actor_id: &Uuid,
        patch: &MemberPatch,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, patch_stmt(target_id, actor_id, patch, now)).await
    }

    pub(crate) fn save_view_props_stmt(
        id: Uuid,
        props: &serde_json::Value,
        now: DateTime<Utc>,
        updated_by: Uuid,
    ) -> UpdateMany<workspace_members::Entity> {
        workspace_members::Entity::update_many()
            .col_expr(M::ViewProps, json(props))
            .col_expr(M::UpdatedAt, Expr::value(now))
            .col_expr(M::UpdatedById, Expr::value(updated_by))
            .filter(M::Id.eq(id))
    }

    /// `POST workspaces/<slug>/workspace-views/`: `view_props` saved
    /// as-is plus the `updated_at`/`updated_by` stamps. Runs on the
    /// caller's connection.
    pub async fn save_view_props(
        db: &impl ConnectionTrait,
        id: Uuid,
        props: &serde_json::Value,
        now: DateTime<Utc>,
        updated_by: Uuid,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, save_view_props_stmt(id, props, now, updated_by)).await
    }

    /// The overlay bulk deactivations (D6 gap PIDASHCONV-1617): the
    /// workspace scope and the workspace+member scope of
    /// `_deactivate_local_access`.
    pub(crate) fn deactivate_by_workspace_stmt(
        workspace_id: Uuid,
        member_id: Option<Uuid>,
    ) -> UpdateMany<workspace_members::Entity> {
        let stmt = workspace_members::Entity::update_many()
            .col_expr(M::IsActive, Expr::value(false))
            .filter(M::WorkspaceId.eq(workspace_id))
            .filter(M::DeletedAt.is_null())
            .filter(M::IsActive.eq(true));
        match member_id {
            Some(user) => stmt.filter(M::MemberId.eq(user)),
            None => stmt,
        }
    }

    /// The platform bulk deactivation (`pi_dash_cloud` `service.py:480-489`):
    /// `WorkspaceMember.objects.filter(workspace, is_active=True)[.filter(member)]
    /// .update(is_active=False)`. A queryset `update()`, so `SET` only — no
    /// `updated_at` stamp and no `updated_by` (unlike [`deactivate`]); the
    /// soft-delete scope comes from the default manager. Runs on the
    /// caller's connection (the overlay holds its own transaction).
    pub async fn deactivate_by_workspace(
        db: &impl ConnectionTrait,
        workspace_id: Uuid,
        member_id: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        exec_update(db, deactivate_by_workspace_stmt(workspace_id, member_id)).await
    }

    // The user-deactivate bulk template (PIDASHCONV-1631; relocated
    // verbatim from `crate::app_workspace::queries_user`, whose
    // `pub use` shim keeps the handler path stable). The handler
    // chunks `:pks` and executes per chunk, so the write runs
    // handler-side until that call site migrates to an executing twin
    // (contrast the project twin, `domain::projects::members`).

    /// Workspace bulk deactivation write (`:310`):
    /// `bulk_update(rows, ["is_active"], batch_size=100)`.
    ///
    /// Ported bug 5: `bulk_update` bypasses `save()`, so `auto_now` does
    /// NOT stamp `updated_at`, and the write is pk-direct with NO
    /// `deleted_at` filter. The handler chunks `:pks` into batches of
    /// [`crate::app_workspace::queries_user::BULK_DEACTIVATE_BATCH_SIZE`];
    /// an empty collection issues NO statement at all. (The project
    /// twin moved to `domain::projects::members`.)
    pub fn workspace_bulk_deactivate_sql() -> String {
        "UPDATE workspace_members SET is_active = FALSE WHERE workspace_members.id IN (:pks)"
            .to_owned()
    }

    /// The seed bot membership insert (`created_at = updated_at`, NULL
    /// audit, both props columns the one default value).
    pub(crate) fn seed_member_insert(id: Uuid, member: &SeedMember<'_>) -> InsertStatement {
        Query::insert()
            .into_table(workspace_members::Entity)
            .columns([
                M::CreatedAt,
                M::UpdatedAt,
                M::Id,
                M::Role,
                M::CreatedById,
                M::MemberId,
                M::UpdatedById,
                M::WorkspaceId,
                M::CompanyRole,
                M::ViewProps,
                M::DefaultProps,
                M::IssueProps,
                M::IsActive,
                M::DeletedAt,
                M::ExploredFeatures,
                M::GettingStartedChecklist,
                M::Tips,
            ])
            .values_panic([
                Expr::value(member.now),
                Expr::value(member.now),
                Expr::value(id),
                Expr::value(member.role),
                Expr::value(None::<Uuid>),
                Expr::value(member.member_id),
                Expr::value(None::<Uuid>),
                Expr::value(member.workspace_id),
                Expr::value(member.company_role),
                json(member.view_props),
                json(member.view_props),
                json(member.issue_props),
                Expr::value(true),
                Expr::value(None::<DateTime<Utc>>),
                empty_object(),
                empty_object(),
                empty_object(),
            ])
            .to_owned()
    }

    /// The seed bot membership insert. Returns the new row id (the old
    /// `RETURNING id` echoed the bound value).
    pub async fn add_seed(
        db: &impl ConnectionTrait,
        member: &SeedMember<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        exec_insert(db, &seed_member_insert(id, member)).await?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::members::*;
    use super::*;
    use pidash_db::orm_tx::TxConn;

    fn orm(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
        pidash_db::pool::sea_orm_connection(pool)
    }

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn pin_time() -> DateTime<Utc> {
        "2026-11-05T10:00:00+00:00".parse().expect("pin time")
    }

    fn render_insert(stmt: &InsertStatement) -> String {
        DbBackend::Postgres.build(stmt).to_string()
    }

    fn render_update<E: EntityTrait>(stmt: UpdateMany<E>) -> String {
        use sea_orm::QueryTrait;
        stmt.build(DbBackend::Postgres).to_string()
    }

    fn pin_workspace() -> NewWorkspace<'static> {
        NewWorkspace {
            id: uid(1),
            name: "Acme",
            slug: "acme",
            owner_id: uid(2),
            created_by: Some(uid(2)),
            logo: None,
            logo_asset_id: None,
            organization_size: Some("10"),
            timezone: "UTC",
            background_color: "#fff",
            deleted_at: None,
            created_at: pin_time(),
            updated_at: pin_time(),
            seed_created_by: None,
        }
    }

    /// Every statement this module builds, rendered with inlined
    /// values: columns, SET order, filters, conflict arms and
    /// RETURNING lists pinned without a database.
    #[test]
    fn statement_shapes() {
        let props = serde_json::json!({"a": 1});
        let member = |ignore_conflicts: bool, db_now: bool| NewMember {
            workspace_id: uid(1),
            user_id: Some(uid(3)),
            role: 15,
            company_role: None,
            created_by: Some(uid(2)),
            view_props: &props,
            default_props: &props,
            issue_props: &props,
            now: pin_time(),
            ignore_conflicts,
            db_now,
        };
        let mut got = vec![
            render_insert(&workspace_insert(&pin_workspace())),
            render_update(rename_stmt(uid(1), "acme-2")),
            render_update(set_name_stmt(uid(1), "Acme Org", pin_time())),
            DbBackend::Postgres
                .build(&super::patch_stmt(
                    uid(1),
                    &["name", "slug", "bogus"],
                    &WorkspacePatch {
                        deleted_at: None,
                        name: "N",
                        logo: None,
                        slug: "s",
                        organization_size: None,
                        timezone: "UTC",
                        background_color: "#fff",
                        logo_asset_id: None,
                    },
                    &uid(2),
                    pin_time(),
                ))
                .to_string(),
            render_update(soft_delete_stmt(uid(1), pin_time(), uid(2), "acme--x")),
            render_update(logo_link_stmt(uid(1), uid(4), pin_time(), &uid(2))),
            render_update(logo_unlink_stmt(uid(1), pin_time(), &uid(2))),
            render_insert(&member_insert(uid(9), &member(false, false))),
            render_insert(&member_insert(uid(9), &member(true, false))),
            render_insert(&member_insert(uid(9), &member(true, true))),
            render_insert(
                &bulk_add_insert(
                    &[
                        BulkMember {
                            workspace_id: uid(1),
                            user_id: uid(3),
                            role: 5,
                            created_by: None,
                        },
                        BulkMember {
                            workspace_id: uid(5),
                            user_id: uid(6),
                            role: 15,
                            created_by: Some(uid(2)),
                        },
                    ],
                    &props,
                    &props,
                    pin_time(),
                )
                .expect("rows"),
            ),
            render_update(update_role_stmt(&RoleUpdate {
                id: uid(9),
                role: 20,
                updated_at: pin_time(),
                updated_by: Some(uid(2)),
                created_by: None,
            })),
            render_update(update_role_stmt(&RoleUpdate {
                id: uid(9),
                role: 20,
                updated_at: pin_time(),
                updated_by: Some(uid(2)),
                created_by: Some(None),
            })),
            render_update(activate_by_membership_stmt(uid(1), uid(3), 15)),
            render_update(activate_by_id_stmt(uid(9), None)),
            render_update(deactivate_stmt(uid(9), pin_time(), uid(2))),
            render_insert(&member_insert_active(
                uid(9),
                &NewMemberWithActive {
                    workspace_id: uid(1),
                    user_id: uid(3),
                    role: 20,
                    is_active: false,
                    view_props: &props,
                    issue_props: &props,
                    now: pin_time(),
                },
            )),
            render_update(stamp_role_active_stmt(
                uid(9),
                pin_time(),
                Some(15),
                Some(true),
            )),
            render_update(stamp_role_active_stmt(uid(9), pin_time(), Some(15), None)),
            render_update(stamp_role_active_stmt(
                uid(9),
                pin_time(),
                None,
                Some(false),
            )),
            render_update(members::patch_stmt(
                &uid(9),
                &uid(2),
                &MemberPatch {
                    role: Some(20),
                    company_role: Some(None),
                    view_props: None,
                    default_props: None,
                    issue_props: Some(props.clone()),
                    is_active: Some(true),
                    getting_started_checklist: None,
                    tips: None,
                    explored_features: None,
                    workspace: None,
                    created_by: Some(Some(uid(2))),
                    deleted_at: Some(None),
                },
                pin_time(),
            )),
            render_update(save_view_props_stmt(uid(9), &props, pin_time(), uid(2))),
            render_update(deactivate_by_workspace_stmt(uid(1), None)),
            render_update(deactivate_by_workspace_stmt(uid(1), Some(uid(3)))),
            render_insert(&seed_member_insert(
                uid(9),
                &SeedMember {
                    workspace_id: uid(1),
                    member_id: uid(3),
                    role: 20,
                    company_role: "",
                    view_props: &props,
                    issue_props: &props,
                    now: pin_time(),
                },
            )),
        ];
        let want = [
            r#"INSERT INTO "workspaces" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "name", "logo", "logo_asset_id", "owner_id", "slug", "organization_size", "timezone", "background_color") VALUES ('00000000-0000-0000-0000-000000000001', '2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', '00000000-0000-0000-0000-000000000002', NULL, NULL, 'Acme', NULL, NULL, '00000000-0000-0000-0000-000000000002', 'acme', '10', 'UTC', '#fff') RETURNING "created_at", "updated_at""#,
            r#"UPDATE "workspaces" SET "slug" = 'acme-2' WHERE "workspaces"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "workspaces" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "name" = 'Acme Org' WHERE "workspaces"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "workspaces" SET "name" = 'N', "slug" = 's', "bogus" = NULL, "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002' WHERE "id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "workspaces" SET "deleted_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002', "slug" = 'acme--x' WHERE "workspaces"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "workspaces" SET "logo" = '', "logo_asset_id" = '00000000-0000-0000-0000-000000000004', "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002' WHERE "workspaces"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"UPDATE "workspaces" SET "logo_asset_id" = NULL, "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002' WHERE "workspaces"."id" = '00000000-0000-0000-0000-000000000001'"#,
            r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_id", "member_id", "role", "company_role", "view_props", "default_props", "issue_props", "getting_started_checklist", "tips", "explored_features", "is_active") VALUES ('00000000-0000-0000-0000-000000000009', '2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', '00000000-0000-0000-0000-000000000002', NULL, NULL, '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000003', 15, NULL, E'{\"a\":1}', E'{\"a\":1}', E'{\"a\":1}', '{}', '{}', '{}', TRUE)"#,
            r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_id", "member_id", "role", "company_role", "view_props", "default_props", "issue_props", "getting_started_checklist", "tips", "explored_features", "is_active") VALUES ('00000000-0000-0000-0000-000000000009', '2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', '00000000-0000-0000-0000-000000000002', NULL, NULL, '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000003', 15, NULL, E'{\"a\":1}', E'{\"a\":1}', E'{\"a\":1}', '{}', '{}', '{}', TRUE) ON CONFLICT  DO NOTHING"#,
            r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_id", "member_id", "role", "company_role", "view_props", "default_props", "issue_props", "getting_started_checklist", "tips", "explored_features", "is_active") VALUES ('00000000-0000-0000-0000-000000000009', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, '00000000-0000-0000-0000-000000000002', NULL, NULL, '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000003', 15, NULL, E'{\"a\":1}', E'{\"a\":1}', E'{\"a\":1}', '{}', '{}', '{}', TRUE)"#,
            r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_id", "member_id", "role", "company_role", "view_props", "default_props", "issue_props", "getting_started_checklist", "tips", "explored_features", "is_active") VALUES (gen_random_uuid(), '2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', NULL, NULL, NULL, '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000003', 5, NULL, E'{\"a\":1}', E'{\"a\":1}', E'{\"a\":1}', '{}', '{}', '{}', TRUE), (gen_random_uuid(), '2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', '00000000-0000-0000-0000-000000000002', NULL, NULL, '00000000-0000-0000-0000-000000000005', '00000000-0000-0000-0000-000000000006', 15, NULL, E'{\"a\":1}', E'{\"a\":1}', E'{\"a\":1}', '{}', '{}', '{}', TRUE) ON CONFLICT  DO NOTHING"#,
            r#"UPDATE "workspace_members" SET "is_active" = TRUE, "role" = 20, "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002' WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "is_active" = TRUE, "role" = 20, "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002', "created_by_id" = NULL WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "is_active" = TRUE, "role" = 15 WHERE "workspace_members"."workspace_id" = '00000000-0000-0000-0000-000000000001' AND "workspace_members"."member_id" = '00000000-0000-0000-0000-000000000003' AND "workspace_members"."deleted_at" IS NULL"#,
            r#"UPDATE "workspace_members" SET "is_active" = TRUE, "updated_at" = CURRENT_TIMESTAMP, "updated_by_id" = NULL WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "is_active" = FALSE, "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002' WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"INSERT INTO "workspace_members" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "workspace_id", "member_id", "role", "company_role", "view_props", "default_props", "issue_props", "is_active", "getting_started_checklist", "tips", "explored_features") VALUES ('00000000-0000-0000-0000-000000000009', '2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', NULL, NULL, NULL, '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000003', 20, NULL, E'{\"a\":1}', E'{\"a\":1}', E'{\"a\":1}', FALSE, '{}', '{}', '{}')"#,
            r#"UPDATE "workspace_members" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "role" = 15, "is_active" = TRUE WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "role" = 15 WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "is_active" = FALSE WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "role" = 20, "company_role" = NULL, "issue_props" = E'{\"a\":1}', "is_active" = TRUE, "created_by_id" = '00000000-0000-0000-0000-000000000002', "deleted_at" = NULL, "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002' WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "view_props" = E'{\"a\":1}', "updated_at" = '2026-11-05 10:00:00.000000 +00:00', "updated_by_id" = '00000000-0000-0000-0000-000000000002' WHERE "workspace_members"."id" = '00000000-0000-0000-0000-000000000009'"#,
            r#"UPDATE "workspace_members" SET "is_active" = FALSE WHERE "workspace_members"."workspace_id" = '00000000-0000-0000-0000-000000000001' AND "workspace_members"."deleted_at" IS NULL AND "workspace_members"."is_active" = TRUE"#,
            r#"UPDATE "workspace_members" SET "is_active" = FALSE WHERE "workspace_members"."workspace_id" = '00000000-0000-0000-0000-000000000001' AND "workspace_members"."deleted_at" IS NULL AND "workspace_members"."is_active" = TRUE AND "workspace_members"."member_id" = '00000000-0000-0000-0000-000000000003'"#,
            r#"INSERT INTO "workspace_members" ("created_at", "updated_at", "id", "role", "created_by_id", "member_id", "updated_by_id", "workspace_id", "company_role", "view_props", "default_props", "issue_props", "is_active", "deleted_at", "explored_features", "getting_started_checklist", "tips") VALUES ('2026-11-05 10:00:00.000000 +00:00', '2026-11-05 10:00:00.000000 +00:00', '00000000-0000-0000-0000-000000000009', 20, NULL, '00000000-0000-0000-0000-000000000003', NULL, '00000000-0000-0000-0000-000000000001', '', E'{\"a\":1}', E'{\"a\":1}', E'{\"a\":1}', TRUE, NULL, '{}', '{}', '{}')"#,
        ];
        assert!(bulk_add_insert(&[], &props, &props, pin_time()).is_none());
        assert_eq!(got.len(), want.len());
        for (got, want) in got.drain(..).zip(want) {
            assert_eq!(got, want);
        }
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

    /// The logo link/unlink twins stamp the temp row with the same
    /// binds the v2 asset paths pass.
    #[tokio::test]
    async fn live_logo_link_round_trip() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(WORKSPACES_DDL)
            .execute(&pool)
            .await
            .expect("temp workspaces");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let actor = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, owner_id, slug, timezone, background_color) VALUES ($1, $2, $2, 'Acme', $3, 'acme', 'UTC', '#fff')",
        )
        .bind(ws)
        .bind(now)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("workspace row");
        let asset = Uuid::new_v4();
        stamp_logo(&orm(&pool), ws, asset, now, &actor)
            .await
            .expect("logo link");
        let linked: (Option<String>, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "SELECT logo, logo_asset_id, updated_by_id FROM workspaces WHERE id = $1",
        )
        .bind(ws)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(linked, (Some(String::new()), Some(asset), Some(actor)));
        clear_logo_asset(&orm(&pool), ws, now, &actor)
            .await
            .expect("logo unlink");
        let unlinked: Option<Uuid> =
            sqlx::query_scalar("SELECT logo_asset_id FROM workspaces WHERE id = $1")
                .bind(ws)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(unlinked, None);
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
            &orm(&pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
        bulk_add(&orm(&pool), &[], &view, &issue, now)
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
        bulk_add(&orm(&pool), &rows, &view, &issue, now)
            .await
            .expect("bulk add");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_members")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 3);
        // Re-run: conflicts skipped, still 3 rows.
        bulk_add(&orm(&pool), &rows, &view, &issue, now)
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
            &orm(&pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
        deactivate(&orm(&pool), id, now, actor)
            .await
            .expect("deactivate");
        let active: bool =
            sqlx::query_scalar("SELECT is_active FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(!active);
        activate_by_id(&orm(&pool), id, Some(actor))
            .await
            .expect("activate_by_id");
        let row: (bool, i16) =
            sqlx::query_as("SELECT is_active, role FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(row, (true, 5));
        deactivate(&orm(&pool), id, now, actor)
            .await
            .expect("deactivate");
        activate_by_membership(&orm(&pool), ws, user, 15)
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
                &orm(&pool),
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
            &orm(&pool),
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
            &TxConn::new(&mut tx),
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
            &orm(&pool),
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
                &orm(pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
            &orm(&pool),
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
        deactivate_without_updated_by(&orm(&pool), id, later)
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
                &orm(&pool),
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
        deactivate_by_workspace(&orm(&pool), ws_a, Some(user_1))
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
        deactivate_by_workspace(&orm(&pool), ws_a, None)
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
        rename(&orm(&pool), id, "acme-2").await.expect("rename");
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
            &orm(&pool),
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
            &TxConn::new(&mut tx),
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
            &orm(&pool),
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
        set_name(&orm(&pool), id, "Acme Org", later)
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

    /// PIDASHCONV-1628: the workspace PATCH renderer pins its exact
    /// bytes (validated columns in order, then the two stamps).
    /// PIDASHCONV-1628: `patch` applies the validated columns plus
    /// the `updated_at`/`updated_by` re-stamp.
    #[tokio::test]
    async fn live_patch_applies_columns_plus_stamps() {
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
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let owner = Uuid::new_v4();
        let id = Uuid::new_v4();
        create(
            &pool,
            &NewWorkspace {
                id,
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
            },
        )
        .await
        .expect("create");
        let actor = Uuid::new_v4();
        super::patch(
            &orm(&pool),
            id,
            &["name", "timezone"],
            &WorkspacePatch {
                deleted_at: None,
                name: "Acme 2",
                logo: None,
                slug: "acme",
                organization_size: None,
                timezone: "UTC",
                background_color: "#fff",
                logo_asset_id: None,
            },
            &actor,
            later,
        )
        .await
        .expect("patch");
        let row: (String, String, DateTime<Utc>, Option<Uuid>) = sqlx::query_as(
            "SELECT name, timezone, updated_at, updated_by_id FROM workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(
            row,
            ("Acme 2".to_owned(), "UTC".to_owned(), later, Some(actor))
        );
    }

    /// PIDASHCONV-1628: `soft_delete` stamps the delete plus the slug
    /// tombstone.
    #[tokio::test]
    async fn live_soft_delete_stamps_delete_and_tombstone() {
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
        let actor = Uuid::new_v4();
        soft_delete(&orm(&pool), id, now, actor, "acme--deleted")
            .await
            .expect("soft_delete");
        let row: (Option<DateTime<Utc>>, DateTime<Utc>, Option<Uuid>, String) = sqlx::query_as(
            "SELECT deleted_at, updated_at, updated_by_id, slug FROM workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(
            row,
            (Some(now), now, Some(actor), "acme--deleted".to_owned())
        );
    }

    /// PIDASHCONV-1628: `members::patch` applies the validated columns
    /// plus the stamps; `save_view_props` saves the props as-is plus
    /// the stamps.
    #[tokio::test]
    async fn live_member_patch_and_view_props() {
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
        let id = add(
            &orm(&pool),
            &NewMember {
                workspace_id: Uuid::new_v4(),
                user_id: Some(Uuid::new_v4()),
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
        members::patch(
            &orm(&pool),
            &id,
            &actor,
            &MemberPatch {
                role: Some(15),
                company_role: Some(Some("eng".to_owned())),
                view_props: None,
                default_props: None,
                issue_props: None,
                is_active: Some(false),
                getting_started_checklist: None,
                tips: None,
                explored_features: None,
                workspace: None,
                created_by: None,
                deleted_at: None,
            },
            later,
        )
        .await
        .expect("member patch");
        let row: (i16, Option<String>, bool, DateTime<Utc>, Option<Uuid>) = sqlx::query_as(
            "SELECT role, company_role, is_active, updated_at, updated_by_id \
             FROM workspace_members WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(row, (15, Some("eng".to_owned()), false, later, Some(actor)));
        let saved = serde_json::json!({"tab": "all"});
        save_view_props(&orm(&pool), id, &saved, later, actor)
            .await
            .expect("save view props");
        let stored: serde_json::Value =
            sqlx::query_scalar("SELECT view_props FROM workspace_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stored, saved);
    }
}
