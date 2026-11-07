//! OSS project writes: `projects`, `project_members`, `states` (ORM-07, PIDASHCONV-1525).
//!
//! Every OSS write to the three project tables lives in this module; the
//! app/v1 create flows, the member endpoints, the invite/join paths, the
//! signup joins, the state endpoints, the intake triage inserts and the
//! ops commands call these functions instead of executing SQL text. Reads
//! stay where they are (design D5).
//!
//! Every statement is a SeaORM entity write or a sea-query statement
//! (`rust-api/docs/orm-migration.md`). Writers take a SeaORM
//! [`ConnectionTrait`]: the caller's pool
//! (`pidash_db::pool::sea_orm_connection(&pool)`) or the caller's open
//! sqlx transaction (`pidash_db::orm_tx::TxConn::new(&mut tx)`), so no
//! transaction boundary moves. Errors stay [`sqlx::Error`] (through
//! [`pidash_db::orm_tx::into_sqlx`]) for the callers' own conflict arms
//! (app class-23 vs v1 23505-only BUG-1, the 23502 ops arms).
//!
//! Execution shapes, by writer kind:
//!
//! * [`create`] and [`create_seed_project`] run on `&PgPool` and own the
//!   one atomic pair in these flows: clearing the workspace default plus
//!   the project `INSERT`, in an internal `sqlx` transaction bridged
//!   through `TxConn`. Every other create leg (the pod `post_save`, the
//!   identifier insert, the admin/lead memberships, the default states,
//!   the `model_activity` enqueue) stays caller-sequenced on autocommit:
//!   the v1 flow's PB-2023 arm sits between the membership leg and the
//!   states leg and must keep observing the partial commit, so no
//!   monolithic create is possible. Callers resolve every input (the
//!   live-default probe, identifier/timezone normalization, the clear
//!   condition).
//! * [`members`] and [`states`] are single-statement writers on the
//!   caller's connection (pool or transaction). Multi-row writers are
//!   single statements too; callers that need several writes (member row
//!   plus property row) sequence the single-row functions, which also
//!   preserves the two create flows' opposite write orders
//!   (property-first on app, member-first on v1).
//!
//! Bind-type uniformity (PIDASHCONV-1509): `sqlx` caches prepared
//! statements by SQL text alone, so one text must always bind the same
//! types. The `int8`-bound ops role arm ([`members::ops_add`]) casts its
//! role explicitly, which keeps its text apart from the `int2`-bound
//! member inserts (pinned by `ops_add_text_differs_from_add`). The app
//! bulk builders bind mixed [`members::RoleBind`] types per row, like the
//! originals.
//!
//! Ported bugs preserved verbatim: PB-1008 retired by PIDASHCONV-1686
//! (Django main sets `project_id` on the `ProjectMember` bulk and the
//! signup arms bind the invite's id); the v1 lead guard, the dual-now
//! member bulks and the per-arm role clamps stay at the callers; the
//! `project_identifiers` insert and the `model_activity`/pod effects
//! are not covered-table writes and stay caller-sequenced.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{
    Alias, Expr, InsertStatement, OnConflict, Query, SelectStatement, SimpleExpr,
};
use sea_orm::{ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, Set, UpdateMany};
use serde_json::Value;
use uuid::Uuid;

use pidash_db::entity::{project_members, project_user_properties, projects, workspaces};
use pidash_db::orm_tx::{into_sqlx, TxConn};

/// Run a sea-query statement on the connection; rows affected.
async fn execute(
    db: &impl ConnectionTrait,
    stmt: &impl sea_orm::StatementBuilder,
) -> Result<u64, sqlx::Error> {
    db.execute(DbBackend::Postgres.build(stmt))
        .await
        .map(|done| done.rows_affected())
        .map_err(into_sqlx)
}

/// `(SELECT id FROM workspaces WHERE slug = $1)`: the slug-scoped
/// member/state writers resolve the workspace inside the statement, as
/// the Django `workspace__slug` joins do.
fn workspace_id_by_slug(slug: &str) -> SelectStatement {
    Query::select()
        .column(workspaces::Column::Id)
        .from(workspaces::Entity)
        .and_where(workspaces::Column::Slug.eq(slug))
        .to_owned()
}

// ---------------------------------------------------------------------------
// create (pool; the clear-default + INSERT atomic pair)
// ---------------------------------------------------------------------------

/// One project row to insert: every column without a DB default, fully
/// resolved by the caller (the live-default probe, identifier and
/// timezone normalization, the int4 fencedown). `deleted_at` is always
/// `NULL` and `updated_by` stays `NULL` on both paths. The v1 flow
/// passes its literals (`network 2`, no descriptions, the agent
/// defaults, ticking on, no asset/estimate/state/archived values) as
/// field values; both flows store one row shape.
pub struct NewProject<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<&'a serde_json::Value>,
    pub description_html: Option<&'a serde_json::Value>,
    /// `PositiveSmallIntegerField` with choices `0`/`2`; the callers'
    /// validators admit only those, so the value is `int2` here.
    pub network: i16,
    pub workspace_id: Uuid,
    pub identifier: &'a str,
    pub default_assignee: Option<Uuid>,
    pub project_lead: Option<Uuid>,
    pub emoji: Option<&'a str>,
    pub icon_prop: Option<&'a serde_json::Value>,
    pub module_view: bool,
    pub cycle_view: bool,
    pub issue_views_view: bool,
    pub page_view: bool,
    pub intake_view: bool,
    pub is_time_tracking_enabled: bool,
    pub is_issue_type_enabled: bool,
    pub guest_view_all_features: bool,
    pub cover_image: Option<&'a str>,
    pub cover_image_asset: Option<Uuid>,
    pub estimate: Option<Uuid>,
    pub archive_in: i32,
    pub close_in: i32,
    pub logo_props: &'a serde_json::Value,
    pub default_state: Option<Uuid>,
    pub archived_at: Option<DateTime<Utc>>,
    pub is_default: bool,
    /// Clear the workspace's other live defaults first: `is_default`
    /// on app, `unsets_other_defaults || is_default` on v1.
    pub clear_others: bool,
    pub timezone: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub repo_url: &'a str,
    pub base_branch: &'a str,
    /// `int4` columns: both callers fence their `i64` inputs down
    /// before the call (`i32_or_500` / `field_int`), so the struct
    /// carries the fenced values.
    pub agent_interval: i32,
    pub agent_max_ticks: i32,
    pub agent_review: i32,
    pub agent_test: i32,
    pub agent_ticking: bool,
    pub default_executor: &'a str,
    pub members_can_edit_states: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The create-path default sweep (`Project.save()` on create): every
/// live default in the workspace clears. Django has both this and the
/// self-excluding update-path clear ([`clear_other_defaults_stmt`]);
/// they stay separate statements (design D6).
fn clear_workspace_defaults_stmt(workspace_id: Uuid) -> UpdateMany<projects::Entity> {
    projects::Entity::update_many()
        .col_expr(projects::Column::IsDefault, Expr::value(false))
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .filter(projects::Column::IsDefault.eq(true))
        .filter(projects::Column::DeletedAt.is_null())
}

/// The `Project` row for [`create`]: every column named, the
/// always-`NULL` audit pair included.
fn project_row(id: Uuid, p: &NewProject<'_>) -> projects::ActiveModel {
    projects::ActiveModel {
        id: Set(id),
        created_at: Set(p.created_at.into()),
        updated_at: Set(p.updated_at.into()),
        created_by_id: Set(Some(p.created_by)),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        name: Set(p.name.to_owned()),
        description: Set(p.description.to_owned()),
        description_text: Set(p.description_text.cloned()),
        description_html: Set(p.description_html.cloned()),
        network: Set(p.network),
        workspace_id: Set(p.workspace_id),
        identifier: Set(p.identifier.to_owned()),
        default_assignee_id: Set(p.default_assignee),
        project_lead_id: Set(p.project_lead),
        emoji: Set(p.emoji.map(str::to_owned)),
        icon_prop: Set(p.icon_prop.cloned()),
        module_view: Set(p.module_view),
        cycle_view: Set(p.cycle_view),
        issue_views_view: Set(p.issue_views_view),
        page_view: Set(p.page_view),
        intake_view: Set(p.intake_view),
        is_time_tracking_enabled: Set(p.is_time_tracking_enabled),
        is_issue_type_enabled: Set(p.is_issue_type_enabled),
        guest_view_all_features: Set(p.guest_view_all_features),
        cover_image: Set(p.cover_image.map(str::to_owned)),
        cover_image_asset_id: Set(p.cover_image_asset),
        estimate_id: Set(p.estimate),
        archive_in: Set(p.archive_in),
        close_in: Set(p.close_in),
        logo_props: Set(p.logo_props.clone()),
        default_state_id: Set(p.default_state),
        archived_at: Set(p.archived_at.map(Into::into)),
        is_default: Set(p.is_default),
        timezone: Set(p.timezone.to_owned()),
        external_source: Set(p.external_source.map(str::to_owned)),
        external_id: Set(p.external_id.map(str::to_owned)),
        repo_url: Set(p.repo_url.to_owned()),
        base_branch: Set(p.base_branch.to_owned()),
        agent_default_interval_seconds: Set(p.agent_interval),
        agent_default_max_ticks: Set(p.agent_max_ticks),
        agent_review_default_interval_seconds: Set(p.agent_review),
        agent_test_default_interval_seconds: Set(p.agent_test),
        agent_ticking_enabled: Set(p.agent_ticking),
        default_agent_executor: Set(p.default_executor.to_owned()),
        members_can_edit_states: Set(p.members_can_edit_states),
    }
}

/// `Project.objects.create(...)`: the default-clear plus the row, in
/// one transaction, returning the new id. Errors propagate as
/// `sqlx::Error` (the transaction aborts) for the caller's own
/// conflict arm.
pub async fn create(pool: &sqlx::PgPool, project: &NewProject<'_>) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    {
        let conn = TxConn::new(&mut tx);
        if project.clear_others {
            clear_workspace_defaults_stmt(project.workspace_id)
                .exec(&conn)
                .await
                .map_err(into_sqlx)?;
        }
        projects::Entity::insert(project_row(id, project))
            .exec_without_returning(&conn)
            .await
            .map_err(into_sqlx)?;
    }
    tx.commit().await?;
    Ok(id)
}

/// `save_project_cover` (`v2.py:631-634`): stamp
/// `cover_image_asset_id` on the re-fetched project; the LAST row
/// wins (the caller loops). The write is a FULL `project.save()`
/// (`db/models/base.py:24-44`), so it also stamps `updated_by` (CRUM
/// request user) and `updated_at`. Moved verbatim from
/// `app_assets::queries_v2_project` (PIDASHCONV-1630). The caller owns
/// the clock (`Utc::now()` at full precision — Postgres rounds to
/// micros on store, as today).
pub async fn save_project_cover(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    asset_id: Uuid,
    updated_at: DateTime<Utc>,
    updated_by: Uuid,
) -> Result<(), sqlx::Error> {
    projects::Entity::update_many()
        .col_expr(projects::Column::CoverImageAssetId, Expr::value(asset_id))
        .col_expr(projects::Column::UpdatedAt, Expr::value(updated_at))
        .col_expr(projects::Column::UpdatedById, Expr::value(updated_by))
        .filter(projects::Column::Id.eq(project_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

// ---------------------------------------------------------------------------
// v1 project PATCH / archive / delete + the git-bind project touch
// ---------------------------------------------------------------------------

/// The v1 archive/unarchive write: `archived_at` to `value` (plus the
/// `save()` audit pair), scoped by workspace, live rows only.
fn archive_v1_stmt(
    value: Option<DateTime<Utc>>,
    id: Uuid,
    workspace_id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> UpdateMany<projects::Entity> {
    projects::Entity::update_many()
        .col_expr(projects::Column::ArchivedAt, Expr::value(value))
        .col_expr(projects::Column::UpdatedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedById, Expr::value(actor))
        .filter(projects::Column::Id.eq(id))
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .filter(projects::Column::DeletedAt.is_null())
}

/// `POST .../<project_id>/archive/` (`views/project.py:528-538`): stamp
/// `archived_at` (plus the `save()` audit pair). Returns the rows
/// matched — the driver 404s on zero.
pub async fn archive_v1_project(
    db: &impl ConnectionTrait,
    id: Uuid,
    workspace_id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    archive_v1_stmt(Some(now), id, workspace_id, actor, now)
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// `DELETE .../<project_id>/archive/` (`views/project.py:552-561`):
/// clear `archived_at` (plus the `save()` audit pair). Returns the rows
/// matched — the driver 404s on zero.
pub async fn unarchive_v1_project(
    db: &impl ConnectionTrait,
    id: Uuid,
    workspace_id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    archive_v1_stmt(None, id, workspace_id, actor, now)
        .exec(db)
        .await
        .map(|done| done.rows_affected)
        .map_err(into_sqlx)
}

/// `Project.save()` default-clear on the update path
/// (`project.py:292-301`): the live defaults in the workspace except
/// the row being saved. The create path
/// ([`clear_workspace_defaults_stmt`]) has no row to exclude, so Django
/// genuinely differs here (D6: same-difference, kept as an explicit
/// variant). The v1 PATCH unset, the app PUT/PATCH clear and the seed
/// demote were byte-different texts of this one statement.
fn clear_other_defaults_stmt(workspace_id: Uuid, except_id: Uuid) -> UpdateMany<projects::Entity> {
    projects::Entity::update_many()
        .col_expr(projects::Column::IsDefault, Expr::value(false))
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .filter(projects::Column::IsDefault.eq(true))
        .filter(projects::Column::Id.ne(except_id))
        .filter(projects::Column::DeletedAt.is_null())
}

/// The PATCH `is_default` atomic unset: every other live default in the
/// workspace clears when this project becomes the default. Runs inside
/// the driver's PATCH transaction.
pub async fn unset_other_project_defaults(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    clear_other_defaults_stmt(workspace_id, id)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// One PATCH SET value. Moved with the PATCH twin; the driver still
/// decides which provided fields join the SET list.
#[derive(Debug, Clone)]
pub enum ProjectPatchBind {
    Str(String),
    OptStr(Option<String>),
    Bool(bool),
    Int(i32),
    OptUuid(Option<Uuid>),
    Json(Value),
}

impl ProjectPatchBind {
    fn into_expr(self) -> SimpleExpr {
        match self {
            ProjectPatchBind::Str(v) => Expr::value(v),
            ProjectPatchBind::OptStr(v) => Expr::value(v),
            ProjectPatchBind::Bool(v) => Expr::value(v),
            ProjectPatchBind::Int(v) => Expr::value(v),
            ProjectPatchBind::OptUuid(v) => Expr::value(v),
            ProjectPatchBind::Json(v) => Expr::value(v),
        }
    }
}

/// One PATCH assignment: the `projects` column by its database name
/// (`"name"`, `"project_lead_id"`, …) and the value to store.
#[derive(Debug, Clone)]
pub struct ProjectPatchSet {
    pub column: String,
    pub value: ProjectPatchBind,
}

/// The PATCH statement for [`apply_project_patch`]: the audit pair
/// first, then the driver's assignments in order; live rows only.
/// A column name the entity does not know is
/// [`sqlx::Error::ColumnNotFound`] (the driver names only real
/// columns).
fn project_patch_stmt(
    sets: &[ProjectPatchSet],
    now: DateTime<Utc>,
    actor: Uuid,
    id: Uuid,
) -> Result<UpdateMany<projects::Entity>, sqlx::Error> {
    let mut update = projects::Entity::update_many()
        .col_expr(projects::Column::UpdatedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedById, Expr::value(actor));
    for set in sets {
        let column = projects::Column::from_str(&set.column)
            .map_err(|_| sqlx::Error::ColumnNotFound(set.column.clone()))?;
        update = update.col_expr(column, set.value.clone().into_expr());
    }
    Ok(update
        .filter(projects::Column::Id.eq(id))
        .filter(projects::Column::DeletedAt.is_null()))
}

/// The PATCH write (`ProjectUpdateSerializer.update` + `Project.save`,
/// `views/project.py`): the audit pair plus one assignment per
/// provided field. Conflict arms stay at the driver (23505 → the 409
/// name body).
pub async fn apply_project_patch(
    db: &impl ConnectionTrait,
    sets: &[ProjectPatchSet],
    now: DateTime<Utc>,
    actor: Uuid,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    project_patch_stmt(sets, now, actor, id)?
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `DELETE workspaces/<slug>/projects/<pk>/` (`views/project.py:480-508`,
/// instance `delete()` → `save()`): `deleted_at` + `updated_*`. The
/// favorite cascade and the related-objects sweep stay driver-side.
pub async fn soft_delete_v1_project(
    db: &impl ConnectionTrait,
    id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    soft_delete(db, id, now, actor).await
}

/// The bind statement for [`save_bind_project_update`]: only the
/// changed columns plus `updated_at`. `None` when nothing changed.
fn bind_project_update_stmt(
    project_id: Uuid,
    repo_url: Option<&str>,
    base_branch: Option<&str>,
    now: DateTime<Utc>,
) -> Option<UpdateMany<projects::Entity>> {
    if repo_url.is_none() && base_branch.is_none() {
        return None;
    }
    let mut update = projects::Entity::update_many();
    if let Some(url) = repo_url {
        update = update.col_expr(projects::Column::RepoUrl, Expr::value(url));
    }
    if let Some(branch) = base_branch {
        update = update.col_expr(projects::Column::BaseBranch, Expr::value(branch));
    }
    Some(
        update
            .col_expr(projects::Column::UpdatedAt, Expr::value(now))
            .filter(projects::Column::Id.eq(project_id)),
    )
}

/// `project.save(update_fields=...)` on git bind (`services.py:341-348`):
/// only the changed columns plus `updated_at`. One twin serves the app
/// and v1 `apply_bind` copies. Unchanged is no statement (Django skips
/// the save).
pub async fn save_bind_project_update(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    repo_url: Option<&str>,
    base_branch: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let Some(update) = bind_project_update_stmt(project_id, repo_url, base_branch, now) else {
        return Ok(());
    };
    update.exec(db).await.map(|_| ()).map_err(into_sqlx)
}

// ---------------------------------------------------------------------------
// update (the app PATCH/PUT write)
// ---------------------------------------------------------------------------

/// Clear every live default in the workspace except this project.
/// The caller owns the guard probe and the tx (the `save()` atomic
/// guard-clear-write triple stays caller-sequenced).
pub async fn clear_other_defaults(
    db: &impl ConnectionTrait,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<(), sqlx::Error> {
    clear_other_defaults_stmt(workspace_id, project_id)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// One project row update: every setter column, fully resolved by
/// the caller (validated-or-instance merge, identifier normalization,
/// the int4 fencedowns). `workspace_id` and `deleted_at` are
/// PUT-only writable; `created_by` is writable on update
/// (`update()` persists it; `save()` overwrites only `updated_by` —
/// `models/base.py`).
pub struct ProjectUpdate<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<&'a Value>,
    pub description_html: Option<&'a Value>,
    /// Choices `0`/`2` (see [`NewProject::network`]).
    pub network: i16,
    pub identifier: &'a str,
    pub default_assignee: Option<Uuid>,
    pub project_lead: Option<Uuid>,
    pub emoji: Option<&'a str>,
    pub icon_prop: Option<&'a Value>,
    pub module_view: bool,
    pub cycle_view: bool,
    pub issue_views_view: bool,
    pub page_view: bool,
    pub intake_view: bool,
    pub is_time_tracking_enabled: bool,
    pub is_issue_type_enabled: bool,
    pub guest_view_all_features: bool,
    pub cover_image: Option<&'a str>,
    pub cover_image_asset: Option<Uuid>,
    pub estimate: Option<Uuid>,
    pub archive_in: i32,
    pub close_in: i32,
    pub logo_props: &'a Value,
    pub default_state: Option<Uuid>,
    pub archived_at: Option<DateTime<Utc>>,
    pub is_default: bool,
    pub timezone: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub repo_url: &'a str,
    pub base_branch: &'a str,
    pub agent_interval: i32,
    pub agent_max_ticks: i32,
    pub agent_review: i32,
    pub agent_test: i32,
    pub agent_ticking: bool,
    pub default_executor: &'a str,
    pub members_can_edit_states: bool,
    pub workspace_id: Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: &'a Uuid,
    pub created_by: Option<Uuid>,
}

/// The app PATCH/PUT statement: all 44 setter columns for one row.
fn project_update_stmt(project_id: Uuid, upd: &ProjectUpdate<'_>) -> UpdateMany<projects::Entity> {
    use projects::Column as C;
    projects::Entity::update_many()
        .col_expr(C::Name, Expr::value(upd.name))
        .col_expr(C::Description, Expr::value(upd.description))
        .col_expr(
            C::DescriptionText,
            Expr::value(upd.description_text.cloned()),
        )
        .col_expr(
            C::DescriptionHtml,
            Expr::value(upd.description_html.cloned()),
        )
        .col_expr(C::Network, Expr::value(upd.network))
        .col_expr(C::Identifier, Expr::value(upd.identifier))
        .col_expr(C::DefaultAssigneeId, Expr::value(upd.default_assignee))
        .col_expr(C::ProjectLeadId, Expr::value(upd.project_lead))
        .col_expr(C::Emoji, Expr::value(upd.emoji))
        .col_expr(C::IconProp, Expr::value(upd.icon_prop.cloned()))
        .col_expr(C::ModuleView, Expr::value(upd.module_view))
        .col_expr(C::CycleView, Expr::value(upd.cycle_view))
        .col_expr(C::IssueViewsView, Expr::value(upd.issue_views_view))
        .col_expr(C::PageView, Expr::value(upd.page_view))
        .col_expr(C::IntakeView, Expr::value(upd.intake_view))
        .col_expr(
            C::IsTimeTrackingEnabled,
            Expr::value(upd.is_time_tracking_enabled),
        )
        .col_expr(
            C::IsIssueTypeEnabled,
            Expr::value(upd.is_issue_type_enabled),
        )
        .col_expr(
            C::GuestViewAllFeatures,
            Expr::value(upd.guest_view_all_features),
        )
        .col_expr(C::CoverImage, Expr::value(upd.cover_image))
        .col_expr(C::CoverImageAssetId, Expr::value(upd.cover_image_asset))
        .col_expr(C::EstimateId, Expr::value(upd.estimate))
        .col_expr(C::ArchiveIn, Expr::value(upd.archive_in))
        .col_expr(C::CloseIn, Expr::value(upd.close_in))
        .col_expr(C::LogoProps, Expr::value(upd.logo_props.clone()))
        .col_expr(C::DefaultStateId, Expr::value(upd.default_state))
        .col_expr(C::ArchivedAt, Expr::value(upd.archived_at))
        .col_expr(C::IsDefault, Expr::value(upd.is_default))
        .col_expr(C::Timezone, Expr::value(upd.timezone))
        .col_expr(C::ExternalSource, Expr::value(upd.external_source))
        .col_expr(C::ExternalId, Expr::value(upd.external_id))
        .col_expr(C::RepoUrl, Expr::value(upd.repo_url))
        .col_expr(C::BaseBranch, Expr::value(upd.base_branch))
        .col_expr(
            C::AgentDefaultIntervalSeconds,
            Expr::value(upd.agent_interval),
        )
        .col_expr(C::AgentDefaultMaxTicks, Expr::value(upd.agent_max_ticks))
        .col_expr(
            C::AgentReviewDefaultIntervalSeconds,
            Expr::value(upd.agent_review),
        )
        .col_expr(
            C::AgentTestDefaultIntervalSeconds,
            Expr::value(upd.agent_test),
        )
        .col_expr(C::AgentTickingEnabled, Expr::value(upd.agent_ticking))
        .col_expr(C::DefaultAgentExecutor, Expr::value(upd.default_executor))
        .col_expr(
            C::MembersCanEditStates,
            Expr::value(upd.members_can_edit_states),
        )
        .col_expr(C::WorkspaceId, Expr::value(upd.workspace_id))
        .col_expr(C::DeletedAt, Expr::value(upd.deleted_at))
        .col_expr(C::UpdatedAt, Expr::value(upd.updated_at))
        .col_expr(C::UpdatedById, Expr::value(*upd.updated_by))
        .col_expr(C::CreatedById, Expr::value(upd.created_by))
        .filter(C::Id.eq(project_id))
}

/// The app project PATCH/PUT write (`ProjectSerializer.update()` +
/// `save()`): all 44 setter columns for one row. Runs on the
/// caller's connection (the update tx also holds the default-clear).
pub async fn apply_update(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    upd: &ProjectUpdate<'_>,
) -> Result<(), sqlx::Error> {
    project_update_stmt(project_id, upd)
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

// ---------------------------------------------------------------------------
// soft delete / archive / cover-asset / text-field stamps
// ---------------------------------------------------------------------------

/// `project.delete()`: the tombstone plus the `save()` stamps. The
/// `soft_delete_related_objects` emit and the cascades stay
/// caller-sequenced (not covered-table writes). Also the v1 delete
/// ([`soft_delete_v1_project`]): same columns, same row.
pub async fn soft_delete(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: Uuid,
) -> Result<(), sqlx::Error> {
    projects::Entity::update_many()
        .col_expr(projects::Column::DeletedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedById, Expr::value(actor_id))
        .filter(projects::Column::Id.eq(project_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// The app archive/unarchive write: `archived_at` to `value` plus the
/// `save()` stamps, by id (no workspace scope, unlike the v1 twin).
async fn stamp_archived_at(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    value: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    actor_id: Uuid,
) -> Result<(), sqlx::Error> {
    projects::Entity::update_many()
        .col_expr(projects::Column::ArchivedAt, Expr::value(value))
        .col_expr(projects::Column::UpdatedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedById, Expr::value(actor_id))
        .filter(projects::Column::Id.eq(project_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `ProjectArchiveUnarchiveEndpoint.post` (`base.py:435-442`): stamp
/// `archived_at` (+ `updated_at`/`updated_by`). The favorites
/// soft-delete stays at the caller.
pub async fn archive(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: Uuid,
) -> Result<(), sqlx::Error> {
    stamp_archived_at(db, project_id, Some(now), now, actor_id).await
}

/// `ProjectArchiveUnarchiveEndpoint.delete` (`base.py:444-446`):
/// clear `archived_at` (+ `updated_at`/`updated_by`).
pub async fn unarchive(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: Uuid,
) -> Result<(), sqlx::Error> {
    stamp_archived_at(db, project_id, None, now, actor_id).await
}

/// `WorkspaceFileAssetEndpoint.entity_asset_save`
/// (`v2.py:247-284`): the cover link plus the full
/// `project.save()` stamps (`auto_now` `updated_at`, crum
/// `updated_by`). The previous-asset delete stays at the caller.
/// Writes `cover_image = ""` (the ported empty-string write).
pub async fn stamp_cover_image(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    asset_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> Result<(), sqlx::Error> {
    projects::Entity::update_many()
        .col_expr(projects::Column::CoverImage, Expr::value(""))
        .col_expr(projects::Column::CoverImageAssetId, Expr::value(asset_id))
        .col_expr(projects::Column::UpdatedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedById, Expr::value(*actor_id))
        .filter(projects::Column::Id.eq(project_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// `WorkspaceFileAssetEndpoint.entity_asset_delete`
/// (`v2.py:286-312`): clear the cover link FK plus the full
/// `project.save()` stamps.
pub async fn clear_cover_image_asset(
    db: &impl ConnectionTrait,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> Result<(), sqlx::Error> {
    projects::Entity::update_many()
        .col_expr(
            projects::Column::CoverImageAssetId,
            Expr::value(None::<Uuid>),
        )
        .col_expr(projects::Column::UpdatedAt, Expr::value(now))
        .col_expr(projects::Column::UpdatedById, Expr::value(*actor_id))
        .filter(projects::Column::Id.eq(project_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

/// The two `project.save(update_fields=[...])` single-column stamps
/// the repo bindings emit: `repo_url` (git connect,
/// `git/services.py:342-349`, and GitHub bind, `github.py:1058-1061`)
/// and `base_branch` (git connect only, when the remote has a
/// default branch and the project has none).
pub enum ProjectTextField {
    RepoUrl,
    BaseBranch,
}

/// Stamp one `repo_url` / `base_branch` value (no `updated_at`: the
/// `update_fields` save skips `auto_now`). The caller's
/// conditional-append loop keeps its shape, one statement per arm.
pub async fn stamp_text_field(
    db: &impl ConnectionTrait,
    field: ProjectTextField,
    value: &str,
    project_id: Uuid,
) -> Result<(), sqlx::Error> {
    let column = match field {
        ProjectTextField::RepoUrl => projects::Column::RepoUrl,
        ProjectTextField::BaseBranch => projects::Column::BaseBranch,
    };
    projects::Entity::update_many()
        .col_expr(column, Expr::value(value))
        .filter(projects::Column::Id.eq(project_id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(into_sqlx)
}

// ---------------------------------------------------------------------------
// seed + demo project inserts (PIDASHCONV-1632)
// ---------------------------------------------------------------------------

/// One workspace-seed project row: `Project(...) :101-111` plus the
/// `save` in `bgtasks/workspace_seed_task.py` — identifier already
/// stripped/upper-cased and timezone already copied from the workspace
/// by the caller, first-project `is_default` already probed. Moved
/// verbatim from the seed store; `updated_by`/`deleted_at` stay NULL.
pub struct SeedProject<'a> {
    pub workspace_id: Uuid,
    pub name: &'a str,
    pub identifier: &'a str,
    pub timezone: &'a str,
    pub is_default: bool,
    pub description: &'a str,
    pub network: i16,
    pub cover_image: Option<&'a str>,
    pub logo_props: &'a serde_json::Value,
    pub default_agent_executor: &'a str,
    pub created_by: Uuid,
    pub now: DateTime<Utc>,
}

/// The seed row: the model defaults (`cycle_view`/`module_view`/
/// `issue_views_view`/`page_view` on, `intake_view` off, zero
/// archive/close windows, the agent knobs, `members_can_edit_states`),
/// every nullable `NULL`.
fn seed_project_row(id: Uuid, p: &SeedProject<'_>) -> projects::ActiveModel {
    projects::ActiveModel {
        id: Set(id),
        created_at: Set(p.now.into()),
        updated_at: Set(p.now.into()),
        created_by_id: Set(Some(p.created_by)),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        name: Set(p.name.to_owned()),
        description: Set(p.description.to_owned()),
        description_text: Set(None),
        description_html: Set(None),
        network: Set(p.network),
        workspace_id: Set(p.workspace_id),
        identifier: Set(p.identifier.to_owned()),
        default_assignee_id: Set(None),
        project_lead_id: Set(None),
        emoji: Set(None),
        icon_prop: Set(None),
        module_view: Set(true),
        cycle_view: Set(true),
        issue_views_view: Set(true),
        page_view: Set(true),
        intake_view: Set(false),
        is_time_tracking_enabled: Set(false),
        is_issue_type_enabled: Set(false),
        guest_view_all_features: Set(false),
        cover_image: Set(p.cover_image.map(str::to_owned)),
        cover_image_asset_id: Set(None),
        estimate_id: Set(None),
        archive_in: Set(0),
        close_in: Set(0),
        logo_props: Set(p.logo_props.clone()),
        default_state_id: Set(None),
        archived_at: Set(None),
        is_default: Set(p.is_default),
        timezone: Set(p.timezone.to_owned()),
        external_source: Set(None),
        external_id: Set(None),
        repo_url: Set(String::new()),
        base_branch: Set("main".to_owned()),
        agent_default_interval_seconds: Set(10800),
        agent_default_max_ticks: Set(10),
        agent_review_default_interval_seconds: Set(10800),
        agent_test_default_interval_seconds: Set(10800),
        agent_ticking_enabled: Set(true),
        default_agent_executor: Set(p.default_agent_executor.to_owned()),
        members_can_edit_states: Set(true),
    }
}

/// The seed project `save`: the row plus the atomic demote-others
/// update (`project.py:292-298`), in one transaction like `Project.save`'s
/// `transaction.atomic` block. Returns the new row id.
pub async fn create_seed_project(
    pool: &sqlx::PgPool,
    project: &SeedProject<'_>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    {
        let conn = TxConn::new(&mut tx);
        projects::Entity::insert(seed_project_row(id, project))
            .exec_without_returning(&conn)
            .await
            .map_err(into_sqlx)?;
        if project.is_default {
            clear_other_defaults_stmt(project.workspace_id, id)
                .exec(&conn)
                .await
                .map_err(into_sqlx)?;
        }
    }
    tx.commit().await?;
    Ok(id)
}

/// One dummy-data project row: `create_project (:44-60)` in
/// `bgtasks/dummy_data_task.py` — the creator call passes only
/// project/member/role, `save()` derives the workspace, fills the
/// model defaults and nulls the audit user (no request user in a
/// task); the timezone inherits the workspace's and `is_default` is
/// the caller's first-project probe. Moved from the dummy store
/// (PIDASHCONV-1632). No demote-others leg: a first project demotes
/// zero rows and a later one inserts `is_default = false`, so the
/// update is a guaranteed no-op on this path.
pub struct DemoProject<'a> {
    pub workspace_id: Uuid,
    pub name: &'a str,
    pub identifier: &'a str,
    pub is_default: bool,
    pub timezone: &'a str,
    pub default_agent_executor: &'a str,
    pub now: DateTime<Utc>,
}

/// The dummy row: `network 2`, `intake_view` and `page_view` on, the
/// other views off, the agent knobs, `NULL` audit user; nullables
/// take their column defaults.
fn demo_project_row(id: Uuid, p: &DemoProject<'_>) -> projects::ActiveModel {
    projects::ActiveModel {
        id: Set(id),
        workspace_id: Set(p.workspace_id),
        name: Set(p.name.to_owned()),
        identifier: Set(p.identifier.to_owned()),
        created_by_id: Set(None),
        intake_view: Set(true),
        created_at: Set(p.now.into()),
        updated_at: Set(p.now.into()),
        description: Set(String::new()),
        network: Set(2),
        cycle_view: Set(false),
        module_view: Set(false),
        issue_views_view: Set(false),
        page_view: Set(true),
        archive_in: Set(0),
        close_in: Set(0),
        logo_props: Set(serde_json::json!({})),
        is_time_tracking_enabled: Set(false),
        is_issue_type_enabled: Set(false),
        is_default: Set(p.is_default),
        guest_view_all_features: Set(false),
        timezone: Set(p.timezone.to_owned()),
        members_can_edit_states: Set(true),
        repo_url: Set(String::new()),
        base_branch: Set("main".to_owned()),
        agent_default_interval_seconds: Set(10800),
        agent_default_max_ticks: Set(10),
        agent_review_default_interval_seconds: Set(10800),
        agent_test_default_interval_seconds: Set(10800),
        agent_ticking_enabled: Set(true),
        default_agent_executor: Set(p.default_agent_executor.to_owned()),
        ..Default::default()
    }
}

/// The dummy-data project insert. Generates the row id client-side
/// (as Django does) and returns it for the pod/member legs.
pub async fn insert_demo_project(
    db: &impl ConnectionTrait,
    project: &DemoProject<'_>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    projects::Entity::insert(demo_project_row(id, project))
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// members
// ---------------------------------------------------------------------------

/// OSS `project_members` writes (plus the `ProjectUserProperty` leg
/// that rides the member save). Callers sequence the single-row
/// writers: which legs run, and in which order, differs per path.
///
/// Every `project_members` / `project_user_properties` insert goes
/// through one by-column row builder per table ([`MemberRow`],
/// [`PropertyRow`]): the single-row, multi-row, caller-stamped and
/// DB-stamped (`now()`) shapes are the same statement with different
/// value expressions, so the column list is written once.
pub mod members {
    use super::*;
    use sea_orm::sea_query::extension::postgres::PgFunc;
    use sea_orm::sea_query::CaseStatement;

    /// One member row to insert. `comment` is always `NULL`,
    /// `updated_by`/`deleted_at` stay `NULL` and `sort_order` is the
    /// 65535 literal on every path; `is_active` is always `TRUE`.
    pub struct NewProjectMember<'a> {
        pub workspace_id: Uuid,
        /// Always `Some` from current callers: the signup arms bind the
        /// invite's `project_id` (PB-1008 retired by PIDASHCONV-1686).
        /// `None` still binds `NULL` and raises 23502 — covered only by
        /// the unit probe, no production caller passes it.
        pub project_id: Option<Uuid>,
        /// `None` on the invite path's unknown-email arm (a bare
        /// `NULL` into the non-null column, like the original bind).
        pub user_id: Option<Uuid>,
        pub role: i16,
        /// The acting user where the path has one (`None` on the
        /// signup replays and the ops rows).
        pub created_by: Option<Uuid>,
        /// Caller-supplied model defaults; each caller module keeps
        /// its own copy.
        pub view_props: &'a serde_json::Value,
        pub default_props: &'a serde_json::Value,
        pub preferences: &'a serde_json::Value,
        pub now: DateTime<Utc>,
        /// `ON CONFLICT DO NOTHING` (the signup per-row replays).
        pub ignore_conflicts: bool,
        /// DB-side `now()` stamps (the v1/app invite writes).
        pub db_now: bool,
    }

    /// Row stamps: the caller's instants, or the database clock
    /// (`now()`, the DB-stamped invite writes).
    enum Stamps {
        Bound {
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
        },
        DbNow,
    }

    impl Stamps {
        fn same(now: DateTime<Utc>) -> Self {
            Stamps::Bound {
                created_at: now,
                updated_at: now,
            }
        }

        fn created(&self) -> SimpleExpr {
            match self {
                Stamps::Bound { created_at, .. } => Expr::value(*created_at),
                Stamps::DbNow => Expr::current_timestamp().into(),
            }
        }

        fn updated(&self) -> SimpleExpr {
            match self {
                Stamps::Bound { updated_at, .. } => Expr::value(*updated_at),
                Stamps::DbNow => Expr::current_timestamp().into(),
            }
        }
    }

    /// One `project_members` row, by column. The always-`NULL`
    /// columns (`comment`, `updated_by_id`, `deleted_at`) are not
    /// named and take the column default.
    struct MemberRow<'a> {
        /// A bound client id, or `gen_random_uuid()` on the bulks.
        id: SimpleExpr,
        stamps: Stamps,
        created_by: Option<Uuid>,
        project_id: Option<Uuid>,
        workspace_id: Uuid,
        member_id: Option<Uuid>,
        /// The role as the caller binds it: `int2` (the model type),
        /// the `int8` ops arm, or a raw [`RoleBind`].
        role: SimpleExpr,
        view_props: &'a Value,
        default_props: &'a Value,
        preferences: &'a Value,
        sort_order: f64,
    }

    fn member_pairs(row: &MemberRow<'_>) -> Vec<(project_members::Column, SimpleExpr)> {
        use project_members::Column as C;
        vec![
            (C::Id, row.id.clone()),
            (C::CreatedAt, row.stamps.created()),
            (C::UpdatedAt, row.stamps.updated()),
            (C::CreatedById, Expr::value(row.created_by)),
            (C::ProjectId, Expr::value(row.project_id)),
            (C::WorkspaceId, Expr::value(row.workspace_id)),
            (C::MemberId, Expr::value(row.member_id)),
            (C::Role, row.role.clone()),
            (C::ViewProps, Expr::value(row.view_props.clone())),
            (C::DefaultProps, Expr::value(row.default_props.clone())),
            (C::Preferences, Expr::value(row.preferences.clone())),
            (C::SortOrder, Expr::value(row.sort_order)),
            (C::IsActive, Expr::value(true)),
        ]
    }

    /// A multi-row `INSERT` from by-column rows: the column list comes
    /// from the first row, every row's values follow in that order.
    fn insert_rows<C>(
        table: impl sea_orm::sea_query::IntoTableRef,
        rows: Vec<Vec<(C, SimpleExpr)>>,
    ) -> InsertStatement
    where
        C: sea_orm::sea_query::IntoIden + Copy,
    {
        let mut stmt = Query::insert();
        stmt.into_table(table);
        if let Some(first) = rows.first() {
            stmt.columns(first.iter().map(|(column, _)| *column));
        }
        for row in rows {
            stmt.values_panic(row.into_iter().map(|(_, value)| value));
        }
        stmt.to_owned()
    }

    fn member_insert_stmt(rows: &[MemberRow<'_>], ignore_conflicts: bool) -> InsertStatement {
        let mut stmt = insert_rows(
            project_members::Entity,
            rows.iter().map(member_pairs).collect(),
        );
        if ignore_conflicts {
            stmt.on_conflict(OnConflict::new().do_nothing().to_owned());
        }
        stmt
    }

    /// `ProjectMember.objects.create(...)` (and the per-row signup
    /// replays). Returns the new row id. The DB-stamped arm carries no
    /// conflict clause (no caller combines the two flags; the original
    /// texts did not either).
    pub async fn add(
        db: &impl ConnectionTrait,
        member: &NewProjectMember<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        let row = MemberRow {
            id: Expr::value(id),
            stamps: if member.db_now {
                Stamps::DbNow
            } else {
                Stamps::same(member.now)
            },
            created_by: member.created_by,
            project_id: member.project_id,
            workspace_id: member.workspace_id,
            member_id: member.user_id,
            role: Expr::value(member.role),
            view_props: member.view_props,
            default_props: member.default_props,
            preferences: member.preferences,
            sort_order: 65535.0,
        };
        let stmt = member_insert_stmt(&[row], member.ignore_conflicts && !member.db_now);
        execute(db, &stmt).await?;
        Ok(id)
    }

    /// One `ProjectUserProperty` row: the member-save side effect
    /// (`db/models/project.py:348-364`). `sort_order` is
    /// caller-resolved (`MIN - 10000`, else 65535).
    pub struct NewMemberProperty<'a> {
        pub workspace_id: Uuid,
        pub project_id: Uuid,
        pub user_id: Uuid,
        pub filters: &'a serde_json::Value,
        pub display_filters: &'a serde_json::Value,
        pub display_properties: &'a serde_json::Value,
        pub rich_filters: &'a serde_json::Value,
        pub preferences: &'a serde_json::Value,
        pub sort_order: f64,
        pub created_by: Option<Uuid>,
        pub now: DateTime<Utc>,
    }

    /// One `project_user_properties` row, by column (`updated_by_id`
    /// and `deleted_at` take the column default, `NULL`).
    struct PropertyRow<'a> {
        id: SimpleExpr,
        stamps: Stamps,
        created_by: Option<Uuid>,
        project_id: Uuid,
        workspace_id: Uuid,
        user_id: Uuid,
        filters: &'a Value,
        display_filters: &'a Value,
        display_properties: &'a Value,
        rich_filters: SimpleExpr,
        preferences: &'a Value,
        sort_order: f64,
    }

    fn property_pairs(row: &PropertyRow<'_>) -> Vec<(project_user_properties::Column, SimpleExpr)> {
        use project_user_properties::Column as C;
        vec![
            (C::Id, row.id.clone()),
            (C::CreatedAt, row.stamps.created()),
            (C::UpdatedAt, row.stamps.updated()),
            (C::CreatedById, Expr::value(row.created_by)),
            (C::ProjectId, Expr::value(row.project_id)),
            (C::WorkspaceId, Expr::value(row.workspace_id)),
            (C::UserId, Expr::value(row.user_id)),
            (C::Filters, Expr::value(row.filters.clone())),
            (C::DisplayFilters, Expr::value(row.display_filters.clone())),
            (
                C::DisplayProperties,
                Expr::value(row.display_properties.clone()),
            ),
            (C::RichFilters, row.rich_filters.clone()),
            (C::Preferences, Expr::value(row.preferences.clone())),
            (C::SortOrder, Expr::value(row.sort_order)),
        ]
    }

    fn property_insert_stmt(
        rows: &[PropertyRow<'_>],
        on_conflict: Option<OnConflict>,
    ) -> InsertStatement {
        let mut stmt = insert_rows(
            project_user_properties::Entity,
            rows.iter().map(property_pairs).collect(),
        );
        if let Some(on_conflict) = on_conflict {
            stmt.on_conflict(on_conflict);
        }
        stmt
    }

    fn property_row<'a>(
        id: SimpleExpr,
        stamps: Stamps,
        property: &NewMemberProperty<'a>,
        rich_filters: SimpleExpr,
    ) -> PropertyRow<'a> {
        PropertyRow {
            id,
            stamps,
            created_by: property.created_by,
            project_id: property.project_id,
            workspace_id: property.workspace_id,
            user_id: property.user_id,
            filters: property.filters,
            display_filters: property.display_filters,
            display_properties: property.display_properties,
            rich_filters,
            preferences: property.preferences,
            sort_order: property.sort_order,
        }
    }

    /// The property leg of the member save. Returns the new row id.
    pub async fn add_property(
        db: &impl ConnectionTrait,
        property: &NewMemberProperty<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        let row = property_row(
            Expr::value(id),
            Stamps::same(property.now),
            property,
            Expr::value(property.rich_filters.clone()),
        );
        execute(db, &property_insert_stmt(&[row], None)).await?;
        Ok(id)
    }

    /// One bulk row: the per-row columns of the multi-row member
    /// insert. Rows may span workspaces/projects (the invite bulks
    /// carry one row per project). Stamps are per-row: the app member
    /// bulk samples `now()` twice per row (`auto_now_add` /
    /// `auto_now` land microseconds apart), while the bound-now
    /// callers pass the same instant twice.
    pub struct BulkRow {
        pub workspace_id: Uuid,
        /// Always `Some` from current callers (PB-1008 retired by
        /// PIDASHCONV-1686; the signup bulks bind the invite's id).
        pub project_id: Option<Uuid>,
        pub user_id: Uuid,
        pub role: RoleBind,
        pub created_by: Option<Uuid>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    /// The multi-row member insert for [`bulk_add`]: `gen_random_uuid()`
    /// ids, per-row stamps (bound or `now()`), `props` for both
    /// `view_props` and `default_props`, `ON CONFLICT DO NOTHING`.
    fn bulk_add_stmt(
        rows: &[BulkRow],
        props: &Value,
        preferences: &Value,
        db_now: bool,
    ) -> InsertStatement {
        let rows: Vec<MemberRow<'_>> = rows
            .iter()
            .map(|row| MemberRow {
                id: PgFunc::gen_random_uuid().into(),
                stamps: if db_now {
                    Stamps::DbNow
                } else {
                    Stamps::Bound {
                        created_at: row.created_at,
                        updated_at: row.updated_at,
                    }
                },
                created_by: row.created_by,
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                member_id: Some(row.user_id),
                role: role_expr(&row.role),
                view_props: props,
                default_props: props,
                preferences,
                sort_order: 65535.0,
            })
            .collect();
        member_insert_stmt(&rows, true)
    }

    /// `ProjectMember.objects.bulk_create([...],
    /// ignore_conflicts=True)`: one multi-row statement. An empty
    /// list emits no statement, like `bulk_create([])`. `props`
    /// serves both `view_props` and `default_props` (every caller
    /// binds identical content for the two columns). `db_now`
    /// selects the DB-stamped variant (the invite bulks).
    pub async fn bulk_add(
        db: &impl ConnectionTrait,
        rows: &[BulkRow],
        props: &serde_json::Value,
        preferences: &serde_json::Value,
        db_now: bool,
    ) -> Result<(), sqlx::Error> {
        if rows.is_empty() {
            return Ok(());
        }
        execute(db, &bulk_add_stmt(rows, props, preferences, db_now)).await?;
        Ok(())
    }

    /// The `get_or_create(user, project)` conflict target: the live
    /// partial unique index `(user_id, project_id) WHERE deleted_at IS
    /// NULL`; a lost race is swallowed (the caller re-reads).
    fn property_get_or_create_conflict() -> OnConflict {
        OnConflict::columns([
            project_user_properties::Column::UserId,
            project_user_properties::Column::ProjectId,
        ])
        .target_and_where(Expr::col(project_user_properties::Column::DeletedAt).is_null())
        .do_nothing()
        .to_owned()
    }

    /// The `get_or_create(user, project)` property write: the
    /// partial-index conflict target swallows a lost race (the
    /// caller re-reads). Returns the attempted row id.
    pub async fn get_or_create_property(
        db: &impl ConnectionTrait,
        property: &NewMemberProperty<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        let row = property_row(
            Expr::value(id),
            Stamps::same(property.now),
            property,
            Expr::value(property.rich_filters.clone()),
        );
        execute(
            db,
            &property_insert_stmt(&[row], Some(property_get_or_create_conflict())),
        )
        .await?;
        Ok(id)
    }

    /// The DB-stamped property leg (the invite member write): stamps
    /// from `now()`, `rich_filters` the model default `{}` (as the
    /// original statement wrote, whatever the struct carries).
    /// Returns the new row id.
    pub async fn add_property_db_now(
        db: &impl ConnectionTrait,
        property: &NewMemberProperty<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        let row = property_row(
            Expr::value(id),
            Stamps::DbNow,
            property,
            Expr::value(serde_json::json!({})),
        );
        execute(db, &property_insert_stmt(&[row], None)).await?;
        Ok(id)
    }

    /// One bulk property row (the app member-bulk and invite
    /// property legs). Stamps are per-row, like the member rows.
    pub struct BulkProperty<'a> {
        pub workspace_id: Uuid,
        pub project_id: Uuid,
        pub user_id: Uuid,
        pub filters: &'a serde_json::Value,
        pub display_filters: &'a serde_json::Value,
        pub display_properties: &'a serde_json::Value,
        pub rich_filters: &'a serde_json::Value,
        pub preferences: &'a serde_json::Value,
        pub sort_order: f64,
        pub created_by: Option<Uuid>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    /// The multi-row property insert for [`bulk_add_properties`]:
    /// `gen_random_uuid()` ids, per-row stamps, `ON CONFLICT DO NOTHING`.
    fn bulk_add_properties_stmt(rows: &[BulkProperty<'_>], db_now: bool) -> InsertStatement {
        let rows: Vec<PropertyRow<'_>> = rows
            .iter()
            .map(|row| PropertyRow {
                id: PgFunc::gen_random_uuid().into(),
                stamps: if db_now {
                    Stamps::DbNow
                } else {
                    Stamps::Bound {
                        created_at: row.created_at,
                        updated_at: row.updated_at,
                    }
                },
                created_by: row.created_by,
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                user_id: row.user_id,
                filters: row.filters,
                display_filters: row.display_filters,
                display_properties: row.display_properties,
                rich_filters: Expr::value(row.rich_filters.clone()),
                preferences: row.preferences,
                sort_order: row.sort_order,
            })
            .collect();
        property_insert_stmt(&rows, Some(OnConflict::new().do_nothing().to_owned()))
    }

    /// The property leg of the member bulks: one multi-row
    /// statement. Empty emits nothing. `db_now` selects the
    /// DB-stamped variant (the invite bulks).
    pub async fn bulk_add_properties(
        db: &impl ConnectionTrait,
        rows: &[BulkProperty<'_>],
        db_now: bool,
    ) -> Result<(), sqlx::Error> {
        if rows.is_empty() {
            return Ok(());
        }
        execute(db, &bulk_add_properties_stmt(rows, db_now)).await?;
        Ok(())
    }

    /// The ops member create: `created_by` `NULL`, and the role bound
    /// as `int8` — in-range values assign to the `smallint` column,
    /// out-of-range ones raise the server's `smallint out of range`,
    /// exactly as the psycopg-bound values do. `None` binds `NULL`
    /// (the BUG-2 write, which Postgres rejects with 23502). The
    /// explicit `CAST(.. AS smallint)` is the same server conversion
    /// and keeps this text apart from the `int2`-bound [`add`] for the
    /// statement cache (PIDASHCONV-1509). Returns the new row id.
    #[allow(clippy::too_many_arguments)]
    pub async fn ops_add(
        db: &impl ConnectionTrait,
        workspace_id: Uuid,
        project_id: Uuid,
        user_id: Uuid,
        role: Option<i64>,
        props: &serde_json::Value,
        preferences: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        let row = MemberRow {
            id: Expr::value(id),
            stamps: Stamps::same(now),
            created_by: None,
            project_id: Some(project_id),
            workspace_id,
            member_id: Some(user_id),
            role: Expr::value(role).cast_as(Alias::new("smallint")),
            view_props: props,
            default_props: props,
            preferences,
            sort_order: 65535.0,
        };
        execute(db, &member_insert_stmt(&[row], false)).await?;
        Ok(id)
    }

    /// The ops role write: queryset `update()` touches ONLY the two
    /// columns — no `save()`, no `auto_now` bump. Role as `int8`
    /// (see [`ops_add`]).
    pub async fn ops_update_role(
        db: &impl ConnectionTrait,
        project_id: Uuid,
        user_id: Uuid,
        role: Option<i64>,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(true))
            .col_expr(project_members::Column::Role, Expr::value(role))
            .filter(project_members::Column::ProjectId.eq(project_id))
            .filter(project_members::Column::MemberId.eq(user_id))
            .filter(project_members::Column::DeletedAt.is_null())
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// One raw role value for the app member bulk: how the JSON body
    /// value binds into the `CAST(CASE ... AS smallint)` update.
    /// `SmallIntegerField.get_prep_value` runs `int(value)` in Python
    /// first: bools → 0/1, floats truncate, strings parse,
    /// composites and unparseable text are the 500 arms (typed TEXT
    /// has no cast to smallint).
    #[derive(Debug, Clone, PartialEq)]
    pub enum RoleBind {
        Null,
        Int(i64),
        Float(f64),
        Text(String),
    }

    /// The bound value for one [`RoleBind`]: a text `NULL`, `int8`,
    /// `float8` or `text`, as the original binds typed them.
    fn role_expr(role: &RoleBind) -> SimpleExpr {
        match role {
            RoleBind::Null => Expr::value(None::<String>),
            RoleBind::Int(int) => Expr::value(*int),
            RoleBind::Float(float) => Expr::value(*float),
            RoleBind::Text(text) => Expr::value(text.clone()),
        }
    }

    /// `bulk_update(["is_active", "role"])` as Django renders it: one
    /// `UPDATE` with `role = CAST(CASE .. END AS smallint)` and
    /// `is_active = CASE .. END` over `id IN (..)`.
    fn update_roles_bulk_stmt(updates: &[(Uuid, RoleBind)]) -> UpdateMany<project_members::Entity> {
        let mut role_case = CaseStatement::new();
        let mut active_case = CaseStatement::new();
        for (id, role) in updates {
            let is_row = Expr::col(project_members::Column::Id).eq(*id);
            role_case = role_case.case(is_row.clone(), role_expr(role));
            active_case = active_case.case(is_row, true);
        }
        project_members::Entity::update_many()
            .col_expr(
                project_members::Column::Role,
                SimpleExpr::Case(Box::new(role_case)).cast_as(Alias::new("smallint")),
            )
            .col_expr(
                project_members::Column::IsActive,
                SimpleExpr::Case(Box::new(active_case)),
            )
            .filter(project_members::Column::Id.is_in(updates.iter().map(|(id, _)| *id)))
    }

    /// `bulk_update(["is_active", "role"])`: one atomic statement
    /// with Django's `CAST(CASE ... AS smallint)` shape. Skipped
    /// entirely when there is nothing to reactivate.
    pub async fn update_roles_bulk(
        db: &impl ConnectionTrait,
        updates: &[(Uuid, RoleBind)],
    ) -> Result<(), sqlx::Error> {
        if updates.is_empty() {
            return Ok(());
        }
        update_roles_bulk_stmt(updates)
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// One validated partial write: `None` per field means "not
    /// present". The caller's validator resolves every value
    /// (including the `updated_by`-ignored rule below).
    #[derive(Default)]
    pub struct MemberPatch<'a> {
        pub deleted_at: Option<Option<DateTime<Utc>>>,
        pub comment: Option<Option<&'a str>>,
        pub role: Option<i64>,
        pub view_props: Option<&'a serde_json::Value>,
        pub default_props: Option<&'a serde_json::Value>,
        pub preferences: Option<&'a serde_json::Value>,
        pub sort_order: Option<f64>,
        pub is_active: Option<bool>,
        pub created_by: Option<Option<Uuid>>,
    }

    /// The PATCH `UPDATE` for the present fields: `updated_at` /
    /// `updated_by_id` first, then the present columns in fixed
    /// order. The `i64` role binds `as i16` (truncation, like the
    /// original).
    fn patch_member_stmt(
        pk: Uuid,
        patch: &MemberPatch<'_>,
        saved_at: DateTime<Utc>,
        user_id: Uuid,
    ) -> UpdateMany<project_members::Entity> {
        use project_members::Column as C;
        let mut update = project_members::Entity::update_many()
            .col_expr(C::UpdatedAt, Expr::value(saved_at))
            .col_expr(C::UpdatedById, Expr::value(user_id));
        if let Some(deleted_at) = patch.deleted_at {
            update = update.col_expr(C::DeletedAt, Expr::value(deleted_at));
        }
        if let Some(comment) = patch.comment {
            update = update.col_expr(C::Comment, Expr::value(comment));
        }
        if let Some(role) = patch.role {
            update = update.col_expr(C::Role, Expr::value(role as i16));
        }
        if let Some(view_props) = patch.view_props {
            update = update.col_expr(C::ViewProps, Expr::value(view_props.clone()));
        }
        if let Some(default_props) = patch.default_props {
            update = update.col_expr(C::DefaultProps, Expr::value(default_props.clone()));
        }
        if let Some(preferences) = patch.preferences {
            update = update.col_expr(C::Preferences, Expr::value(preferences.clone()));
        }
        if let Some(sort_order) = patch.sort_order {
            update = update.col_expr(C::SortOrder, Expr::value(sort_order));
        }
        if let Some(is_active) = patch.is_active {
            update = update.col_expr(C::IsActive, Expr::value(is_active));
        }
        if let Some(created_by) = patch.created_by {
            update = update.col_expr(C::CreatedById, Expr::value(created_by));
        }
        update.filter(C::Id.eq(pk))
    }

    /// The app member PATCH: `save()` stamps `updated_at`/`updated_by`
    /// (a validated `updated_by` is ignored — `save()` overwrites it
    /// with the request user) while a validated `created_by`
    /// persists.
    pub async fn patch_member(
        db: &impl ConnectionTrait,
        pk: Uuid,
        patch: &MemberPatch<'_>,
        saved_at: DateTime<Utc>,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        patch_member_stmt(pk, patch, saved_at, user_id)
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The v1 member PATCH: `member` may go null (renders `null`),
    /// `role` is non-nullable in the model (a `None` role 500s at the
    /// caller after the write — the write itself binds it). The role
    /// binds as `int4`, assigning into the `smallint` column;
    /// `updated_at` is the database clock.
    pub async fn update_member_identity(
        db: &impl ConnectionTrait,
        id: Uuid,
        member_id: Option<Uuid>,
        role: Option<i32>,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::MemberId, Expr::value(member_id))
            .col_expr(project_members::Column::Role, Expr::value(role))
            .col_expr(project_members::Column::UpdatedById, Expr::value(actor_id))
            .col_expr(
                project_members::Column::UpdatedAt,
                Expr::current_timestamp().into(),
            )
            .filter(project_members::Column::Id.eq(id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The app member-props reset: full JSON replacement plus the
    /// float `sort_order` (explicit null 400s at the caller, before
    /// the write).
    #[allow(clippy::too_many_arguments)]
    pub async fn reset_props(
        db: &impl ConnectionTrait,
        id: Uuid,
        view_props: &serde_json::Value,
        default_props: &serde_json::Value,
        preferences: &serde_json::Value,
        sort_order: f64,
        now: DateTime<Utc>,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(
                project_members::Column::ViewProps,
                Expr::value(view_props.clone()),
            )
            .col_expr(
                project_members::Column::DefaultProps,
                Expr::value(default_props.clone()),
            )
            .col_expr(
                project_members::Column::Preferences,
                Expr::value(preferences.clone()),
            )
            .col_expr(project_members::Column::SortOrder, Expr::value(sort_order))
            .col_expr(project_members::Column::UpdatedAt, Expr::value(now))
            .col_expr(project_members::Column::UpdatedById, Expr::value(actor_id))
            .filter(project_members::Column::Id.eq(id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The app member-preferences save: the caller merges the patch
    /// into the stored document; the merged value binds as jsonb.
    pub async fn save_preferences(
        db: &impl ConnectionTrait,
        id: Uuid,
        merged: &serde_json::Value,
        now: DateTime<Utc>,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(
                project_members::Column::Preferences,
                Expr::value(merged.clone()),
            )
            .col_expr(project_members::Column::UpdatedAt, Expr::value(now))
            .col_expr(project_members::Column::UpdatedById, Expr::value(user_id))
            .filter(project_members::Column::Id.eq(id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The app member destroy/leave: bound stamps, row-addressed.
    pub async fn deactivate_by_id(
        db: &impl ConnectionTrait,
        id: Uuid,
        now: DateTime<Utc>,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(false))
            .col_expr(project_members::Column::UpdatedAt, Expr::value(now))
            .col_expr(project_members::Column::UpdatedById, Expr::value(user_id))
            .filter(project_members::Column::Id.eq(id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The v1 member deactivate: DB-stamped `updated_at`, by id.
    pub async fn deactivate_actor_first(
        db: &impl ConnectionTrait,
        id: Uuid,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(false))
            .col_expr(project_members::Column::UpdatedById, Expr::value(actor_id))
            .col_expr(
                project_members::Column::UpdatedAt,
                Expr::current_timestamp().into(),
            )
            .filter(project_members::Column::Id.eq(id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The user-deactivate chunk size (`batch_size=100`).
    pub(crate) const BULK_DEACTIVATE_CHUNK: usize = 100;

    /// One bulk-deactivate chunk: pk-direct, no `updated_at`, no
    /// `deleted_at` filter (bug 5).
    fn deactivate_many_stmt(ids: &[Uuid]) -> UpdateMany<project_members::Entity> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(false))
            .filter(project_members::Column::Id.is_in(ids.iter().copied()))
    }

    /// The user-deactivate bulk: chunked at 100, an empty list issues
    /// no statement.
    pub async fn deactivate_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<(), sqlx::Error> {
        for chunk in ids.chunks(BULK_DEACTIVATE_CHUNK) {
            deactivate_many_stmt(chunk)
                .exec(db)
                .await
                .map_err(into_sqlx)?;
        }
        Ok(())
    }

    /// `project_members.workspace_id IN (SELECT id FROM workspaces WHERE
    /// slug = ..)`: the slug scope of the cascades and the invite
    /// reactivation.
    fn in_workspace_by_slug(slug: &str) -> SimpleExpr {
        Expr::col((
            project_members::Entity,
            project_members::Column::WorkspaceId,
        ))
        .in_subquery(workspace_id_by_slug(slug))
    }

    /// The R4 guest-demote cascade: `role = 5` over the target's
    /// project rows in the workspace. Ported bug B6: no `is_active`
    /// filter — inactive rows are rewritten too.
    pub async fn demote_cascade(
        db: &impl ConnectionTrait,
        slug: &str,
        target_user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::Role, Expr::value(5_i16))
            .filter(in_workspace_by_slug(slug))
            .filter(project_members::Column::MemberId.eq(target_user_id))
            .filter(project_members::Column::DeletedAt.is_null())
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The R5/R6 project deactivation: the queryset `update()` stamps
    /// `updated_at` only (no `updated_by` — unlike the member
    /// `.save()` beside it).
    pub async fn cascade_deactivate_ws(
        db: &impl ConnectionTrait,
        slug: &str,
        target_user_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(false))
            .col_expr(project_members::Column::UpdatedAt, Expr::value(now))
            .filter(in_workspace_by_slug(slug))
            .filter(project_members::Column::MemberId.eq(target_user_id))
            .filter(project_members::Column::IsActive.eq(true))
            .filter(project_members::Column::DeletedAt.is_null())
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The invite-join reactivation: `QuerySet.update` stamps no
    /// `updated_at`.
    pub async fn reactivate_slug(
        db: &impl ConnectionTrait,
        slug: &str,
        project_ids: &[Uuid],
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(true))
            .filter(in_workspace_by_slug(slug))
            .filter(project_members::Column::ProjectId.is_in(project_ids.iter().copied()))
            .filter(project_members::Column::MemberId.eq(user_id))
            .filter(project_members::Column::DeletedAt.is_null())
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The join-accept reactivation (`project_member.role =
    /// project_member.role`: the role re-saves unchanged — the
    /// statement touches only the activity columns). The stamp is
    /// `None` when the path has no actor (a bare `NULL`, like the
    /// original bind); `updated_at` is the database clock.
    pub async fn reactivate_by_id(
        db: &impl ConnectionTrait,
        id: Uuid,
        updated_by: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(true))
            .col_expr(
                project_members::Column::UpdatedAt,
                Expr::current_timestamp().into(),
            )
            .col_expr(
                project_members::Column::UpdatedById,
                Expr::value(updated_by),
            )
            .filter(project_members::Column::Id.eq(id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// The platform bulk deactivation (`pi_dash_cloud` `service.py:480-489`):
    /// `ProjectMember.objects.filter(workspace, is_active=True)[.filter(member)]
    /// .update(is_active=False)`. A queryset `update()`, so `SET` only — no
    /// `updated_at` stamp and no `updated_by` (unlike the id-addressed
    /// deactivations above, and unlike [`cascade_deactivate_ws`], which
    /// scopes by slug and stamps `updated_at`); the soft-delete scope
    /// comes from the default manager. Runs on the caller's connection
    /// (the overlay holds its own transaction).
    pub async fn deactivate_by_workspace(
        db: &impl ConnectionTrait,
        workspace_id: Uuid,
        member_id: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        let mut update = project_members::Entity::update_many()
            .col_expr(project_members::Column::IsActive, Expr::value(false))
            .filter(project_members::Column::WorkspaceId.eq(workspace_id))
            .filter(project_members::Column::DeletedAt.is_null())
            .filter(project_members::Column::IsActive.eq(true));
        if let Some(user) = member_id {
            update = update.filter(project_members::Column::MemberId.eq(user));
        }
        update.exec(db).await.map(|_| ()).map_err(into_sqlx)
    }

    /// The active-member project role sync (`pi_dash_cloud`
    /// `service.py:622-625`): `ProjectMember.objects.filter(workspace,
    /// member, is_active=True).update(role=role)`. A queryset
    /// `update()`, so `SET role` only — no `updated_at` stamp and no
    /// `is_active` flip (unlike [`ops_update_role`], which also
    /// activates and scopes by project). The role binds `int2`, as
    /// the overlay does. Runs on the caller's connection.
    pub async fn sync_active_role(
        db: &impl ConnectionTrait,
        workspace_id: Uuid,
        user_id: Uuid,
        role: i16,
    ) -> Result<(), sqlx::Error> {
        project_members::Entity::update_many()
            .col_expr(project_members::Column::Role, Expr::value(role))
            .filter(project_members::Column::WorkspaceId.eq(workspace_id))
            .filter(project_members::Column::DeletedAt.is_null())
            .filter(project_members::Column::MemberId.eq(user_id))
            .filter(project_members::Column::IsActive.eq(true))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// One workspace-seed project-member row: one `bulk_create` row
    /// (`:115-126` in `bgtasks/workspace_seed_task.py`).
    /// `bulk_create` skips `ProjectMember.save`, so no auto
    /// `ProjectUserProperty` row rides this insert. Moved verbatim
    /// from the seed store (PIDASHCONV-1632); the caller keeps its
    /// props copy (`get_default_props` without `display_properties`,
    /// `db/models/project.py:43`, plus `get_default_preferences`,
    /// `db/models/project.py:68`).
    pub struct SeedProjectMember<'a> {
        pub project_id: Uuid,
        pub workspace_id: Uuid,
        pub member_id: Uuid,
        pub role: i16,
        pub created_by: Uuid,
        pub view_props: &'a serde_json::Value,
        pub preferences: &'a serde_json::Value,
        pub now: DateTime<Utc>,
    }

    /// The seed member-row insert (`view_props` serves both props
    /// columns). Returns the new row id.
    pub async fn add_seed(
        db: &impl ConnectionTrait,
        member: &SeedProjectMember<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        let row = MemberRow {
            id: Expr::value(id),
            stamps: Stamps::same(member.now),
            created_by: Some(member.created_by),
            project_id: Some(member.project_id),
            workspace_id: member.workspace_id,
            member_id: Some(member.member_id),
            role: Expr::value(member.role),
            view_props: member.view_props,
            default_props: member.view_props,
            preferences: member.preferences,
            sort_order: 65535.0,
        };
        execute(db, &member_insert_stmt(&[row], false)).await?;
        Ok(id)
    }

    /// One dummy-data project-member row: the `create_project`
    /// creator row (`ProjectMember.objects.create`, NULL audit,
    /// `sort_order` 65535) and the `create_project_members` loop rows
    /// (`:63-79` in `bgtasks/dummy_data_task.py`: role 20, random
    /// sort, `bulk_create(..., ignore_conflicts=True)` replayed one
    /// row at a time). Moved from the dummy store (PIDASHCONV-1632).
    pub struct DemoProjectMember<'a> {
        pub project_id: Uuid,
        pub workspace_id: Uuid,
        pub member_id: Uuid,
        pub role: i16,
        pub sort_order: f64,
        pub view_props: &'a serde_json::Value,
        pub preferences: &'a serde_json::Value,
        pub now: DateTime<Utc>,
        /// Per-row `ON CONFLICT DO NOTHING` (the members loop only).
        pub ignore_conflicts: bool,
    }

    /// The dummy-data member-row insert (`view_props` serves both
    /// props columns). The id is generated client-side (as Django
    /// does); the row id is never read back.
    pub async fn add_demo(
        db: &impl ConnectionTrait,
        member: &DemoProjectMember<'_>,
    ) -> Result<(), sqlx::Error> {
        let row = MemberRow {
            id: Expr::value(Uuid::new_v4()),
            stamps: Stamps::same(member.now),
            created_by: None,
            project_id: Some(member.project_id),
            workspace_id: member.workspace_id,
            member_id: Some(member.member_id),
            role: Expr::value(member.role),
            view_props: member.view_props,
            default_props: member.view_props,
            preferences: member.preferences,
            sort_order: member.sort_order,
        };
        execute(db, &member_insert_stmt(&[row], member.ignore_conflicts)).await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) mod shapes {
        //! Statement renderers for the shape pins in the module tests.
        use super::*;

        pub(crate) fn add_insert(member: &NewProjectMember<'_>, id: Uuid) -> String {
            let row = MemberRow {
                id: Expr::value(id),
                stamps: if member.db_now {
                    Stamps::DbNow
                } else {
                    Stamps::same(member.now)
                },
                created_by: member.created_by,
                project_id: member.project_id,
                workspace_id: member.workspace_id,
                member_id: member.user_id,
                role: Expr::value(member.role),
                view_props: member.view_props,
                default_props: member.default_props,
                preferences: member.preferences,
                sort_order: 65535.0,
            };
            DbBackend::Postgres
                .build(&member_insert_stmt(
                    &[row],
                    member.ignore_conflicts && !member.db_now,
                ))
                .to_string()
        }

        pub(crate) fn ops_add_insert(id: Uuid, role: Option<i64>, props: &Value) -> String {
            let row = MemberRow {
                id: Expr::value(id),
                stamps: Stamps::same(DateTime::<Utc>::UNIX_EPOCH),
                created_by: None,
                project_id: Some(Uuid::nil()),
                workspace_id: Uuid::nil(),
                member_id: Some(Uuid::nil()),
                role: Expr::value(role).cast_as(Alias::new("smallint")),
                view_props: props,
                default_props: props,
                preferences: props,
                sort_order: 65535.0,
            };
            DbBackend::Postgres
                .build(&member_insert_stmt(&[row], false))
                .to_string()
        }

        pub(crate) fn bulk_add_insert(
            rows: &[BulkRow],
            props: &Value,
            preferences: &Value,
            db_now: bool,
        ) -> String {
            DbBackend::Postgres
                .build(&bulk_add_stmt(rows, props, preferences, db_now))
                .to_string()
        }

        pub(crate) fn bulk_properties_insert(rows: &[BulkProperty<'_>], db_now: bool) -> String {
            DbBackend::Postgres
                .build(&bulk_add_properties_stmt(rows, db_now))
                .to_string()
        }

        pub(crate) fn get_or_create_insert(property: &NewMemberProperty<'_>, id: Uuid) -> String {
            let row = property_row(
                Expr::value(id),
                Stamps::same(property.now),
                property,
                Expr::value(property.rich_filters.clone()),
            );
            DbBackend::Postgres
                .build(&property_insert_stmt(
                    &[row],
                    Some(property_get_or_create_conflict()),
                ))
                .to_string()
        }

        pub(crate) fn update_roles_bulk(updates: &[(Uuid, RoleBind)]) -> String {
            sea_orm::QueryTrait::build(&update_roles_bulk_stmt(updates), DbBackend::Postgres)
                .to_string()
        }

        pub(crate) fn patch_member(
            pk: Uuid,
            patch: &MemberPatch<'_>,
            saved_at: DateTime<Utc>,
            user_id: Uuid,
        ) -> String {
            sea_orm::QueryTrait::build(
                &patch_member_stmt(pk, patch, saved_at, user_id),
                DbBackend::Postgres,
            )
            .to_string()
        }

        pub(crate) fn deactivate_many(ids: &[Uuid]) -> String {
            sea_orm::QueryTrait::build(&deactivate_many_stmt(ids), DbBackend::Postgres).to_string()
        }

        pub(crate) fn in_workspace_by_slug(slug: &str) -> SimpleExpr {
            super::in_workspace_by_slug(slug)
        }
    }
}

// ---------------------------------------------------------------------------
// states
// ---------------------------------------------------------------------------

/// OSS `states` writes: the endpoint creates, the per-row default
/// inserts (both create flows loop these — `bulk_create` is one
/// statement in Django but the ports already loop single rows, kept
/// verbatim), and the intake triage inserts.
pub mod states {
    use super::*;
    use pidash_db::entity::states as tbl;

    /// One state row: every column without a DB default. The app
    /// endpoint passes `is_triage == false`, no externals and no
    /// `updated_by`; the v1 endpoint passes its validated values. Both
    /// store the one row shape.
    pub struct NewState<'a> {
        pub project_id: Uuid,
        pub workspace_id: Uuid,
        pub name: &'a str,
        pub description: &'a str,
        pub color: &'a str,
        /// The caller slugifies (`slugify_name`); defaults keep `\"\"`.
        pub slug: &'a str,
        pub sequence: f64,
        pub group: &'a str,
        pub is_triage: bool,
        pub default: bool,
        pub external_source: Option<&'a str>,
        pub external_id: Option<&'a str>,
        pub created_by: Uuid,
        pub updated_by: Option<Uuid>,
        pub now: DateTime<Utc>,
    }

    fn state_row(id: Uuid, s: &NewState<'_>) -> tbl::ActiveModel {
        tbl::ActiveModel {
            id: Set(id),
            created_at: Set(s.now.into()),
            updated_at: Set(s.now.into()),
            created_by_id: Set(Some(s.created_by)),
            updated_by_id: Set(s.updated_by),
            deleted_at: Set(None),
            project_id: Set(s.project_id),
            workspace_id: Set(s.workspace_id),
            name: Set(s.name.to_owned()),
            description: Set(s.description.to_owned()),
            color: Set(s.color.to_owned()),
            slug: Set(s.slug.to_owned()),
            sequence: Set(s.sequence),
            group: Set(s.group.to_owned()),
            is_triage: Set(s.is_triage),
            default: Set(s.default),
            external_source: Set(s.external_source.map(str::to_owned)),
            external_id: Set(s.external_id.map(str::to_owned)),
        }
    }

    /// `State.objects.create(...)` (both state endpoints). Returns
    /// the new row id; conflict arms stay at the callers (app dup
    /// 400 vs v1 holder-echo 409).
    pub async fn create(
        db: &impl ConnectionTrait,
        state: &NewState<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        tbl::Entity::insert(state_row(id, state))
            .exec_without_returning(db)
            .await
            .map_err(into_sqlx)?;
        Ok(id)
    }

    /// The default-flip side effect (`state.py:21-22`):
    /// `State.objects.filter(project_id=...).update(default=False)`.
    ///
    /// The queryset starts from `StateManager.get_queryset`, which is the
    /// soft-delete filter (`deleted_at IS NULL`, `db/mixins.py:49-51`) plus
    /// the triage exclusion (`db/models/state.py:82-84`) — so triage and
    /// soft-deleted siblings keep their `default`. Moved from
    /// `v1_projects::ser_workflow` (PIDASHCONV-1630).
    pub(crate) fn clear_project_defaults_stmt(project_id: Uuid) -> UpdateMany<tbl::Entity> {
        tbl::Entity::update_many()
            .col_expr(tbl::Column::Default, Expr::value(false))
            .filter(tbl::Column::ProjectId.eq(project_id))
            .filter(tbl::Column::DeletedAt.is_null())
            .filter(tbl::Column::Group.ne("triage"))
    }

    /// Clear `default` on every sibling state in the project
    /// (soft-deleted and triage rows keep theirs, via the
    /// default-manager scope in [`clear_project_defaults_stmt`]).
    /// Unscoped by workspace slug, unlike the `mark_as_default`
    /// clear leg ([`clear_defaults`]) — the v1 flip has no slug.
    pub async fn clear_project_defaults(
        db: &impl ConnectionTrait,
        project_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        clear_project_defaults_stmt(project_id)
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// One `DEFAULT_STATES` row: `description`/`slug` `\"\"`,
    /// `is_triage` false, nullables `NULL` (`bulk_create` calls no
    /// `save()`).
    pub struct NewDefaultState<'a> {
        pub project_id: Uuid,
        pub workspace_id: Uuid,
        pub name: &'a str,
        pub color: &'a str,
        pub sequence: f64,
        pub group: &'a str,
        pub default: bool,
        pub created_by: Uuid,
        pub now: DateTime<Utc>,
    }

    fn default_state_row(id: Uuid, s: &NewDefaultState<'_>) -> tbl::ActiveModel {
        tbl::ActiveModel {
            id: Set(id),
            created_at: Set(s.now.into()),
            updated_at: Set(s.now.into()),
            created_by_id: Set(Some(s.created_by)),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            project_id: Set(s.project_id),
            workspace_id: Set(s.workspace_id),
            name: Set(s.name.to_owned()),
            description: Set(String::new()),
            color: Set(s.color.to_owned()),
            slug: Set(String::new()),
            sequence: Set(s.sequence),
            group: Set(s.group.to_owned()),
            is_triage: Set(false),
            default: Set(s.default),
            external_source: Set(None),
            external_id: Set(None),
        }
    }

    /// One default-state row (both create flows loop this over
    /// `DEFAULT_STATES`).
    pub async fn add_default(
        db: &impl ConnectionTrait,
        state: &NewDefaultState<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        tbl::Entity::insert(default_state_row(id, state))
            .exec_without_returning(db)
            .await
            .map_err(into_sqlx)?;
        Ok(id)
    }

    /// The intake triage row: `name 'Triage'`, `description ''`,
    /// `slug 'triage'`, `color '#4E5355'`, `group 'triage'`,
    /// `default`/`is_triage` false, externals `NULL`.
    fn triage_row(
        id: Uuid,
        project_id: Uuid,
        workspace_id: Uuid,
        sequence: f64,
        created_by: Uuid,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> tbl::ActiveModel {
        tbl::ActiveModel {
            id: Set(id),
            created_at: Set(created_at.into()),
            updated_at: Set(updated_at.into()),
            created_by_id: Set(Some(created_by)),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            project_id: Set(project_id),
            workspace_id: Set(workspace_id),
            name: Set("Triage".to_owned()),
            description: Set(String::new()),
            color: Set("#4E5355".to_owned()),
            slug: Set("triage".to_owned()),
            sequence: Set(sequence),
            group: Set("triage".to_owned()),
            is_triage: Set(false),
            default: Set(false),
            external_source: Set(None),
            external_id: Set(None),
        }
    }

    /// The intake triage-state insert (`MAX(sequence) + 15000`, else
    /// `65000`, resolved by the caller). Stamps are per-leg: the v1
    /// path samples twice (microseconds apart), the others pass one
    /// instant twice. The space, app and v1 intake texts stored the
    /// same row; one statement serves all three. Returns the new row
    /// id; conflict arms stay at the callers.
    pub async fn add_triage(
        db: &impl ConnectionTrait,
        project_id: Uuid,
        workspace_id: Uuid,
        sequence: f64,
        created_by: Uuid,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        tbl::Entity::insert(triage_row(
            id,
            project_id,
            workspace_id,
            sequence,
            created_by,
            created_at,
            updated_at,
        ))
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
        Ok(id)
    }

    /// The v1 state PATCH save (`views/state.py`): supplied fields only
    /// (merged driver-side), the slug recomputed unconditionally
    /// (`db/models/state.py:132`), `updated_by` stamped (crum) plus
    /// `updated_at` (auto_now). Conflict arms stay at the driver
    /// (unique violation → 400).
    pub struct SaveState<'a> {
        pub id: Uuid,
        pub updated_at: DateTime<Utc>,
        pub updated_by: Option<Uuid>,
        pub name: &'a str,
        pub description: &'a str,
        pub color: &'a str,
        pub slug: &'a str,
        pub sequence: f64,
        pub group: &'a str,
        pub is_triage: bool,
        pub default: bool,
        pub external_source: Option<&'a str>,
        pub external_id: Option<&'a str>,
    }

    /// See [`SaveState`]: the state `UPDATE` only.
    pub async fn save(db: &impl ConnectionTrait, v: &SaveState<'_>) -> Result<(), sqlx::Error> {
        use tbl::Column as C;
        tbl::Entity::update_many()
            .col_expr(C::UpdatedAt, Expr::value(v.updated_at))
            .col_expr(C::UpdatedById, Expr::value(v.updated_by))
            .col_expr(C::Name, Expr::value(v.name))
            .col_expr(C::Description, Expr::value(v.description))
            .col_expr(C::Color, Expr::value(v.color))
            .col_expr(C::Slug, Expr::value(v.slug))
            .col_expr(C::Sequence, Expr::value(v.sequence))
            .col_expr(C::Group, Expr::value(v.group))
            .col_expr(C::IsTriage, Expr::value(v.is_triage))
            .col_expr(C::Default, Expr::value(v.default))
            .col_expr(C::ExternalSource, Expr::value(v.external_source))
            .col_expr(C::ExternalId, Expr::value(v.external_id))
            .filter(C::Id.eq(v.id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// One workspace-seed state row: `create_project_states` in
    /// `bgtasks/workspace_seed_task.py` (max-sequence probe through
    /// the triage-excluding `StateManager`, then the insert).
    /// `description` is `''`, `is_triage` false, `updated_by` NULL.
    /// Moved verbatim from the seed store (PIDASHCONV-1632).
    pub struct SeedState<'a> {
        pub workspace_id: Uuid,
        pub project_id: Uuid,
        pub created_by: Uuid,
        pub name: &'a str,
        pub color: &'a str,
        pub slug: &'a str,
        pub group: &'a str,
        pub sequence: f64,
        pub is_default: bool,
        pub now: DateTime<Utc>,
    }

    /// The seed state-row insert. Returns the new row id.
    pub async fn add_seed(
        db: &impl ConnectionTrait,
        state: &SeedState<'_>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        tbl::Entity::insert(tbl::ActiveModel {
            id: Set(id),
            created_at: Set(state.now.into()),
            updated_at: Set(state.now.into()),
            created_by_id: Set(Some(state.created_by)),
            updated_by_id: Set(None),
            deleted_at: Set(None),
            project_id: Set(state.project_id),
            workspace_id: Set(state.workspace_id),
            name: Set(state.name.to_owned()),
            description: Set(String::new()),
            color: Set(state.color.to_owned()),
            slug: Set(state.slug.to_owned()),
            sequence: Set(state.sequence),
            group: Set(state.group.to_owned()),
            is_triage: Set(false),
            default: Set(state.is_default),
            external_source: Set(None),
            external_id: Set(None),
        })
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
        Ok(id)
    }

    /// One dummy-data state row: `create_states (:82-123)` in
    /// `bgtasks/dummy_data_task.py` — the five-row literal, no
    /// conflict clause. Moved from the dummy store (PIDASHCONV-1632).
    pub struct DemoState<'a> {
        pub project_id: Uuid,
        pub workspace_id: Uuid,
        pub created_by: Uuid,
        pub name: &'a str,
        pub color: &'a str,
        pub sequence: f64,
        pub group: &'a str,
        pub default: bool,
        pub now: DateTime<Utc>,
    }

    /// The dummy-data state-row insert (`description`/`slug` `''`,
    /// `is_triage` false; nullables take the column default).
    pub async fn add_demo(
        db: &impl ConnectionTrait,
        state: &DemoState<'_>,
    ) -> Result<(), sqlx::Error> {
        tbl::Entity::insert(tbl::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(state.name.to_owned()),
            color: Set(state.color.to_owned()),
            project_id: Set(state.project_id),
            sequence: Set(state.sequence),
            workspace_id: Set(state.workspace_id),
            group: Set(state.group.to_owned()),
            default: Set(state.default),
            created_by_id: Set(Some(state.created_by)),
            created_at: Set(state.now.into()),
            updated_at: Set(state.now.into()),
            description: Set(String::new()),
            slug: Set(String::new()),
            is_triage: Set(false),
            ..Default::default()
        })
        .exec_without_returning(db)
        .await
        .map_err(into_sqlx)?;
        Ok(())
    }

    /// `SoftDeleteModel.delete()` → `save()`: `deleted_at` plus the
    /// `save()` stamps (`updated_at`, CRUM `updated_by`), by id.
    async fn tombstone(
        db: &impl ConnectionTrait,
        id: Uuid,
        deleted_at: DateTime<Utc>,
        updated_by: Uuid,
    ) -> Result<(), sqlx::Error> {
        tbl::Entity::update_many()
            .col_expr(tbl::Column::DeletedAt, Expr::value(deleted_at))
            .col_expr(tbl::Column::UpdatedAt, Expr::value(deleted_at))
            .col_expr(tbl::Column::UpdatedById, Expr::value(updated_by))
            .filter(tbl::Column::Id.eq(id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// `DELETE states/<state_id>/` (`views/state.py:225-249`,
    /// `SoftDeleteModel.delete()` → `save()`): `deleted_at` plus the
    /// `save()` stamps (`updated_at`, CRUM `updated_by`). The
    /// related-objects sweep stays driver-side.
    pub async fn soft_delete(
        db: &impl ConnectionTrait,
        id: Uuid,
        updated_by: Uuid,
        deleted_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        tombstone(db, id, deleted_at, updated_by).await
    }

    /// One state PATCH write: every setter column, fully resolved by
    /// the caller (input-or-current merge; the caller slugifies).
    /// `StateViewSet.partial_update` (`state/base.py:66-82`).
    pub struct StatePatch<'a> {
        pub name: &'a str,
        pub color: &'a str,
        pub group: &'a str,
        pub default: bool,
        pub description: &'a str,
        pub sequence: f64,
        pub slug: &'a str,
        pub updated_at: DateTime<Utc>,
        pub updated_by: Uuid,
    }

    /// Patch one state row. Errors propagate as `sqlx::Error` — the
    /// caller owns the 23505 duplicate-name arm.
    pub async fn update(
        db: &impl ConnectionTrait,
        state_id: Uuid,
        patch: &StatePatch<'_>,
    ) -> Result<(), sqlx::Error> {
        use tbl::Column as C;
        tbl::Entity::update_many()
            .col_expr(C::Name, Expr::value(patch.name))
            .col_expr(C::Color, Expr::value(patch.color))
            .col_expr(C::Group, Expr::value(patch.group))
            .col_expr(C::Default, Expr::value(patch.default))
            .col_expr(C::Description, Expr::value(patch.description))
            .col_expr(C::Sequence, Expr::value(patch.sequence))
            .col_expr(C::Slug, Expr::value(patch.slug))
            .col_expr(C::UpdatedAt, Expr::value(patch.updated_at))
            .col_expr(C::UpdatedById, Expr::value(patch.updated_by))
            .filter(C::Id.eq(state_id))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// `states.workspace_id IN (SELECT id FROM workspaces WHERE slug =
    /// ..)`: the `mark_as_default` legs' slug scope.
    fn in_workspace_by_slug(slug: &str) -> SimpleExpr {
        Expr::col((tbl::Entity, tbl::Column::WorkspaceId)).in_subquery(workspace_id_by_slug(slug))
    }

    /// `StateViewSet.mark_as_default` (`state/base.py:111-119`): the
    /// clear leg — plain `.update()`, no `updated_at`/`updated_by`
    /// touch (queryset updates bypass `save()`). Clears every live
    /// non-triage default in the project.
    pub async fn clear_defaults(
        db: &impl ConnectionTrait,
        slug: &str,
        project_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        tbl::Entity::update_many()
            .col_expr(tbl::Column::Default, Expr::value(false))
            .filter(in_workspace_by_slug(slug))
            .filter(tbl::Column::ProjectId.eq(project_id))
            .filter(tbl::Column::Default.eq(true))
            .filter(tbl::Column::DeletedAt.is_null())
            .filter(tbl::Column::Group.ne("triage"))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// `StateViewSet.mark_as_default` (`state/base.py:111-119`): the
    /// set leg — same scoping plus the `pk`. Marks one live
    /// non-triage state the default.
    pub async fn mark_default(
        db: &impl ConnectionTrait,
        slug: &str,
        project_id: Uuid,
        state_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        tbl::Entity::update_many()
            .col_expr(tbl::Column::Default, Expr::value(true))
            .filter(in_workspace_by_slug(slug))
            .filter(tbl::Column::ProjectId.eq(project_id))
            .filter(tbl::Column::Id.eq(state_id))
            .filter(tbl::Column::DeletedAt.is_null())
            .filter(tbl::Column::Group.ne("triage"))
            .exec(db)
            .await
            .map(|_| ())
            .map_err(into_sqlx)
    }

    /// `StateViewSet.destroy` (`state/base.py:121-144`):
    /// `SoftDeleteModel.delete(soft=True)` — tombstone plus the
    /// `save()` stamps. The veto probes and the
    /// `soft_delete_related_objects` publish stay at the caller.
    pub async fn soft_delete_app(
        db: &impl ConnectionTrait,
        state_id: Uuid,
        now: DateTime<Utc>,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        tombstone(db, state_id, now, actor_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::members::*;
    use super::states::{add_default, add_triage, NewDefaultState, NewState};
    use super::*;
    use pidash_db::pool::sea_orm_connection;
    use sea_orm::sea_query::PostgresQueryBuilder;
    use sea_orm::QueryTrait;

    /// The rendered statement (values inlined), for shape pins.
    fn sql<Q: QueryTrait>(stmt: &Q) -> String {
        stmt.build(DbBackend::Postgres).to_string()
    }

    fn t0() -> DateTime<Utc> {
        "2026-10-03T12:00:00+00:00".parse().expect("T0")
    }

    /// Both default clears flip `is_default` on live rows of the
    /// workspace; only the update-path one excludes the saved row. The
    /// create row names every column (the audit pair as `NULL`).
    #[test]
    fn project_create_statement_shapes() {
        let ws = Uuid::nil();
        let id = Uuid::new_v4();
        let create_sweep = sql(&clear_workspace_defaults_stmt(ws));
        assert!(create_sweep.starts_with("UPDATE \"projects\" SET \"is_default\" = FALSE WHERE"));
        assert!(create_sweep.contains("\"projects\".\"is_default\" = TRUE"));
        assert!(create_sweep.contains("\"projects\".\"deleted_at\" IS NULL"));
        assert!(!create_sweep.contains("<>"));
        let update_sweep = sql(&clear_other_defaults_stmt(ws, id));
        assert!(update_sweep.contains(&format!("\"projects\".\"id\" <> '{id}'")));
        assert!(update_sweep.contains("\"projects\".\"deleted_at\" IS NULL"));
        let logo = serde_json::json!({});
        let insert = sql(&projects::Entity::insert(project_row(
            id,
            &new_project(ws, Uuid::nil(), t0(), &logo),
        )));
        assert!(insert.starts_with("INSERT INTO \"projects\" ("));
        for column in [
            "\"created_by_id\"",
            "\"updated_by_id\"",
            "\"deleted_at\"",
            "\"network\"",
            "\"default_agent_executor\"",
            "\"members_can_edit_states\"",
            "\"agent_test_default_interval_seconds\"",
        ] {
            assert!(insert.contains(column), "{column} named in {insert}");
        }
        assert_eq!(
            insert.matches(", ").count(),
            2 * 45,
            "46 columns, 46 values"
        );
    }

    /// The v1 archive pair scopes by workspace and live rows; the app
    /// PATCH/PUT writes every setter column; the v1 PATCH stamps the
    /// audit pair first, then the driver's assignments, and refuses an
    /// unknown column.
    #[test]
    fn project_update_statement_shapes() {
        let (id, ws, actor) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let now = t0();
        let archive = sql(&archive_v1_stmt(Some(now), id, ws, actor, now));
        assert!(
            archive.contains("\"archived_at\" = '2026-10-03 12:00:00.000000 +00:00'"),
            "{archive}"
        );
        assert!(archive.contains(&format!("\"projects\".\"workspace_id\" = '{ws}'")));
        assert!(archive.contains("\"projects\".\"deleted_at\" IS NULL"));
        let unarchive = sql(&archive_v1_stmt(None, id, ws, actor, now));
        assert!(unarchive.contains("\"archived_at\" = NULL"));

        let sets = vec![
            ProjectPatchSet {
                column: "name".to_owned(),
                value: ProjectPatchBind::Str("N".to_owned()),
            },
            ProjectPatchSet {
                column: "project_lead_id".to_owned(),
                value: ProjectPatchBind::OptUuid(None),
            },
            ProjectPatchSet {
                column: "archive_in".to_owned(),
                value: ProjectPatchBind::Int(7),
            },
        ];
        let patch = sql(&project_patch_stmt(&sets, now, actor, id).expect("known columns"));
        assert_eq!(
            patch,
            format!(
                "UPDATE \"projects\" SET \"updated_at\" = '2026-10-03 12:00:00.000000 +00:00', \
                 \"updated_by_id\" = '{actor}', \"name\" = 'N', \"project_lead_id\" = NULL, \
                 \"archive_in\" = 7 WHERE \"projects\".\"id\" = '{id}' AND \
                 \"projects\".\"deleted_at\" IS NULL"
            )
        );
        let unknown = [ProjectPatchSet {
            column: "no_such_column".to_owned(),
            value: ProjectPatchBind::Bool(true),
        }];
        assert!(matches!(
            project_patch_stmt(&unknown, now, actor, id),
            Err(sqlx::Error::ColumnNotFound(name)) if name == "no_such_column"
        ));

        // The bind twin: only the changed columns plus `updated_at`;
        // unchanged is no statement.
        assert!(bind_project_update_stmt(id, None, None, now).is_none());
        let repo = sql(&bind_project_update_stmt(id, Some("u"), None, now).expect("repo arm"));
        assert!(repo.starts_with("UPDATE \"projects\" SET \"repo_url\" = 'u', \"updated_at\" = "));
        assert!(!repo.contains("base_branch"));
        let both = sql(&bind_project_update_stmt(id, Some("u"), Some("b"), now).expect("both"));
        assert!(both.contains("\"repo_url\" = 'u', \"base_branch\" = 'b', \"updated_at\" = "));

        let logo = serde_json::json!({});
        let upd = ProjectUpdate {
            name: "Renamed",
            description: "d",
            description_text: None,
            description_html: None,
            network: 2,
            identifier: "REN",
            default_assignee: None,
            project_lead: None,
            emoji: None,
            icon_prop: None,
            module_view: true,
            cycle_view: false,
            issue_views_view: false,
            page_view: true,
            intake_view: false,
            is_time_tracking_enabled: true,
            is_issue_type_enabled: false,
            guest_view_all_features: false,
            cover_image: None,
            cover_image_asset: None,
            estimate: None,
            archive_in: 7,
            close_in: 8,
            logo_props: &logo,
            default_state: None,
            archived_at: None,
            is_default: false,
            timezone: "UTC",
            external_source: None,
            external_id: None,
            repo_url: "r",
            base_branch: "trunk",
            agent_interval: 1,
            agent_max_ticks: 2,
            agent_review: 3,
            agent_test: 4,
            agent_ticking: false,
            default_executor: "x",
            members_can_edit_states: false,
            workspace_id: ws,
            deleted_at: None,
            updated_at: now,
            updated_by: &actor,
            created_by: Some(actor),
        };
        let update = sql(&project_update_stmt(id, &upd));
        assert_eq!(
            update.matches(" = ").count(),
            44 + 1,
            "44 setters plus the id filter"
        );
        assert!(update.contains(&format!("\"created_by_id\" = '{actor}'")));
        assert!(update.contains("\"deleted_at\" = NULL"));
        assert!(update.ends_with(&format!("WHERE \"projects\".\"id\" = '{id}'")));
    }

    /// The member insert names its columns once; the replay arm adds
    /// `ON CONFLICT DO NOTHING`, the DB-stamped arm uses the database
    /// clock (and never a conflict clause).
    #[test]
    fn member_insert_statement_shapes() {
        let view = props();
        let prefs = prefs();
        let member = NewProjectMember {
            workspace_id: Uuid::nil(),
            project_id: Some(Uuid::nil()),
            user_id: Some(Uuid::nil()),
            role: 20,
            created_by: None,
            view_props: &view,
            default_props: &view,
            preferences: &prefs,
            now: t0(),
            ignore_conflicts: false,
            db_now: false,
        };
        let id = Uuid::new_v4();
        let plain = shapes::add_insert(&member, id);
        assert!(plain.starts_with(
            "INSERT INTO \"project_members\" (\"id\", \"created_at\", \"updated_at\", \
             \"created_by_id\", \"project_id\", \"workspace_id\", \"member_id\", \"role\", \
             \"view_props\", \"default_props\", \"preferences\", \"sort_order\", \"is_active\") \
             VALUES ("
        ));
        assert!(plain.ends_with(
            r#", 20, E'{\"filters\":{}}', E'{\"filters\":{}}', E'{\"theme\":\"dark\"}', 65535, TRUE)"#
        ), "{plain}");
        assert!(!plain.contains("ON CONFLICT"));
        let ignored = shapes::add_insert(
            &NewProjectMember {
                ignore_conflicts: true,
                ..member_for_replay(&member)
            },
            id,
        );
        assert!(ignored.ends_with(") ON CONFLICT  DO NOTHING"));
        let db_now = shapes::add_insert(
            &NewProjectMember {
                db_now: true,
                ignore_conflicts: true,
                ..member_for_replay(&member)
            },
            id,
        );
        assert!(db_now.contains("CURRENT_TIMESTAMP, CURRENT_TIMESTAMP"));
        assert!(!db_now.contains("ON CONFLICT"));
    }

    /// The ops arm binds `int8` and casts explicitly, so its text can
    /// never share a prepared statement with the `int2` member insert
    /// (PIDASHCONV-1509).
    #[test]
    fn ops_add_text_differs_from_add() {
        let view = props();
        let id = Uuid::nil();
        let ops = shapes::ops_add_insert(id, Some(15), &view);
        assert!(ops.contains(", CAST(15 AS smallint), "), "{ops}");
        let null_role = shapes::ops_add_insert(id, None, &view);
        assert!(
            null_role.contains(", CAST(NULL AS smallint), "),
            "{null_role}"
        );
        let member = NewProjectMember {
            workspace_id: Uuid::nil(),
            project_id: Some(Uuid::nil()),
            user_id: Some(Uuid::nil()),
            role: 15,
            created_by: None,
            view_props: &view,
            default_props: &view,
            preferences: &view,
            now: DateTime::<Utc>::UNIX_EPOCH,
            ignore_conflicts: false,
            db_now: false,
        };
        let add = shapes::add_insert(&member, id);
        assert!(!add.contains("CAST("));
        assert_ne!(ops, add);
    }

    /// The bulks are one multi-row statement with `gen_random_uuid()`
    /// ids, per-row stamps (or the database clock) and `ON CONFLICT DO
    /// NOTHING`; the raw role arms bind as `NULL`, number, float or
    /// text.
    #[test]
    fn bulk_statement_shapes() {
        let view = props();
        let prefs = prefs();
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let rows = vec![
            BulkRow {
                workspace_id: Uuid::nil(),
                project_id: Some(Uuid::nil()),
                user_id: Uuid::nil(),
                role: RoleBind::Int(15),
                created_by: None,
                created_at: t0(),
                updated_at: later,
            },
            BulkRow {
                workspace_id: Uuid::nil(),
                project_id: Some(Uuid::nil()),
                user_id: Uuid::nil(),
                role: RoleBind::Text("20".to_owned()),
                created_by: None,
                created_at: t0(),
                updated_at: later,
            },
            BulkRow {
                workspace_id: Uuid::nil(),
                project_id: Some(Uuid::nil()),
                user_id: Uuid::nil(),
                role: RoleBind::Null,
                created_by: None,
                created_at: t0(),
                updated_at: later,
            },
            BulkRow {
                workspace_id: Uuid::nil(),
                project_id: Some(Uuid::nil()),
                user_id: Uuid::nil(),
                role: RoleBind::Float(2.5),
                created_by: None,
                created_at: t0(),
                updated_at: later,
            },
        ];
        let bound = shapes::bulk_add_insert(&rows, &view, &prefs, false);
        assert_eq!(bound.matches("(GEN_RANDOM_UUID(), ").count(), 4, "{bound}");
        assert!(bound
            .contains("'2026-10-03 12:00:00.000000 +00:00', '2026-10-04 12:00:00.000000 +00:00'"));
        assert!(bound.ends_with(" ON CONFLICT  DO NOTHING"));
        for role in [", 15, ", ", '20', ", ", NULL, ", ", 2.5, "] {
            assert!(bound.contains(role), "{role} in {bound}");
        }
        let db_now = shapes::bulk_add_insert(&rows[..1], &view, &prefs, true);
        assert_eq!(db_now.matches("CURRENT_TIMESTAMP").count(), 2);
        assert!(!db_now.contains("2026-10-03"));

        let filters = serde_json::json!({});
        let property_rows = vec![
            BulkProperty {
                workspace_id: Uuid::nil(),
                project_id: Uuid::nil(),
                user_id: Uuid::nil(),
                filters: &filters,
                display_filters: &filters,
                display_properties: &filters,
                rich_filters: &filters,
                preferences: &prefs,
                sort_order: 55535.0,
                created_by: None,
                created_at: t0(),
                updated_at: later,
            },
            BulkProperty {
                workspace_id: Uuid::nil(),
                project_id: Uuid::nil(),
                user_id: Uuid::nil(),
                filters: &filters,
                display_filters: &filters,
                display_properties: &filters,
                rich_filters: &filters,
                preferences: &prefs,
                sort_order: 65535.0,
                created_by: None,
                created_at: t0(),
                updated_at: later,
            },
        ];
        let properties = shapes::bulk_properties_insert(&property_rows, false);
        assert!(properties.starts_with(
            "INSERT INTO \"project_user_properties\" (\"id\", \"created_at\", \"updated_at\", \
             \"created_by_id\", \"project_id\", \"workspace_id\", \"user_id\", \"filters\", \
             \"display_filters\", \"display_properties\", \"rich_filters\", \"preferences\", \
             \"sort_order\") VALUES (GEN_RANDOM_UUID(), "
        ));
        assert_eq!(properties.matches("(GEN_RANDOM_UUID(), ").count(), 2);
        assert!(properties.ends_with(" ON CONFLICT  DO NOTHING"));
        let properties_db_now = shapes::bulk_properties_insert(&property_rows, true);
        assert_eq!(properties_db_now.matches("CURRENT_TIMESTAMP").count(), 4);

        // `get_or_create`: the live partial unique index is the target.
        let property = NewMemberProperty {
            workspace_id: Uuid::nil(),
            project_id: Uuid::nil(),
            user_id: Uuid::nil(),
            filters: &filters,
            display_filters: &filters,
            display_properties: &filters,
            rich_filters: &filters,
            preferences: &prefs,
            sort_order: 65535.0,
            created_by: None,
            now: t0(),
        };
        let get_or_create = shapes::get_or_create_insert(&property, Uuid::nil());
        assert!(get_or_create.ends_with(
            ") ON CONFLICT (\"user_id\", \"project_id\") WHERE \"deleted_at\" IS NULL DO NOTHING"
        ), "{get_or_create}");
    }

    /// `bulk_update` keeps Django's `CAST(CASE .. AS smallint)` shape;
    /// the PATCH sets the stamps first and the present fields in fixed
    /// order; the chunked deactivation is pk-direct with no stamp; the
    /// slug scopes resolve the workspace in a subquery.
    #[test]
    fn member_update_statement_shapes() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let roles =
            shapes::update_roles_bulk(&[(first, RoleBind::Int(20)), (second, RoleBind::Null)]);
        assert!(roles.starts_with(&format!(
            "UPDATE \"project_members\" SET \"role\" = CAST((CASE WHEN (\"id\" = '{first}') THEN 20 \
             WHEN (\"id\" = '{second}') THEN NULL END) AS smallint), \"is_active\" = (CASE WHEN \
             (\"id\" = '{first}') THEN TRUE WHEN (\"id\" = '{second}') THEN TRUE END) WHERE \
             \"project_members\".\"id\" IN ('{first}', '{second}')"
        )), "{roles}");

        let full = MemberPatch {
            deleted_at: Some(None),
            comment: Some(Some("hi")),
            role: Some(20),
            view_props: Some(&serde_json::json!({})),
            default_props: Some(&serde_json::json!({})),
            preferences: Some(&serde_json::json!({})),
            sort_order: Some(1.0),
            is_active: Some(true),
            created_by: Some(None),
        };
        let patch = shapes::patch_member(first, &full, t0(), second);
        assert_eq!(
            patch,
            format!(
                "UPDATE \"project_members\" SET \"updated_at\" = '2026-10-03 12:00:00.000000 +00:00', \
                 \"updated_by_id\" = '{second}', \"deleted_at\" = NULL, \"comment\" = 'hi', \
                 \"role\" = 20, \"view_props\" = '{{}}', \"default_props\" = '{{}}', \
                 \"preferences\" = '{{}}', \"sort_order\" = 1, \"is_active\" = TRUE, \
                 \"created_by_id\" = NULL WHERE \"project_members\".\"id\" = '{first}'"
            )
        );
        let stamps_only = shapes::patch_member(first, &MemberPatch::default(), t0(), second);
        assert!(stamps_only.contains(&format!(
            "\"updated_by_id\" = '{second}' WHERE \"project_members\".\"id\" = '{first}'"
        )));

        let bulk = shapes::deactivate_many(&[first, second]);
        assert_eq!(
            bulk,
            format!(
                "UPDATE \"project_members\" SET \"is_active\" = FALSE WHERE \
                 \"project_members\".\"id\" IN ('{first}', '{second}')"
            )
        );
        assert_eq!(BULK_DEACTIVATE_CHUNK, 100);

        let scope = Query::select()
            .expr(shapes::in_workspace_by_slug("acme"))
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            scope,
            "SELECT \"project_members\".\"workspace_id\" IN (SELECT \"id\" FROM \"workspaces\" \
             WHERE \"workspaces\".\"slug\" = 'acme')"
        );
    }

    /// The v1 default flip clears live non-triage siblings only and
    /// touches no stamp.
    #[test]
    fn state_default_flip_statement_shape() {
        let project = Uuid::new_v4();
        let flip = sql(&states::clear_project_defaults_stmt(project));
        assert_eq!(
            flip,
            format!(
                "UPDATE \"states\" SET \"default\" = FALSE WHERE \"states\".\"project_id\" = \
                 '{project}' AND \"states\".\"deleted_at\" IS NULL AND \"states\".\"group\" <> 'triage'"
            )
        );
    }

    #[tokio::test]
    async fn live_bind_project_update_executes_all_arms() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PROJECTS_DDL)
            .execute(&pool)
            .await
            .expect("temp projects");
        let id = Uuid::new_v4();
        let ws = Uuid::new_v4();
        let t0: DateTime<Utc> = "2026-10-28T12:00:00+00:00".parse().expect("T0");
        let t1: DateTime<Utc> = "2026-10-29T12:00:00+00:00".parse().expect("T1");
        sqlx::query(
            "INSERT INTO projects (id, created_at, updated_at, name, description, network, \
             workspace_id, identifier, module_view, cycle_view, issue_views_view, page_view, \
             intake_view, is_time_tracking_enabled, is_issue_type_enabled, \
             guest_view_all_features, archive_in, close_in, logo_props, is_default, timezone, \
             repo_url, base_branch, agent_default_interval_seconds, agent_default_max_ticks, \
             agent_review_default_interval_seconds, agent_test_default_interval_seconds, \
             agent_ticking_enabled, default_agent_executor, members_can_edit_states) \
             VALUES ($1, $2, $2, 'Bind', '', 2, $3, 'BIND', false, false, false, true, false, \
             false, false, false, 30, 30, '{}', false, 'UTC', $4, $5, 60, 10, 60, 60, true, \
             'local_runner', true)",
        )
        .bind(id)
        .bind(t0)
        .bind(ws)
        .bind("https://example.test/old.git")
        .bind("develop")
        .execute(&pool)
        .await
        .expect("seed project");
        save_bind_project_update(&db, id, Some("https://example.test/new.git"), None, t1)
            .await
            .expect("repo arm");
        assert_eq!(
            read_bind_row(&pool, id).await,
            (
                "https://example.test/new.git".to_owned(),
                "develop".to_owned(),
                t1
            )
        );
        save_bind_project_update(&db, id, None, Some("main"), t1)
            .await
            .expect("branch arm");
        assert_eq!(
            read_bind_row(&pool, id).await,
            (
                "https://example.test/new.git".to_owned(),
                "main".to_owned(),
                t1
            )
        );
        save_bind_project_update(
            &db,
            id,
            Some("https://example.test/both.git"),
            Some("trunk"),
            t1,
        )
        .await
        .expect("both arm");
        assert_eq!(
            read_bind_row(&pool, id).await,
            (
                "https://example.test/both.git".to_owned(),
                "trunk".to_owned(),
                t1
            )
        );
        save_bind_project_update(&db, id, None, None, t0)
            .await
            .expect("no-op arm");
        assert_eq!(
            read_bind_row(&pool, id).await,
            (
                "https://example.test/both.git".to_owned(),
                "trunk".to_owned(),
                t1
            )
        );
    }

    async fn read_bind_row(pool: &sqlx::PgPool, id: Uuid) -> (String, String, DateTime<Utc>) {
        sqlx::query_as::<_, (String, String, DateTime<Utc>)>(
            "SELECT repo_url, base_branch, updated_at FROM projects WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read back")
    }

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

    const PROJECTS_DDL: &str = "CREATE TEMPORARY TABLE projects (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL,
        created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ,
        name TEXT NOT NULL, description TEXT NOT NULL, description_text JSONB,
        description_html JSONB, network INTEGER NOT NULL, workspace_id UUID NOT NULL,
        identifier TEXT NOT NULL, default_assignee_id UUID, project_lead_id UUID,
        emoji TEXT, icon_prop JSONB, module_view BOOLEAN NOT NULL,
        cycle_view BOOLEAN NOT NULL, issue_views_view BOOLEAN NOT NULL,
        page_view BOOLEAN NOT NULL, intake_view BOOLEAN NOT NULL,
        is_time_tracking_enabled BOOLEAN NOT NULL, is_issue_type_enabled BOOLEAN NOT NULL,
        guest_view_all_features BOOLEAN NOT NULL, cover_image TEXT,
        cover_image_asset_id UUID, estimate_id UUID, archive_in INTEGER NOT NULL,
        close_in INTEGER NOT NULL, logo_props JSONB NOT NULL, default_state_id UUID,
        archived_at TIMESTAMPTZ, is_default BOOLEAN NOT NULL, timezone TEXT NOT NULL,
        external_source TEXT, external_id TEXT, repo_url TEXT NOT NULL,
        base_branch TEXT NOT NULL, agent_default_interval_seconds INTEGER NOT NULL,
        agent_default_max_ticks INTEGER NOT NULL,
        agent_review_default_interval_seconds INTEGER NOT NULL,
        agent_test_default_interval_seconds INTEGER NOT NULL,
        agent_ticking_enabled BOOLEAN NOT NULL, default_agent_executor TEXT NOT NULL,
        members_can_edit_states BOOLEAN NOT NULL)";

    const WORKSPACES_DDL: &str = "CREATE TEMPORARY TABLE workspaces (
        id UUID PRIMARY KEY, slug TEXT NOT NULL)";

    const PMEMBERS_DDL: &str = "CREATE TEMPORARY TABLE project_members (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL,
        created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ,
        project_id UUID NOT NULL, workspace_id UUID NOT NULL, member_id UUID,
        comment TEXT, role SMALLINT NOT NULL, view_props JSONB NOT NULL,
        default_props JSONB NOT NULL, preferences JSONB NOT NULL,
        sort_order DOUBLE PRECISION NOT NULL, is_active BOOLEAN NOT NULL)";

    const PROPS_DDL: &str = "CREATE TEMPORARY TABLE project_user_properties (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL,
        created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ,
        project_id UUID NOT NULL, workspace_id UUID NOT NULL, user_id UUID NOT NULL,
        filters JSONB NOT NULL, display_filters JSONB NOT NULL,
        display_properties JSONB NOT NULL, rich_filters JSONB NOT NULL,
        preferences JSONB NOT NULL, sort_order DOUBLE PRECISION NOT NULL)";

    const STATES_DDL: &str = "CREATE TEMPORARY TABLE states (
        id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL,
        created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ,
        project_id UUID NOT NULL, workspace_id UUID NOT NULL, name TEXT NOT NULL,
        description TEXT NOT NULL, color TEXT NOT NULL, slug TEXT NOT NULL,
        sequence DOUBLE PRECISION NOT NULL, \"group\" TEXT NOT NULL,
        is_triage BOOLEAN NOT NULL, \"default\" BOOLEAN NOT NULL,
        external_source TEXT, external_id TEXT)";

    fn props() -> serde_json::Value {
        serde_json::json!({"filters": {}})
    }

    fn prefs() -> serde_json::Value {
        serde_json::json!({"theme": "dark"})
    }

    fn new_project<'a>(
        ws: Uuid,
        actor: Uuid,
        now: DateTime<Utc>,
        logo: &'a serde_json::Value,
    ) -> NewProject<'a> {
        NewProject {
            name: "Website",
            description: "",
            description_text: None,
            description_html: None,
            network: 2,
            workspace_id: ws,
            identifier: "WEB",
            default_assignee: None,
            project_lead: None,
            emoji: None,
            icon_prop: None,
            module_view: false,
            cycle_view: false,
            issue_views_view: false,
            page_view: true,
            intake_view: false,
            is_time_tracking_enabled: false,
            is_issue_type_enabled: false,
            guest_view_all_features: false,
            cover_image: None,
            cover_image_asset: None,
            estimate: None,
            archive_in: 0,
            close_in: 0,
            logo_props: logo,
            default_state: None,
            archived_at: None,
            is_default: true,
            clear_others: true,
            timezone: "UTC",
            external_source: None,
            external_id: None,
            repo_url: "",
            base_branch: "main",
            agent_interval: 10800,
            agent_max_ticks: 10,
            agent_review: 10800,
            agent_test: 10800,
            agent_ticking: true,
            default_executor: "local_runner",
            members_can_edit_states: true,
            created_by: actor,
            created_at: now,
            updated_at: now,
        }
    }

    /// `create` inserts the row and clears the previous live default
    /// in the same transaction; a second insert with the same inputs
    /// stores the v1 flow's literal values.
    #[tokio::test]
    async fn live_create_clears_default_and_inserts() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        sqlx::query(PROJECTS_DDL)
            .execute(&pool)
            .await
            .expect("temp projects");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let logo = serde_json::json!({});
        let first = create(&pool, &new_project(ws, actor, now, &logo))
            .await
            .expect("first create");
        let second = create(&pool, &new_project(ws, actor, now, &logo))
            .await
            .expect("second create");
        for (id, expected) in [(first, false), (second, true)] {
            let flag: bool = sqlx::query_scalar("SELECT is_default FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
            assert_eq!(flag, expected);
        }
        let row: (String, i32, bool, String) = sqlx::query_as(
            "SELECT identifier, network, agent_ticking_enabled, default_agent_executor \
             FROM projects WHERE id = $1",
        )
        .bind(second)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(row, ("WEB".to_owned(), 2, true, "local_runner".to_owned()));
        let v1_id = create(&pool, &new_project(ws, actor, now, &logo))
            .await
            .expect("v1 create");
        let v1_row: (Option<serde_json::Value>, Option<Uuid>, i32, bool) = sqlx::query_as(
            "SELECT description_text, cover_image_asset_id, agent_default_interval_seconds, \
             agent_ticking_enabled FROM projects WHERE id = $1",
        )
        .bind(v1_id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(v1_row, (None, None, 10800, true));
    }

    /// The update-path twins round-trip through the temp table with
    /// the same binds the handlers pass: self-excluding
    /// default-clear, full update, archive/unarchive, soft delete,
    /// cover link/unlink, and both text-field stamps.
    #[tokio::test]
    async fn live_update_twins_round_trip() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PROJECTS_DDL)
            .execute(&pool)
            .await
            .expect("temp projects");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let logo = serde_json::json!({});
        let id = create(&pool, &new_project(ws, actor, now, &logo))
            .await
            .expect("create");
        clear_other_defaults(&db, ws, id)
            .await
            .expect("clear others");
        let flag: bool = sqlx::query_scalar("SELECT is_default FROM projects WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert!(flag, "self excluded from the clear");
        let upd = ProjectUpdate {
            name: "Renamed",
            description: "d",
            description_text: None,
            description_html: None,
            network: 2,
            identifier: "REN",
            default_assignee: None,
            project_lead: None,
            emoji: Some(":)"),
            icon_prop: None,
            module_view: true,
            cycle_view: false,
            issue_views_view: false,
            page_view: true,
            intake_view: false,
            is_time_tracking_enabled: true,
            is_issue_type_enabled: false,
            guest_view_all_features: false,
            cover_image: None,
            cover_image_asset: None,
            estimate: None,
            archive_in: 7,
            close_in: 8,
            logo_props: &logo,
            default_state: None,
            archived_at: None,
            is_default: false,
            timezone: "UTC",
            external_source: None,
            external_id: None,
            repo_url: "https://example.test/r",
            base_branch: "trunk",
            agent_interval: 1,
            agent_max_ticks: 2,
            agent_review: 3,
            agent_test: 4,
            agent_ticking: false,
            default_executor: "x",
            members_can_edit_states: false,
            workspace_id: ws,
            deleted_at: None,
            updated_at: now,
            updated_by: &actor,
            created_by: Some(actor),
        };
        apply_update(&db, id, &upd).await.expect("update");
        let row: (String, String, bool, i32, String) = sqlx::query_as(
            "SELECT name, identifier, module_view, archive_in, repo_url FROM projects WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(row.0, "Renamed");
        assert_eq!(row.1, "REN");
        assert!(row.2);
        assert_eq!(row.3, 7);
        assert_eq!(row.4, "https://example.test/r");
        archive(&db, id, now, actor).await.expect("archive");
        let stamped: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT archived_at FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stamped, Some(now));
        unarchive(&db, id, now, actor).await.expect("unarchive");
        let cleared: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT archived_at FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(cleared, None);
        let asset = Uuid::new_v4();
        stamp_cover_image(&db, id, asset, now, &actor)
            .await
            .expect("cover link");
        let cover: (Option<String>, Option<Uuid>) =
            sqlx::query_as("SELECT cover_image, cover_image_asset_id FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(cover, (Some(String::new()), Some(asset)));
        clear_cover_image_asset(&db, id, now, &actor)
            .await
            .expect("cover unlink");
        let unlinked: Option<Uuid> =
            sqlx::query_scalar("SELECT cover_image_asset_id FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(unlinked, None);
        let last = Uuid::new_v4();
        save_project_cover(&db, id, last, now, actor)
            .await
            .expect("v2 cover save");
        let saved: (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "SELECT cover_image_asset_id, updated_by_id FROM projects WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(saved, (Some(last), Some(actor)));
        stamp_text_field(
            &db,
            ProjectTextField::RepoUrl,
            "https://example.test/r2",
            id,
        )
        .await
        .expect("repo stamp");
        stamp_text_field(&db, ProjectTextField::BaseBranch, "dev", id)
            .await
            .expect("branch stamp");
        let urls: (String, String) =
            sqlx::query_as("SELECT repo_url, base_branch FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(
            urls,
            ("https://example.test/r2".to_owned(), "dev".to_owned())
        );
        soft_delete(&db, id, now, actor).await.expect("delete");
        let tomb: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT deleted_at FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(tomb, Some(now));
    }

    /// `add` inserts the full row; the ignore arm replays the signup
    /// per-row `ON CONFLICT DO NOTHING`; `db_now` stamps from the
    /// database clock; a `None` project still raises 23502 (writer
    /// probe — PB-1008 retired by PIDASHCONV-1686, no production
    /// caller passes `None`).
    #[tokio::test]
    async fn live_member_add_variants() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let user = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let view = props();
        let prefs = prefs();
        let member = NewProjectMember {
            workspace_id: ws,
            project_id: Some(project),
            user_id: Some(user),
            role: 20,
            created_by: Some(actor),
            view_props: &view,
            default_props: &view,
            preferences: &prefs,
            now,
            ignore_conflicts: false,
            db_now: false,
        };
        let id = add(&db, &member).await.expect("add");
        let row: (i16, bool, DateTime<Utc>) =
            sqlx::query_as("SELECT role, is_active, created_at FROM project_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(row, (20, true, now));
        let ignored = NewProjectMember {
            project_id: member.project_id,
            user_id: member.user_id,
            ignore_conflicts: true,
            ..member_for_replay(&member)
        };
        add(&db, &ignored).await.expect("ignore replay");
        let db_now = NewProjectMember {
            user_id: Some(Uuid::new_v4()),
            db_now: true,
            ..member_for_replay(&member)
        };
        add(&db, &db_now).await.expect("db_now add");
        let stamped: DateTime<Utc> =
            sqlx::query_scalar("SELECT created_at FROM project_members WHERE member_id = $1")
                .bind(db_now.user_id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(stamped > now, "db clock stamps, got {stamped}");
        // Retired-bug probe (PB-1008, PIDASHCONV-1686): `None` still
        // binds NULL into the NOT NULL column and raises 23502.
        let null_project = NewProjectMember {
            project_id: None,
            user_id: Some(Uuid::new_v4()),
            ..member_for_replay(&member)
        };
        let err = add(&db, &null_project)
            .await
            .expect_err("null project 23502");
        assert_eq!(
            err.as_database_error().and_then(|db| db.code()).as_deref(),
            Some("23502")
        );
    }

    /// Field-level copy for the replay probes (`NewProjectMember`
    /// borrows; the replays reuse every value but the flags).
    fn member_for_replay<'a>(member: &NewProjectMember<'a>) -> NewProjectMember<'a> {
        NewProjectMember {
            workspace_id: member.workspace_id,
            project_id: member.project_id,
            user_id: member.user_id,
            role: member.role,
            created_by: member.created_by,
            view_props: member.view_props,
            default_props: member.default_props,
            preferences: member.preferences,
            now: member.now,
            ignore_conflicts: false,
            db_now: false,
        }
    }

    /// `add_property` (both stamp styles) and the two multi-row
    /// writers insert their rows; empty inputs emit no statement.
    #[tokio::test]
    async fn live_properties_and_bulks() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        sqlx::query(PROPS_DDL)
            .execute(&pool)
            .await
            .expect("temp properties");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let user = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let filters = serde_json::json!({});
        let prefs = prefs();
        let property = NewMemberProperty {
            workspace_id: ws,
            project_id: project,
            user_id: user,
            filters: &filters,
            display_filters: &filters,
            display_properties: &filters,
            rich_filters: &filters,
            preferences: &prefs,
            sort_order: 65535.0,
            created_by: Some(actor),
            now,
        };
        add_property(&db, &property).await.expect("property");
        add_property_db_now(&db, &property)
            .await
            .expect("db_now property");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_user_properties")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 2);
        let view = props();
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        let rows = vec![
            BulkRow {
                workspace_id: ws,
                project_id: Some(project),
                user_id: Uuid::new_v4(),
                role: RoleBind::Int(15),
                created_by: Some(actor),
                created_at: now,
                updated_at: later,
            },
            BulkRow {
                workspace_id: ws,
                project_id: Some(project),
                user_id: Uuid::new_v4(),
                role: RoleBind::Int(5),
                created_by: Some(actor),
                created_at: now,
                updated_at: later,
            },
        ];
        bulk_add(&db, &rows, &view, &prefs, false)
            .await
            .expect("bulk");
        bulk_add(&db, &rows, &view, &prefs, true)
            .await
            .expect("db_now bulk");
        bulk_add(&db, &[], &view, &prefs, false)
            .await
            .expect("empty bulk is a no-op");
        let stored: Vec<(i16, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
            "SELECT role, created_at, updated_at FROM project_members ORDER BY role",
        )
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(stored.len(), 4);
        assert!(stored.iter().any(|row| row.1 == now && row.2 == later));
        let prop_rows = vec![BulkProperty {
            workspace_id: ws,
            project_id: project,
            user_id: Uuid::new_v4(),
            filters: &filters,
            display_filters: &filters,
            display_properties: &filters,
            rich_filters: &filters,
            preferences: &prefs,
            sort_order: 55535.0,
            created_by: Some(actor),
            created_at: now,
            updated_at: later,
        }];
        bulk_add_properties(&db, &prop_rows, false)
            .await
            .expect("bulk properties");
        bulk_add_properties(&db, &prop_rows, true)
            .await
            .expect("db_now bulk properties");
        bulk_add_properties(&db, &[], false)
            .await
            .expect("empty properties bulk is a no-op");
        let sorts: Vec<f64> = sqlx::query_scalar(
            "SELECT sort_order FROM project_user_properties ORDER BY sort_order",
        )
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(sorts, vec![55535.0, 55535.0, 65535.0, 65535.0]);
    }

    /// The `get_or_create` arm inserts on a miss and swallows a
    /// lost race through the partial-index conflict target.
    #[tokio::test]
    async fn live_get_or_create_property() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PROPS_DDL)
            .execute(&pool)
            .await
            .expect("temp properties");
        sqlx::query(
            "CREATE UNIQUE INDEX props_user_project_uidx ON project_user_properties \
             (user_id, project_id) WHERE deleted_at IS NULL",
        )
        .execute(&pool)
        .await
        .expect("partial index");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let filters = serde_json::json!({});
        let prefs = prefs();
        let property = NewMemberProperty {
            workspace_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            filters: &filters,
            display_filters: &filters,
            display_properties: &filters,
            rich_filters: &filters,
            preferences: &prefs,
            sort_order: 65535.0,
            created_by: Some(Uuid::new_v4()),
            now,
        };
        get_or_create_property(&db, &property)
            .await
            .expect("get_or_create inserts");
        get_or_create_property(&db, &property)
            .await
            .expect("get_or_create swallows the race");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_user_properties")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 1);
    }

    /// The ops arms bind the `int8` role (in-range assigns, `None`
    /// rejects) and the queryset update touches only its columns.
    #[tokio::test]
    async fn live_ops_add_and_update_role() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let user = Uuid::new_v4();
        let view = props();
        let prefs = prefs();
        ops_add(&db, ws, project, user, Some(15), &view, &prefs, now)
            .await
            .expect("ops add");
        ops_update_role(&db, project, user, Some(20))
            .await
            .expect("ops role");
        let row: (i16, bool, DateTime<Utc>) =
            sqlx::query_as("SELECT role, is_active, updated_at FROM project_members")
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(row, (20, true, now));
        let err = ops_add(&db, ws, project, Uuid::new_v4(), None, &view, &prefs, now)
            .await
            .expect_err("null role 23502");
        assert_eq!(
            err.as_database_error().and_then(|db| db.code()).as_deref(),
            Some("23502")
        );
    }

    /// The `CAST(CASE ...)` bulk reactivates with per-row roles;
    /// the `NULL` arm is the 23502 the caller's 500 arm keys on; the
    /// PATCH writes the present fields only and always stamps.
    #[tokio::test]
    async fn live_update_roles_bulk_and_patch() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let view = props();
        let prefs = prefs();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        for user in [first, second] {
            let member = NewProjectMember {
                workspace_id: ws,
                project_id: Some(project),
                user_id: Some(user),
                role: 5,
                created_by: None,
                view_props: &view,
                default_props: &view,
                preferences: &prefs,
                now,
                ignore_conflicts: false,
                db_now: false,
            };
            add(&db, &member).await.expect("seed");
        }
        let ids: Vec<(Uuid, Uuid)> =
            sqlx::query_as("SELECT id, member_id FROM project_members ORDER BY member_id")
                .fetch_all(&pool)
                .await
                .expect("ids");
        let (first_id, second_id) = (ids[0].0, ids[1].0);
        sqlx::query("UPDATE project_members SET is_active = FALSE")
            .execute(&pool)
            .await
            .expect("deactivate seeds");
        update_roles_bulk(&db, &[(first_id, RoleBind::Int(20))])
            .await
            .expect("bulk roles");
        update_roles_bulk(&db, &[]).await.expect("empty is a no-op");
        let rows: Vec<(i16, bool)> = sqlx::query_as(
            "SELECT role, is_active FROM project_members WHERE id = ANY($1) ORDER BY member_id",
        )
        .bind(vec![first_id, second_id])
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(rows, vec![(20, true), (5, false)]);
        // The NULL arm binds NULL into the NOT NULL column: 23502 for
        // the caller's 500 arm.
        let err = update_roles_bulk(&db, &[(second_id, RoleBind::Null)])
            .await
            .expect_err("null role 23502");
        assert_eq!(
            err.as_database_error().and_then(|db| db.code()).as_deref(),
            Some("23502")
        );
        let patch = MemberPatch {
            role: Some(15),
            comment: Some(Some("hello")),
            ..MemberPatch::default()
        };
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        patch_member(&db, first_id, &patch, later, first)
            .await
            .expect("patch");
        let patched: (i16, Option<String>, DateTime<Utc>, Uuid) = sqlx::query_as(
            "SELECT role, comment, updated_at, updated_by_id FROM project_members WHERE id = $1",
        )
        .bind(first_id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(patched, (15, Some("hello".to_owned()), later, first));
        patch_member(&db, first_id, &MemberPatch::default(), later, first)
            .await
            .expect("empty patch stamps only");
    }

    /// `deactivate_by_workspace` flips only live active rows in the
    /// workspace (both scopes), leaves `updated_at` untouched, and
    /// skips inactive, soft-deleted and foreign-workspace rows.
    #[tokio::test]
    async fn live_deactivate_by_workspace_scopes() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws_a = Uuid::new_v4();
        let ws_b = Uuid::new_v4();
        let project = Uuid::new_v4();
        let view = props();
        let prefs = prefs();
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
            let member = NewProjectMember {
                workspace_id: ws,
                project_id: Some(project),
                user_id: Some(user),
                role: 5,
                created_by: None,
                view_props: &view,
                default_props: &view,
                preferences: &prefs,
                now,
                ignore_conflicts: false,
                db_now: false,
            };
            add(&db, &member).await.expect("seed");
        }
        // Already-inactive and soft-deleted (but active-flagged) rows.
        sqlx::query("UPDATE project_members SET is_active = FALSE WHERE member_id = $1")
            .bind(user_3)
            .execute(&pool)
            .await
            .expect("inactive seed");
        sqlx::query("UPDATE project_members SET deleted_at = $1 WHERE member_id = $2")
            .bind(now)
            .bind(user_4)
            .execute(&pool)
            .await
            .expect("deleted seed");
        async fn read_back(pool: &sqlx::PgPool, ws: Uuid, user: Uuid) -> (bool, DateTime<Utc>) {
            sqlx::query_as(
                "SELECT is_active, updated_at FROM project_members \
                WHERE workspace_id = $1 AND member_id = $2",
            )
            .bind(ws)
            .bind(user)
            .fetch_one(pool)
            .await
            .expect("read back")
        }
        // Member scope: only that user's live row flips.
        deactivate_by_workspace(&db, ws_a, Some(user_1))
            .await
            .expect("member scope");
        assert_eq!(read_back(&pool, ws_a, user_1).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_2).await, (true, now));
        assert_eq!(read_back(&pool, ws_b, user_1).await, (true, now));
        // Workspace scope: every live active row in the workspace
        // flips; inactive, deleted and foreign rows are untouched.
        sqlx::query("UPDATE project_members SET is_active = TRUE WHERE member_id = $1")
            .bind(user_1)
            .execute(&pool)
            .await
            .expect("restore");
        deactivate_by_workspace(&db, ws_a, None)
            .await
            .expect("workspace scope");
        assert_eq!(read_back(&pool, ws_a, user_1).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_2).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_3).await, (false, now));
        assert_eq!(read_back(&pool, ws_a, user_4).await, (true, now));
        assert_eq!(read_back(&pool, ws_b, user_1).await, (true, now));
    }

    /// `sync_active_role` rewrites the role on the member's live
    /// active project rows only — same workspace and member, but an
    /// inactive, soft-deleted or foreign-workspace row keeps its
    /// role, and `updated_at` is untouched everywhere.
    #[tokio::test]
    async fn live_sync_active_role_sets_role_only() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws_a = Uuid::new_v4();
        let ws_b = Uuid::new_v4();
        let project = Uuid::new_v4();
        let other_project = Uuid::new_v4();
        let view = props();
        let prefs = prefs();
        let user = Uuid::new_v4();
        let other_user = Uuid::new_v4();
        // Two live rows for the member (role sync spans projects),
        // one inactive, one deleted, one foreign-workspace, one
        // foreign-member.
        for (ws, member_project, member, role) in [
            (ws_a, project, user, 5),
            (ws_a, other_project, user, 5),
            (ws_a, project, other_user, 5),
            (ws_b, project, user, 5),
        ] {
            let row = NewProjectMember {
                workspace_id: ws,
                project_id: Some(member_project),
                user_id: Some(member),
                role,
                created_by: None,
                view_props: &view,
                default_props: &view,
                preferences: &prefs,
                now,
                ignore_conflicts: false,
                db_now: false,
            };
            add(&db, &row).await.expect("seed");
        }
        let inactive_id = add(
            &db,
            &NewProjectMember {
                workspace_id: ws_a,
                project_id: Some(project),
                user_id: Some(user),
                role: 5,
                created_by: None,
                view_props: &view,
                default_props: &view,
                preferences: &prefs,
                now,
                ignore_conflicts: false,
                db_now: false,
            },
        )
        .await
        .expect("inactive seed");
        sqlx::query("UPDATE project_members SET is_active = FALSE WHERE id = $1")
            .bind(inactive_id)
            .execute(&pool)
            .await
            .expect("deactivate one");
        let deleted_id = add(
            &db,
            &NewProjectMember {
                workspace_id: ws_a,
                project_id: Some(project),
                user_id: Some(user),
                role: 5,
                created_by: None,
                view_props: &view,
                default_props: &view,
                preferences: &prefs,
                now,
                ignore_conflicts: false,
                db_now: false,
            },
        )
        .await
        .expect("deleted seed");
        sqlx::query("UPDATE project_members SET deleted_at = $1 WHERE id = $2")
            .bind(now)
            .bind(deleted_id)
            .execute(&pool)
            .await
            .expect("soft-delete one");
        sync_active_role(&db, ws_a, user, 20)
            .await
            .expect("sync role");
        let seeded: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_members WHERE workspace_id = $1 AND member_id = $2",
        )
        .bind(ws_a)
        .bind(user)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(seeded, 4);
        // Both live rows (both projects) take the new role with no
        // stamp move; the inactive and deleted rows are selected
        // below by id.
        let live: Vec<(i16, DateTime<Utc>)> = sqlx::query_as(
            "SELECT role, updated_at FROM project_members \
            WHERE workspace_id = $1 AND member_id = $2 AND is_active AND deleted_at IS NULL",
        )
        .bind(ws_a)
        .bind(user)
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(live.len(), 2);
        assert!(live
            .iter()
            .all(|(role, stamped)| *role == 20 && *stamped == now));
        let skipped: Vec<(i16, DateTime<Utc>)> = sqlx::query_as(
            "SELECT role, updated_at FROM project_members WHERE id = ANY($1) ORDER BY id",
        )
        .bind(vec![inactive_id, deleted_id])
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(skipped, vec![(5, now), (5, now)]);
        let others: Vec<(i16, DateTime<Utc>)> = sqlx::query_as(
            "SELECT role, updated_at FROM project_members \
            WHERE (workspace_id = $1 AND member_id = $2) OR workspace_id = $3 ORDER BY member_id",
        )
        .bind(ws_a)
        .bind(other_user)
        .bind(ws_b)
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(others, vec![(5, now), (5, now)]);
    }

    /// The v1 identity write, the props reset and the preferences
    /// save land their columns.
    #[tokio::test]
    async fn live_identity_props_prefs() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let user = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let view = props();
        let prefs = prefs();
        let member = NewProjectMember {
            workspace_id: ws,
            project_id: Some(project),
            user_id: Some(user),
            role: 5,
            created_by: Some(actor),
            view_props: &view,
            default_props: &view,
            preferences: &prefs,
            now,
            ignore_conflicts: false,
            db_now: false,
        };
        let id = add(&db, &member).await.expect("seed");
        let other = Uuid::new_v4();
        update_member_identity(&db, id, Some(other), Some(20), actor)
            .await
            .expect("identity");
        let identity: (Uuid, i16) =
            sqlx::query_as("SELECT member_id, role FROM project_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(identity, (other, 20));
        let reset = serde_json::json!({"reset": true});
        reset_props(&db, id, &reset, &reset, &reset, 12.5, now, actor)
            .await
            .expect("reset");
        let stored: (serde_json::Value, f64) =
            sqlx::query_as("SELECT view_props, sort_order FROM project_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stored, (reset, 12.5));
        let merged = serde_json::json!({"theme": "light"});
        save_preferences(&db, id, &merged, now, actor)
            .await
            .expect("prefs");
        let saved: serde_json::Value =
            sqlx::query_scalar("SELECT preferences FROM project_members WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(saved, merged);
    }

    /// Every (de)activation writer: row-addressed, actor-first,
    /// chunked bulk, the demote cascade, the workspace cascade, and
    /// the two invite reactivations.
    #[tokio::test]
    async fn live_deactivations() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(WORKSPACES_DDL)
            .execute(&pool)
            .await
            .expect("temp workspaces");
        sqlx::query(PMEMBERS_DDL)
            .execute(&pool)
            .await
            .expect("temp members");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, 'acme')")
            .bind(ws)
            .execute(&pool)
            .await
            .expect("workspace");
        let project = Uuid::new_v4();
        let other_project = Uuid::new_v4();
        let user = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let view = props();
        let prefs = prefs();
        let mut ids = Vec::new();
        for project_id in [project, other_project] {
            let member = NewProjectMember {
                workspace_id: ws,
                project_id: Some(project_id),
                user_id: Some(user),
                role: 20,
                created_by: Some(actor),
                view_props: &view,
                default_props: &view,
                preferences: &prefs,
                now,
                ignore_conflicts: false,
                db_now: false,
            };
            ids.push(add(&db, &member).await.expect("seed"));
        }
        deactivate_by_id(&db, ids[0], now, actor)
            .await
            .expect("deactivate");
        let active: bool =
            sqlx::query_scalar("SELECT is_active FROM project_members WHERE id = $1")
                .bind(ids[0])
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(!active);
        reactivate_by_id(&db, ids[0], Some(actor))
            .await
            .expect("reactivate");
        deactivate_actor_first(&db, ids[1], actor)
            .await
            .expect("v1 deactivate");
        reactivate_slug(&db, "acme", &[project, other_project], user)
            .await
            .expect("slug reactivate");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_members WHERE is_active")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 2);
        // Ported bug B6: no `is_active` filter — the inactive row is
        // rewritten too.
        deactivate_by_id(&db, ids[0], now, actor)
            .await
            .expect("deactivate one");
        demote_cascade(&db, "acme", user).await.expect("demote");
        let demoted: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_members WHERE role = 5")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(demoted, 2);
        let still_inactive: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_members WHERE role = 5 AND NOT is_active",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(still_inactive, 1);
        cascade_deactivate_ws(&db, "acme", user, now)
            .await
            .expect("ws cascade");
        deactivate_many(&db, &ids).await.expect("bulk");
        deactivate_many(&db, &[]).await.expect("empty bulk");
        let inactive: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_members WHERE NOT is_active")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(inactive, 2);
    }

    /// Endpoint creates (both texts), default rows (both texts) and
    /// triage rows (both texts) store their rows.
    #[tokio::test]
    async fn live_states() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let db = sea_orm_connection(&pool);
        sqlx::query(STATES_DDL)
            .execute(&pool)
            .await
            .expect("temp states");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let app = NewState {
            project_id: project,
            workspace_id: ws,
            name: "Todo",
            description: "",
            color: "#60646C",
            slug: "todo",
            sequence: 25000.0,
            group: "unstarted",
            is_triage: false,
            default: false,
            external_source: None,
            external_id: None,
            created_by: actor,
            updated_by: None,
            now,
        };
        let app_id = states::create(&db, &app).await.expect("app state");
        let stored: (String, bool, Option<String>) =
            sqlx::query_as("SELECT slug, is_triage, external_source FROM states WHERE id = $1")
                .bind(app_id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stored, ("todo".to_owned(), false, None));
        let v1 = NewState {
            slug: "todo",
            is_triage: true,
            external_source: Some("jira"),
            external_id: Some("T-1"),
            updated_by: Some(actor),
            ..state_for_replay(&app)
        };
        states::create(&db, &v1).await.expect("v1 state");
        let external: (bool, Option<String>) = sqlx::query_as(
            "SELECT is_triage, external_id FROM states WHERE external_source = 'jira'",
        )
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(external, (true, Some("T-1".to_owned())));
        let default = NewDefaultState {
            project_id: project,
            workspace_id: ws,
            name: "Backlog",
            color: "#60646C",
            sequence: 15000.0,
            group: "backlog",
            default: true,
            created_by: actor,
            now,
        };
        add_default(&db, &default).await.expect("default");
        let v1_default = default_for_replay(&default);
        add_default(&db, &v1_default).await.expect("v1 default");
        let defaults: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM states WHERE name = 'Backlog'")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(defaults, 2);
        add_triage(&db, project, ws, 65000.0, actor, now, now)
            .await
            .expect("triage");
        add_triage(&db, project, ws, 80000.0, actor, now, now)
            .await
            .expect("quoted triage");
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        add_triage(&db, project, ws, 95000.0, actor, now, later)
            .await
            .expect("full triage");
        let triage: Vec<(String, f64)> = sqlx::query_as(
            "SELECT slug, sequence FROM states WHERE name = 'Triage' ORDER BY sequence",
        )
        .fetch_all(&pool)
        .await
        .expect("read back");
        assert_eq!(
            triage,
            vec![
                ("triage".to_owned(), 65000.0),
                ("triage".to_owned(), 80000.0),
                ("triage".to_owned(), 95000.0)
            ]
        );
        let stamps: (DateTime<Utc>, DateTime<Utc>) =
            sqlx::query_as("SELECT created_at, updated_at FROM states WHERE sequence = 95000.0")
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stamps, (now, later));
        sqlx::query(WORKSPACES_DDL)
            .execute(&pool)
            .await
            .expect("temp workspaces");
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, $2)")
            .bind(ws)
            .bind("acme")
            .execute(&pool)
            .await
            .expect("workspace row");
        let patch = states::StatePatch {
            name: "Doing",
            color: "#000000",
            group: "started",
            default: false,
            description: "wip",
            sequence: 26000.0,
            slug: "doing",
            updated_at: now,
            updated_by: actor,
        };
        states::update(&db, app_id, &patch)
            .await
            .expect("state patch");
        let renamed: (String, String, f64) =
            sqlx::query_as("SELECT name, slug, sequence FROM states WHERE id = $1")
                .bind(app_id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(renamed, ("Doing".to_owned(), "doing".to_owned(), 26000.0));
        states::clear_defaults(&db, "acme", project)
            .await
            .expect("clear defaults");
        let cleared: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM states WHERE \"default\" AND deleted_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(cleared, 0);
        states::mark_default(&db, "acme", project, app_id)
            .await
            .expect("mark default");
        let marked: bool = sqlx::query_scalar("SELECT \"default\" FROM states WHERE id = $1")
            .bind(app_id)
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert!(marked);
        states::soft_delete_app(&db, app_id, now, actor)
            .await
            .expect("state delete");
        let tomb: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT deleted_at FROM states WHERE id = $1")
                .bind(app_id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(tomb, Some(now));
    }

    /// Field-level copy for the replay probes (`NewState` borrows).
    fn state_for_replay<'a>(state: &NewState<'a>) -> NewState<'a> {
        NewState {
            project_id: state.project_id,
            workspace_id: state.workspace_id,
            name: state.name,
            description: state.description,
            color: state.color,
            slug: state.slug,
            sequence: state.sequence,
            group: state.group,
            is_triage: state.is_triage,
            default: state.default,
            external_source: state.external_source,
            external_id: state.external_id,
            created_by: state.created_by,
            updated_by: state.updated_by,
            now: state.now,
        }
    }

    /// Field-level copy for the replay probes (`NewDefaultState`
    /// borrows).
    fn default_for_replay<'a>(state: &NewDefaultState<'a>) -> NewDefaultState<'a> {
        NewDefaultState {
            project_id: state.project_id,
            workspace_id: state.workspace_id,
            name: state.name,
            color: state.color,
            sequence: state.sequence,
            group: state.group,
            default: state.default,
            created_by: state.created_by,
            now: state.now,
        }
    }
}
