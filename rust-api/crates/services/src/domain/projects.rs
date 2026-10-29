//! OSS project writes: `projects`, `project_members`, `states` (ORM-07, PIDASHCONV-1525).
//!
//! Every OSS write to the three project tables lives in this module; the
//! app/v1 create flows, the member endpoints, the invite/join paths, the
//! signup joins, the state endpoints, the intake triage inserts and the
//! ops commands call these functions instead of executing SQL text. Reads
//! stay where they are (design D5).
//!
//! Execution shapes, by writer kind:
//!
//! * [`create`] runs on `&PgPool` and owns the one atomic pair in these
//!   flows: clearing the workspace default plus the project `INSERT`
//!   run in an internal `sqlx` transaction, exactly like the two
//!   callers' current `tx` blocks. Every other create leg (the pod
//!   `post_save`, the identifier insert, the admin/lead memberships,
//!   the default states, the `model_activity` enqueue) stays
//!   caller-sequenced on autocommit: the v1 flow's PB-2023 arm sits
//!   between the membership leg and the states leg and must keep
//!   observing the partial commit, so no monolithic create is
//!   possible. Callers resolve every input (the live-default probe,
//!   identifier/timezone normalization, the clear condition) and keep
//!   their own conflict arms (app class-23 vs v1 23505-only BUG-1).
//! * [`members`] and [`states`] are generic over `sqlx::Executor`,
//!   like [`super::tokens`]: single-statement writers run on the
//!   caller's pool or inside the caller's existing `sqlx` transaction
//!   (the v1 member-create tx) with no restructuring. Multi-row
//!   writers are single statements too; callers that need several
//!   writes (member row plus property row) sequence the single-row
//!   functions, which also preserves the two create flows' opposite
//!   write orders (property-first on app, member-first on v1).
//!
//! Bind-type uniformity (PIDASHCONV-1509): `sqlx` caches prepared
//! statements by SQL text alone, so one text must always bind the
//! same types. Every consolidated statement below binds one fixed
//! signature; callers whose bind types differ keep their own text:
//! the ops i64 role arms carry a marker comment so their text can
//! never collide with the i16 arms, and the app bulk builders (mixed
//! `RoleBind` types per row, like the originals) moved verbatim.
//!
//! Ported bugs preserved verbatim: the signup project arms' missing
//! `project_id` (PB-1008) binds an explicit `NULL`, which raises the
//! same 23502 with the same message as the omitted column; the v1
//! lead guard, the dual-now member bulks and the per-arm role clamps
//! stay at the callers; the `project_identifiers` insert and the
//! `model_activity`/pod effects are not covered-table writes and stay
//! caller-sequenced.

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// create (pool; the clear-default + INSERT atomic pair)
// ---------------------------------------------------------------------------

/// One project row to insert: every column without a DB default, fully
/// resolved by the caller (the live-default probe, identifier and
/// timezone normalization, the int4 fencedown). `deleted_at` is always
/// `NULL` and `updated_by` stays `NULL` on both paths.
pub struct NewProject<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<&'a serde_json::Value>,
    pub description_html: Option<&'a serde_json::Value>,
    pub network: i32,
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
    /// The v1 text: same stored values, but quoted columns and the
    /// unset-on-v1 fields as literals (`description_text/html`
    /// `NULL`, `network 2`, the agent knobs, `cover_image_asset`,
    /// `estimate`, `default_state` and `archived_at` `NULL`).
    pub v1: bool,
}

const CLEAR_DEFAULTS_SQL: &str = "UPDATE projects SET is_default = FALSE \
    WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL";

const PROJECT_INSERT_SQL: &str = "INSERT INTO \"projects\" \
    (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
    \"name\", \"description\", \"description_text\", \"description_html\", \"network\", \"workspace_id\", \
    \"identifier\", \"default_assignee_id\", \"project_lead_id\", \"emoji\", \"icon_prop\", \
    \"module_view\", \"cycle_view\", \"issue_views_view\", \"page_view\", \"intake_view\", \
    \"is_time_tracking_enabled\", \"is_issue_type_enabled\", \"guest_view_all_features\", \
    \"cover_image\", \"cover_image_asset_id\", \"estimate_id\", \"archive_in\", \"close_in\", \
    \"logo_props\", \"default_state_id\", \"archived_at\", \"is_default\", \"timezone\", \
    \"external_source\", \"external_id\", \"repo_url\", \"base_branch\", \
    \"agent_default_interval_seconds\", \"agent_default_max_ticks\", \
    \"agent_review_default_interval_seconds\", \"agent_test_default_interval_seconds\", \
    \"agent_ticking_enabled\", \"default_agent_executor\", \"members_can_edit_states\") \
    VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
    $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, \
    $33, $34, $35, $36, $37, $38, $39, $40, $41, $42, $43, $44)";

const PROJECT_INSERT_V1_SQL: &str = "INSERT INTO \"projects\" (\"id\", \"created_at\", \"updated_at\", \
    \"created_by_id\", \"updated_by_id\", \"name\", \"description\", \"description_text\", \
    \"description_html\", \"network\", \"workspace_id\", \"identifier\", \"default_assignee_id\", \
    \"project_lead_id\", \"emoji\", \"icon_prop\", \"module_view\", \"cycle_view\", \"issue_views_view\", \
    \"page_view\", \"intake_view\", \"is_time_tracking_enabled\", \"is_issue_type_enabled\", \"is_default\", \
    \"guest_view_all_features\", \"members_can_edit_states\", \"cover_image\", \"cover_image_asset_id\", \
    \"estimate_id\", \"archive_in\", \"close_in\", \"logo_props\", \"default_state_id\", \"timezone\", \
    \"external_source\", \"external_id\", \"repo_url\", \"base_branch\", \
    \"agent_default_interval_seconds\", \"agent_default_max_ticks\", \
    \"agent_review_default_interval_seconds\", \"agent_test_default_interval_seconds\", \
    \"agent_ticking_enabled\", \"default_agent_executor\") \
    VALUES ($1,$2,$3,$4,NULL,$5,$6,NULL,NULL,2,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,\
    $20,$21,$22,$23,NULL,NULL,$24,$25,$26,NULL,$27,$28,$29,$30,$31,10800,10,10800,10800,TRUE,$32)";

/// `Project.objects.create(...)`: the default-clear plus the row, in
/// one transaction, returning the new id. Errors propagate as
/// `sqlx::Error` (the transaction aborts) for the caller's own
/// conflict arm.
pub async fn create(pool: &sqlx::PgPool, project: &NewProject<'_>) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    if project.clear_others {
        sqlx::query(CLEAR_DEFAULTS_SQL)
            .bind(project.workspace_id)
            .execute(&mut *tx)
            .await?;
    }
    if project.v1 {
        insert_v1(&mut *tx, id, project).await?;
        tx.commit().await?;
        return Ok(id);
    }
    sqlx::query(PROJECT_INSERT_SQL)
        .bind(id)
        .bind(project.created_at)
        .bind(project.updated_at)
        .bind(project.created_by)
        .bind(project.name)
        .bind(project.description)
        .bind(project.description_text)
        .bind(project.description_html)
        .bind(project.network)
        .bind(project.workspace_id)
        .bind(project.identifier)
        .bind(project.default_assignee)
        .bind(project.project_lead)
        .bind(project.emoji)
        .bind(project.icon_prop)
        .bind(project.module_view)
        .bind(project.cycle_view)
        .bind(project.issue_views_view)
        .bind(project.page_view)
        .bind(project.intake_view)
        .bind(project.is_time_tracking_enabled)
        .bind(project.is_issue_type_enabled)
        .bind(project.guest_view_all_features)
        .bind(project.cover_image)
        .bind(project.cover_image_asset)
        .bind(project.estimate)
        .bind(project.archive_in)
        .bind(project.close_in)
        .bind(project.logo_props)
        .bind(project.default_state)
        .bind(project.archived_at)
        .bind(project.is_default)
        .bind(project.timezone)
        .bind(project.external_source)
        .bind(project.external_id)
        .bind(project.repo_url)
        .bind(project.base_branch)
        .bind(project.agent_interval)
        .bind(project.agent_max_ticks)
        .bind(project.agent_review)
        .bind(project.agent_test)
        .bind(project.agent_ticking)
        .bind(project.default_executor)
        .bind(project.members_can_edit_states)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

pub(crate) const SAVE_PROJECT_COVER_SQL: &str =
    "UPDATE \"projects\" SET \"cover_image_asset_id\" = $2, \"updated_at\" = $3, \"updated_by_id\" = $4 WHERE \"projects\".\"id\" = $1";

/// `save_project_cover` (`v2.py:631-634`): stamp
/// `cover_image_asset_id` on the re-fetched project; the LAST row
/// wins (the caller loops). The write is a FULL `project.save()`
/// (`db/models/base.py:24-44`), so it also stamps `updated_by` (CRUM
/// request user) and `updated_at`. Moved verbatim from
/// `app_assets::queries_v2_project` (PIDASHCONV-1630). The caller owns
/// the clock (`Utc::now()` at full precision — Postgres rounds to
/// micros on store, as today).
pub async fn save_project_cover<'e, E>(
    ex: E,
    project_id: Uuid,
    asset_id: Uuid,
    updated_at: DateTime<Utc>,
    updated_by: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(SAVE_PROJECT_COVER_SQL)
        .bind(project_id)
        .bind(asset_id)
        .bind(updated_at)
        .bind(updated_by)
        .execute(ex)
        .await?;
    Ok(())
}

/// Project `repo_url`/`base_branch` write (`services.py:341-349`).
///
/// Returns the `UPDATE` statement, or `None` when `update_fields` is
/// empty (Django skips the save). Parameters: `$1` project id, then
/// one placeholder per `Some` field in order (`repo_url`,
/// `base_branch`), then `updated_at` last. Moved verbatim from
/// `integrations::repositories` (PIDASHCONV-1630): `projects` is a
/// covered table, so the builder lives in domain/.
pub fn project_repo_update_sql(
    repo_url: Option<&str>,
    base_branch: Option<&str>,
) -> Option<String> {
    let mut sets = Vec::new();
    let mut placeholder = 2u8;
    if repo_url.is_some() {
        sets.push(format!("repo_url = ${placeholder}"));
        placeholder += 1;
    }
    if base_branch.is_some() {
        sets.push(format!("base_branch = ${placeholder}"));
        placeholder += 1;
    }
    if sets.is_empty() {
        return None;
    }
    sets.push(format!("updated_at = ${placeholder}"));
    Some(format!(
        "UPDATE projects SET {} WHERE id = $1",
        sets.join(", ")
    ))
}

/// The v1 bind arm: positional mapping of [`NewProject`] onto the
/// literal-heavy v1 text. The v1 caller always passes
/// `network == 2`, no descriptions, the agent defaults, ticking on,
/// and no asset/estimate/state/archived values, so the literals
/// store exactly what the app arm would bind.
async fn insert_v1<'e, E>(ex: E, id: Uuid, project: &NewProject<'_>) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PROJECT_INSERT_V1_SQL)
        .bind(id)
        .bind(project.created_at)
        .bind(project.updated_at)
        .bind(project.created_by)
        .bind(project.name)
        .bind(project.description)
        .bind(project.workspace_id)
        .bind(project.identifier)
        .bind(project.default_assignee)
        .bind(project.project_lead)
        .bind(project.emoji)
        .bind(project.icon_prop)
        .bind(project.module_view)
        .bind(project.cycle_view)
        .bind(project.issue_views_view)
        .bind(project.page_view)
        .bind(project.intake_view)
        .bind(project.is_time_tracking_enabled)
        .bind(project.is_issue_type_enabled)
        .bind(project.is_default)
        .bind(project.guest_view_all_features)
        .bind(project.members_can_edit_states)
        .bind(project.cover_image)
        .bind(project.archive_in)
        .bind(project.close_in)
        .bind(project.logo_props)
        .bind(project.timezone)
        .bind(project.external_source)
        .bind(project.external_id)
        .bind(project.repo_url)
        .bind(project.base_branch)
        .bind(project.default_executor)
        .execute(ex)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// v1 project PATCH / archive / delete + the git-bind project touch
// ---------------------------------------------------------------------------

/// `POST .../<project_id>/archive/` (`views/project.py:528-538`): stamp
/// `archived_at` (plus the `save()` audit pair). Returns the rows
/// matched — the driver 404s on zero.
pub async fn archive_v1_project<'e, E>(
    ex: E,
    id: Uuid,
    workspace_id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(ARCHIVE_PROJECT_SQL)
        .bind(now)
        .bind(actor)
        .bind(id)
        .bind(workspace_id)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const ARCHIVE_PROJECT_SQL: &str = "UPDATE \"projects\" SET \"archived_at\" = $1, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3 AND \"workspace_id\" = $4 AND \"deleted_at\" IS NULL";

/// `DELETE .../<project_id>/archive/` (`views/project.py:552-561`):
/// clear `archived_at` (plus the `save()` audit pair). Returns the rows
/// matched — the driver 404s on zero.
pub async fn unarchive_v1_project<'e, E>(
    ex: E,
    id: Uuid,
    workspace_id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(UNARCHIVE_PROJECT_SQL)
        .bind(now)
        .bind(actor)
        .bind(id)
        .bind(workspace_id)
        .execute(ex)
        .await
        .map(|done| done.rows_affected())
}

const UNARCHIVE_PROJECT_SQL: &str = "UPDATE \"projects\" SET \"archived_at\" = NULL, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3 AND \"workspace_id\" = $4 AND \"deleted_at\" IS NULL";

/// The PATCH `is_default` atomic unset: every other live default in the
/// workspace clears when this project becomes the default. Contrast
/// `CLEAR_DEFAULTS_SQL`, the create-path sweep with no `id != $2`
/// arm — Django has both shapes, so they stay separate twins (design
/// D6). Runs inside the driver's PATCH transaction.
pub async fn unset_other_project_defaults<'e, E>(
    ex: E,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(UNSET_OTHER_DEFAULTS_SQL)
        .bind(workspace_id)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

const UNSET_OTHER_DEFAULTS_SQL: &str = "UPDATE \"projects\" SET \"is_default\" = FALSE WHERE \"workspace_id\" = $1 AND \"is_default\" AND \"id\" != $2 AND \"deleted_at\" IS NULL";

/// One PATCH SET value, in bind order (`$3…` — `$1`/`$2` are the audit
/// pair). Moved with the PATCH twin; the driver still decides which
/// provided fields join the SET list.
#[derive(Debug, Clone)]
pub enum ProjectPatchBind {
    Str(String),
    OptStr(Option<String>),
    Bool(bool),
    Int(i32),
    OptUuid(Option<Uuid>),
    Json(Value),
}

/// The PATCH write (`ProjectUpdateSerializer.update` + `Project.save`,
/// `views/project.py`): the driver's pre-built SET list (audit pair
/// first, then one `"col" = $N` per provided field) plus the bind
/// values. `$N` in the WHERE clause is the project id. Conflict arms
/// stay at the driver (23505 → the 409 name body).
pub async fn apply_project_patch<'e, E>(
    ex: E,
    sets: &[String],
    binds: &[ProjectPatchBind],
    now: DateTime<Utc>,
    actor: Uuid,
    id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = project_patch_sql(sets, binds.len());
    let mut query = sqlx::query(&sql).bind(now).bind(actor);
    for bind in binds {
        query = match bind {
            ProjectPatchBind::Str(v) => query.bind(v),
            ProjectPatchBind::OptStr(v) => query.bind(v),
            ProjectPatchBind::Bool(v) => query.bind(v),
            ProjectPatchBind::Int(v) => query.bind(v),
            ProjectPatchBind::OptUuid(v) => query.bind(v),
            ProjectPatchBind::Json(v) => query.bind(v),
        };
    }
    query = query.bind(id);
    query.execute(ex).await.map(|_| ())
}

/// The PATCH statement for [`apply_project_patch`]: the driver's
/// pre-built SET list, `$N` in the WHERE clause the project id.
fn project_patch_sql(sets: &[String], binds_len: usize) -> String {
    format!(
        "UPDATE \"projects\" SET {} WHERE \"id\" = ${} AND \"deleted_at\" IS NULL",
        sets.join(", "),
        binds_len + 3
    )
}

/// `DELETE workspaces/<slug>/projects/<pk>/` (`views/project.py:480-508`,
/// instance `delete()` → `save()`): `deleted_at` + `updated_*`. The
/// favorite cascade and the related-objects sweep stay driver-side.
pub async fn soft_delete_v1_project<'e, E>(
    ex: E,
    id: Uuid,
    actor: Uuid,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(SOFT_DELETE_PROJECT_SQL)
        .bind(now)
        .bind(actor)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

const SOFT_DELETE_PROJECT_SQL: &str = "UPDATE \"projects\" SET \"deleted_at\" = $1, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3";

/// `project.save(update_fields=...)` on git bind (`services.py:341-348`):
/// only the changed columns plus `updated_at`. One twin serves the app
/// and v1 `apply_bind` copies (their builders were byte-identical).
///
/// Placeholders number sequentially: `$1` is the project id, then one
/// placeholder per changed field in arm order (`repo_url`,
/// `base_branch`), then `updated_at` last — every referenced `$N` is
/// bound, matching Django's `update_fields` numbering (PIDASHCONV-1642
/// fixed the skipped `$N` the 1629 move preserved verbatim).
pub async fn save_bind_project_update<'e, E>(
    ex: E,
    project_id: Uuid,
    repo_url: Option<&str>,
    base_branch: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(sql) = bind_project_update_sql(repo_url.is_some(), base_branch.is_some()) else {
        return Ok(());
    };
    let mut query = sqlx::query(&sql).bind(project_id);
    if let Some(url) = repo_url {
        query = query.bind(url);
    }
    if let Some(branch) = base_branch {
        query = query.bind(branch);
    }
    query = query.bind(now);
    query.execute(ex).await.map(|_| ())
}

/// The bind statement for [`save_bind_project_update`]: only the
/// changed columns plus `updated_at`. `None` when nothing changed.
/// `$1` is the id; the SET list numbers `$2…` sequentially in arm
/// order so every referenced placeholder is bound.
fn bind_project_update_sql(repo_url: bool, base_branch: bool) -> Option<String> {
    if !repo_url && !base_branch {
        return None;
    }
    let mut sets: Vec<String> = Vec::new();
    let mut placeholder = 2u32;
    if repo_url {
        sets.push(format!("repo_url = ${placeholder}"));
        placeholder += 1;
    }
    if base_branch {
        sets.push(format!("base_branch = ${placeholder}"));
        placeholder += 1;
    }
    sets.push(format!("updated_at = ${placeholder}"));
    Some(format!(
        "UPDATE projects SET {} WHERE id = $1",
        sets.join(", ")
    ))
}

// update (executor-generic; the app PATCH/PUT write)
// ---------------------------------------------------------------------------

/// `Project.save()` default-clear on the update path
/// (`project.py:292-301`): the live defaults in the workspace except
/// the row being saved. The create path ([`CLEAR_DEFAULTS_SQL`]) has
/// no row to exclude, so Django genuinely differs here (D6:
/// same-difference, kept as an explicit variant).
const CLEAR_OTHER_DEFAULTS_SQL: &str = "UPDATE projects SET is_default = FALSE
               WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL AND id <> $2";

/// Clear every live default in the workspace except this project.
/// The caller owns the guard probe and the tx (the `save()` atomic
/// guard-clear-write triple stays caller-sequenced).
pub async fn clear_other_defaults<'e, E>(
    ex: E,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(CLEAR_OTHER_DEFAULTS_SQL)
        .bind(workspace_id)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
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
    pub network: i32,
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

const PROJECT_UPDATE_SQL: &str = "UPDATE projects SET
             name = $1, description = $2, description_text = $3, description_html = $4,
             network = $5, identifier = $6,
             default_assignee_id = $7, project_lead_id = $8, emoji = $9, icon_prop = $10,
             module_view = $11, cycle_view = $12, issue_views_view = $13, page_view = $14,
             intake_view = $15, is_time_tracking_enabled = $16, is_issue_type_enabled = $17,
             guest_view_all_features = $18,
             cover_image = $19, cover_image_asset_id = $20, estimate_id = $21,
             archive_in = $22, close_in = $23, logo_props = $24,
             default_state_id = $25, archived_at = $26, is_default = $27, timezone = $28,
             external_source = $29, external_id = $30, repo_url = $31, base_branch = $32,
             agent_default_interval_seconds = $33, agent_default_max_ticks = $34,
             agent_review_default_interval_seconds = $35, agent_test_default_interval_seconds = $36,
             agent_ticking_enabled = $37, default_agent_executor = $38,
             members_can_edit_states = $39,
             workspace_id = $40, deleted_at = $41,
             updated_at = $42, updated_by_id = $43, created_by_id = $44
           WHERE id = $45";

/// The app project PATCH/PUT write (`ProjectSerializer.update()` +
/// `save()`): all 44 setter columns for one row. Runs on the
/// caller's executor (the update tx also holds the default-clear).
pub async fn apply_update<'e, E>(
    ex: E,
    project_id: Uuid,
    upd: &ProjectUpdate<'_>,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PROJECT_UPDATE_SQL)
        .bind(upd.name)
        .bind(upd.description)
        .bind(upd.description_text)
        .bind(upd.description_html)
        .bind(upd.network)
        .bind(upd.identifier)
        .bind(upd.default_assignee)
        .bind(upd.project_lead)
        .bind(upd.emoji)
        .bind(upd.icon_prop)
        .bind(upd.module_view)
        .bind(upd.cycle_view)
        .bind(upd.issue_views_view)
        .bind(upd.page_view)
        .bind(upd.intake_view)
        .bind(upd.is_time_tracking_enabled)
        .bind(upd.is_issue_type_enabled)
        .bind(upd.guest_view_all_features)
        .bind(upd.cover_image)
        .bind(upd.cover_image_asset)
        .bind(upd.estimate)
        .bind(upd.archive_in)
        .bind(upd.close_in)
        .bind(upd.logo_props)
        .bind(upd.default_state)
        .bind(upd.archived_at)
        .bind(upd.is_default)
        .bind(upd.timezone)
        .bind(upd.external_source)
        .bind(upd.external_id)
        .bind(upd.repo_url)
        .bind(upd.base_branch)
        .bind(upd.agent_interval)
        .bind(upd.agent_max_ticks)
        .bind(upd.agent_review)
        .bind(upd.agent_test)
        .bind(upd.agent_ticking)
        .bind(upd.default_executor)
        .bind(upd.members_can_edit_states)
        .bind(upd.workspace_id)
        .bind(upd.deleted_at)
        .bind(upd.updated_at)
        .bind(upd.updated_by)
        .bind(upd.created_by)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
}

// ---------------------------------------------------------------------------
// soft delete / archive / cover-asset / text-field stamps
// ---------------------------------------------------------------------------

/// `project.delete()`: the tombstone plus the `save()` stamps. The
/// `soft_delete_related_objects` emit and the cascades stay
/// caller-sequenced (not covered-table writes).
const PROJECT_SOFT_DELETE_SQL: &str =
    "UPDATE projects SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3";

/// Soft-delete one project row.
pub async fn soft_delete<'e, E>(
    ex: E,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PROJECT_SOFT_DELETE_SQL)
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `ProjectArchiveUnarchiveEndpoint.post` (`base.py:435-442`): stamp
/// `archived_at` (+ `updated_at`/`updated_by`). The favorites
/// soft-delete stays at the caller.
const PROJECT_ARCHIVE_SQL: &str =
    "UPDATE projects SET archived_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3";

/// Archive one project row.
pub async fn archive<'e, E>(
    ex: E,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PROJECT_ARCHIVE_SQL)
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `ProjectArchiveUnarchiveEndpoint.delete` (`base.py:444-446`):
/// clear `archived_at` (+ `updated_at`/`updated_by`).
const PROJECT_UNARCHIVE_SQL: &str =
    "UPDATE projects SET archived_at = NULL, updated_at = $1, updated_by_id = $2 WHERE id = $3";

/// Unarchive one project row.
pub async fn unarchive<'e, E>(
    ex: E,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PROJECT_UNARCHIVE_SQL)
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `WorkspaceFileAssetEndpoint.entity_asset_save`
/// (`v2.py:247-284`): the cover link plus the full
/// `project.save()` stamps (`auto_now` `updated_at`, crum
/// `updated_by`). The previous-asset delete stays at the caller.
const PROJECT_COVER_LINK_SQL: &str = "UPDATE \"projects\" SET \"cover_image\" = $1, \"cover_image_asset_id\" = $2, \"updated_at\" = $3, \"updated_by_id\" = $4 WHERE \"id\" = $5";

/// Link a cover asset onto one project row
/// (`cover_image = ""`, the ported empty-string write).
pub async fn stamp_cover_image<'e, E>(
    ex: E,
    project_id: Uuid,
    asset_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PROJECT_COVER_LINK_SQL)
        .bind("")
        .bind(asset_id)
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
}

/// `WorkspaceFileAssetEndpoint.entity_asset_delete`
/// (`v2.py:286-312`): clear the cover link FK plus the full
/// `project.save()` stamps.
const PROJECT_COVER_UNLINK_SQL: &str = "UPDATE \"projects\" SET \"cover_image_asset_id\" = NULL, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3";

/// Clear one project row's cover link FK.
pub async fn clear_cover_image_asset<'e, E>(
    ex: E,
    project_id: Uuid,
    now: DateTime<Utc>,
    actor_id: &Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(PROJECT_COVER_UNLINK_SQL)
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
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

const PROJECT_REPO_URL_SQL: &str = "UPDATE projects SET repo_url = $1 WHERE id = $2";

const PROJECT_BASE_BRANCH_SQL: &str = "UPDATE projects SET base_branch = $1 WHERE id = $2";

/// Stamp one `repo_url` / `base_branch` value. The column set is
/// fixed (the two enum arms); the caller's conditional-append loop
/// keeps its shape, emitting the same bytes per arm.
pub async fn stamp_text_field<'e, E>(
    ex: E,
    field: ProjectTextField,
    value: &str,
    project_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = match field {
        ProjectTextField::RepoUrl => PROJECT_REPO_URL_SQL,
        ProjectTextField::BaseBranch => PROJECT_BASE_BRANCH_SQL,
    };
    sqlx::query(sql)
        .bind(value)
        .bind(project_id)
        .execute(ex)
        .await
        .map(|_| ())
}

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

pub(crate) const SEED_PROJECT_INSERT_SQL: &str = "INSERT INTO projects \
    (created_at, updated_at, id, name, description, description_text, description_html, \
    network, identifier, created_by_id, default_assignee_id, project_lead_id, updated_by_id, \
    workspace_id, emoji, cycle_view, module_view, cover_image, issue_views_view, page_view, \
    estimate_id, icon_prop, intake_view, archive_in, close_in, default_state_id, logo_props, \
    archived_at, is_time_tracking_enabled, is_issue_type_enabled, deleted_at, \
    guest_view_all_features, timezone, cover_image_asset_id, external_id, external_source, \
    members_can_edit_states, repo_url, base_branch, agent_default_interval_seconds, \
    agent_default_max_ticks, agent_ticking_enabled, is_default, \
    agent_review_default_interval_seconds, default_agent_executor, \
    agent_test_default_interval_seconds) \
    VALUES ($1, $1, $2, $3, $4, NULL, NULL, $5, $6, $7, NULL, NULL, NULL, $8, NULL, \
    true, true, $9, true, true, NULL, NULL, false, 0, 0, NULL, $10, NULL, false, false, \
    NULL, false, $11, NULL, NULL, NULL, true, '', 'main', 10800, 10, true, $12, 10800, \
    $13, 10800) \
    RETURNING id";

pub(crate) const SEED_PROJECT_DEMOTE_SQL: &str = "UPDATE projects SET is_default = false \
    WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL AND id <> $2";

/// The seed project `save`: the row plus the atomic demote-others
/// update (`project.py:292-298`), in one transaction like `Project.save`'s
/// `transaction.atomic` block. Returns the new row id.
pub async fn create_seed_project(
    pool: &sqlx::PgPool,
    project: &SeedProject<'_>,
) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(SEED_PROJECT_INSERT_SQL)
        .bind(project.now)
        .bind(Uuid::new_v4())
        .bind(project.name)
        .bind(project.description)
        .bind(project.network)
        .bind(project.identifier)
        .bind(project.created_by)
        .bind(project.workspace_id)
        .bind(project.cover_image)
        .bind(sqlx::types::Json(project.logo_props))
        .bind(project.timezone)
        .bind(project.is_default)
        .bind(project.default_agent_executor)
        .fetch_one(&mut *tx)
        .await?;
    if project.is_default {
        sqlx::query(SEED_PROJECT_DEMOTE_SQL)
            .bind(project.workspace_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// One dummy-data project row: `create_project (:44-60)` in
/// `bgtasks/dummy_data_task.py` — the creator call passes only
/// project/member/role, `save()` derives the workspace, fills the
/// model defaults and nulls the audit user (no request user in a
/// task); the timezone inherits the workspace's and `is_default` is
/// the caller's first-project probe. Ids are the task's `::uuid`
/// strings. Moved verbatim from the dummy store (PIDASHCONV-1632).
/// No demote-others leg: a first project demotes zero rows and a
/// later one inserts `is_default = false`, so the update is a
/// guaranteed no-op on this path.
pub struct DemoProject<'a> {
    pub workspace_id: &'a str,
    pub name: &'a str,
    pub identifier: &'a str,
    pub is_default: bool,
    pub timezone: &'a str,
    pub default_agent_executor: &'a str,
    pub now: DateTime<Utc>,
}

pub(crate) const DEMO_PROJECT_INSERT_SQL: &str = "INSERT INTO projects \
    (id, workspace_id, name, identifier, created_by_id, intake_view, created_at, updated_at, \
    description, network, cycle_view, module_view, issue_views_view, page_view, archive_in, \
    close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, is_default, \
    guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
    agent_default_interval_seconds, agent_default_max_ticks, \
    agent_review_default_interval_seconds, agent_test_default_interval_seconds, \
    agent_ticking_enabled, default_agent_executor) \
    VALUES ($1::uuid, $2::uuid, $3, $4, NULL, TRUE, $5, $5, '', 2, FALSE, FALSE, FALSE, \
    TRUE, 0, 0, '{}', FALSE, FALSE, $6, FALSE, $7, TRUE, '', 'main', 10800, 10, 10800, \
    10800, TRUE, $8)";

/// The dummy-data project insert. Generates the row id client-side
/// (as Django does) and returns it for the pod/member legs.
pub async fn insert_demo_project<'e, E>(
    ex: E,
    project: &DemoProject<'_>,
) -> Result<String, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let id = Uuid::new_v4().to_string();
    sqlx::query(DEMO_PROJECT_INSERT_SQL)
        .bind(&id)
        .bind(project.workspace_id)
        .bind(project.name)
        .bind(project.identifier)
        .bind(project.now)
        .bind(project.is_default)
        .bind(project.timezone)
        .bind(project.default_agent_executor)
        .execute(ex)
        .await?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// members
// ---------------------------------------------------------------------------

/// OSS `project_members` writes (plus the `ProjectUserProperty` leg
/// that rides the member save). Callers sequence the single-row
/// writers: which legs run, and in which order, differs per path.
pub mod members {
    use super::*;

    /// One member row to insert. `comment` is always `NULL`,
    /// `updated_by`/`deleted_at` stay `NULL` and `sort_order` is the
    /// 65535 literal on every path; `is_active` is always `TRUE`.
    pub struct NewProjectMember<'a> {
        pub workspace_id: Uuid,
        /// Always `Some` except the signup project arms (PB-1008),
        /// which bind an explicit `NULL` and raise the same 23502 the
        /// omitted column raises (identical message, identical effect).
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

    pub(crate) const MEMBER_INSERT_SQL: &str = "INSERT INTO \"project_members\" \
        (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
        \"project_id\", \"workspace_id\", \"member_id\", \"comment\", \"role\", \
        \"view_props\", \"default_props\", \"preferences\", \"sort_order\", \"is_active\") \
        VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, NULL, $8, $9, $10, $11, 65535, TRUE)";

    pub(crate) const MEMBER_INSERT_IGNORE_SQL: &str = "INSERT INTO \"project_members\" \
        (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
        \"project_id\", \"workspace_id\", \"member_id\", \"comment\", \"role\", \
        \"view_props\", \"default_props\", \"preferences\", \"sort_order\", \"is_active\") \
        VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, NULL, $8, $9, $10, $11, 65535, TRUE) \
        ON CONFLICT DO NOTHING";

    pub(crate) const MEMBER_INSERT_DB_NOW_SQL: &str = "INSERT INTO \"project_members\" \
        (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
        \"project_id\", \"workspace_id\", \"member_id\", \"comment\", \"role\", \
        \"view_props\", \"default_props\", \"preferences\", \"sort_order\", \"is_active\") \
        VALUES ($1, now(), now(), $2, NULL, NULL, $3, $4, $5, NULL, $6, $7, $8, $9, 65535, TRUE)";

    /// `ProjectMember.objects.create(...)` (and the per-row signup
    /// replays). Returns the new row id.
    pub async fn add<'e, E>(ex: E, member: &NewProjectMember<'_>) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        if member.db_now {
            sqlx::query(MEMBER_INSERT_DB_NOW_SQL)
                .bind(id)
                .bind(member.created_by)
                .bind(member.project_id)
                .bind(member.workspace_id)
                .bind(member.user_id)
                .bind(member.role)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.default_props))
                .bind(sqlx::types::Json(member.preferences))
                .execute(ex)
                .await?;
        } else if member.ignore_conflicts {
            sqlx::query(MEMBER_INSERT_IGNORE_SQL)
                .bind(id)
                .bind(member.now)
                .bind(member.now)
                .bind(member.created_by)
                .bind(member.project_id)
                .bind(member.workspace_id)
                .bind(member.user_id)
                .bind(member.role)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.default_props))
                .bind(sqlx::types::Json(member.preferences))
                .execute(ex)
                .await?;
        } else {
            sqlx::query(MEMBER_INSERT_SQL)
                .bind(id)
                .bind(member.now)
                .bind(member.now)
                .bind(member.created_by)
                .bind(member.project_id)
                .bind(member.workspace_id)
                .bind(member.user_id)
                .bind(member.role)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.default_props))
                .bind(sqlx::types::Json(member.preferences))
                .execute(ex)
                .await?;
        }
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

    pub(crate) const PROPERTY_INSERT_SQL: &str = "INSERT INTO \"project_user_properties\" \
        (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
        \"project_id\", \"workspace_id\", \"user_id\", \"filters\", \"display_filters\", \
        \"display_properties\", \"rich_filters\", \"preferences\", \"sort_order\") \
        VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

    /// The property leg of the member save. Returns the new row id.
    pub async fn add_property<'e, E>(
        ex: E,
        property: &NewMemberProperty<'_>,
    ) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        sqlx::query(PROPERTY_INSERT_SQL)
            .bind(id)
            .bind(property.now)
            .bind(property.now)
            .bind(property.created_by)
            .bind(property.project_id)
            .bind(property.workspace_id)
            .bind(property.user_id)
            .bind(sqlx::types::Json(property.filters))
            .bind(sqlx::types::Json(property.display_filters))
            .bind(sqlx::types::Json(property.display_properties))
            .bind(sqlx::types::Json(property.rich_filters))
            .bind(sqlx::types::Json(property.preferences))
            .bind(property.sort_order)
            .execute(ex)
            .await?;
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
        pub project_id: Option<Uuid>,
        pub user_id: Uuid,
        pub role: RoleBind,
        pub created_by: Option<Uuid>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    /// Render the multi-row `INSERT ... ON CONFLICT DO NOTHING` for
    /// `nrows` rows (9 binds per row). Pure; unit-tested.
    pub(crate) fn bulk_add_sql(nrows: usize) -> String {
        let mut sql = String::from(
            "INSERT INTO \"project_members\" \
            (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
            \"project_id\", \"workspace_id\", \"member_id\", \"comment\", \"role\", \
            \"view_props\", \"default_props\", \"preferences\", \"sort_order\", \"is_active\") \
            VALUES ",
        );
        for index in 0..nrows {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * 9 + 1;
            sql.push_str(&format!(
                "(gen_random_uuid(), ${c}, ${u}, ${b}, NULL, NULL, ${p}, ${w}, ${m}, NULL, \
                 ${r}, ${v}, ${v}, ${f}, 65535, TRUE)",
                c = base,
                u = base + 1,
                b = base + 2,
                p = base + 3,
                w = base + 4,
                m = base + 5,
                r = base + 6,
                v = base + 7,
                f = base + 8,
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        sql
    }

    /// Render the DB-stamped multi-row variant (7 binds per row).
    /// Pure; unit-tested.
    pub(crate) fn bulk_add_db_now_sql(nrows: usize) -> String {
        let mut sql = String::from(
            "INSERT INTO \"project_members\" \
            (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
            \"project_id\", \"workspace_id\", \"member_id\", \"comment\", \"role\", \
            \"view_props\", \"default_props\", \"preferences\", \"sort_order\", \"is_active\") \
            VALUES ",
        );
        for index in 0..nrows {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * 7 + 1;
            sql.push_str(&format!(
                "(gen_random_uuid(), now(), now(), ${c}, NULL, NULL, ${p}, ${w}, ${m}, NULL, \
                 ${r}, ${v}, ${v}, ${f}, 65535, TRUE)",
                c = base,
                p = base + 1,
                w = base + 2,
                m = base + 3,
                r = base + 4,
                v = base + 5,
                f = base + 6,
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        sql
    }

    /// Bind one [`RoleBind`] into the running bulk query (shared by
    /// both stamp variants and [`update_roles_bulk`]).
    fn bind_role<'q>(
        query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
        role: &'q RoleBind,
    ) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
        match role {
            RoleBind::Null => query.bind(None::<String>),
            RoleBind::Int(int) => query.bind(int),
            RoleBind::Float(float) => query.bind(float),
            RoleBind::Text(text) => query.bind(text),
        }
    }

    /// `ProjectMember.objects.bulk_create([...],
    /// ignore_conflicts=True)`: one multi-row statement. An empty
    /// list emits no statement, like `bulk_create([])`. `props`
    /// serves both `view_props` and `default_props` (every caller
    /// binds identical content for the two columns). `db_now`
    /// selects the DB-stamped variant (the invite bulks).
    pub async fn bulk_add<'e, E>(
        ex: E,
        rows: &[BulkRow],
        props: &serde_json::Value,
        preferences: &serde_json::Value,
        db_now: bool,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if rows.is_empty() {
            return Ok(());
        }
        if db_now {
            let sql = bulk_add_db_now_sql(rows.len());
            let mut query = sqlx::query(&sql);
            for row in rows {
                query = query
                    .bind(row.created_by)
                    .bind(row.project_id)
                    .bind(row.workspace_id)
                    .bind(row.user_id);
                query = bind_role(query, &row.role);
                query = query
                    .bind(sqlx::types::Json(props))
                    .bind(sqlx::types::Json(preferences));
            }
            query.execute(ex).await?;
            return Ok(());
        }
        let sql = bulk_add_sql(rows.len());
        let mut query = sqlx::query(&sql);
        for row in rows {
            query = query
                .bind(row.created_at)
                .bind(row.updated_at)
                .bind(row.created_by)
                .bind(row.project_id)
                .bind(row.workspace_id)
                .bind(row.user_id);
            query = bind_role(query, &row.role);
            query = query
                .bind(sqlx::types::Json(props))
                .bind(sqlx::types::Json(preferences));
        }
        query.execute(ex).await?;
        Ok(())
    }

    pub(crate) const PROPERTY_INSERT_DB_NOW_SQL: &str = "INSERT INTO project_user_properties \
        (id, workspace_id, project_id, user_id, filters, display_filters, display_properties, \
        rich_filters, preferences, sort_order, created_by_id, updated_by_id, created_at, \
        updated_at, deleted_at) \
        VALUES ($1, $2, $3, $4, $5, $6, $7, '{}', $8, $9, $10, NULL, now(), now(), NULL)";

    pub(crate) const PROPERTY_GET_OR_CREATE_SQL: &str = "INSERT INTO project_user_properties \
        (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, \
        workspace_id, user_id, filters, display_filters, display_properties, rich_filters, \
        preferences, sort_order) \
        VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
        ON CONFLICT (user_id, project_id) WHERE deleted_at IS NULL DO NOTHING";

    /// The `get_or_create(user, project)` property write: the
    /// partial-index conflict target swallows a lost race (the
    /// caller re-reads). Returns the attempted row id.
    pub async fn get_or_create_property<'e, E>(
        ex: E,
        property: &NewMemberProperty<'_>,
    ) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        sqlx::query(PROPERTY_GET_OR_CREATE_SQL)
            .bind(id)
            .bind(property.now)
            .bind(property.now)
            .bind(property.created_by)
            .bind(property.project_id)
            .bind(property.workspace_id)
            .bind(property.user_id)
            .bind(sqlx::types::Json(property.filters))
            .bind(sqlx::types::Json(property.display_filters))
            .bind(sqlx::types::Json(property.display_properties))
            .bind(sqlx::types::Json(property.rich_filters))
            .bind(sqlx::types::Json(property.preferences))
            .bind(property.sort_order)
            .execute(ex)
            .await?;
        Ok(id)
    }

    /// The DB-stamped property leg (the invite member write):
    /// `rich_filters '{}'`, stamps from `now()`. Returns the new row
    /// id.
    pub async fn add_property_db_now<'e, E>(
        ex: E,
        property: &NewMemberProperty<'_>,
    ) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        sqlx::query(PROPERTY_INSERT_DB_NOW_SQL)
            .bind(id)
            .bind(property.workspace_id)
            .bind(property.project_id)
            .bind(property.user_id)
            .bind(sqlx::types::Json(property.filters))
            .bind(sqlx::types::Json(property.display_filters))
            .bind(sqlx::types::Json(property.display_properties))
            .bind(sqlx::types::Json(property.preferences))
            .bind(property.sort_order)
            .bind(property.created_by)
            .execute(ex)
            .await?;
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

    /// Render the multi-row property `INSERT ... ON CONFLICT DO
    /// NOTHING` for `nrows` rows (12 binds per row). Pure;
    /// unit-tested.
    pub(crate) fn bulk_add_properties_sql(nrows: usize) -> String {
        let mut sql = String::from(
            "INSERT INTO \"project_user_properties\" \
            (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
            \"project_id\", \"workspace_id\", \"user_id\", \"filters\", \"display_filters\", \
            \"display_properties\", \"rich_filters\", \"preferences\", \"sort_order\") \
            VALUES ",
        );
        for index in 0..nrows {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * 12 + 1;
            sql.push_str(&format!(
                "(gen_random_uuid(), ${c}, ${u}, ${b}, NULL, NULL, ${p}, ${w}, ${m}, \
                 ${f}, ${d}, ${g}, ${r}, ${e}, ${s})",
                c = base,
                u = base + 1,
                b = base + 2,
                p = base + 3,
                w = base + 4,
                m = base + 5,
                f = base + 6,
                d = base + 7,
                g = base + 8,
                r = base + 9,
                e = base + 10,
                s = base + 11,
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        sql
    }

    /// Render the DB-stamped multi-row property variant (10 binds per
    /// row). Pure; unit-tested.
    pub(crate) fn bulk_add_properties_db_now_sql(nrows: usize) -> String {
        let mut sql = String::from(
            "INSERT INTO \"project_user_properties\" \
            (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
            \"project_id\", \"workspace_id\", \"user_id\", \"filters\", \"display_filters\", \
            \"display_properties\", \"rich_filters\", \"preferences\", \"sort_order\") \
            VALUES ",
        );
        for index in 0..nrows {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * 10 + 1;
            sql.push_str(&format!(
                "(gen_random_uuid(), now(), now(), ${b}, NULL, NULL, ${p}, ${w}, ${m}, \
                 ${f}, ${d}, ${g}, ${r}, ${e}, ${s})",
                b = base,
                p = base + 1,
                w = base + 2,
                m = base + 3,
                f = base + 4,
                d = base + 5,
                g = base + 6,
                r = base + 7,
                e = base + 8,
                s = base + 9,
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        sql
    }

    /// The property leg of the member bulks: one multi-row
    /// statement. Empty emits nothing. `db_now` selects the
    /// DB-stamped variant (the invite bulks).
    pub async fn bulk_add_properties<'e, E>(
        ex: E,
        rows: &[BulkProperty<'_>],
        db_now: bool,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if rows.is_empty() {
            return Ok(());
        }
        if db_now {
            let sql = bulk_add_properties_db_now_sql(rows.len());
            let mut query = sqlx::query(&sql);
            for row in rows {
                query = query
                    .bind(row.created_by)
                    .bind(row.project_id)
                    .bind(row.workspace_id)
                    .bind(row.user_id)
                    .bind(sqlx::types::Json(row.filters))
                    .bind(sqlx::types::Json(row.display_filters))
                    .bind(sqlx::types::Json(row.display_properties))
                    .bind(sqlx::types::Json(row.rich_filters))
                    .bind(sqlx::types::Json(row.preferences))
                    .bind(row.sort_order);
            }
            query.execute(ex).await?;
            return Ok(());
        }
        let sql = bulk_add_properties_sql(rows.len());
        let mut query = sqlx::query(&sql);
        for row in rows {
            query = query
                .bind(row.created_at)
                .bind(row.updated_at)
                .bind(row.created_by)
                .bind(row.project_id)
                .bind(row.workspace_id)
                .bind(row.user_id)
                .bind(sqlx::types::Json(row.filters))
                .bind(sqlx::types::Json(row.display_filters))
                .bind(sqlx::types::Json(row.display_properties))
                .bind(sqlx::types::Json(row.rich_filters))
                .bind(sqlx::types::Json(row.preferences))
                .bind(row.sort_order);
        }
        query.execute(ex).await?;
        Ok(())
    }

    /// The ops member create: compact text (no spaces), `created_by`
    /// `NULL`, and the role bound as `int8` — in-range values assign
    /// to the `smallint` column, out-of-range ones raise the server's
    /// `smallint out of range`, exactly as the psycopg-bound values
    /// do. `None` binds `NULL` (the BUG-2 write, which Postgres
    /// rejects). The text differs from [`MEMBER_INSERT_SQL`] so the
    /// type cache can never confuse the two signatures. Returns the
    /// new row id.
    #[allow(clippy::too_many_arguments)]
    pub async fn ops_add<'e, E>(
        ex: E,
        workspace_id: Uuid,
        project_id: Uuid,
        user_id: Uuid,
        role: Option<i64>,
        props: &serde_json::Value,
        preferences: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO \"project_members\" \
             (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \
             \"project_id\", \"workspace_id\", \"member_id\", \"comment\", \"role\", \
             \"view_props\", \"default_props\", \"preferences\", \"sort_order\", \"is_active\") \
             VALUES ($1,$2,$2,NULL,NULL,NULL, $3,$4,$5,NULL,$6, $7,$7,$8,65535,TRUE)",
        )
        .bind(id)
        .bind(now)
        .bind(project_id)
        .bind(workspace_id)
        .bind(user_id)
        .bind(role)
        .bind(sqlx::types::Json(props))
        .bind(sqlx::types::Json(preferences))
        .execute(ex)
        .await?;
        Ok(id)
    }

    pub(crate) const OPS_UPDATE_ROLE_SQL: &str =
        "UPDATE \"project_members\" SET \"is_active\" = TRUE, \
        \"role\" = $3 WHERE \"project_id\" = $1 AND \"member_id\" = $2 AND \"deleted_at\" IS NULL";

    /// The ops role write: queryset `update()` touches ONLY the two
    /// columns — no `save()`, no `auto_now` bump. Role as `int8`
    /// (see [`ops_add`]).
    pub async fn ops_update_role<'e, E>(
        ex: E,
        project_id: Uuid,
        user_id: Uuid,
        role: Option<i64>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(OPS_UPDATE_ROLE_SQL)
            .bind(project_id)
            .bind(user_id)
            .bind(role)
            .execute(ex)
            .await?;
        Ok(())
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

    /// Render the app member-bulk `CAST(CASE ...)` update for `nrows`
    /// rows. Pure; unit-tested. Moved verbatim: role `WHEN`s first,
    /// then `is_active` `WHEN`s, then the `id IN (...)` list.
    pub(crate) fn update_roles_bulk_sql(nrows: usize) -> String {
        let mut sql = String::from("UPDATE project_members SET role = CAST(CASE id");
        let mut index = 0;
        for _ in 0..nrows {
            index += 1;
            let id_arg = index;
            index += 1;
            let role_arg = index;
            sql.push_str(&format!(" WHEN ${id_arg} THEN ${role_arg}"));
        }
        sql.push_str(" END AS smallint), is_active = CASE id");
        for _ in 0..nrows {
            index += 1;
            let id_arg = index;
            index += 1;
            let active_arg = index;
            sql.push_str(&format!(" WHEN ${id_arg} THEN ${active_arg}"));
        }
        sql.push_str(" END WHERE id IN (");
        for position in 0..nrows {
            if position > 0 {
                sql.push_str(", ");
            }
            index += 1;
            sql.push_str(&format!("${index}"));
        }
        sql.push(')');
        sql
    }

    /// `bulk_update(["is_active", "role"])`: one atomic statement
    /// with Django's `CAST(CASE ... AS smallint)` shape. Skipped
    /// entirely when there is nothing to reactivate.
    pub async fn update_roles_bulk<'e, E>(
        ex: E,
        updates: &[(Uuid, RoleBind)],
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if updates.is_empty() {
            return Ok(());
        }
        let sql = update_roles_bulk_sql(updates.len());
        let mut query = sqlx::query(&sql);
        for (id, role) in updates {
            query = bind_role(query.bind(id), role);
        }
        for (id, _) in updates {
            query = query.bind(id).bind(true);
        }
        for (id, _) in updates {
            query = query.bind(id);
        }
        query.execute(ex).await?;
        Ok(())
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

    /// Render the PATCH `UPDATE` for the present fields: `updated_at`
    /// / `updated_by_id` first, then the present columns in fixed
    /// order, `WHERE id` last. Pure; unit-tested.
    pub(crate) fn patch_member_sql(patch: &MemberPatch<'_>) -> String {
        let mut sets = vec![
            "updated_at = $1".to_owned(),
            "updated_by_id = $2".to_owned(),
        ];
        let mut index = 2;
        if patch.deleted_at.is_some() {
            index += 1;
            sets.push(format!("deleted_at = ${index}"));
        }
        if patch.comment.is_some() {
            index += 1;
            sets.push(format!("comment = ${index}"));
        }
        if patch.role.is_some() {
            index += 1;
            sets.push(format!("role = ${index}"));
        }
        if patch.view_props.is_some() {
            index += 1;
            sets.push(format!("view_props = ${index}"));
        }
        if patch.default_props.is_some() {
            index += 1;
            sets.push(format!("default_props = ${index}"));
        }
        if patch.preferences.is_some() {
            index += 1;
            sets.push(format!("preferences = ${index}"));
        }
        if patch.sort_order.is_some() {
            index += 1;
            sets.push(format!("sort_order = ${index}"));
        }
        if patch.is_active.is_some() {
            index += 1;
            sets.push(format!("is_active = ${index}"));
        }
        if patch.created_by.is_some() {
            index += 1;
            sets.push(format!("created_by_id = ${index}"));
        }
        index += 1;
        let id_arg = index;
        format!(
            "UPDATE project_members SET {} WHERE id = ${id_arg}",
            sets.join(", ")
        )
    }

    /// The app member PATCH: `save()` stamps `updated_at`/`updated_by`
    /// (a validated `updated_by` is ignored — `save()` overwrites it
    /// with the request user) while a validated `created_by`
    /// persists. The `i64` role binds `as i16` (truncation, like the
    /// original).
    pub async fn patch_member<'e, E>(
        ex: E,
        pk: Uuid,
        patch: &MemberPatch<'_>,
        saved_at: DateTime<Utc>,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let sql = patch_member_sql(patch);
        let mut query = sqlx::query(&sql).bind(saved_at).bind(user_id);
        if let Some(deleted_at) = patch.deleted_at {
            query = query.bind(deleted_at);
        }
        if let Some(comment) = patch.comment {
            query = query.bind(comment);
        }
        if let Some(role) = patch.role {
            query = query.bind(role as i16);
        }
        if let Some(view_props) = patch.view_props {
            query = query.bind(view_props);
        }
        if let Some(default_props) = patch.default_props {
            query = query.bind(default_props);
        }
        if let Some(preferences) = patch.preferences {
            query = query.bind(preferences);
        }
        if let Some(sort_order) = patch.sort_order {
            query = query.bind(sort_order);
        }
        if let Some(is_active) = patch.is_active {
            query = query.bind(is_active);
        }
        if let Some(created_by) = patch.created_by {
            query = query.bind(created_by);
        }
        query = query.bind(pk);
        query.execute(ex).await?;
        Ok(())
    }

    pub(crate) const UPDATE_IDENTITY_SQL: &str =
        "UPDATE project_members SET member_id = $1, role = $2, \
        updated_by_id = $3, updated_at = now() WHERE id = $4";

    /// The v1 member PATCH: `member` may go null (renders `null`),
    /// `role` is non-nullable in the model (a `None` role 500s at the
    /// caller after the write — the write itself binds it). The role
    /// binds as `int4`, assigning into the `smallint` column.
    pub async fn update_member_identity<'e, E>(
        ex: E,
        id: Uuid,
        member_id: Option<Uuid>,
        role: Option<i32>,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(UPDATE_IDENTITY_SQL)
            .bind(member_id)
            .bind(role)
            .bind(actor_id)
            .bind(id)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const RESET_PROPS_SQL: &str =
        "UPDATE project_members SET view_props = $1, default_props = $2, \
        preferences = $3, sort_order = $4, updated_at = $5, updated_by_id = $6 WHERE id = $7";

    /// The app member-props reset: full JSON replacement plus the
    /// float `sort_order` (explicit null 400s at the caller, before
    /// the write).
    #[allow(clippy::too_many_arguments)]
    pub async fn reset_props<'e, E>(
        ex: E,
        id: Uuid,
        view_props: &serde_json::Value,
        default_props: &serde_json::Value,
        preferences: &serde_json::Value,
        sort_order: f64,
        now: DateTime<Utc>,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(RESET_PROPS_SQL)
            .bind(view_props)
            .bind(default_props)
            .bind(preferences)
            .bind(sort_order)
            .bind(now)
            .bind(actor_id)
            .bind(id)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const SAVE_PREFERENCES_SQL: &str = "UPDATE project_members SET preferences = $1, \
        updated_at = $2, updated_by_id = $3 WHERE id = $4";

    /// The app member-preferences save: the caller merges the patch
    /// into the stored document; the merged value binds as jsonb.
    pub async fn save_preferences<'e, E>(
        ex: E,
        id: Uuid,
        merged: &serde_json::Value,
        now: DateTime<Utc>,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(SAVE_PREFERENCES_SQL)
            .bind(merged)
            .bind(now)
            .bind(user_id)
            .bind(id)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const DEACTIVATE_BY_ID_SQL: &str = "UPDATE project_members SET is_active = false, \
        updated_at = $1, updated_by_id = $2 WHERE id = $3";

    /// The app member destroy/leave: bound stamps, row-addressed.
    pub async fn deactivate_by_id<'e, E>(
        ex: E,
        id: Uuid,
        now: DateTime<Utc>,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(DEACTIVATE_BY_ID_SQL)
            .bind(now)
            .bind(user_id)
            .bind(id)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const DEACTIVATE_ACTOR_FIRST_SQL: &str =
        "UPDATE project_members SET is_active = false, \
        updated_by_id = $1, updated_at = now() WHERE id = $2";

    /// The v1 member deactivate: DB-stamped, actor-first binds.
    pub async fn deactivate_actor_first<'e, E>(
        ex: E,
        id: Uuid,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(DEACTIVATE_ACTOR_FIRST_SQL)
            .bind(actor_id)
            .bind(id)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// The user-deactivate chunk size (`batch_size=100`).
    pub(crate) const BULK_DEACTIVATE_CHUNK: usize = 100;

    /// Render one bulk-deactivate chunk: pk-direct, no `updated_at`,
    /// no `deleted_at` filter (bug 5). Pure; unit-tested.
    pub(crate) fn deactivate_many_sql(ids: usize) -> String {
        let placeholders: Vec<String> = (1..=ids).map(|n| format!("${n}")).collect();
        format!(
            "UPDATE project_members SET is_active = FALSE WHERE project_members.id IN ({})",
            placeholders.join(", ")
        )
    }

    /// The user-deactivate bulk: chunked at 100, empty chunks issue
    /// no statement.
    pub async fn deactivate_many<'e, E>(ex: E, ids: &[Uuid]) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        for chunk in ids.chunks(BULK_DEACTIVATE_CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let sql = deactivate_many_sql(chunk.len());
            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(*id);
            }
            query.execute(ex).await?;
        }
        Ok(())
    }

    pub(crate) const DEMOTE_CASCADE_SQL: &str = "UPDATE project_members SET role = 5 \
        WHERE project_members.workspace_id = (SELECT id FROM workspaces WHERE slug = $1) \
        AND project_members.member_id = $2 AND project_members.deleted_at IS NULL";

    /// The R4 guest-demote cascade: `role = 5` over the target's
    /// project rows in the workspace. Ported bug B6: no `is_active`
    /// filter — inactive rows are rewritten too.
    pub async fn demote_cascade<'e, E>(
        ex: E,
        slug: &str,
        target_user_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(DEMOTE_CASCADE_SQL)
            .bind(slug)
            .bind(target_user_id)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const CASCADE_DEACTIVATE_WS_SQL: &str =
        "UPDATE project_members SET is_active = FALSE, \
        updated_at = $3 WHERE project_members.workspace_id = \
        (SELECT id FROM workspaces WHERE slug = $1) AND project_members.member_id = $2 \
        AND project_members.is_active = TRUE AND project_members.deleted_at IS NULL";

    /// The R5/R6 project deactivation: the queryset `update()` stamps
    /// `updated_at` only (no `updated_by` — unlike the member
    /// `.save()` beside it).
    pub async fn cascade_deactivate_ws<'e, E>(
        ex: E,
        slug: &str,
        target_user_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(CASCADE_DEACTIVATE_WS_SQL)
            .bind(slug)
            .bind(target_user_id)
            .bind(now)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const REACTIVATE_SLUG_SQL: &str = "UPDATE project_members pm SET is_active = true \
        FROM workspaces w WHERE pm.workspace_id = w.id AND w.slug = $1 \
        AND pm.project_id = ANY($2) AND pm.member_id = $3 AND pm.deleted_at IS NULL";

    /// The invite-join reactivation: `QuerySet.update` stamps no
    /// `updated_at`.
    pub async fn reactivate_slug<'e, E>(
        ex: E,
        slug: &str,
        project_ids: &[Uuid],
        user_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(REACTIVATE_SLUG_SQL)
            .bind(slug)
            .bind(project_ids)
            .bind(user_id)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const REACTIVATE_BY_ID_SQL: &str =
        "UPDATE project_members SET is_active = true, updated_at = now(), updated_by_id = $1 \
         WHERE id = $2";

    /// The join-accept reactivation (`project_member.role =
    /// project_member.role`: the role re-saves unchanged — the
    /// statement touches only the activity columns). The stamp is
    /// `None` when the path has no actor (a bare `NULL`, like the
    /// original bind).
    pub async fn reactivate_by_id<'e, E>(
        ex: E,
        id: Uuid,
        updated_by: Option<Uuid>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(REACTIVATE_BY_ID_SQL)
            .bind(updated_by)
            .bind(id)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// The overlay bulk-deactivate texts verbatim (D6 gap
    /// PIDASHCONV-1617): the workspace scope and the
    /// workspace+member scope of `_deactivate_local_access`.
    pub(crate) const DEACTIVATE_BY_WORKSPACE_SQL: &str = "UPDATE project_members \
        SET is_active = FALSE \
        WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE";

    pub(crate) const DEACTIVATE_BY_WORKSPACE_MEMBER_SQL: &str = "UPDATE project_members \
        SET is_active = FALSE \
        WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE AND member_id = $2";

    /// The platform bulk deactivation (`pi_dash_cloud` `service.py:480-489`):
    /// `ProjectMember.objects.filter(workspace, is_active=True)[.filter(member)]
    /// .update(is_active=False)`. A queryset `update()`, so `SET` only — no
    /// `updated_at` stamp and no `updated_by` (unlike the id-addressed
    /// deactivations above, and unlike [`cascade_deactivate_ws`], which
    /// scopes by slug and stamps `updated_at`); the soft-delete scope
    /// comes from the default manager. Runs on the caller's executor
    /// (the overlay holds its own transaction).
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

    /// The overlay role-sync text verbatim (D6 gap PIDASHCONV-1617).
    pub(crate) const SYNC_ACTIVE_ROLE_SQL: &str = "UPDATE project_members \
        SET role = $1 \
        WHERE workspace_id = $2 AND deleted_at IS NULL AND member_id = $3 AND is_active = TRUE";

    /// The active-member project role sync (`pi_dash_cloud`
    /// `service.py:622-625`): `ProjectMember.objects.filter(workspace,
    /// member, is_active=True).update(role=role)`. A queryset
    /// `update()`, so `SET role` only — no `updated_at` stamp and no
    /// `is_active` flip (unlike [`ops_update_role`], which also
    /// activates and scopes by project). The role binds `int2`, as
    /// the overlay does. Runs on the caller's executor.
    pub async fn sync_active_role<'e, E>(
        ex: E,
        workspace_id: Uuid,
        user_id: Uuid,
        role: i16,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(SYNC_ACTIVE_ROLE_SQL)
            .bind(role)
            .bind(workspace_id)
            .bind(user_id)
            .execute(ex)
            .await
            .map(|_| ())
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

    pub(crate) const SEED_MEMBER_INSERT_SQL: &str = "INSERT INTO project_members \
        (created_at, updated_at, id, comment, role, created_by_id, member_id, project_id, \
        updated_by_id, workspace_id, view_props, default_props, sort_order, preferences, \
        is_active, deleted_at) \
        VALUES ($1, $1, $2, NULL, $3, $4, $5, $6, NULL, $7, $8, $8, 65535, $9, true, NULL) \
        RETURNING id";

    /// The seed member-row insert. Returns the new row id.
    pub async fn add_seed<'e, E>(ex: E, member: &SeedProjectMember<'_>) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id: Uuid = sqlx::query_scalar(SEED_MEMBER_INSERT_SQL)
            .bind(member.now)
            .bind(Uuid::new_v4())
            .bind(member.role)
            .bind(member.created_by)
            .bind(member.member_id)
            .bind(member.project_id)
            .bind(member.workspace_id)
            .bind(sqlx::types::Json(member.view_props))
            .bind(sqlx::types::Json(member.preferences))
            .fetch_one(ex)
            .await?;
        Ok(id)
    }

    /// One dummy-data project-member row: the `create_project`
    /// creator row (`ProjectMember.objects.create`, NULL audit) and
    /// the `create_project_members` loop rows (`:63-79` in
    /// `bgtasks/dummy_data_task.py`: role 20, random sort,
    /// `bulk_create(..., ignore_conflicts=True)` replayed one row at
    /// a time). Ids are the task's `::uuid` strings. Moved verbatim
    /// from the dummy store (PIDASHCONV-1632).
    pub struct DemoProjectMember<'a> {
        pub project_id: &'a str,
        pub workspace_id: &'a str,
        pub member_id: &'a str,
        pub role: i32,
        pub sort_order: f64,
        pub view_props: &'a serde_json::Value,
        pub preferences: &'a serde_json::Value,
        pub now: DateTime<Utc>,
        /// Per-row `ON CONFLICT DO NOTHING` (the members loop only).
        pub ignore_conflicts: bool,
    }

    pub(crate) const DEMO_MEMBER_INSERT_SQL: &str = "INSERT INTO project_members \
        (id, project_id, workspace_id, member_id, role, created_at, updated_at, view_props, \
        default_props, preferences, sort_order, is_active) \
        VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6, $6, $7, $7, $8, 65535, TRUE)";

    pub(crate) const DEMO_MEMBER_INSERT_IGNORE_SQL: &str = "INSERT INTO project_members \
        (id, project_id, workspace_id, member_id, role, sort_order, created_at, updated_at, \
        view_props, default_props, preferences, is_active) \
        VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6, $7, $7, $8, $8, $9, TRUE) \
        ON CONFLICT DO NOTHING";

    /// The dummy-data member-row insert. The id is generated
    /// client-side (as Django does); the row id is never read back.
    pub async fn add_demo<'e, E>(ex: E, member: &DemoProjectMember<'_>) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4().to_string();
        if member.ignore_conflicts {
            sqlx::query(DEMO_MEMBER_INSERT_IGNORE_SQL)
                .bind(&id)
                .bind(member.project_id)
                .bind(member.workspace_id)
                .bind(member.member_id)
                .bind(member.role)
                .bind(member.sort_order)
                .bind(member.now)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.preferences))
                .execute(ex)
                .await?;
        } else {
            sqlx::query(DEMO_MEMBER_INSERT_SQL)
                .bind(&id)
                .bind(member.project_id)
                .bind(member.workspace_id)
                .bind(member.member_id)
                .bind(member.role)
                .bind(member.now)
                .bind(sqlx::types::Json(member.view_props))
                .bind(sqlx::types::Json(member.preferences))
                .execute(ex)
                .await?;
        }
        Ok(())
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

    /// One state row: every column without a DB default. The app
    /// endpoint passes `is_triage == false`, no externals and no
    /// `updated_by` (its text carries those as literals); the v1
    /// endpoint binds them.
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
        /// The v1 text: audit-first columns, every value bound.
        pub v1: bool,
    }

    pub(crate) const STATE_INSERT_SQL: &str = "INSERT INTO states \
        (id, project_id, workspace_id, name, description, color, slug, sequence, \
        \"group\", is_triage, \"default\", external_source, external_id, \
        created_at, updated_at, created_by_id, updated_by_id, deleted_at) \
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, FALSE, $10, NULL, NULL, $11, $11, $12, NULL, NULL)";

    pub(crate) const STATE_INSERT_V1_SQL: &str = "INSERT INTO states \
        (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
        project_id, workspace_id, name, description, color, slug, sequence, \
        \"group\", is_triage, \"default\", external_source, external_id) \
        VALUES ($1,$2,$3,$4,$5,NULL,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)";

    /// `State.objects.create(...)` (both state endpoints). Returns
    /// the new row id; conflict arms stay at the callers (app dup
    /// 400 vs v1 holder-echo 409).
    pub async fn create<'e, E>(ex: E, state: &NewState<'_>) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        if state.v1 {
            sqlx::query(STATE_INSERT_V1_SQL)
                .bind(id)
                .bind(state.now)
                .bind(state.now)
                .bind(state.created_by)
                .bind(state.updated_by)
                .bind(state.project_id)
                .bind(state.workspace_id)
                .bind(state.name)
                .bind(state.description)
                .bind(state.color)
                .bind(state.slug)
                .bind(state.sequence)
                .bind(state.group)
                .bind(state.is_triage)
                .bind(state.default)
                .bind(state.external_source)
                .bind(state.external_id)
                .execute(ex)
                .await?;
            return Ok(id);
        }
        sqlx::query(STATE_INSERT_SQL)
            .bind(id)
            .bind(state.project_id)
            .bind(state.workspace_id)
            .bind(state.name)
            .bind(state.description)
            .bind(state.color)
            .bind(state.slug)
            .bind(state.sequence)
            .bind(state.group)
            .bind(state.default)
            .bind(state.now)
            .bind(state.created_by)
            .execute(ex)
            .await?;
        Ok(id)
    }

    /// The default-flip side effect (`state.py:21-22`):
    /// `State.objects.filter(project_id=...).update(default=False)`.
    ///
    /// The queryset starts from `StateManager.get_queryset`, which is the
    /// soft-delete filter (`deleted_at IS NULL`, `db/mixins.py:49-51`) plus
    /// the triage exclusion (`db/models/state.py:82-84`) — so triage and
    /// soft-deleted siblings keep their `default`. Django quotes the
    /// identifiers (`"default"` is reserved); `$1` is the project id.
    /// Moved verbatim from `v1_projects::ser_workflow`
    /// (PIDASHCONV-1630).
    pub(crate) const STATE_DEFAULT_FLIP_SQL: &str = concat!(
        "UPDATE \"states\" SET \"default\" = false ",
        "WHERE \"project_id\" = $1 ",
        "AND \"deleted_at\" IS NULL ",
        "AND NOT (\"group\" = 'triage')"
    );

    /// Clear `default` on every sibling state in the project
    /// (soft-deleted and triage rows keep theirs, via the
    /// default-manager scope in [`STATE_DEFAULT_FLIP_SQL`]).
    /// Unscoped by workspace slug, unlike the `mark_as_default`
    /// clear leg ([`clear_defaults`]) — the v1 flip has no slug.
    pub async fn clear_project_defaults<'e, E>(ex: E, project_id: Uuid) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(STATE_DEFAULT_FLIP_SQL)
            .bind(project_id)
            .execute(ex)
            .await?;
        Ok(())
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
        /// The v1 text: short quoted column list, `slug` bound.
        pub v1: bool,
    }

    pub(crate) const DEFAULT_STATE_INSERT_SQL: &str = "INSERT INTO states \
        (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
        project_id, workspace_id, name, description, color, slug, sequence, \
        \"group\", is_triage, \"default\", external_source, external_id) \
        VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, '', $7, '', $8, $9, FALSE, $10, NULL, NULL)";

    pub(crate) const DEFAULT_STATE_INSERT_V1_SQL: &str = "INSERT INTO \"states\" (\"id\", \"created_at\", \
        \"updated_at\", \"created_by_id\", \"project_id\", \"workspace_id\", \"name\", \"description\", \
        \"color\", \"slug\", \"sequence\", \"group\", \"is_triage\", \"default\") \
        VALUES ($1,$2,$3,$4,$5,$6,$7,'',$8,$9,$10,$11,FALSE,$12)";

    /// One default-state row (both create flows loop this over
    /// `DEFAULT_STATES`).
    pub async fn add_default<'e, E>(ex: E, state: &NewDefaultState<'_>) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        if state.v1 {
            sqlx::query(DEFAULT_STATE_INSERT_V1_SQL)
                .bind(id)
                .bind(state.now)
                .bind(state.now)
                .bind(state.created_by)
                .bind(state.project_id)
                .bind(state.workspace_id)
                .bind(state.name)
                .bind(state.color)
                .bind("")
                .bind(state.sequence)
                .bind(state.group)
                .bind(state.default)
                .execute(ex)
                .await?;
            return Ok(id);
        }
        sqlx::query(DEFAULT_STATE_INSERT_SQL)
            .bind(id)
            .bind(state.now)
            .bind(state.created_by)
            .bind(state.project_id)
            .bind(state.workspace_id)
            .bind(state.name)
            .bind(state.color)
            .bind(state.sequence)
            .bind(state.group)
            .bind(state.default)
            .execute(ex)
            .await?;
        Ok(id)
    }

    pub(crate) const TRIAGE_INSERT_SQL: &str = "INSERT INTO \"states\" (id, created_at, updated_at, \
        created_by_id, name, description, slug, color, project_id, workspace_id, sequence, \
        \"group\", \"default\", is_triage) \
        VALUES ($1, $2, $3, $4, 'Triage', '', 'triage', '#4E5355', $5, $6, $7, 'triage', false, false)";

    pub(crate) const TRIAGE_INSERT_QUOTED_SQL: &str = "INSERT INTO \"states\" (\"id\", \"created_at\", \
        \"updated_at\", \"created_by_id\", \"name\", \"description\", \"slug\", \"color\", \"project_id\", \
        \"workspace_id\", \"sequence\", \"group\", \"default\", \"is_triage\") \
        VALUES ($1, $2, $3, $4, 'Triage', '', 'triage', '#4E5355', $5, $6, $7, 'triage', false, false)";

    pub(crate) const TRIAGE_INSERT_FULL_SQL: &str =
        "INSERT INTO \"states\" (\"id\", \"created_at\", \
        \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \
        \"workspace_id\", \"name\", \"description\", \"color\", \"slug\", \"sequence\", \"group\", \
        \"is_triage\", \"default\", \"external_source\", \"external_id\") \
        VALUES ($1, $2, $3, $6, NULL, NULL, $4, $5, 'Triage', '', '#4E5355', 'triage', $7, \
        'triage', FALSE, FALSE, NULL, NULL)";

    /// Which triage text to emit: the short column list (space
    /// intake), the fully-quoted short list (app intake), or the
    /// full ORM column list (v1 intake). All three store the same
    /// row.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TriageText {
        Short,
        Quoted,
        Full,
    }

    /// The intake triage-state insert (`MAX(sequence) + 15000`, else
    /// `65000`, resolved by the caller). Stamps are per-leg: the v1
    /// path samples twice (microseconds apart), the others pass one
    /// instant twice. Returns the new row id; conflict arms stay at
    /// the callers.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_triage<'e, E>(
        ex: E,
        project_id: Uuid,
        workspace_id: Uuid,
        sequence: f64,
        created_by: Uuid,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        style: TriageText,
    ) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = Uuid::new_v4();
        let sql = match style {
            TriageText::Short => TRIAGE_INSERT_SQL,
            TriageText::Quoted => TRIAGE_INSERT_QUOTED_SQL,
            TriageText::Full => TRIAGE_INSERT_FULL_SQL,
        };
        if style == TriageText::Full {
            sqlx::query(sql)
                .bind(id)
                .bind(created_at)
                .bind(updated_at)
                .bind(project_id)
                .bind(workspace_id)
                .bind(created_by)
                .bind(sequence)
                .execute(ex)
                .await?;
            return Ok(id);
        }
        sqlx::query(sql)
            .bind(id)
            .bind(created_at)
            .bind(updated_at)
            .bind(created_by)
            .bind(project_id)
            .bind(workspace_id)
            .bind(sequence)
            .execute(ex)
            .await?;
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

    pub(crate) const STATE_UPDATE_SQL: &str = "UPDATE states SET updated_at = $1, updated_by_id = $2, name = $3, description = $4, color = $5, slug = $6, sequence = $7, \"group\" = $8, is_triage = $9, \"default\" = $10, external_source = $11, external_id = $12 WHERE id = $13";

    /// See [`SaveState`]: the state `UPDATE` only.
    pub async fn save<'e, E>(ex: E, v: &SaveState<'_>) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(STATE_UPDATE_SQL)
            .bind(v.updated_at)
            .bind(v.updated_by)
            .bind(v.name)
            .bind(v.description)
            .bind(v.color)
            .bind(v.slug)
            .bind(v.sequence)
            .bind(v.group)
            .bind(v.is_triage)
            .bind(v.default)
            .bind(v.external_source)
            .bind(v.external_id)
            .bind(v.id)
            .execute(ex)
            .await?;
        Ok(())
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

    pub(crate) const SEED_STATE_INSERT_SQL: &str = "INSERT INTO states \
        (created_at, updated_at, id, name, description, color, slug, created_by_id, project_id, \
        updated_by_id, workspace_id, sequence, \"group\", \"default\", external_id, \
        external_source, is_triage, deleted_at) \
        VALUES ($1, $1, $2, $3, '', $4, $5, $6, $7, NULL, $8, $9, $10, $11, NULL, NULL, \
        false, NULL) \
        RETURNING id";

    /// The seed state-row insert. Returns the new row id.
    pub async fn add_seed<'e, E>(ex: E, state: &SeedState<'_>) -> Result<Uuid, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id: Uuid = sqlx::query_scalar(SEED_STATE_INSERT_SQL)
            .bind(state.now)
            .bind(Uuid::new_v4())
            .bind(state.name)
            .bind(state.color)
            .bind(state.slug)
            .bind(state.created_by)
            .bind(state.project_id)
            .bind(state.workspace_id)
            .bind(state.sequence)
            .bind(state.group)
            .bind(state.is_default)
            .fetch_one(ex)
            .await?;
        Ok(id)
    }

    /// One dummy-data state row: `create_states (:82-123)` in
    /// `bgtasks/dummy_data_task.py` — the five-row literal, no
    /// conflict clause. Ids are the task's `::uuid` strings. Moved
    /// verbatim from the dummy store (PIDASHCONV-1632).
    pub struct DemoState<'a> {
        pub project_id: &'a str,
        pub workspace_id: &'a str,
        pub created_by: &'a str,
        pub name: &'a str,
        pub color: &'a str,
        pub sequence: f64,
        pub group: &'a str,
        pub default: bool,
        pub now: DateTime<Utc>,
    }

    pub(crate) const DEMO_STATE_INSERT_SQL: &str = "INSERT INTO states \
        (id, name, color, project_id, sequence, workspace_id, \"group\", \"default\", \
        created_by_id, created_at, updated_at, description, slug, is_triage) \
        VALUES ($1::uuid, $2, $3, $4::uuid, $5, $6::uuid, $7, $8, $9::uuid, $10, $10, '', \
        '', FALSE)";

    /// The dummy-data state-row insert.
    pub async fn add_demo<'e, E>(ex: E, state: &DemoState<'_>) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(DEMO_STATE_INSERT_SQL)
            .bind(Uuid::new_v4().to_string())
            .bind(state.name)
            .bind(state.color)
            .bind(state.project_id)
            .bind(state.sequence)
            .bind(state.workspace_id)
            .bind(state.group)
            .bind(state.default)
            .bind(state.created_by)
            .bind(state.now)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// `DELETE states/<state_id>/` (`views/state.py:225-249`,
    /// `SoftDeleteModel.delete()` → `save()`): `deleted_at` plus the
    /// `save()` stamps (`updated_at`, CRUM `updated_by`). The
    /// related-objects sweep stays driver-side.
    pub async fn soft_delete<'e, E>(
        ex: E,
        id: Uuid,
        updated_by: Uuid,
        deleted_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(STATE_SOFT_DELETE_SQL)
            .bind(deleted_at)
            .bind(updated_by)
            .bind(id)
            .execute(ex)
            .await?;
        Ok(())
    }

    pub(crate) const STATE_SOFT_DELETE_SQL: &str =
        "UPDATE states SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3";
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

    pub(crate) const STATE_APP_UPDATE_SQL: &str =
        "UPDATE states SET name = $1, color = $2, \"group\" = $3, \"default\" = $4,
                  description = $5, sequence = $6, slug = $7,
                  updated_at = $8, updated_by_id = $9 WHERE id = $10";

    /// Patch one state row. Errors propagate as `sqlx::Error` — the
    /// caller owns the 23505 duplicate-name arm.
    pub async fn update<'e, E>(
        ex: E,
        state_id: Uuid,
        patch: &StatePatch<'_>,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(STATE_APP_UPDATE_SQL)
            .bind(patch.name)
            .bind(patch.color)
            .bind(patch.group)
            .bind(patch.default)
            .bind(patch.description)
            .bind(patch.sequence)
            .bind(patch.slug)
            .bind(patch.updated_at)
            .bind(patch.updated_by)
            .bind(state_id)
            .execute(ex)
            .await
            .map(|_| ())
    }

    /// `StateViewSet.mark_as_default` (`state/base.py:111-119`): the
    /// clear leg — plain `.update()`, no `updated_at`/`updated_by`
    /// touch (queryset updates bypass `save()`).
    pub(crate) const STATE_CLEAR_DEFAULTS_SQL: &str =
        "UPDATE states s SET \"default\" = FALSE FROM workspaces w
           WHERE s.workspace_id = w.id AND w.slug = $1 AND s.project_id = $2
             AND s.\"default\" AND s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage')";

    /// Clear every live non-triage default in the project.
    pub async fn clear_defaults<'e, E>(
        ex: E,
        slug: &str,
        project_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(STATE_CLEAR_DEFAULTS_SQL)
            .bind(slug)
            .bind(project_id)
            .execute(ex)
            .await
            .map(|_| ())
    }

    /// `StateViewSet.mark_as_default` (`state/base.py:111-119`): the
    /// set leg — same scoping plus the `pk`.
    pub(crate) const STATE_MARK_DEFAULT_SQL: &str =
        "UPDATE states s SET \"default\" = TRUE FROM workspaces w
           WHERE s.workspace_id = w.id AND w.slug = $1 AND s.project_id = $2 AND s.id = $3
             AND s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage')";

    /// Mark one live non-triage state the default.
    pub async fn mark_default<'e, E>(
        ex: E,
        slug: &str,
        project_id: Uuid,
        state_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(STATE_MARK_DEFAULT_SQL)
            .bind(slug)
            .bind(project_id)
            .bind(state_id)
            .execute(ex)
            .await
            .map(|_| ())
    }

    /// `StateViewSet.destroy` (`state/base.py:121-144`):
    /// `SoftDeleteModel.delete(soft=True)` — tombstone plus the
    /// `save()` stamps. The veto probes and the
    /// `soft_delete_related_objects` publish stay at the caller.
    pub(crate) const STATE_APP_SOFT_DELETE_SQL: &str =
        "UPDATE states SET deleted_at = $1, updated_at = $1, updated_by_id = $2
           WHERE id = $3";

    /// Soft-delete one state row.
    pub async fn soft_delete_app<'e, E>(
        ex: E,
        state_id: Uuid,
        now: DateTime<Utc>,
        actor_id: Uuid,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(STATE_APP_SOFT_DELETE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(state_id)
            .execute(ex)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::members::*;
    use super::states::{
        add_default, add_triage, NewDefaultState, NewState, TriageText, DEFAULT_STATE_INSERT_SQL,
        DEFAULT_STATE_INSERT_V1_SQL, STATE_APP_SOFT_DELETE_SQL, STATE_APP_UPDATE_SQL,
        STATE_CLEAR_DEFAULTS_SQL, STATE_INSERT_SQL, STATE_INSERT_V1_SQL, STATE_MARK_DEFAULT_SQL,
        TRIAGE_INSERT_FULL_SQL, TRIAGE_INSERT_QUOTED_SQL, TRIAGE_INSERT_SQL,
    };
    use super::*;

    #[test]
    fn v1_project_and_state_update_sql_shapes() {
        // Moved verbatim from the retired v1 project/state drivers and
        // the two `apply_bind` copies. Full-text pins: the statements
        // moved byte-for-byte.
        assert_eq!(
            ARCHIVE_PROJECT_SQL,
            "UPDATE \"projects\" SET \"archived_at\" = $1, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3 AND \"workspace_id\" = $4 AND \"deleted_at\" IS NULL"
        );
        assert_eq!(
            UNARCHIVE_PROJECT_SQL,
            "UPDATE \"projects\" SET \"archived_at\" = NULL, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3 AND \"workspace_id\" = $4 AND \"deleted_at\" IS NULL"
        );
        assert_eq!(
            UNSET_OTHER_DEFAULTS_SQL,
            "UPDATE \"projects\" SET \"is_default\" = FALSE WHERE \"workspace_id\" = $1 AND \"is_default\" AND \"id\" != $2 AND \"deleted_at\" IS NULL"
        );
        assert_eq!(
            SOFT_DELETE_PROJECT_SQL,
            "UPDATE \"projects\" SET \"deleted_at\" = $1, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3"
        );
        assert_eq!(
            super::states::STATE_UPDATE_SQL,
            "UPDATE states SET updated_at = $1, updated_by_id = $2, name = $3, description = $4, color = $5, slug = $6, sequence = $7, \"group\" = $8, is_triage = $9, \"default\" = $10, external_source = $11, external_id = $12 WHERE id = $13"
        );
        assert_eq!(
            super::states::STATE_SOFT_DELETE_SQL,
            "UPDATE states SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3"
        );
        // The PATCH shell: the driver-built SET list, `$N` the id.
        assert_eq!(
            project_patch_sql(
                &[
                    "\"updated_at\" = $1".to_owned(),
                    "\"updated_by_id\" = $2".to_owned(),
                    "\"name\" = $3".to_owned(),
                ],
                1,
            ),
            "UPDATE \"projects\" SET \"updated_at\" = $1, \"updated_by_id\" = $2, \"name\" = $3 WHERE \"id\" = $4 AND \"deleted_at\" IS NULL"
        );
        // The bind shell: sequential `$N`, every referenced
        // placeholder bound; unchanged is no statement.
        assert_eq!(bind_project_update_sql(false, false), None);
        assert_eq!(
            bind_project_update_sql(true, false).as_deref(),
            Some("UPDATE projects SET repo_url = $2, updated_at = $3 WHERE id = $1")
        );
        assert_eq!(
            bind_project_update_sql(false, true).as_deref(),
            Some("UPDATE projects SET base_branch = $2, updated_at = $3 WHERE id = $1")
        );
        assert_eq!(
            bind_project_update_sql(true, true).as_deref(),
            Some("UPDATE projects SET repo_url = $2, base_branch = $3, updated_at = $4 WHERE id = $1")
        );
    }

    #[test]
    fn bind_project_update_binds_every_placeholder() {
        // (repo_url, base_branch, binds incl. id + updated_at).
        // Catches both a skipped `$N` below the max and a reference
        // past the binds (PIDASHCONV-1642).
        for (repo, branch, binds) in [(true, false, 3), (false, true, 3), (true, true, 4)] {
            let sql = bind_project_update_sql(repo, branch).expect("arm builds");
            for n in 1..=binds {
                assert!(sql.contains(&format!("${n}")), "{sql} binds ${n}");
            }
            assert!(
                !sql.contains(&format!("${}", binds + 1)),
                "{sql} references unbound ${}",
                binds + 1
            );
        }
    }

    /// All three bind arms execute (PIDASHCONV-1642): each writes its
    /// columns plus `updated_at` and leaves the other column alone;
    /// the no-change arm runs no statement at all.
    #[tokio::test]
    async fn live_bind_project_update_executes_all_arms() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
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
        save_bind_project_update(&pool, id, Some("https://example.test/new.git"), None, t1)
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
        save_bind_project_update(&pool, id, None, Some("main"), t1)
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
            &pool,
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
        save_bind_project_update(&pool, id, None, None, t0)
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

    #[test]
    fn project_inserts_bind_every_placeholder() {
        for bind in 1..=44 {
            assert!(
                PROJECT_INSERT_SQL.contains(&format!("${bind}")),
                "bind ${bind} present"
            );
        }
        for bind in 1..=32 {
            assert!(
                PROJECT_INSERT_V1_SQL.contains(&format!("${bind}")),
                "v1 bind ${bind} present"
            );
        }
        for literal in ["NULL,NULL,2", "10800,10,10800,10800,TRUE"] {
            assert!(
                PROJECT_INSERT_V1_SQL.contains(literal),
                "v1 literal {literal} present"
            );
        }
        assert!(
            CLEAR_DEFAULTS_SQL.contains("is_default = FALSE"),
            "clear flips defaults"
        );
    }

    #[test]
    fn project_update_binds_every_placeholder() {
        for bind in 1..=45 {
            assert!(
                PROJECT_UPDATE_SQL.contains(&format!("${bind}")),
                "bind ${bind} present"
            );
        }
        assert!(PROJECT_UPDATE_SQL.ends_with("WHERE id = $45"));
        assert!(PROJECT_UPDATE_SQL.contains("created_by_id = $44"));
        assert_eq!(
            CLEAR_OTHER_DEFAULTS_SQL,
            "UPDATE projects SET is_default = FALSE\n               WHERE workspace_id = $1 AND is_default AND deleted_at IS NULL AND id <> $2"
        );
        assert_eq!(
            PROJECT_SOFT_DELETE_SQL,
            "UPDATE projects SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3"
        );
        assert_eq!(
            PROJECT_ARCHIVE_SQL,
            "UPDATE projects SET archived_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3"
        );
        assert_eq!(
            PROJECT_UNARCHIVE_SQL,
            "UPDATE projects SET archived_at = NULL, updated_at = $1, updated_by_id = $2 WHERE id = $3"
        );
        assert_eq!(
            PROJECT_COVER_LINK_SQL,
            "UPDATE \"projects\" SET \"cover_image\" = $1, \"cover_image_asset_id\" = $2, \"updated_at\" = $3, \"updated_by_id\" = $4 WHERE \"id\" = $5"
        );
        assert_eq!(
            PROJECT_COVER_UNLINK_SQL,
            "UPDATE \"projects\" SET \"cover_image_asset_id\" = NULL, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"id\" = $3"
        );
        assert_eq!(
            PROJECT_REPO_URL_SQL,
            "UPDATE projects SET repo_url = $1 WHERE id = $2"
        );
        assert_eq!(
            PROJECT_BASE_BRANCH_SQL,
            "UPDATE projects SET base_branch = $1 WHERE id = $2"
        );
    }

    #[test]
    fn state_update_sql_matches_handler_text() {
        assert_eq!(
            STATE_APP_UPDATE_SQL,
            "UPDATE states SET name = $1, color = $2, \"group\" = $3, \"default\" = $4,\n                  description = $5, sequence = $6, slug = $7,\n                  updated_at = $8, updated_by_id = $9 WHERE id = $10"
        );
        assert_eq!(
            STATE_CLEAR_DEFAULTS_SQL,
            "UPDATE states s SET \"default\" = FALSE FROM workspaces w\n           WHERE s.workspace_id = w.id AND w.slug = $1 AND s.project_id = $2\n             AND s.\"default\" AND s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage')"
        );
        assert_eq!(
            STATE_MARK_DEFAULT_SQL,
            "UPDATE states s SET \"default\" = TRUE FROM workspaces w\n           WHERE s.workspace_id = w.id AND w.slug = $1 AND s.project_id = $2 AND s.id = $3\n             AND s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage')"
        );
        assert_eq!(
            STATE_APP_SOFT_DELETE_SQL,
            "UPDATE states SET deleted_at = $1, updated_at = $1, updated_by_id = $2\n           WHERE id = $3"
        );
    }

    #[test]
    fn member_inserts_share_columns_and_differ_only_in_tail() {
        for column in [
            "\"project_id\"",
            "\"workspace_id\"",
            "\"member_id\"",
            "\"role\"",
            "\"view_props\"",
            "\"default_props\"",
            "\"preferences\"",
        ] {
            for sql in [
                MEMBER_INSERT_SQL,
                MEMBER_INSERT_IGNORE_SQL,
                MEMBER_INSERT_DB_NOW_SQL,
            ] {
                assert!(sql.contains(column), "{column} listed");
            }
        }
        assert!(MEMBER_INSERT_IGNORE_SQL.ends_with("ON CONFLICT DO NOTHING"));
        assert!(!MEMBER_INSERT_SQL.contains("ON CONFLICT"));
        assert!(MEMBER_INSERT_DB_NOW_SQL.contains("now(), now()"));
        assert!(PROPERTY_INSERT_SQL.contains("\"project_user_properties\""));
        assert!(PROPERTY_INSERT_DB_NOW_SQL.contains("now(), now(), NULL)"));
    }

    #[test]
    fn bulk_platform_statements_match_overlay() {
        assert_eq!(
            DEACTIVATE_BY_WORKSPACE_SQL,
            "UPDATE project_members SET is_active = FALSE \
            WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE"
        );
        assert_eq!(
            DEACTIVATE_BY_WORKSPACE_MEMBER_SQL,
            "UPDATE project_members SET is_active = FALSE \
            WHERE workspace_id = $1 AND deleted_at IS NULL AND is_active = TRUE AND member_id = $2"
        );
        assert_eq!(
            SYNC_ACTIVE_ROLE_SQL,
            "UPDATE project_members SET role = $1 \
            WHERE workspace_id = $2 AND deleted_at IS NULL AND member_id = $3 AND is_active = TRUE"
        );
        assert!(!DEACTIVATE_BY_WORKSPACE_SQL.contains("updated_at"));
        assert!(!DEACTIVATE_BY_WORKSPACE_MEMBER_SQL.contains("updated_at"));
        assert!(!SYNC_ACTIVE_ROLE_SQL.contains("updated_at"));
    }

    #[test]
    fn bulk_add_sql_numbers_rows() {
        let one = bulk_add_sql(1);
        assert!(one.starts_with("INSERT INTO \"project_members\""));
        assert!(one.ends_with(" ON CONFLICT DO NOTHING"));
        for bind in 1..=9 {
            assert!(one.contains(&format!("${bind}")), "bind ${bind} present");
        }
        assert!(!one.contains("$10"));
        let two = bulk_add_sql(2);
        assert!(two.contains("$18"));
        assert!(!two.contains("$19"));
        // Shared props bind: view and default ride the same placeholder.
        assert_eq!(two.matches("$8").count() + two.matches("$17").count(), 4);
        let db_now = bulk_add_db_now_sql(2);
        assert!(db_now.contains("now(), now()"));
        assert!(db_now.contains("$14"));
        assert!(!db_now.contains("$15"));
        let props = bulk_add_properties_sql(2);
        assert!(props.starts_with("INSERT INTO \"project_user_properties\""));
        assert!(props.contains("$24"));
        assert!(!props.contains("$25"));
        assert!(props.ends_with(" ON CONFLICT DO NOTHING"));
        let props_db_now = bulk_add_properties_db_now_sql(2);
        assert!(props_db_now.contains("now(), now()"));
        assert!(props_db_now.contains("$20"));
        assert!(!props_db_now.contains("$21"));
    }

    #[test]
    fn update_roles_bulk_sql_matches_django_shape() {
        assert_eq!(
            update_roles_bulk_sql(1),
            "UPDATE project_members SET role = CAST(CASE id WHEN $1 THEN $2 \
             END AS smallint), is_active = CASE id WHEN $3 THEN $4 END WHERE id IN ($5)"
        );
        let two = update_roles_bulk_sql(2);
        assert!(two.contains("WHEN $3 THEN $4"));
        assert!(two.contains("WHEN $7 THEN $8"));
        assert!(two.ends_with("WHERE id IN ($9, $10)"));
    }

    #[test]
    fn patch_member_sql_orders_sets_with_id_last() {
        let patch = MemberPatch {
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
        assert_eq!(
            patch_member_sql(&patch),
            "UPDATE project_members SET updated_at = $1, updated_by_id = $2, deleted_at = $3, \
             comment = $4, role = $5, view_props = $6, default_props = $7, preferences = $8, \
             sort_order = $9, is_active = $10, created_by_id = $11 WHERE id = $12"
        );
        assert_eq!(
            patch_member_sql(&MemberPatch::default()),
            "UPDATE project_members SET updated_at = $1, updated_by_id = $2 WHERE id = $3"
        );
    }

    #[test]
    fn deactivate_many_sql_placeholders() {
        assert_eq!(
            deactivate_many_sql(2),
            "UPDATE project_members SET is_active = FALSE \
             WHERE project_members.id IN ($1, $2)"
        );
        assert_eq!(BULK_DEACTIVATE_CHUNK, 100);
    }

    #[test]
    fn state_inserts_bind_every_placeholder() {
        for bind in 1..=12 {
            assert!(
                STATE_INSERT_SQL.contains(&format!("${bind}")),
                "bind ${bind} present"
            );
        }
        assert!(STATE_INSERT_SQL.contains("FALSE, $10, NULL, NULL"));
        for bind in 1..=17 {
            assert!(
                STATE_INSERT_V1_SQL.contains(&format!("${bind}")),
                "v1 bind ${bind} present"
            );
        }
        for bind in 1..=10 {
            assert!(
                DEFAULT_STATE_INSERT_SQL.contains(&format!("${bind}")),
                "default bind ${bind} present"
            );
        }
        for bind in 1..=12 {
            assert!(
                DEFAULT_STATE_INSERT_V1_SQL.contains(&format!("${bind}")),
                "default v1 bind ${bind} present"
            );
        }
        assert!(TRIAGE_INSERT_SQL.contains("'Triage'"));
        assert!(TRIAGE_INSERT_QUOTED_SQL.contains("\"is_triage\""));
        assert!(TRIAGE_INSERT_FULL_SQL.contains("$7"));
        assert!(TRIAGE_INSERT_FULL_SQL.contains("'#4E5355'"));
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
        v1: bool,
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
            v1,
        }
    }

    /// `create` inserts the row and clears the previous live default
    /// in the same transaction; the v1 arm stores the same values
    /// through the literal-heavy text.
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
        let first = create(&pool, &new_project(ws, actor, now, &logo, false))
            .await
            .expect("first create");
        let second = create(&pool, &new_project(ws, actor, now, &logo, false))
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
        let v1_id = create(&pool, &new_project(ws, actor, now, &logo, true))
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
        sqlx::query(PROJECTS_DDL)
            .execute(&pool)
            .await
            .expect("temp projects");
        let now: DateTime<Utc> = "2026-10-03T12:00:00+00:00".parse().expect("T0");
        let ws = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let logo = serde_json::json!({});
        let id = create(&pool, &new_project(ws, actor, now, &logo, false))
            .await
            .expect("create");
        clear_other_defaults(&pool, ws, id)
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
        apply_update(&pool, id, &upd).await.expect("update");
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
        archive(&pool, id, now, actor).await.expect("archive");
        let stamped: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT archived_at FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(stamped, Some(now));
        unarchive(&pool, id, now, actor).await.expect("unarchive");
        let cleared: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT archived_at FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(cleared, None);
        let asset = Uuid::new_v4();
        stamp_cover_image(&pool, id, asset, now, &actor)
            .await
            .expect("cover link");
        let cover: (Option<String>, Option<Uuid>) =
            sqlx::query_as("SELECT cover_image, cover_image_asset_id FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(cover, (Some(String::new()), Some(asset)));
        clear_cover_image_asset(&pool, id, now, &actor)
            .await
            .expect("cover unlink");
        let unlinked: Option<Uuid> =
            sqlx::query_scalar("SELECT cover_image_asset_id FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(unlinked, None);
        stamp_text_field(
            &pool,
            ProjectTextField::RepoUrl,
            "https://example.test/r2",
            id,
        )
        .await
        .expect("repo stamp");
        stamp_text_field(&pool, ProjectTextField::BaseBranch, "dev", id)
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
        soft_delete(&pool, id, now, actor).await.expect("delete");
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
    /// database clock; a `None` project is the PB-1008 23502.
    #[tokio::test]
    async fn live_member_add_variants() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
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
        let id = add(&pool, &member).await.expect("add");
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
        add(&pool, &ignored).await.expect("ignore replay");
        let db_now = NewProjectMember {
            user_id: Some(Uuid::new_v4()),
            db_now: true,
            ..member_for_replay(&member)
        };
        add(&pool, &db_now).await.expect("db_now add");
        let stamped: DateTime<Utc> =
            sqlx::query_scalar("SELECT created_at FROM project_members WHERE member_id = $1")
                .bind(db_now.user_id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(stamped > now, "db clock stamps, got {stamped}");
        // PB-1008: the missing project binds an explicit NULL into the
        // NOT NULL column — the same 23502 the omitted column raises.
        let null_project = NewProjectMember {
            project_id: None,
            user_id: Some(Uuid::new_v4()),
            ..member_for_replay(&member)
        };
        let err = add(&pool, &null_project)
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
        add_property(&pool, &property).await.expect("property");
        add_property_db_now(&pool, &property)
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
        bulk_add(&pool, &rows, &view, &prefs, false)
            .await
            .expect("bulk");
        bulk_add(&pool, &rows, &view, &prefs, true)
            .await
            .expect("db_now bulk");
        bulk_add(&pool, &[], &view, &prefs, false)
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
        bulk_add_properties(&pool, &prop_rows, false)
            .await
            .expect("bulk properties");
        bulk_add_properties(&pool, &prop_rows, true)
            .await
            .expect("db_now bulk properties");
        bulk_add_properties(&pool, &[], false)
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
        get_or_create_property(&pool, &property)
            .await
            .expect("get_or_create inserts");
        get_or_create_property(&pool, &property)
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
        ops_add(&pool, ws, project, user, Some(15), &view, &prefs, now)
            .await
            .expect("ops add");
        ops_update_role(&pool, project, user, Some(20))
            .await
            .expect("ops role");
        let row: (i16, bool, DateTime<Utc>) =
            sqlx::query_as("SELECT role, is_active, updated_at FROM project_members")
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(row, (20, true, now));
        let err = ops_add(&pool, ws, project, Uuid::new_v4(), None, &view, &prefs, now)
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
            add(&pool, &member).await.expect("seed");
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
        update_roles_bulk(&pool, &[(first_id, RoleBind::Int(20))])
            .await
            .expect("bulk roles");
        update_roles_bulk(&pool, &[])
            .await
            .expect("empty is a no-op");
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
        let err = update_roles_bulk(&pool, &[(second_id, RoleBind::Null)])
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
        patch_member(&pool, first_id, &patch, later, first)
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
        patch_member(&pool, first_id, &MemberPatch::default(), later, first)
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
            add(&pool, &member).await.expect("seed");
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
        deactivate_by_workspace(&pool, ws_a, Some(user_1))
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
        deactivate_by_workspace(&pool, ws_a, None)
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
            add(&pool, &row).await.expect("seed");
        }
        let inactive_id = add(
            &pool,
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
            &pool,
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
        sync_active_role(&pool, ws_a, user, 20)
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
        let id = add(&pool, &member).await.expect("seed");
        let other = Uuid::new_v4();
        update_member_identity(&pool, id, Some(other), Some(20), actor)
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
        reset_props(&pool, id, &reset, &reset, &reset, 12.5, now, actor)
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
        save_preferences(&pool, id, &merged, now, actor)
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
            ids.push(add(&pool, &member).await.expect("seed"));
        }
        deactivate_by_id(&pool, ids[0], now, actor)
            .await
            .expect("deactivate");
        let active: bool =
            sqlx::query_scalar("SELECT is_active FROM project_members WHERE id = $1")
                .bind(ids[0])
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert!(!active);
        reactivate_by_id(&pool, ids[0], Some(actor))
            .await
            .expect("reactivate");
        deactivate_actor_first(&pool, ids[1], actor)
            .await
            .expect("v1 deactivate");
        reactivate_slug(&pool, "acme", &[project, other_project], user)
            .await
            .expect("slug reactivate");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_members WHERE is_active")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 2);
        // Ported bug B6: no `is_active` filter — the inactive row is
        // rewritten too.
        deactivate_by_id(&pool, ids[0], now, actor)
            .await
            .expect("deactivate one");
        demote_cascade(&pool, "acme", user).await.expect("demote");
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
        cascade_deactivate_ws(&pool, "acme", user, now)
            .await
            .expect("ws cascade");
        deactivate_many(&pool, &ids).await.expect("bulk");
        deactivate_many(&pool, &[]).await.expect("empty bulk");
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
            v1: false,
        };
        let app_id = states::create(&pool, &app).await.expect("app state");
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
            v1: true,
            ..state_for_replay(&app)
        };
        states::create(&pool, &v1).await.expect("v1 state");
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
            v1: false,
        };
        add_default(&pool, &default).await.expect("default");
        let v1_default = NewDefaultState {
            v1: true,
            ..default_for_replay(&default)
        };
        add_default(&pool, &v1_default).await.expect("v1 default");
        let defaults: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM states WHERE name = 'Backlog'")
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(defaults, 2);
        add_triage(
            &pool,
            project,
            ws,
            65000.0,
            actor,
            now,
            now,
            TriageText::Short,
        )
        .await
        .expect("triage");
        add_triage(
            &pool,
            project,
            ws,
            80000.0,
            actor,
            now,
            now,
            TriageText::Quoted,
        )
        .await
        .expect("quoted triage");
        let later: DateTime<Utc> = "2026-10-04T12:00:00+00:00".parse().expect("T1");
        add_triage(
            &pool,
            project,
            ws,
            95000.0,
            actor,
            now,
            later,
            TriageText::Full,
        )
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
        states::update(&pool, app_id, &patch)
            .await
            .expect("state patch");
        let renamed: (String, String, f64) =
            sqlx::query_as("SELECT name, slug, sequence FROM states WHERE id = $1")
                .bind(app_id)
                .fetch_one(&pool)
                .await
                .expect("read back");
        assert_eq!(renamed, ("Doing".to_owned(), "doing".to_owned(), 26000.0));
        states::clear_defaults(&pool, "acme", project)
            .await
            .expect("clear defaults");
        let cleared: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM states WHERE \"default\" AND deleted_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(cleared, 0);
        states::mark_default(&pool, "acme", project, app_id)
            .await
            .expect("mark default");
        let marked: bool = sqlx::query_scalar("SELECT \"default\" FROM states WHERE id = $1")
            .bind(app_id)
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert!(marked);
        states::soft_delete_app(&pool, app_id, now, actor)
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
            v1: false,
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
            v1: false,
        }
    }
}
