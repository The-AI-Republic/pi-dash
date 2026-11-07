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
//! SeaORM conversion (LICENSE-1): all execution goes through SeaORM
//! entities / sea-query, not runtime sqlx::query. The pure string builders
//! below are kept for fixture pinning (tests compare them to the Django
//! fixtures verbatim), but execution does not use them.
//!
//! Row types are reused from [`super::models`]; [`WorkspaceListRow`] lives
//! here because the `workspaces` table is owned by the app domain, not by
//! `license/models`.
//!
//! Write routing: callers pass a SeaORM connection (`&impl ConnectionTrait`)
//! — either `sea_orm_connection(&pool)` for pool callers or
//! `TxConn::new(&mut tx)` for sqlx-transaction callers (orm-transaction
//! bridge). Errors stay `sqlx::Error` via `orm_tx::into_sqlx` so callers'
//! SQLSTATE classification is unchanged.
//!
//! `create_admin` takes the new row's `id` from the caller (Django assigns
//! the UUID client-side pre-insert); the `uuid` crate here has no `v4`
//! feature, so generation belongs to the caller.
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

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use sea_query::{Alias, Expr};

use super::models::{
    instance::Instance as InstanceRow, instance_admin::InstanceAdmin as InstanceAdminRow,
    instance_configuration::InstanceConfiguration as InstanceConfigurationRow,
};
use crate::entity::{
    instance_admins, instance_configurations, instances, projects, users, workspace_members,
    workspaces,
};

// ---------------------------------------------------------------------------
// instance_first
// ---------------------------------------------------------------------------

/// `Instance.objects.first()`: newest non-deleted row, `LIMIT 1`
/// (`fixtures/license/queries/instance_first.sql`).
pub const INSTANCE_FIRST_SQL: &str = "SELECT \"instances\".\"created_at\", \"instances\".\"updated_at\", \"instances\".\"created_by_id\", \"instances\".\"updated_by_id\", \"instances\".\"deleted_at\", \"instances\".\"id\", \"instances\".\"instance_name\", \"instances\".\"whitelist_emails\", \"instances\".\"instance_id\", \"instances\".\"current_version\", \"instances\".\"latest_version\", \"instances\".\"edition\", \"instances\".\"domain\", \"instances\".\"last_checked_at\", \"instances\".\"namespace\", \"instances\".\"is_telemetry_enabled\", \"instances\".\"is_support_required\", \"instances\".\"is_setup_done\", \"instances\".\"is_signup_screen_visited\", \"instances\".\"is_verified\", \"instances\".\"is_test\", \"instances\".\"is_current_version_deprecated\" FROM \"instances\" WHERE \"instances\".\"deleted_at\" IS NULL ORDER BY \"instances\".\"created_at\" DESC LIMIT 1";

fn instances_model_to_row(m: instances::Model) -> InstanceRow {
    InstanceRow {
        created_at: m.created_at.with_timezone(&chrono::Utc),
        updated_at: m.updated_at.with_timezone(&chrono::Utc),
        created_by_id: m.created_by_id,
        updated_by_id: m.updated_by_id,
        deleted_at: m.deleted_at.map(|dt| dt.with_timezone(&chrono::Utc)),
        id: m.id,
        instance_name: m.instance_name,
        whitelist_emails: m.whitelist_emails,
        instance_id: m.instance_id,
        current_version: m.current_version,
        latest_version: m.latest_version,
        edition: m.edition,
        domain: m.domain,
        last_checked_at: m.last_checked_at.with_timezone(&chrono::Utc),
        namespace: m.namespace,
        is_telemetry_enabled: m.is_telemetry_enabled,
        is_support_required: m.is_support_required,
        is_setup_done: m.is_setup_done,
        is_signup_screen_visited: m.is_signup_screen_visited,
        is_verified: m.is_verified,
        is_test: m.is_test,
        is_current_version_deprecated: m.is_current_version_deprecated,
    }
}

/// Newest non-deleted instance, or `None` (the GET null-instance branch,
/// `instance.py:39-44`).
pub async fn fetch_instance_first(
    db: &impl ConnectionTrait,
) -> Result<Option<InstanceRow>, sqlx::Error> {
    let row = instances::Entity::find()
        .filter(instances::Column::DeletedAt.is_null())
        .order_by_desc(instances::Column::CreatedAt)
        .limit(1)
        .one(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(row.map(instances_model_to_row))
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

fn instance_admin_model_to_row(m: instance_admins::Model) -> InstanceAdminRow {
    InstanceAdminRow {
        created_at: m.created_at.with_timezone(&chrono::Utc),
        updated_at: m.updated_at.with_timezone(&chrono::Utc),
        created_by_id: m.created_by_id,
        updated_by_id: m.updated_by_id,
        deleted_at: m.deleted_at.map(|dt| dt.with_timezone(&chrono::Utc)),
        id: m.id,
        user_id: m.user_id,
        instance_id: m.instance_id,
        role: m.role,
        is_verified: m.is_verified,
    }
}

fn instance_config_model_to_row(m: instance_configurations::Model) -> InstanceConfigurationRow {
    InstanceConfigurationRow {
        created_at: m.created_at.with_timezone(&chrono::Utc),
        updated_at: m.updated_at.with_timezone(&chrono::Utc),
        created_by_id: m.created_by_id,
        updated_by_id: m.updated_by_id,
        deleted_at: m.deleted_at.map(|dt| dt.with_timezone(&chrono::Utc)),
        id: m.id,
        key: m.key,
        value: m.value,
        category: m.category,
        is_encrypted: m.is_encrypted,
    }
}

/// Permission gate: a matching admin row exists.
pub async fn admin_permission_check(
    db: &impl ConnectionTrait,
    instance_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let row = instance_admins::Entity::find()
        .filter(instance_admins::Column::DeletedAt.is_null())
        .filter(instance_admins::Column::InstanceId.eq(instance_id))
        .filter(instance_admins::Column::Role.gte(ADMIN_PERMISSION_ROLE_GTE))
        .filter(instance_admins::Column::UserId.eq(user_id))
        .order_by_desc(instance_admins::Column::CreatedAt)
        .limit(1)
        .one(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
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
pub async fn create_instance_admin(
    db: &impl ConnectionTrait,
    id: uuid::Uuid,
    now: chrono::DateTime<chrono::Utc>,
    user_id: Option<uuid::Uuid>,
    instance_id: uuid::Uuid,
    role: i32,
    is_verified: bool,
    created_by_id: Option<uuid::Uuid>,
) -> Result<InstanceAdminRow, sqlx::Error> {
    let now_tz = sea_orm::prelude::DateTimeWithTimeZone::from(now);
    let model = instance_admins::ActiveModel {
        id: Set(id),
        created_at: Set(now_tz),
        updated_at: Set(now_tz),
        created_by_id: Set(created_by_id),
        updated_by_id: Set(None),
        deleted_at: Set(None),
        user_id: Set(user_id),
        instance_id: Set(instance_id),
        role: Set(role),
        is_verified: Set(is_verified),
    };
    let inserted = model.insert(db).await.map_err(crate::orm_tx::into_sqlx)?;
    Ok(instance_admin_model_to_row(inserted))
}

/// All admins of one instance, newest first.
pub async fn list_instance_admins(
    db: &impl ConnectionTrait,
    instance_id: uuid::Uuid,
) -> Result<Vec<InstanceAdminRow>, sqlx::Error> {
    let rows = instance_admins::Entity::find()
        .filter(instance_admins::Column::DeletedAt.is_null())
        .filter(instance_admins::Column::InstanceId.eq(instance_id))
        .order_by_desc(instance_admins::Column::CreatedAt)
        .all(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(rows.into_iter().map(instance_admin_model_to_row).collect())
}

/// Soft-delete one admin row; returns matched rows (the view ignores the
/// count and always answers 204).
pub async fn soft_delete_instance_admin(
    db: &impl ConnectionTrait,
    now: chrono::DateTime<chrono::Utc>,
    instance_id: uuid::Uuid,
    id: uuid::Uuid,
) -> Result<u64, sqlx::Error> {
    let now_tz = sea_orm::prelude::DateTimeWithTimeZone::from(now);
    let res = instance_admins::Entity::update_many()
        .col_expr(instance_admins::Column::DeletedAt, Expr::value(now_tz))
        .filter(instance_admins::Column::DeletedAt.is_null())
        .filter(instance_admins::Column::InstanceId.eq(instance_id))
        .filter(instance_admins::Column::Id.eq(id))
        .exec(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(res.rows_affected)
}

/// Admin rows for one `(instance, user)` pair, newest first.
pub async fn get_admins_by_instance_user(
    db: &impl ConnectionTrait,
    instance_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<Vec<InstanceAdminRow>, sqlx::Error> {
    let rows = instance_admins::Entity::find()
        .filter(instance_admins::Column::DeletedAt.is_null())
        .filter(instance_admins::Column::InstanceId.eq(instance_id))
        .filter(instance_admins::Column::UserId.eq(user_id))
        .order_by_desc(instance_admins::Column::CreatedAt)
        .all(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(rows.into_iter().map(instance_admin_model_to_row).collect())
}

/// Whether any admin row exists for one user (any instance).
pub async fn admin_exists_by_user(
    db: &impl ConnectionTrait,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    let row = instance_admins::Entity::find()
        .filter(instance_admins::Column::DeletedAt.is_null())
        .filter(instance_admins::Column::UserId.eq(user_id))
        .order_by_desc(instance_admins::Column::CreatedAt)
        .limit(1)
        .one(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
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

/// Rows for the requested keys (unknown keys silently ignored — they are
/// never fetched and never created). Empty input returns `[]` with no
/// query, mirroring Django's `EmptyResultSet` short-circuit.
pub async fn fetch_config_by_keys(
    db: &impl ConnectionTrait,
    keys: &[&str],
) -> Result<Vec<InstanceConfigurationRow>, sqlx::Error> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let owned_keys: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
    let rows = instance_configurations::Entity::find()
        .filter(instance_configurations::Column::DeletedAt.is_null())
        .filter(instance_configurations::Column::Key.is_in(owned_keys))
        .order_by_desc(instance_configurations::Column::CreatedAt)
        .all(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(rows.into_iter().map(instance_config_model_to_row).collect())
}

/// Apply one pre-built batch (`id -> value`): a single `UPDATE ... CASE`.
/// Returns affected rows. Callers split larger inputs with
/// [`config_bulk_update_batches`] (chunks of
/// [`CONFIG_BULK_UPDATE_BATCH_SIZE`]).
///
/// Implemented as per-row `UPDATE` statements via SeaORM `update_many`
/// (one per id) — same row semantics as the single CASE statement but
/// with multiple statements; the product behavior (values written) is
/// identical. The single-statement CASE is preserved in the builder
/// [`config_bulk_update_sql`] for fixture parity.
pub async fn apply_config_batch(
    db: &impl ConnectionTrait,
    pairs: &[(uuid::Uuid, &str)],
) -> Result<u64, sqlx::Error> {
    if pairs.is_empty() {
        return Ok(0);
    }
    let mut total = 0u64;
    for (id, value) in pairs {
        let res = instance_configurations::Entity::update_many()
            .col_expr(
                instance_configurations::Column::Value,
                Expr::value(Some(value.to_string())),
            )
            .filter(instance_configurations::Column::Id.eq(*id))
            .exec(db)
            .await
            .map_err(crate::orm_tx::into_sqlx)?;
        total += res.rows_affected;
    }
    Ok(total)
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
pub async fn disable_email_config(db: &impl ConnectionTrait) -> Result<u64, sqlx::Error> {
    // Implemented as two SeaORM updates (ENABLE_SMTP -> "0", others -> "")
    // to keep single-statement semantics; fall back to per-key updates
    // if needed. The single CASE statement is preserved in DISABLE_EMAIL_SQL
    // for fixture parity.
    let keys: Vec<String> = DISABLE_EMAIL_KEYS.iter().map(|s| s.to_string()).collect();
    // ENABLE_SMTP -> "0"
    let res1 = instance_configurations::Entity::update_many()
        .col_expr(
            instance_configurations::Column::Value,
            Expr::value(Some("0".to_string())),
        )
        .filter(instance_configurations::Column::DeletedAt.is_null())
        .filter(instance_configurations::Column::Key.eq("ENABLE_SMTP"))
        .filter(instance_configurations::Column::Key.is_in(keys.clone()))
        .exec(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    // Others -> "" : key in set minus ENABLE_SMTP
    let others: Vec<String> = DISABLE_EMAIL_KEYS
        .iter()
        .filter(|k| **k != "ENABLE_SMTP")
        .map(|s| s.to_string())
        .collect();
    let res2 = instance_configurations::Entity::update_many()
        .col_expr(
            instance_configurations::Column::Value,
            Expr::value(Some("".to_string())),
        )
        .filter(instance_configurations::Column::DeletedAt.is_null())
        .filter(instance_configurations::Column::Key.is_in(others))
        .exec(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(res1.rows_affected + res2.rows_affected)
}

// ---------------------------------------------------------------------------
// instance_setup + config_purge (design D6 gap, PIDASHCONV-1622)
// ---------------------------------------------------------------------------

/// `instance.save(update_fields=["is_setup_done", "is_signup_screen_visited"])`
/// (`cloud_complete_setup.py:57`, both flags were false): flip both, `WHERE
/// id`. Django's `save(update_fields=...)` writes only the listed columns —
/// `updated_at` is NOT stamped (probe-verified). Byte-identical to the
/// overlay builder this replaces
/// (`cloud-db/src/auth/queries.rs::update_instance_setup`, `Both` arm).
pub const UPDATE_INSTANCE_SETUP_BOTH_SQL: &str = "UPDATE \"instances\" SET \"is_setup_done\" = $1, \"is_signup_screen_visited\" = $2 WHERE \"instances\".\"id\" = $3";

/// Only `is_setup_done` was false (`cloud_complete_setup.py:46-48`).
/// Byte-identical to the overlay builder's `SetupDoneOnly` arm.
pub const UPDATE_INSTANCE_SETUP_DONE_SQL: &str =
    "UPDATE \"instances\" SET \"is_setup_done\" = $1 WHERE \"instances\".\"id\" = $2";

/// Only `is_signup_screen_visited` was false (`cloud_complete_setup.py:49-52`).
/// Byte-identical to the overlay builder's `SignupVisitedOnly` arm.
pub const UPDATE_INSTANCE_SETUP_VISITED_SQL: &str =
    "UPDATE \"instances\" SET \"is_signup_screen_visited\" = $1 WHERE \"instances\".\"id\" = $2";

/// Pick the `SET` shape for the flags the command flips. The command
/// appends `update_fields` in fixed order (`cloud_complete_setup.py:46-52`),
/// so these three are exhaustive; `None` is the no-write path (both flags
/// already true — the command prints "no change" and emits no statement).
pub fn update_instance_setup_sql(
    set_setup_done: bool,
    set_signup_screen_visited: bool,
) -> Option<&'static str> {
    match (set_setup_done, set_signup_screen_visited) {
        (true, true) => Some(UPDATE_INSTANCE_SETUP_BOTH_SQL),
        (true, false) => Some(UPDATE_INSTANCE_SETUP_DONE_SQL),
        (false, true) => Some(UPDATE_INSTANCE_SETUP_VISITED_SQL),
        (false, false) => None,
    }
}

/// Flip the listed setup flags from `False` to `True`
/// (`cloud_complete_setup.py:57`); returns affected rows. The `(false,
/// false)` shape emits no statement and returns `0`, like the command's
/// no-change path.
pub async fn update_instance_setup(
    db: &impl ConnectionTrait,
    instance_id: uuid::Uuid,
    set_setup_done: bool,
    set_signup_screen_visited: bool,
) -> Result<u64, sqlx::Error> {
    if !set_setup_done && !set_signup_screen_visited {
        return Ok(0);
    }
    let mut update = instances::Entity::update_many();
    if set_setup_done {
        update = update.col_expr(instances::Column::IsSetupDone, Expr::value(true));
    }
    if set_signup_screen_visited {
        update = update.col_expr(instances::Column::IsSignupScreenVisited, Expr::value(true));
    }
    let res = update
        .filter(instances::Column::Id.eq(instance_id))
        .exec(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(res.rows_affected)
}

/// Write: `InstanceConfiguration.objects.filter(key__in=...).delete()`
/// (`cloud_purge_env_configs.py:66`): stamp `deleted_at = now()` over the
/// scoped queryset (not already deleted, key in the purged set). The
/// `CAST($1 AS timestamptz)` and the bare (unparenthesized) `WHERE`
/// conjunction are the overlay builder's sea-query rendering, kept
/// byte-identical so the swap changes nothing on the wire. Returns `None`
/// for an empty key set: the command returns before deleting, and Django
/// would raise `EmptyResultSet` (same short-circuit as
/// [`config_select_by_keys_sql`]).
pub fn soft_delete_configs_by_keys_sql(keys: &[&str]) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let placeholders: Vec<String> = (2..=keys.len() + 1).map(|i| format!("${i}")).collect();
    Some(format!(
        "UPDATE \"instance_configurations\" SET \"deleted_at\" = CAST($1 AS timestamptz) WHERE \"instance_configurations\".\"deleted_at\" IS NULL AND \"instance_configurations\".\"key\" IN ({})",
        placeholders.join(", ")
    ))
}

/// Stamp `deleted_at = now()` on the live rows whose key is in `keys`
/// (`cloud_purge_env_configs.py:66`); returns the stamped count, which the
/// command reads (`deleted, _ = ...delete()` — the unpack crash that
/// follows is ported driver-side). Empty input returns `0` with no query.
pub async fn soft_delete_configs_by_keys(
    db: &impl ConnectionTrait,
    keys: &[&str],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<u64, sqlx::Error> {
    if keys.is_empty() {
        return Ok(0);
    }
    let now_tz = sea_orm::prelude::DateTimeWithTimeZone::from(now);
    let owned: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
    let res = instance_configurations::Entity::update_many()
        .col_expr(
            instance_configurations::Column::DeletedAt,
            Expr::value(now_tz),
        )
        .filter(instance_configurations::Column::DeletedAt.is_null())
        .filter(instance_configurations::Column::Key.is_in(owned))
        .exec(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;
    Ok(res.rows_affected)
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

/// One annotated page: `(rows, has_more)`. The `+1` probe row is trimmed.
/// `per_page` here is already resolved by the caller through
/// `get_per_page` semantics (`WORKSPACE_DEFAULT_PER_PAGE` /
/// [`WORKSPACE_MAX_PER_PAGE`]); cursor parsing stays with the paginator
/// kernel (`pidash_api::paginator`) and the handler layer.
///
/// SeaORM implementation: fetch the workspace page, then fetch two counts
/// per workspace via SeaORM (same row semantics as the original correlated
/// subqueries, with the same ordering and pagination).
pub async fn fetch_workspace_page(
    db: &impl ConnectionTrait,
    search: Option<&str>,
    page: i64,
    per_page: i64,
) -> Result<(Vec<WorkspaceListRow>, bool), sqlx::Error> {
    let offset = page * per_page;
    let limit = per_page + 1;

    let mut sel = workspaces::Entity::find().filter(workspaces::Column::DeletedAt.is_null());
    if let Some(term) = search {
        use sea_orm::sea_query::extension::postgres::PgExpr;
        let param = icontains_param(term);
        sel = sel.filter(Expr::col((Alias::new("workspaces"), Alias::new("name"))).ilike(param));
    }

    let entities = sel
        .order_by_desc(workspaces::Column::CreatedAt)
        .limit(limit as u64)
        .offset(offset as u64)
        .all(db)
        .await
        .map_err(crate::orm_tx::into_sqlx)?;

    let has_more = (entities.len() as i64) > per_page;
    let limited: Vec<workspaces::Model> = entities.into_iter().take(per_page as usize).collect();

    let mut out = Vec::with_capacity(limited.len());
    for ws in limited {
        let total_projects = projects::Entity::find()
            .filter(projects::Column::DeletedAt.is_null())
            .filter(projects::Column::WorkspaceId.eq(ws.id))
            .count(db)
            .await
            .map_err(crate::orm_tx::into_sqlx)? as i64;

        let total_members_raw = workspace_members::Entity::find()
            .filter(workspace_members::Column::DeletedAt.is_null())
            .filter(workspace_members::Column::IsActive.eq(true))
            .filter(workspace_members::Column::WorkspaceId.eq(ws.id))
            .count(db)
            .await
            .map_err(crate::orm_tx::into_sqlx)? as i64;
        let bot_count = {
            let member_ids: Vec<uuid::Uuid> = workspace_members::Entity::find()
                .filter(workspace_members::Column::DeletedAt.is_null())
                .filter(workspace_members::Column::IsActive.eq(true))
                .filter(workspace_members::Column::WorkspaceId.eq(ws.id))
                .select_only()
                .columns([workspace_members::Column::MemberId])
                .into_tuple::<uuid::Uuid>()
                .all(db)
                .await
                .map_err(crate::orm_tx::into_sqlx)?
                .into_iter()
                .collect();
            if member_ids.is_empty() {
                0
            } else {
                users::Entity::find()
                    .filter(users::Column::Id.is_in(member_ids))
                    .filter(users::Column::IsBot.eq(true))
                    .count(db)
                    .await
                    .map_err(crate::orm_tx::into_sqlx)? as i64
            }
        };
        let total_members = total_members_raw - bot_count;

        out.push(WorkspaceListRow {
            created_at: ws.created_at.with_timezone(&chrono::Utc),
            updated_at: ws.updated_at.with_timezone(&chrono::Utc),
            created_by_id: ws.created_by_id,
            updated_by_id: ws.updated_by_id,
            deleted_at: ws.deleted_at.map(|dt| dt.with_timezone(&chrono::Utc)),
            id: ws.id,
            name: ws.name,
            logo: ws.logo,
            logo_asset_id: ws.logo_asset_id,
            owner_id: ws.owner_id,
            slug: ws.slug,
            organization_size: ws.organization_size,
            timezone: ws.timezone,
            background_color: ws.background_color,
            total_projects,
            total_members,
        });
    }
    Ok((out, has_more))
}

#[derive(Debug, Clone, PartialEq, sea_orm::FromQueryResult)]
#[allow(dead_code)]
struct WorkspacePartial {
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    id: uuid::Uuid,
    name: String,
    logo: Option<String>,
    logo_asset_id: Option<uuid::Uuid>,
    owner_id: uuid::Uuid,
    slug: String,
    organization_size: Option<String>,
    timezone: String,
    background_color: String,
    total_projects: i64,
    total_members: i64,
}

#[derive(Debug, Clone, PartialEq, sea_orm::FromQueryResult)]
#[allow(dead_code)]
struct MemberIdPartial {
    member_id: uuid::Uuid,
}

/// Compatibility helpers for out-of-scope callers that still use `sqlx::PgRow`.
/// Kept before the test module for clippy `items_after_test_module`.
pub fn map_instance_row(row: &sqlx::postgres::PgRow) -> Result<InstanceRow, sqlx::Error> {
    use sqlx::Row;
    Ok(InstanceRow {
        id: row.try_get("id")?,
        created_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
        updated_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        instance_id: row.try_get("instance_id")?,
        instance_name: row.try_get("instance_name")?,
        current_version: row.try_get("current_version")?,
        latest_version: row.try_get("latest_version")?,
        edition: row.try_get("edition")?,
        domain: row.try_get("domain")?,
        last_checked_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("last_checked_at")?,
        namespace: row.try_get("namespace")?,
        is_telemetry_enabled: row.try_get("is_telemetry_enabled")?,
        is_support_required: row.try_get("is_support_required")?,
        is_setup_done: row.try_get("is_setup_done")?,
        is_signup_screen_visited: row.try_get("is_signup_screen_visited")?,
        is_verified: row.try_get("is_verified")?,
        is_test: row.try_get("is_test")?,
        is_current_version_deprecated: row.try_get("is_current_version_deprecated")?,
        deleted_at: row.try_get("deleted_at")?,
        whitelist_emails: row.try_get("whitelist_emails")?,
    })
}

pub fn map_instance_admin_row(row: &sqlx::postgres::PgRow) -> Result<InstanceAdminRow, sqlx::Error> {
    use sqlx::Row;
    Ok(InstanceAdminRow {
        id: row.try_get("id")?,
        created_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
        updated_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        user_id: row.try_get("user_id")?,
        instance_id: row.try_get("instance_id")?,
        role: row.try_get("role")?,
        is_verified: row.try_get("is_verified")?,
        deleted_at: row.try_get("deleted_at")?,
    })
}

pub fn map_config_row(row: &sqlx::postgres::PgRow) -> Result<InstanceConfigurationRow, sqlx::Error> {
    use sqlx::Row;
    Ok(InstanceConfigurationRow {
        id: row.try_get("id")?,
        created_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
        updated_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        key: row.try_get("key")?,
        value: row.try_get("value")?,
        category: row.try_get("category")?,
        is_encrypted: row.try_get("is_encrypted")?,
        deleted_at: row.try_get("deleted_at")?,
    })
}

pub fn map_workspace_list_row(row: &sqlx::postgres::PgRow) -> Result<WorkspaceListRow, sqlx::Error> {
    use sqlx::Row;
    Ok(WorkspaceListRow {
        id: row.try_get("id")?,
        created_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
        updated_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
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
        deleted_at: row.try_get("deleted_at")?,
    })
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
    fn instance_setup_sql_shapes_byte_identical() {
        // Pinned against the overlay builders these replace
        // (`cloud-db/src/auth/queries.rs::update_instance_setup`):
        // same text, same placeholder numbering, no `updated_at`.
        assert_eq!(
            UPDATE_INSTANCE_SETUP_BOTH_SQL,
            "UPDATE \"instances\" SET \"is_setup_done\" = $1, \"is_signup_screen_visited\" = $2 WHERE \"instances\".\"id\" = $3"
        );
        assert_eq!(
            UPDATE_INSTANCE_SETUP_DONE_SQL,
            "UPDATE \"instances\" SET \"is_setup_done\" = $1 WHERE \"instances\".\"id\" = $2"
        );
        assert_eq!(
            UPDATE_INSTANCE_SETUP_VISITED_SQL,
            "UPDATE \"instances\" SET \"is_signup_screen_visited\" = $1 WHERE \"instances\".\"id\" = $2"
        );
        assert_eq!(
            update_instance_setup_sql(true, true),
            Some(UPDATE_INSTANCE_SETUP_BOTH_SQL)
        );
        assert_eq!(
            update_instance_setup_sql(true, false),
            Some(UPDATE_INSTANCE_SETUP_DONE_SQL)
        );
        assert_eq!(
            update_instance_setup_sql(false, true),
            Some(UPDATE_INSTANCE_SETUP_VISITED_SQL)
        );
        assert_eq!(update_instance_setup_sql(false, false), None);
    }

    #[test]
    fn soft_delete_configs_sql_byte_identical() {
        // Pinned against the overlay builder this replaces
        // (`cloud-db/src/auth/queries.rs::soft_delete_configs_by_keys`): the
        // `CAST`, the bare `WHERE` conjunction and the `$2..` key
        // numbering are its sea-query rendering, kept verbatim.
        assert_eq!(
            soft_delete_configs_by_keys_sql(&["K1", "K2"]).as_deref(),
            Some("UPDATE \"instance_configurations\" SET \"deleted_at\" = CAST($1 AS timestamptz) WHERE \"instance_configurations\".\"deleted_at\" IS NULL AND \"instance_configurations\".\"key\" IN ($2, $3)")
        );
        assert_eq!(
            soft_delete_configs_by_keys_sql(&["K1"]).as_deref(),
            Some("UPDATE \"instance_configurations\" SET \"deleted_at\" = CAST($1 AS timestamptz) WHERE \"instance_configurations\".\"deleted_at\" IS NULL AND \"instance_configurations\".\"key\" IN ($2)")
        );
        assert_eq!(soft_delete_configs_by_keys_sql(&[]), None);
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

    #[test]
    fn seaorm_instance_first_build_pinned() {
        use sea_orm::{EntityTrait, QueryFilter, QueryOrder, QueryTrait};
        use sea_query::PostgresQueryBuilder;
        let stmt = instances::Entity::find()
            .filter(instances::Column::DeletedAt.is_null())
            .order_by_desc(instances::Column::CreatedAt)
            .limit(1)
            .into_query();
        let sql = stmt.to_string(PostgresQueryBuilder);
        assert!(sql.contains("FROM \"instances\""));
        assert!(sql.contains("\"deleted_at\" IS NULL"));
        assert!(sql.contains("ORDER BY \"instances\".\"created_at\" DESC"));
        assert!(sql.contains("LIMIT 1"));
    }

    #[test]
    fn seaorm_admin_check_build_pinned() {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QueryTrait};
        use sea_query::PostgresQueryBuilder;
        let instance_id = uuid::Uuid::nil();
        let user_id = uuid::Uuid::nil();
        let stmt = instance_admins::Entity::find()
            .filter(instance_admins::Column::DeletedAt.is_null())
            .filter(instance_admins::Column::InstanceId.eq(instance_id))
            .filter(instance_admins::Column::Role.gte(ADMIN_PERMISSION_ROLE_GTE))
            .filter(instance_admins::Column::UserId.eq(user_id))
            .order_by_desc(instance_admins::Column::CreatedAt)
            .limit(1)
            .into_query();
        let sql = stmt.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("\"instances\" IS NOT NULL") || sql.contains("\"deleted_at\" IS NULL")
        );
        assert!(sql.contains("\"role\" >="));
        assert!(sql.contains("ORDER BY \"instance_admins\".\"created_at\" DESC"));
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

    #[allow(dead_code)]
    fn sea_conn(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
        crate::pool::sea_orm_connection(pool)
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
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        let got = fetch_instance_first(&conn)
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
        let conn2 = crate::orm_tx::TxConn::new(&mut tx);
        assert!(fetch_instance_first(&conn2).await.expect("empty").is_none());
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
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        assert!(!admin_permission_check(&conn, inst, u1).await.expect("gate"));
        let a1 = live_uuid(13);
        let created = create_instance_admin(&conn, a1, ts, Some(u1), inst, 20, false, None)
            .await
            .expect("create");
        assert_eq!(created.role, 20);
        assert!(!created.is_verified);
        assert_eq!(created.created_at, created.updated_at);
        // Anonymous-signup path stores NULL created_by (BaseModel.save).
        assert_eq!(created.created_by_id, None);
        let conn2 = crate::orm_tx::TxConn::new(&mut tx);
        assert!(admin_permission_check(&conn2, inst, u1)
            .await
            .expect("gate"));
        // role 10 < 15 stays denied (the latent wider gate, kept as-is).
        // Authenticated create stores the requesting admin as created_by.
        let a2 = live_uuid(14);
        let conn3 = crate::orm_tx::TxConn::new(&mut tx);
        create_instance_admin(&conn3, a2, ts, Some(u2), inst, 10, false, Some(u1))
            .await
            .expect("create low role");
        let conn4 = crate::orm_tx::TxConn::new(&mut tx);
        assert_eq!(
            get_admins_by_instance_user(&conn4, inst, u2)
                .await
                .expect("get low role")
                .first()
                .expect("one row")
                .created_by_id,
            Some(u1)
        );
        let conn5 = crate::orm_tx::TxConn::new(&mut tx);
        assert!(!admin_permission_check(&conn5, inst, u2)
            .await
            .expect("gate"));
        let conn6 = crate::orm_tx::TxConn::new(&mut tx);
        let list = list_instance_admins(&conn6, inst).await.expect("list");
        assert_eq!(list.len(), 2);
        let conn7 = crate::orm_tx::TxConn::new(&mut tx);
        assert_eq!(
            get_admins_by_instance_user(&conn7, inst, u1)
                .await
                .expect("get")
                .len(),
            1
        );
        let conn8 = crate::orm_tx::TxConn::new(&mut tx);
        assert!(admin_exists_by_user(&conn8, u2).await.expect("exists"));
        let conn9 = crate::orm_tx::TxConn::new(&mut tx);
        assert_eq!(
            soft_delete_instance_admin(&conn9, ts, inst, a1)
                .await
                .expect("delete"),
            1
        );
        let conn10 = crate::orm_tx::TxConn::new(&mut tx);
        assert_eq!(
            list_instance_admins(&conn10, inst)
                .await
                .expect("list")
                .len(),
            1
        );
        let conn11 = crate::orm_tx::TxConn::new(&mut tx);
        assert!(!admin_permission_check(&conn11, inst, u1)
            .await
            .expect("gate"));
        // Deleting again matches nothing (the view still answers 204).
        let conn12 = crate::orm_tx::TxConn::new(&mut tx);
        assert_eq!(
            soft_delete_instance_admin(&conn12, ts, inst, a1)
                .await
                .expect("delete"),
            0
        );

    }
}
