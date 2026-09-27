//! License / instance-console query units (D-01, stage 3).
//!
//! Ports the five query units named by PIDASHCONV-117, with the exact SQL
//! Django emits (recorded in `rust-api/fixtures/license/queries/*.sql`) and
//! the same row semantics (`*.rows.json`):
//!
//! * `instance_first` — `Instance.objects.first()` (`Meta.ordering =
//!   ("-created_at",)`); call sites `api/views/instance.py:37,178,191`,
//!   `api/views/admin.py:56,72,84`, `api/permissions/instance.py:17`.
//! * `admin_crud` — `InstanceAdmin` create / `filter(instance)` /
//!   `filter(instance, pk).delete()` / `get(user=)` (`api/views/admin.py:56,
//!   64,66,78,85,335,373`; permission gate `api/permissions/instance.py:18`).
//! * `config_patch` — `InstanceConfiguration` `filter(key__in=...)` +
//!   per-row `strip` / `None -> ""` + encrypt-if-flag +
//!   `bulk_update(["value"], batch_size=100)`
//!   (`api/views/configuration.py:44-57`).
//! * `disable_email` — single `UPDATE` with `Case(When(key="ENABLE_SMTP",
//!   then "0"), default "")` over six keys
//!   (`api/views/configuration.py:68-81`).
//! * `workspace_list` — `Count` annotations over `OuterRef("id")` with
//!   `member__is_bot=False, is_active=True`, `select_related("owner")`,
//!   optional `name__icontains` search, `paginate(max_per_page=10,
//!   default_per_page=10)` (`api/views/workspace.py:40-69`).
//!
//! Static statements are string constants; statements with a dynamic arity
//! (`key__in` lists, `bulk_update` batches, the optional search predicate)
//! are builder functions emitting the same text Django's compiler emits,
//! with Postgres `$N` placeholders where Django renders `%s`.
//!
//! SQL execution uses runtime `sqlx::query` (no `query!` macros): there is
//! no build-time database and no `.sqlx` offline cache, and the merged
//! precedent (`api/src/app_issues`) uses runtime queries throughout.
//! Dynamic fragments are assembled directly so the emitted text matches
//! Django's parenthesization exactly.
//!
//! Row types are reused from [`super::models`]; [`WorkspaceListRow`] lives
//! here because the `workspaces` table is owned by the app domain, not by
//! `license/models`.
//!
//! Write routing: callers pick the pool (`Pools::pool_for` / `ScopedWrites`)
//! and pass it (or a transaction) in — every executor is generic over
//! `sqlx::Executor`. `create_admin` takes the new row's `id` from the
//! caller (Django assigns the UUID client-side pre-insert); the `uuid`
//! crate here has no `v4` feature, so generation belongs to the caller.
//!
//! Wiring note: the crate root declares `pub mod license;` and this
//! module's parent declares `pub mod queries;` (foundation changes, tracked
//! separately); this file is new-files-only for this issue.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `role__gte=15` in the permission check is kept exactly
//!   (`permissions/instance.py:18`); `ROLE_CHOICES` only defines `20`, so
//!   `15..20` is a latent wider gate.
//! * `filter(instance, pk).delete()` is a soft delete (`deleted_at = now`);
//!   the view returns 204 even when zero rows matched (no existence check,
//!   `admin.py:83-86`).
//! * `config_patch` ignores unknown request keys (never fetched, never
//!   created) and skips the write entirely when nothing matched.
//! * `disable_email` overwrites the encrypted `EMAIL_HOST_PASSWORD` row with
//!   `""` in the clear and leaves `is_encrypted` untouched; keys with no DB
//!   row are not created.
//! * `select_related("owner")` on the member count is a no-op that still
//!   emits the `INNER JOIN` (the join also comes from `member__is_bot`);
//!   the SQL below reproduces it verbatim.
//! * `bulk_update(["value"])` writes only `value`; `auto_now` timestamps
//!   are intentionally not touched.

use sqlx::postgres::PgRow;
use sqlx::Row;

use super::models::{
    instance::Instance as InstanceRow, instance_admin::InstanceAdmin as InstanceAdminRow,
    instance_configuration::InstanceConfiguration as InstanceConfigurationRow,
};

// ---------------------------------------------------------------------------
// instance_first
// ---------------------------------------------------------------------------

/// `Instance.objects.first()`: newest non-deleted row, `LIMIT 1`
/// (`fixtures/license/queries/instance_first.sql`).
pub const INSTANCE_FIRST_SQL: &str = "SELECT \"instances\".\"created_at\", \"instances\".\"updated_at\", \"instances\".\"created_by_id\", \"instances\".\"updated_by_id\", \"instances\".\"deleted_at\", \"instances\".\"id\", \"instances\".\"instance_name\", \"instances\".\"whitelist_emails\", \"instances\".\"instance_id\", \"instances\".\"current_version\", \"instances\".\"latest_version\", \"instances\".\"edition\", \"instances\".\"domain\", \"instances\".\"last_checked_at\", \"instances\".\"namespace\", \"instances\".\"is_telemetry_enabled\", \"instances\".\"is_support_required\", \"instances\".\"is_setup_done\", \"instances\".\"is_signup_screen_visited\", \"instances\".\"is_verified\", \"instances\".\"is_test\", \"instances\".\"is_current_version_deprecated\" FROM \"instances\" WHERE \"instances\".\"deleted_at\" IS NULL ORDER BY \"instances\".\"created_at\" DESC LIMIT 1";

/// Map one `instances` row (column names, order-independent).
pub fn map_instance_row(row: &PgRow) -> Result<InstanceRow, sqlx::Error> {
    Ok(InstanceRow {
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        id: row.try_get("id")?,
        instance_name: row.try_get("instance_name")?,
        whitelist_emails: row.try_get("whitelist_emails")?,
        instance_id: row.try_get("instance_id")?,
        current_version: row.try_get("current_version")?,
        latest_version: row.try_get("latest_version")?,
        edition: row.try_get("edition")?,
        domain: row.try_get("domain")?,
        last_checked_at: row.try_get("last_checked_at")?,
        namespace: row.try_get("namespace")?,
        is_telemetry_enabled: row.try_get("is_telemetry_enabled")?,
        is_support_required: row.try_get("is_support_required")?,
        is_setup_done: row.try_get("is_setup_done")?,
        is_signup_screen_visited: row.try_get("is_signup_screen_visited")?,
        is_verified: row.try_get("is_verified")?,
        is_test: row.try_get("is_test")?,
        is_current_version_deprecated: row.try_get("is_current_version_deprecated")?,
    })
}

/// Newest non-deleted instance, or `None` (the GET null-instance branch,
/// `instance.py:39-44`).
pub async fn fetch_instance_first<'e, E>(ex: E) -> Result<Option<InstanceRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(INSTANCE_FIRST_SQL).fetch_optional(ex).await?;
    row.map(|r| map_instance_row(&r)).transpose()
}

// ---------------------------------------------------------------------------
// admin_crud
// ---------------------------------------------------------------------------

/// `instance_admins` columns in Django `_meta` order, as `table.column`
/// fragments for the SELECT lists below.
pub const ADMIN_COLUMNS: &[&str] = &[
    "\"instance_admins\".\"created_at\"",
    "\"instance_admins\".\"updated_at\"",
    "\"instance_admins\".\"created_by_id\"",
    "\"instance_admins\".\"updated_by_id\"",
    "\"instance_admins\".\"deleted_at\"",
    "\"instance_admins\".\"id\"",
    "\"instance_admins\".\"user_id\"",
    "\"instance_admins\".\"instance_id\"",
    "\"instance_admins\".\"role\"",
    "\"instance_admins\".\"is_verified\"",
];

/// The `role__gte` threshold in `InstanceAdminPermission`
/// (`permissions/instance.py:18`). Kept as-is; see module docs.
pub const ADMIN_PERMISSION_ROLE_GTE: i32 = 15;

/// Permission check: `filter(role__gte=15, instance, user)`
/// (`fixtures/license/queries/admin_crud.sql` §1). Binds
/// `(instance_id, user_id)`; presence means allowed.
pub fn admin_permission_check_sql() -> String {
    format!(
        "SELECT {} FROM \"instance_admins\" WHERE (\"instance_admins\".\"deleted_at\" IS NULL AND \"instance_admins\".\"instance_id\" = $1 AND \"instance_admins\".\"role\" >= $2 AND \"instance_admins\".\"user_id\" = $3) ORDER BY \"instance_admins\".\"created_at\" DESC LIMIT 1",
        ADMIN_COLUMNS.join(", ")
    )
}

/// POST create: `InstanceAdmin.objects.create(instance, user, role)`
/// (`admin.py:66`, fixture §2). Binds all ten columns in `_meta` order;
/// `updated_by` is `NULL` (Django leaves it unset on insert,
/// `db/models/base.py:36-40`).
pub const ADMIN_INSERT_SQL: &str = "INSERT INTO \"instance_admins\" (\"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"id\", \"user_id\", \"instance_id\", \"role\", \"is_verified\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

/// GET list: `filter(instance=instance)` (`admin.py:78`, fixture §3).
/// Binds `(instance_id)`.
pub fn admin_list_sql() -> String {
    format!(
        "SELECT {} FROM \"instance_admins\" WHERE (\"instance_admins\".\"deleted_at\" IS NULL AND \"instance_admins\".\"instance_id\" = $1) ORDER BY \"instance_admins\".\"created_at\" DESC",
        ADMIN_COLUMNS.join(", ")
    )
}

/// DELETE: `filter(instance, pk).delete()` (`admin.py:85`, fixture §4).
/// Soft delete — sets `deleted_at`; binds `(now, instance_id, id)`.
pub const ADMIN_SOFT_DELETE_SQL: &str = "UPDATE \"instance_admins\" SET \"deleted_at\" = $1 WHERE (\"instance_admins\".\"deleted_at\" IS NULL AND \"instance_admins\".\"instance_id\" = $2 AND \"instance_admins\".\"id\" = $3)";

/// Sign-in gate: `filter(instance=instance, user=user)` (`admin.py:335`;
/// queryset truthiness fetches rows). Binds `(instance_id, user_id)`.
pub fn admin_get_by_instance_user_sql() -> String {
    format!(
        "SELECT {} FROM \"instance_admins\" WHERE (\"instance_admins\".\"deleted_at\" IS NULL AND \"instance_admins\".\"instance_id\" = $1 AND \"instance_admins\".\"user_id\" = $2) ORDER BY \"instance_admins\".\"created_at\" DESC",
        ADMIN_COLUMNS.join(", ")
    )
}

/// Session probe: `filter(user=...).exists()` (`admin.py:373`, fixture
/// shape §1 without the instance/role predicates). Binds `(user_id)`.
pub fn admin_exists_by_user_sql() -> String {
    format!(
        "SELECT {} FROM \"instance_admins\" WHERE (\"instance_admins\".\"deleted_at\" IS NULL AND \"instance_admins\".\"user_id\" = $1) ORDER BY \"instance_admins\".\"created_at\" DESC LIMIT 1",
        ADMIN_COLUMNS.join(", ")
    )
}

/// Map one `instance_admins` row (column names, order-independent).
pub fn map_instance_admin_row(row: &PgRow) -> Result<InstanceAdminRow, sqlx::Error> {
    Ok(InstanceAdminRow {
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        instance_id: row.try_get("instance_id")?,
        role: row.try_get("role")?,
        is_verified: row.try_get("is_verified")?,
    })
}

/// Permission gate: a matching admin row exists.
pub async fn admin_permission_check<'e, E>(
    ex: E,
    instance_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&admin_permission_check_sql())
        .bind(instance_id)
        .bind(ADMIN_PERMISSION_ROLE_GTE)
        .bind(user_id)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

/// Insert one admin row; returns the row as written. `now` binds both
/// `created_at` and `updated_at` (Django calls `timezone.now()` twice;
/// the values are identical for readers).
///
/// `created_by_id` is caller-supplied: `BaseModel.save` fills it from the
/// request user via crum (`db/models/base.py:27-40`), so the authenticated
/// create path (`admin.py:66`) stores the requesting admin while the
/// anonymous signup path (`admin.py:229`) stores `NULL`. It is exposed in
/// `InstanceAdminSerializer` (`fields = "__all__"`), so it must round-trip.
#[allow(clippy::too_many_arguments)]
pub async fn create_instance_admin<'e, E>(
    ex: E,
    id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
    user_id: Option<uuid::Uuid>,
    instance_id: uuid::Uuid,
    role: i32,
    is_verified: bool,
    created_by_id: Option<uuid::Uuid>,
) -> Result<InstanceAdminRow, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(ADMIN_INSERT_SQL)
        .bind(now)
        .bind(now)
        .bind(created_by_id)
        .bind(None::<uuid::Uuid>)
        .bind(None::<chrono::DateTime<chrono::Utc>>)
        .bind(id)
        .bind(user_id)
        .bind(instance_id)
        .bind(role)
        .bind(is_verified)
        .execute(ex)
        .await?;
    Ok(InstanceAdminRow {
        created_at: now,
        updated_at: now,
        created_by_id,
        updated_by_id: None,
        deleted_at: None,
        id,
        user_id,
        instance_id,
        role,
        is_verified,
    })
}

/// All admins of one instance, newest first.
pub async fn list_instance_admins<'e, E>(
    ex: E,
    instance_id: uuid::Uuid,
) -> Result<Vec<InstanceAdminRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&admin_list_sql())
        .bind(instance_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_instance_admin_row).collect()
}

/// Soft-delete one admin row; returns matched rows (the view ignores the
/// count and always answers 204).
pub async fn soft_delete_instance_admin<'e, E>(
    ex: E,
    now: chrono::DateTime<chrono::Utc>,
    instance_id: uuid::Uuid,
    id: uuid::Uuid,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    Ok(sqlx::query(ADMIN_SOFT_DELETE_SQL)
        .bind(now)
        .bind(instance_id)
        .bind(id)
        .execute(ex)
        .await?
        .rows_affected())
}

/// Admin rows for one `(instance, user)` pair, newest first.
pub async fn get_admins_by_instance_user<'e, E>(
    ex: E,
    instance_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Vec<InstanceAdminRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&admin_get_by_instance_user_sql())
        .bind(instance_id)
        .bind(user_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_instance_admin_row).collect()
}

/// Whether any admin row exists for one user (any instance).
pub async fn admin_exists_by_user<'e, E>(ex: E, user_id: uuid::Uuid) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&admin_exists_by_user_sql())
        .bind(user_id)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// config_patch
// ---------------------------------------------------------------------------

/// `instance_configurations` columns in Django `_meta` order, as
/// `table.column` fragments for the read SELECT.
pub const CONFIG_COLUMNS: &[&str] = &[
    "\"instance_configurations\".\"created_at\"",
    "\"instance_configurations\".\"updated_at\"",
    "\"instance_configurations\".\"created_by_id\"",
    "\"instance_configurations\".\"updated_by_id\"",
    "\"instance_configurations\".\"deleted_at\"",
    "\"instance_configurations\".\"id\"",
    "\"instance_configurations\".\"key\"",
    "\"instance_configurations\".\"value\"",
    "\"instance_configurations\".\"category\"",
    "\"instance_configurations\".\"is_encrypted\"",
];

/// `bulk_update(..., ["value"], batch_size=100)` (`configuration.py:57`).
pub const CONFIG_BULK_UPDATE_BATCH_SIZE: usize = 100;

/// Read: `filter(key__in=request.data.keys())` (`configuration.py:44`,
/// fixture `config_patch.sql` §read). Returns `None` for an empty key set:
/// Django raises `EmptyResultSet` there and never hits the database, so the
/// caller returns `[]` without querying — exactly what
/// [`fetch_config_by_keys`] does.
pub fn config_select_by_keys_sql(keys: &[&str]) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let placeholders: Vec<String> = (1..=keys.len()).map(|i| format!("${i}")).collect();
    Some(format!(
        "SELECT {} FROM \"instance_configurations\" WHERE (\"instance_configurations\".\"deleted_at\" IS NULL AND \"instance_configurations\".\"key\" IN ({})) ORDER BY \"instance_configurations\".\"created_at\" DESC",
        CONFIG_COLUMNS.join(", "),
        placeholders.join(", ")
    ))
}

/// Per-row write transform (`configuration.py:48-54`): `""` when the
/// request value is `None`, else `str(raw).strip()`.
///
/// The caller stringifies non-string JSON scalars before calling: Python's
/// `str(True)` is `"True"` while `serde_json` renders `true`, so the
/// coercion belongs to the handler layer and is pinned there, not here.
/// Whitespace stripped is Unicode on both sides (`str.strip` /
/// `str::trim`; a few exotic code points differ and are noted in the PR).
/// Encryption (`encrypt_data` when `is_encrypted`) is applied by the caller
/// through the services layer (`db` must not depend on it upward); this
/// function is the pure pre-encryption step.
pub fn normalize_config_value(raw: Option<&str>) -> String {
    raw.map(|s| s.trim().to_owned()).unwrap_or_default()
}

/// Number of `UPDATE` statements `bulk_update` emits for `total` rows at
/// [`CONFIG_BULK_UPDATE_BATCH_SIZE`] rows per batch.
pub fn config_bulk_update_batches(total: usize) -> usize {
    total.div_ceil(CONFIG_BULK_UPDATE_BATCH_SIZE)
}

/// Write: one `bulk_update` batch as a single `UPDATE ... CASE` statement
/// (fixture `config_patch.sql` §write). `pairs` holds `(id, value)` with
/// the value already normalized (and encrypted when the row's
/// `is_encrypted` flag is set). Returns `None` for an empty batch: the view
/// skips the write when nothing matched (`configuration.py:56`).
pub fn config_bulk_update_sql(pairs: &[(uuid::Uuid, &str)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let mut sql = String::from("UPDATE \"instance_configurations\" SET \"value\" = CASE \"id\"");
    let n = pairs.len();
    for i in 1..=n {
        sql.push_str(&format!(" WHEN ${} THEN ${}", 2 * i - 1, 2 * i));
    }
    // Django binds the `WHERE ... IN` ids as fresh trailing params (ids
    // appear twice in the param list), so numbering continues past the
    // `CASE` pairs instead of reusing their placeholders.
    sql.push_str(" END WHERE \"id\" IN (");
    let ids: Vec<String> = (1..=n).map(|i| format!("${}", 2 * n + i)).collect();
    sql.push_str(&ids.join(", "));
    sql.push(')');
    Some(sql)
}

/// Map one `instance_configurations` row (column names, order-independent).
pub fn map_config_row(row: &PgRow) -> Result<InstanceConfigurationRow, sqlx::Error> {
    Ok(InstanceConfigurationRow {
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        id: row.try_get("id")?,
        key: row.try_get("key")?,
        value: row.try_get("value")?,
        category: row.try_get("category")?,
        is_encrypted: row.try_get("is_encrypted")?,
    })
}

/// Rows for the requested keys (unknown keys silently ignored — they are
/// never fetched and never created). Empty input returns `[]` with no
/// query, mirroring Django's `EmptyResultSet` short-circuit.
pub async fn fetch_config_by_keys<'e, E>(
    ex: E,
    keys: &[&str],
) -> Result<Vec<InstanceConfigurationRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(sql) = config_select_by_keys_sql(keys) else {
        return Ok(Vec::new());
    };
    let mut query = sqlx::query(&sql);
    for key in keys {
        query = query.bind(key);
    }
    let rows: Vec<PgRow> = query.fetch_all(ex).await?;
    rows.iter().map(map_config_row).collect()
}

/// Apply one pre-built batch (`id -> value`): a single `UPDATE ... CASE`.
/// Returns affected rows. Callers split larger inputs with
/// [`config_bulk_update_batches`] (chunks of
/// [`CONFIG_BULK_UPDATE_BATCH_SIZE`]).
pub async fn apply_config_batch<'e, E>(
    ex: E,
    pairs: &[(uuid::Uuid, &str)],
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let Some(sql) = config_bulk_update_sql(pairs) else {
        return Ok(0);
    };
    let mut query = sqlx::query(&sql);
    for (id, value) in pairs {
        query = query.bind(id).bind(value);
    }
    // The trailing `IN` ids are fresh params (see builder docs).
    for (id, _) in pairs {
        query = query.bind(id);
    }
    Ok(query.execute(ex).await?.rows_affected())
}

// ---------------------------------------------------------------------------
// disable_email
// ---------------------------------------------------------------------------

/// The six keys `DisableEmailFeatureEndpoint.delete` resets
/// (`configuration.py:70-77`, fixture order).
pub const DISABLE_EMAIL_KEYS: &[&str] = &[
    "EMAIL_HOST",
    "EMAIL_HOST_USER",
    "EMAIL_HOST_PASSWORD",
    "ENABLE_SMTP",
    "EMAIL_PORT",
    "EMAIL_FROM",
];

/// Single `UPDATE`: `ENABLE_SMTP` becomes `"0"`, the other five become `""`
/// (`configuration.py:79-80`, fixture `disable_email.sql`). Keys with no DB
/// row are not created (`update`, not `get_or_create`); `is_encrypted` is
/// untouched, so an encrypted `EMAIL_HOST_PASSWORD` is overwritten with
/// `""` in the clear. Any failure surfaces to the caller, which answers
/// 400 `{"error": "Failed to disable email configuration"}`.
pub const DISABLE_EMAIL_SQL: &str = "UPDATE \"instance_configurations\" SET \"value\" = CASE WHEN (\"instance_configurations\".\"key\" = 'ENABLE_SMTP') THEN '0' ELSE '' END WHERE (\"instance_configurations\".\"deleted_at\" IS NULL AND \"instance_configurations\".\"key\" IN ('EMAIL_HOST', 'EMAIL_HOST_USER', 'EMAIL_HOST_PASSWORD', 'ENABLE_SMTP', 'EMAIL_PORT', 'EMAIL_FROM'))";

/// Run the disable-email `UPDATE`; returns affected rows.
pub async fn disable_email_config<'e, E>(ex: E) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    Ok(sqlx::query(DISABLE_EMAIL_SQL)
        .execute(ex)
        .await?
        .rows_affected())
}

// ---------------------------------------------------------------------------
// workspace_list
// ---------------------------------------------------------------------------

/// Project-count annotation (`workspace.py:41-46`, fixture
/// `workspace_list.sql` §1). `.order_by()` clears the default ordering and
/// `Func(F("id"), function="Count")` renders `Count("id")`.
pub const PROJECT_COUNT_SUBQUERY: &str = "(SELECT Count(U0.\"id\") AS \"count\" FROM \"projects\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"workspace_id\" = (\"workspaces\".\"id\"))) AS \"total_projects\"";

/// Member-count annotation (`workspace.py:48-54`, fixture §2).
/// `select_related("owner")` selects no owner column but forces the
/// `INNER JOIN` (which the `member__is_bot` filter needs anyway).
pub const MEMBER_COUNT_SUBQUERY: &str = "(SELECT Count(U0.\"id\") AS \"count\" FROM \"workspace_members\" U0 INNER JOIN \"users\" U1 ON (U0.\"member_id\" = U1.\"id\") WHERE (U0.\"deleted_at\" IS NULL AND U0.\"is_active\" AND NOT U1.\"is_bot\" AND U0.\"workspace_id\" = (\"workspaces\".\"id\"))) AS \"total_members\"";

/// `workspaces` columns in the list SELECT (Django `_meta` order), before
/// the two annotations.
pub const WORKSPACE_LIST_COLUMNS: &[&str] = &[
    "\"workspaces\".\"created_at\"",
    "\"workspaces\".\"updated_at\"",
    "\"workspaces\".\"created_by_id\"",
    "\"workspaces\".\"updated_by_id\"",
    "\"workspaces\".\"deleted_at\"",
    "\"workspaces\".\"id\"",
    "\"workspaces\".\"name\"",
    "\"workspaces\".\"logo\"",
    "\"workspaces\".\"logo_asset_id\"",
    "\"workspaces\".\"owner_id\"",
    "\"workspaces\".\"slug\"",
    "\"workspaces\".\"organization_size\"",
    "\"workspaces\".\"timezone\"",
    "\"workspaces\".\"background_color\"",
];

/// `paginate(..., max_per_page=10, default_per_page=10)`
/// (`workspace.py:63-69`).
pub const WORKSPACE_DEFAULT_PER_PAGE: i64 = 10;
/// Ceiling for `?per_page` on this endpoint.
pub const WORKSPACE_MAX_PER_PAGE: i64 = 10;

/// Default cursor when `?cursor` is absent (`"10:0:0"`,
/// `paginator.py:678`).
pub fn workspace_default_cursor() -> String {
    format!("{WORKSPACE_DEFAULT_PER_PAGE}:0:0")
}

/// Escape a raw search term for `name__icontains`: Django escapes the LIKE
/// metacharacters (`\`, `%`, `_`) with a backslash (no explicit `ESCAPE`
/// clause — Postgres `LIKE` treats backslash as the default escape) and
/// wraps the term in `%...%` (case folding happens in SQL via
/// `UPPER(name::text) LIKE UPPER($1)`).
pub fn icontains_param(search: &str) -> String {
    let mut escaped = String::with_capacity(search.len() + 2);
    for ch in search.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    format!("%{escaped}%")
}

/// List query (`workspace.py:56-61`, fixture `workspace_list.sql` §3). With
/// `search`, the `icontains` predicate binds `$1` (caller binds
/// [`icontains_param`]); without it the `LIKE` clause is absent. No
/// `LIMIT`/`OFFSET` here — pagination slices via [`workspace_page_sql`].
pub fn workspace_list_sql(search: Option<&str>) -> String {
    let mut sql = format!(
        "SELECT {}, {PROJECT_COUNT_SUBQUERY}, {MEMBER_COUNT_SUBQUERY} FROM \"workspaces\"",
        WORKSPACE_LIST_COLUMNS.join(", ")
    );
    match search {
        Some(_) => sql.push_str(" WHERE (\"workspaces\".\"deleted_at\" IS NULL AND UPPER(\"workspaces\".\"name\"::text) LIKE UPPER($1)) ORDER BY \"workspaces\".\"created_at\" DESC"),
        // No parens: Django renders a single-condition WHERE bare
        // (`WHERE "workspaces"."deleted_at" IS NULL`); the parenthesized
        // form only appears with two or more ANDed predicates.
        None => sql.push_str(" WHERE \"workspaces\".\"deleted_at\" IS NULL ORDER BY \"workspaces\".\"created_at\" DESC"),
    }
    sql
}

/// Fetch window for one `OffsetPaginator` page: `(fetch_limit, offset)`.
/// `offset = page * per_page`; `fetch_limit = per_page + 1` (the extra row
/// is the `has_more` probe, `paginator.py:144,151,163`). Django inlines both
/// as integer literals, so this returns SQL text, not params.
pub fn workspace_page_sql(search: Option<&str>, page: i64, per_page: i64) -> String {
    let offset = page * per_page;
    format!(
        "{} LIMIT {} OFFSET {}",
        workspace_list_sql(search),
        per_page + 1,
        offset
    )
}

/// `next` cursor text: `Cursor(limit, page + 1, False)`
/// (`paginator.py:163`). Page 0 at 10/per_page yields `"10:1:0"`.
/// NOTE: `fixtures/license/queries/workspace_list.rows.json` records
/// `"10:1:10"` here; that contradicts `Cursor.__str__`
/// (`f"{value}:{offset}:{int(is_prev)}"`), the F-07 kernel
/// (`next_cursor(50, 0, true) == "50:1:0"`), and the live Django behavior —
/// a fixture-recording erratum, not a ported bug. The canonical cursor
/// owner is `pidash_api::paginator`; these helpers only project the same
/// math into the db layer and are tested against the kernel's vectors.
pub fn workspace_next_cursor(limit: i64, page: i64) -> String {
    format!("{limit}:{}:0", page + 1)
}

/// `prev` cursor text: `Cursor(limit, page - 1, True)`
/// (`paginator.py:165`).
pub fn workspace_prev_cursor(limit: i64, page: i64) -> String {
    format!("{limit}:{}:1", page - 1)
}

/// One workspace list row: the selected columns plus the two `Count`
/// annotations (`total_projects`, `total_members`). The table itself is
/// app-owned (`db/models/workspace.py`); only this read shape lives here.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceListRow {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by_id: Option<uuid::Uuid>,
    pub updated_by_id: Option<uuid::Uuid>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub id: uuid::Uuid,
    pub name: String,
    pub logo: Option<String>,
    pub logo_asset_id: Option<uuid::Uuid>,
    pub owner_id: uuid::Uuid,
    pub slug: String,
    pub organization_size: Option<String>,
    pub timezone: String,
    pub background_color: String,
    pub total_projects: i64,
    pub total_members: i64,
}

/// Map one workspace list row (column names, order-independent).
pub fn map_workspace_list_row(row: &PgRow) -> Result<WorkspaceListRow, sqlx::Error> {
    Ok(WorkspaceListRow {
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        logo: row.try_get("logo")?,
        logo_asset_id: row.try_get("logo_asset_id")?,
        owner_id: row.try_get("owner_id")?,
        slug: row.try_get("slug")?,
        organization_size: row.try_get("organization_size")?,
        timezone: row.try_get("timezone")?,
        background_color: row.try_get("background_color")?,
        total_projects: row.try_get("total_projects")?,
        total_members: row.try_get("total_members")?,
    })
}

/// One annotated page: `(rows, has_more)`. The `+1` probe row is trimmed.
/// `per_page` here is already resolved by the caller through
/// `get_per_page` semantics (`WORKSPACE_DEFAULT_PER_PAGE` /
/// [`WORKSPACE_MAX_PER_PAGE`]); cursor parsing stays with the paginator
/// kernel (`pidash_api::paginator`) and the handler layer.
pub async fn fetch_workspace_page<'e, E>(
    ex: E,
    search: Option<&str>,
    page: i64,
    per_page: i64,
) -> Result<(Vec<WorkspaceListRow>, bool), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = workspace_page_sql(search, page, per_page);
    let mut query = sqlx::query(&sql);
    if let Some(term) = search {
        query = query.bind(icontains_param(term));
    }
    let rows: Vec<PgRow> = query.fetch_all(ex).await?;
    let has_more = rows.len() as i64 > per_page;
    let out: Vec<WorkspaceListRow> = rows
        .iter()
        .take(per_page as usize)
        .map(map_workspace_list_row)
        .collect::<Result<_, _>>()?;
    Ok((out, has_more))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/license/queries")
    }

    fn read_fixture(name: &str) -> String {
        let path = fixtures_dir().join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
    }

    /// Django renders params as `%s`; this crate renders Postgres `$N`.
    /// Number them in order for comparison.
    fn normalize_params(sql: &str) -> String {
        let mut out = sql.to_string();
        let mut i = 1;
        while let Some(pos) = out.find("%s") {
            out.replace_range(pos..pos + 2, &format!("${i}"));
            i += 1;
        }
        out
    }

    /// Statement lines of a fixture file: several fixtures record their SQL
    /// inside `--` comment lines (sometimes behind prose like
    /// `... delete -> UPDATE ...`), so the marker is stripped, the line is
    /// cut at the first statement keyword, and trailing `;` is stripped.
    fn statement_lines(name: &str) -> Vec<String> {
        read_fixture(name)
            .replace("<same column list>", &ADMIN_COLUMNS.join(", "))
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| l.strip_prefix("--").unwrap_or(l).trim())
            .filter_map(|l| {
                ["SELECT \"", "INSERT INTO \"", "UPDATE \"", "(SELECT"]
                    .iter()
                    .filter_map(|kw| l.find(kw).map(|pos| &l[pos..]))
                    .max_by_key(|s| s.len())
                    .map(|s| normalize_params(s.trim_end_matches(';')))
            })
            .collect()
    }

    fn statement_starting(name: &str, prefix: &str) -> String {
        statement_lines(name)
            .into_iter()
            .find(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("{name} has no statement starting with {prefix}"))
    }

    #[test]
    fn instance_first_sql_matches_fixture() {
        let expected = statement_starting("instance_first.sql", "SELECT");
        assert_eq!(INSTANCE_FIRST_SQL, expected);
    }

    #[test]
    fn admin_exists_sql_matches_fixture() {
        // The fixture inlines the threshold (`>= 15`); Django binds it, so
        // restore the placeholder before numbering (instance `$1`, role
        // `$2`, user `$3`).
        let raw = read_fixture("admin_crud.sql").replace("\"role\" >= 15", "\"role\" >= %s");
        let expected = raw
            .lines()
            .map(str::trim)
            .filter_map(|l| {
                let l = l.strip_prefix("--").unwrap_or(l).trim();
                l.find("SELECT \"")
                    .map(|pos| normalize_params(l[pos..].trim_end_matches(';')))
            })
            .find(|l| l.contains("\"role\" >="))
            .expect("permission-check SELECT");
        assert_eq!(admin_permission_check_sql(), expected);
        assert!(expected.contains("ORDER BY \"instance_admins\".\"created_at\" DESC LIMIT 1"));
        let gate: i32 = ADMIN_PERMISSION_ROLE_GTE;
        assert_eq!(gate, 15);
    }

    #[test]
    fn admin_insert_sql_matches_fixture() {
        let expected = statement_starting("admin_crud.sql", "INSERT");
        assert_eq!(ADMIN_INSERT_SQL, expected);
    }

    #[test]
    fn admin_list_sql_matches_fixture() {
        let raw = statement_lines("admin_crud.sql")
            .into_iter()
            .find(|l| {
                l.starts_with("SELECT")
                    && l.contains("FROM \"instance_admins\"")
                    && !l.contains("\"role\" >=")
                    && !l.contains("LIMIT 1")
            })
            .expect("list SELECT");
        let expected = raw.replace("<same column list>", &ADMIN_COLUMNS.join(", "));
        assert_eq!(admin_list_sql(), expected);
    }

    #[test]
    fn admin_soft_delete_sql_matches_fixture() {
        let expected = statement_starting("admin_crud.sql", "UPDATE");
        assert_eq!(ADMIN_SOFT_DELETE_SQL, expected);
    }

    #[test]
    fn config_select_sql_matches_fixture() {
        let expected = statement_starting("config_patch.sql", "SELECT");
        assert_eq!(
            config_select_by_keys_sql(&["EMAIL_HOST", "LLM_API_KEY"]).as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn config_select_empty_short_circuits() {
        assert_eq!(config_select_by_keys_sql(&[]), None);
    }

    #[test]
    fn bulk_update_single_pair_exact() {
        let nil = uuid::Uuid::nil();
        let sql = config_bulk_update_sql(&[(nil, "v")]).expect("one pair");
        assert_eq!(
            sql,
            "UPDATE \"instance_configurations\" SET \"value\" = CASE \"id\" WHEN $1 THEN $2 END WHERE \"id\" IN ($3)"
        );
    }

    #[test]
    fn bulk_update_numbering_two_pairs() {
        let nil = uuid::Uuid::nil();
        let sql = config_bulk_update_sql(&[(nil, "a"), (nil, "b")]).expect("two pairs");
        assert_eq!(
            sql,
            "UPDATE \"instance_configurations\" SET \"value\" = CASE \"id\" WHEN $1 THEN $2 WHEN $3 THEN $4 END WHERE \"id\" IN ($5, $6)"
        );
    }

    #[test]
    fn bulk_update_empty_is_none() {
        assert_eq!(config_bulk_update_sql(&[]), None);
        assert_eq!(config_bulk_update_batches(0), 0);
        assert_eq!(config_bulk_update_batches(1), 1);
        assert_eq!(config_bulk_update_batches(100), 1);
        assert_eq!(config_bulk_update_batches(101), 2);
        assert_eq!(config_bulk_update_batches(250), 3);
        let size: usize = CONFIG_BULK_UPDATE_BATCH_SIZE;
        assert_eq!(size, 100);
    }

    #[test]
    fn normalize_config_value_cases() {
        assert_eq!(normalize_config_value(None), "");
        assert_eq!(
            normalize_config_value(Some("  smtp.acme.test  ")),
            "smtp.acme.test"
        );
        assert_eq!(normalize_config_value(Some("")), "");
        assert_eq!(normalize_config_value(Some("   ")), "");
        assert_eq!(normalize_config_value(Some("a  b")), "a  b");
    }

    #[test]
    fn disable_email_sql_matches_fixture() {
        let expected = statement_starting("disable_email.sql", "UPDATE");
        assert_eq!(DISABLE_EMAIL_SQL, expected);
        assert_eq!(
            DISABLE_EMAIL_KEYS,
            &[
                "EMAIL_HOST",
                "EMAIL_HOST_USER",
                "EMAIL_HOST_PASSWORD",
                "ENABLE_SMTP",
                "EMAIL_PORT",
                "EMAIL_FROM",
            ]
        );
    }

    #[test]
    fn workspace_subqueries_match_fixture() {
        let lines = statement_lines("workspace_list.sql");
        let projects = lines
            .iter()
            .find(|l| l.starts_with("(SELECT") && l.contains("FROM \"projects\""))
            .expect("project count subquery");
        assert_eq!(PROJECT_COUNT_SUBQUERY, projects.as_str());
        let members = lines
            .iter()
            .find(|l| l.starts_with("(SELECT") && l.contains("FROM \"workspace_members\""))
            .expect("member count subquery");
        assert_eq!(MEMBER_COUNT_SUBQUERY, members.as_str());
    }

    #[test]
    fn workspace_list_search_sql_matches_fixture() {
        let expected = statement_lines("workspace_list.sql")
            .into_iter()
            .find(|l| l.starts_with("SELECT \"workspaces\""))
            .expect("full list query");
        assert_eq!(workspace_list_sql(Some("acme")), expected);
    }

    #[test]
    fn workspace_list_no_search_shape() {
        // Single-condition WHERE is bare (no parens) in real Django output;
        // verified against `str(Workspace.objects.annotate(...).query)`.
        assert_eq!(
            workspace_list_sql(None),
            format!(
                "SELECT {}, {PROJECT_COUNT_SUBQUERY}, {MEMBER_COUNT_SUBQUERY} FROM \"workspaces\" WHERE \"workspaces\".\"deleted_at\" IS NULL ORDER BY \"workspaces\".\"created_at\" DESC",
                WORKSPACE_LIST_COLUMNS.join(", ")
            )
        );
    }

    #[test]
    fn icontains_escape_cases() {
        assert_eq!(icontains_param("acme"), "%acme%");
        assert_eq!(icontains_param("a%b_c\\d"), "%a\\%b\\_c\\\\d%");
        assert_eq!(icontains_param(""), "%%");
    }

    #[test]
    fn workspace_pagination_consts_and_cursors() {
        let default: i64 = WORKSPACE_DEFAULT_PER_PAGE;
        assert_eq!(default, 10);
        let max: i64 = WORKSPACE_MAX_PER_PAGE;
        assert_eq!(max, 10);
        assert_eq!(workspace_default_cursor(), "10:0:0");
        // Kernel vectors (`api/src/paginator.rs`): next is
        // `Cursor(limit, page + 1, False)`, prev is
        // `Cursor(limit, page - 1, True)`.
        assert_eq!(workspace_next_cursor(50, 0), "50:1:0");
        assert_eq!(workspace_prev_cursor(50, 1), "50:0:1");
        // This endpoint's first page: see NOTE on workspace_next_cursor
        // about the rows.json erratum ("10:1:10" is not emitted).
        assert_eq!(workspace_next_cursor(10, 0), "10:1:0");
        assert_eq!(workspace_prev_cursor(10, 0), "10:-1:1");
    }

    #[test]
    fn workspace_page_window() {
        assert_eq!(
            workspace_page_sql(None, 0, 10),
            format!("{} LIMIT 11 OFFSET 0", workspace_list_sql(None))
        );
        assert_eq!(
            workspace_page_sql(Some("acme"), 1, 10),
            format!("{} LIMIT 11 OFFSET 10", workspace_list_sql(Some("acme")))
        );
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

    /// Scratch Postgres for the Done-when verification. Unset (plain
    /// `cargo test`) skips these; CI sets no database either, so the suite
    /// stays green offline. Run with e.g.
    /// `export DATABASE_URL=postgresql://user@host/db` for the real check.
    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    const LIVE_DDL: &[&str] = &[
        "CREATE TEMPORARY TABLE instances (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, instance_name VARCHAR(255) NOT NULL, whitelist_emails TEXT, instance_id VARCHAR(255) NOT NULL UNIQUE, current_version VARCHAR(255) NOT NULL, latest_version VARCHAR(255), edition VARCHAR(255) NOT NULL, domain TEXT NOT NULL, last_checked_at TIMESTAMPTZ NOT NULL, namespace VARCHAR(255), is_telemetry_enabled BOOLEAN NOT NULL, is_support_required BOOLEAN NOT NULL, is_setup_done BOOLEAN NOT NULL, is_signup_screen_visited BOOLEAN NOT NULL, is_verified BOOLEAN NOT NULL, is_test BOOLEAN NOT NULL, is_current_version_deprecated BOOLEAN NOT NULL)",
        "CREATE TEMPORARY TABLE users (id UUID PRIMARY KEY, is_bot BOOLEAN NOT NULL DEFAULT FALSE)",
        "CREATE TEMPORARY TABLE instance_admins (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, user_id UUID, instance_id UUID NOT NULL, role INTEGER NOT NULL, is_verified BOOLEAN NOT NULL)",
        "CREATE TEMPORARY TABLE instance_configurations (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, key VARCHAR(100) NOT NULL UNIQUE, value TEXT, category TEXT NOT NULL, is_encrypted BOOLEAN NOT NULL DEFAULT FALSE)",
        "CREATE TEMPORARY TABLE workspaces (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, name VARCHAR(80) NOT NULL, logo TEXT, logo_asset_id UUID, owner_id UUID NOT NULL, slug VARCHAR(48) NOT NULL UNIQUE, organization_size VARCHAR(20), timezone VARCHAR(255) NOT NULL, background_color VARCHAR(255) NOT NULL)",
        "CREATE TEMPORARY TABLE projects (id UUID PRIMARY KEY, deleted_at TIMESTAMPTZ, workspace_id UUID NOT NULL)",
        "CREATE TEMPORARY TABLE workspace_members (id UUID PRIMARY KEY, deleted_at TIMESTAMPTZ, is_active BOOLEAN NOT NULL DEFAULT TRUE, workspace_id UUID NOT NULL, member_id UUID NOT NULL)",
    ];

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        for ddl in LIVE_DDL {
            sqlx::query(ddl)
                .execute(&mut *tx)
                .await
                .expect("create temp table");
        }
        tx
    }

    fn live_uuid(suffix: u8) -> uuid::Uuid {
        uuid::Uuid::parse_str(&format!("22222222-2222-2222-2222-2222222222{suffix:02}"))
            .expect("fixed test uuid")
    }

    fn live_ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).expect("fixed test timestamp")
    }

    #[tokio::test]
    async fn live_instance_first() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let seed = "INSERT INTO instances (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, instance_name, whitelist_emails, instance_id, current_version, latest_version, edition, domain, last_checked_at, namespace, is_telemetry_enabled, is_support_required, is_setup_done, is_signup_screen_visited, is_verified, is_test, is_current_version_deprecated) VALUES ($1, $2, $2, NULL, NULL, $3, 'Acme', NULL, $4, 'v1.0.0', NULL, 'PI_DASH_COMMUNITY', '', $2, NULL, TRUE, TRUE, TRUE, FALSE, TRUE, FALSE, FALSE)";
        for (id, ts, deleted, iid) in [
            (live_uuid(1), live_ts(1_700_000_000), None, "old"),
            (live_uuid(2), live_ts(1_700_000_100), None, "new"),
            (
                live_uuid(3),
                live_ts(1_700_000_200),
                Some(live_ts(1_700_000_300)),
                "gone",
            ),
        ] {
            sqlx::query(seed)
                .bind(id)
                .bind(ts)
                .bind(deleted)
                .bind(iid)
                .execute(&mut *tx)
                .await
                .expect("seed instance");
        }
        // Newest non-deleted row wins; the soft-deleted newest is skipped.
        let got = fetch_instance_first(&mut *tx)
            .await
            .expect("first")
            .expect("a row");
        assert_eq!(got.instance_id, "new");
        assert_eq!(got.id, live_uuid(2));
        // Empty table -> None (the GET null-instance branch).
        sqlx::query("DELETE FROM instances")
            .execute(&mut *tx)
            .await
            .expect("clear");
        assert!(fetch_instance_first(&mut *tx)
            .await
            .expect("empty")
            .is_none());
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_admin_crud() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let inst = live_uuid(10);
        let (u1, u2) = (live_uuid(11), live_uuid(12));
        let ts = live_ts(1_700_000_000);
        sqlx::query("INSERT INTO instances (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, instance_name, whitelist_emails, instance_id, current_version, latest_version, edition, domain, last_checked_at, namespace, is_telemetry_enabled, is_support_required, is_setup_done, is_signup_screen_visited, is_verified, is_test, is_current_version_deprecated) VALUES ($1, $2, $2, NULL, NULL, NULL, 'Acme', NULL, 'i1', 'v1', NULL, 'PI_DASH_COMMUNITY', '', $2, NULL, TRUE, TRUE, TRUE, FALSE, TRUE, FALSE, FALSE)")
            .bind(inst).bind(ts).execute(&mut *tx).await.expect("seed instance");
        for u in [u1, u2] {
            sqlx::query("INSERT INTO users (id, is_bot) VALUES ($1, FALSE)")
                .bind(u)
                .execute(&mut *tx)
                .await
                .expect("seed user");
        }
        assert!(!admin_permission_check(&mut *tx, inst, u1)
            .await
            .expect("gate"));
        let a1 = live_uuid(13);
        let created = create_instance_admin(&mut *tx, a1, ts, Some(u1), inst, 20, false, None)
            .await
            .expect("create");
        assert_eq!(created.role, 20);
        assert!(!created.is_verified);
        assert_eq!(created.created_at, created.updated_at);
        // Anonymous-signup path stores NULL created_by (BaseModel.save).
        assert_eq!(created.created_by_id, None);
        assert!(admin_permission_check(&mut *tx, inst, u1)
            .await
            .expect("gate"));
        // role 10 < 15 stays denied (the latent wider gate, kept as-is).
        // Authenticated create stores the requesting admin as created_by.
        let a2 = live_uuid(14);
        create_instance_admin(&mut *tx, a2, ts, Some(u2), inst, 10, false, Some(u1))
            .await
            .expect("create low role");
        assert_eq!(
            get_admins_by_instance_user(&mut *tx, inst, u2)
                .await
                .expect("get low role")
                .first()
                .expect("one row")
                .created_by_id,
            Some(u1)
        );
        assert!(!admin_permission_check(&mut *tx, inst, u2)
            .await
            .expect("gate"));
        let list = list_instance_admins(&mut *tx, inst).await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(
            get_admins_by_instance_user(&mut *tx, inst, u1)
                .await
                .expect("get")
                .len(),
            1
        );
        assert!(admin_exists_by_user(&mut *tx, u2).await.expect("exists"));
        assert_eq!(
            soft_delete_instance_admin(&mut *tx, ts, inst, a1)
                .await
                .expect("delete"),
            1
        );
        assert_eq!(
            list_instance_admins(&mut *tx, inst)
                .await
                .expect("list")
                .len(),
            1
        );
        assert!(!admin_permission_check(&mut *tx, inst, u1)
            .await
            .expect("gate"));
        // Deleting again matches nothing (the view still answers 204).
        assert_eq!(
            soft_delete_instance_admin(&mut *tx, ts, inst, a1)
                .await
                .expect("delete"),
            0
        );
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_config_patch() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let (c1, c2) = (live_uuid(20), live_uuid(21));
        let (t_old, t_new) = (live_ts(1_700_000_000), live_ts(1_700_000_100));
        for (id, ts, key, value, enc) in [
            (c1, t_old, "EMAIL_HOST", "smtp.old", false),
            (c2, t_new, "LLM_API_KEY", "old-token", true),
        ] {
            sqlx::query("INSERT INTO instance_configurations (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, key, value, category, is_encrypted) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, 'general', $5)")
                .bind(id).bind(ts).bind(key).bind(value).bind(enc)
                .execute(&mut *tx).await.expect("seed config");
        }
        // Unknown keys ignored; newest first.
        let rows = fetch_config_by_keys(&mut *tx, &["EMAIL_HOST", "LLM_API_KEY", "NOPE"])
            .await
            .expect("fetch");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "LLM_API_KEY");
        // Per-row transform then write.
        let v1 = normalize_config_value(Some("  smtp.new  "));
        let v2 = normalize_config_value(Some("tok"));
        assert_eq!(
            apply_config_batch(&mut *tx, &[(c1, &v1), (c2, &v2)])
                .await
                .expect("bulk"),
            2
        );
        let after = fetch_config_by_keys(&mut *tx, &["EMAIL_HOST", "LLM_API_KEY"])
            .await
            .expect("refetch");
        assert_eq!(
            after
                .iter()
                .find(|r| r.key == "EMAIL_HOST")
                .expect("host")
                .value
                .as_deref(),
            Some("smtp.new")
        );
        // Empty fetch / empty batch touch nothing.
        assert_eq!(
            fetch_config_by_keys(&mut *tx, &[])
                .await
                .expect("empty")
                .len(),
            0
        );
        assert_eq!(apply_config_batch(&mut *tx, &[]).await.expect("empty"), 0);
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_disable_email() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ts = live_ts(1_700_000_000);
        for (i, (key, value, enc)) in [
            ("EMAIL_HOST", "smtp.acme.test", false),
            ("EMAIL_HOST_USER", "ops@acme.test", false),
            ("EMAIL_HOST_PASSWORD", "gAAAAA-token", true),
            ("ENABLE_SMTP", "1", false),
            ("EMAIL_PORT", "587", false),
            ("EMAIL_FROM", "Team <t@e.test>", false),
            ("POSTHOG_API_KEY", "z", false),
        ]
        .into_iter()
        .enumerate()
        {
            sqlx::query("INSERT INTO instance_configurations (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, key, value, category, is_encrypted) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, 'email', $5)")
                .bind(live_uuid(30 + i as u8)).bind(ts).bind(key).bind(value).bind(enc)
                .execute(&mut *tx).await.expect("seed config");
        }
        assert_eq!(disable_email_config(&mut *tx).await.expect("disable"), 6);
        let rows = fetch_config_by_keys(
            &mut *tx,
            &[
                "EMAIL_HOST",
                "EMAIL_HOST_USER",
                "EMAIL_HOST_PASSWORD",
                "ENABLE_SMTP",
                "EMAIL_PORT",
                "EMAIL_FROM",
                "POSTHOG_API_KEY",
            ],
        )
        .await
        .expect("refetch");
        for row in &rows {
            let want = if row.key == "ENABLE_SMTP" {
                "0"
            } else if row.key == "POSTHOG_API_KEY" {
                "z"
            } else {
                ""
            };
            assert_eq!(row.value.as_deref(), Some(want), "key {}", row.key);
        }
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_workspace_list() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ts = live_ts(1_700_000_000);
        let (owner, m1, m2, m3, m4) = (
            live_uuid(40),
            live_uuid(41),
            live_uuid(42),
            live_uuid(43),
            live_uuid(44),
        );
        for (u, bot) in [
            (owner, false),
            (m1, false),
            (m2, false),
            (m3, true),
            (m4, false),
        ] {
            sqlx::query("INSERT INTO users (id, is_bot) VALUES ($1, $2)")
                .bind(u)
                .bind(bot)
                .execute(&mut *tx)
                .await
                .expect("seed user");
        }
        let (acme, other) = (live_uuid(45), live_uuid(46));
        for (id, name, slug) in [
            (acme, "Acme Works", "acme-works"),
            (other, "Other", "other"),
        ] {
            sqlx::query("INSERT INTO workspaces (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, name, logo, logo_asset_id, owner_id, slug, organization_size, timezone, background_color) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, NULL, NULL, $4, $5, NULL, 'UTC', '#fff')")
                .bind(id).bind(ts).bind(name).bind(owner).bind(slug)
                .execute(&mut *tx).await.expect("seed workspace");
        }
        for i in 0..3 {
            sqlx::query(
                "INSERT INTO projects (id, deleted_at, workspace_id) VALUES ($1, NULL, $2)",
            )
            .bind(live_uuid(50 + i))
            .bind(acme)
            .execute(&mut *tx)
            .await
            .expect("seed project");
        }
        sqlx::query("INSERT INTO projects (id, deleted_at, workspace_id) VALUES ($1, $2, $3)")
            .bind(live_uuid(53))
            .bind(ts)
            .bind(acme)
            .execute(&mut *tx)
            .await
            .expect("seed deleted project");
        // m1 active human, m4 active human, m2 inactive, m3 bot, m4-dupe deleted.
        for (i, (member, active, deleted)) in [
            (m1, true, false),
            (m2, false, false),
            (m3, true, false),
            (m4, true, false),
            (m4, true, true),
        ]
        .into_iter()
        .enumerate()
        {
            sqlx::query("INSERT INTO workspace_members (id, deleted_at, is_active, workspace_id, member_id) VALUES ($1, $2, $3, $4, $5)")
                .bind(live_uuid(60 + i as u8))
                .bind(if deleted { Some(ts) } else { None })
                .bind(active)
                .bind(acme)
                .bind(member)
                .execute(&mut *tx).await.expect("seed member");
        }
        // icontains search (case-insensitive via UPPER).
        let (rows, more) = fetch_workspace_page(&mut *tx, Some("ACME"), 0, 10)
            .await
            .expect("search page");
        assert!(!more);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Acme Works");
        assert_eq!(rows[0].total_projects, 3);
        assert_eq!(rows[0].total_members, 2);
        // No search: both rows; windowing with per_page=1.
        let (p0, more0) = fetch_workspace_page(&mut *tx, None, 0, 1)
            .await
            .expect("p0");
        assert_eq!((p0.len(), more0), (1, true));
        let (p1, more1) = fetch_workspace_page(&mut *tx, None, 1, 1)
            .await
            .expect("p1");
        assert_eq!((p1.len(), more1), (1, false));
        assert_ne!(p0[0].id, p1[0].id);
        // Miss: empty page, no more.
        let (miss, miss_more) = fetch_workspace_page(&mut *tx, Some("zzz"), 0, 10)
            .await
            .expect("miss");
        assert_eq!((miss.len(), miss_more), (0, false));
        tx.rollback().await.expect("rollback");
    }
}
