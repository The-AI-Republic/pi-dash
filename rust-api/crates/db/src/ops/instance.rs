//! SQL for the ops `instance` command group (D-37, PIDASHCONV-809).
//!
//! Ports the queries behind `ensure_project_pods.py:30-83` and
//! `dry_run_scheduler_migration.py:71-117`. `configure_instance` and
//! `register_instance` reuse the D-01 stores
//! (`pidash_jobs::license::commands::{PgSeedStore, PgInstanceStore}`),
//! so no `InstanceConfiguration`/`Instance` SQL lives here.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

// ---------------------------------------------------------------------------
// ensure_project_pods
// ---------------------------------------------------------------------------

/// One project row needed by the pod scan
/// (`ensure_project_pods.py:42-45,66-79`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodScanRow {
    pub id: uuid::Uuid,
    pub identifier: String,
    pub workspace_id: uuid::Uuid,
    pub project_lead_id: Option<uuid::Uuid>,
    pub default_assignee_id: Option<uuid::Uuid>,
}

/// Projects without an active pod, newest first. `Project.objects.all()`
/// carries `Meta.ordering = ("-created_at",)` over the soft-delete-scoped
/// default manager (`db/mixins.py:62-67`); the per-project
/// `Pod.objects.filter(project=...).exists()` folds into the `NOT EXISTS`.
pub const POD_SCAN_SQL: &str = "SELECT p.id, p.identifier, p.workspace_id, p.project_lead_id, p.default_assignee_id FROM projects p WHERE p.deleted_at IS NULL AND NOT EXISTS (SELECT 1 FROM pod WHERE pod.project_id = p.id AND pod.deleted_at IS NULL) ORDER BY p.created_at DESC";

/// Fetch the projects missing a pod, in scan order.
pub async fn scan_projects_missing_pods(pool: &PgPool) -> Result<Vec<PodScanRow>, sqlx::Error> {
    let rows = sqlx::query(POD_SCAN_SQL).fetch_all(pool).await?;
    rows.iter()
        .map(|row| {
            Ok(PodScanRow {
                id: row.try_get("id")?,
                identifier: row.try_get("identifier")?,
                workspace_id: row.try_get("workspace_id")?,
                project_lead_id: row.try_get("project_lead_id")?,
                default_assignee_id: row.try_get("default_assignee_id")?,
            })
        })
        .collect()
}

/// `Pod.objects.get_or_create(project, name, defaults=...)` as one
/// statement. The partial unique index
/// `pod_unique_name_per_project_when_active` ignores soft-deleted rows,
/// exactly like the `get_or_create` lookup under `PodManager`, so
/// `ON CONFLICT ... WHERE deleted_at IS NULL DO NOTHING` resolves races
/// and deleted-name shadows to not-created, like Django's retry-`get`.
/// A single statement is already atomic, matching the per-project
/// `transaction.atomic()` block (`:65-79`).
pub const POD_INSERT_SQL: &str = "INSERT INTO pod (id, name, description, is_default, deleted_at, created_at, updated_at, created_by_id, workspace_id, project_id) VALUES ($1, $2, 'Auto-created default pod by ensure_project_pods.', true, NULL, now(), now(), $3, $4, $5) ON CONFLICT (project_id, name) WHERE deleted_at IS NULL DO NOTHING RETURNING id";

/// Insert the default pod; `Ok(true)` when this call created it.
pub async fn insert_default_pod(
    pool: &PgPool,
    project: &PodScanRow,
    pod_name: &str,
    created_by_id: Option<uuid::Uuid>,
) -> Result<bool, sqlx::Error> {
    let inserted: Option<uuid::Uuid> = sqlx::query_scalar(POD_INSERT_SQL)
        .bind(uuid::Uuid::new_v4())
        .bind(pod_name)
        .bind(created_by_id)
        .bind(project.workspace_id)
        .bind(project.id)
        .fetch_optional(pool)
        .await?;
    Ok(inserted.is_some())
}

// ---------------------------------------------------------------------------
// dry_run_scheduler_migration
// ---------------------------------------------------------------------------

/// `information_schema` probe for the pre-migration `cron` column
/// (`dry_run_scheduler_migration.py:74-80`).
pub const CRON_COLUMN_PROBE_SQL: &str = "SELECT 1 FROM information_schema.columns WHERE table_name = 'scheduler_bindings' AND column_name = 'cron'";

/// Whether the `cron` column still exists (pre-migration).
pub async fn has_cron_column(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(CRON_COLUMN_PROBE_SQL)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// One dry-run input row: the `.values("id", "workspace_id",
/// "workspace__slug", "created_at", "cron", "enabled")` shape
/// (`:108-117`) read through raw SQL, per the comment at `:111-114`
/// (the model no longer carries the field, so `.values("cron")`
/// raises `FieldError` — the live Django branch crashes; the Rust
/// port implements the evident intent). `workspace` is non-null in
/// practice (`workspace` FK is non-nullable, hence the inner join).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunRow {
    pub id: uuid::Uuid,
    pub workspace_slug: Option<String>,
    pub created_at: DateTime<Utc>,
    pub cron: Option<String>,
    pub enabled: bool,
}

/// Unfiltered rows in `.values()` query order: the model declares
/// `Meta.ordering = ("-created_at",)`, preserved by `.values()`
/// (`scheduler.py:235`), soft-delete-scoped like every
/// default-manager query.
pub const DRY_RUN_ROWS_SQL: &str = "SELECT b.id, w.slug AS workspace_slug, b.created_at, b.cron, b.enabled FROM scheduler_bindings b INNER JOIN workspaces w ON w.id = b.workspace_id WHERE b.deleted_at IS NULL ORDER BY b.created_at DESC";

/// Rows limited to one workspace slug (`--workspace`, `:108-109`),
/// same newest-first order.
pub const DRY_RUN_ROWS_BY_WORKSPACE_SQL: &str = "SELECT b.id, w.slug AS workspace_slug, b.created_at, b.cron, b.enabled FROM scheduler_bindings b INNER JOIN workspaces w ON w.id = b.workspace_id WHERE b.deleted_at IS NULL AND w.slug = $1 ORDER BY b.created_at DESC";

fn map_dry_run_row(row: &sqlx::postgres::PgRow) -> Result<DryRunRow, sqlx::Error> {
    Ok(DryRunRow {
        id: row.try_get("id")?,
        workspace_slug: row.try_get("workspace_slug")?,
        created_at: row.try_get("created_at")?,
        cron: row.try_get("cron")?,
        enabled: row.try_get("enabled")?,
    })
}

/// Fetch the dry-run input rows, optionally limited to one workspace.
pub async fn fetch_dry_run_rows(
    pool: &PgPool,
    workspace_slug: Option<&str>,
) -> Result<Vec<DryRunRow>, sqlx::Error> {
    let pg_rows = match workspace_slug {
        Some(slug) => {
            sqlx::query(DRY_RUN_ROWS_BY_WORKSPACE_SQL)
                .bind(slug)
                .fetch_all(pool)
                .await?
        }
        None => sqlx::query(DRY_RUN_ROWS_SQL).fetch_all(pool).await?,
    };
    pg_rows.iter().map(map_dry_run_row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pod_scan_orders_newest_first_over_active_rows() {
        assert!(POD_SCAN_SQL.contains("p.deleted_at IS NULL"));
        assert!(POD_SCAN_SQL.contains("pod.deleted_at IS NULL"));
        assert!(POD_SCAN_SQL.contains("ORDER BY p.created_at DESC"));
    }

    #[test]
    fn pod_insert_targets_the_partial_unique_index() {
        assert!(POD_INSERT_SQL
            .contains("ON CONFLICT (project_id, name) WHERE deleted_at IS NULL DO NOTHING"));
        assert!(POD_INSERT_SQL.contains("Auto-created default pod by ensure_project_pods."));
    }

    #[test]
    fn dry_run_probe_matches_python() {
        assert_eq!(
            CRON_COLUMN_PROBE_SQL,
            "SELECT 1 FROM information_schema.columns WHERE table_name = 'scheduler_bindings' AND column_name = 'cron'"
        );
    }

    #[test]
    fn dry_run_rows_join_workspace_newest_first() {
        for sql in [DRY_RUN_ROWS_SQL, DRY_RUN_ROWS_BY_WORKSPACE_SQL] {
            assert!(sql.contains("INNER JOIN workspaces w ON w.id = b.workspace_id"));
            assert!(sql.contains("b.deleted_at IS NULL"));
            assert!(sql.contains("ORDER BY b.created_at DESC"));
        }
        assert!(DRY_RUN_ROWS_BY_WORKSPACE_SQL.contains("w.slug = $1"));
    }
}
