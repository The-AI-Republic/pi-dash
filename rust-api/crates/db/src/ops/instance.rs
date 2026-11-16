//! SQL for the ops `instance` command group (D-37, PIDASHCONV-809).
//!
//! Ports the queries behind `ensure_project_pods.py:30-83` and
//! `dry_run_scheduler_migration.py:71-117`. `configure_instance` and
//! `register_instance` reuse the D-01 stores
//! (`pidash_jobs::license::commands::{PgSeedStore, PgInstanceStore}`),
//! so no `InstanceConfiguration`/`Instance` SQL lives here.
//!
//! Every executor is SeaORM over `sea_orm_connection(pool)` with the
//! `sqlx::Error` signatures kept through `orm_tx::into_sqlx`
//! (`orm-migration.md`). The pod upsert keeps the partial-unique-index
//! `ON CONFLICT ... WHERE deleted_at IS NULL DO NOTHING RETURNING id`
//! shape via a sea-query `InsertStatement` (`OnConflict::target_cond_where`),
//! and the dry-run read uses a sea-query `SelectStatement` because the
//! pre-migration `cron` column is not on the `scheduler_bindings` entity.

use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QueryOrder, QueryResult, QuerySelect, RelationTrait, Select,
};
use sea_query::{Alias, Expr, OnConflict, Query};

use crate::entity::{pod, projects, scheduler_bindings, workspaces};
use crate::orm_tx::into_sqlx;
use crate::pool::sea_orm_connection;

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

#[derive(Debug, Clone, FromQueryResult)]
struct PodScanLookupRow {
    id: uuid::Uuid,
    identifier: String,
    workspace_id: uuid::Uuid,
    project_lead_id: Option<uuid::Uuid>,
    default_assignee_id: Option<uuid::Uuid>,
}

/// Projects without an active pod, newest first. `Project.objects.all()`
/// carries `Meta.ordering = ("-created_at",)` over the soft-delete-scoped
/// default manager (`db/mixins.py:62-67`); the per-project
/// `Pod.objects.filter(project=...).exists()` folds into the
/// `NOT EXISTS`. The port renders the same fold as a `LEFT OUTER JOIN`
/// anti-join (`pod.id IS NULL`): per-project multiplicity from the join
/// is impossible because only pod ids are joined, and rows the `NOT
/// EXISTS` keeps are exactly the rows whose join partner is absent.
fn scan_projects_missing_pods_query() -> Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::Identifier,
            projects::Column::WorkspaceId,
            projects::Column::ProjectLeadId,
            projects::Column::DefaultAssigneeId,
        ])
        .join_rev(JoinType::LeftJoin, pod::Relation::Projects.def())
        .filter(projects::Column::DeletedAt.is_null())
        .filter(pod::Column::DeletedAt.is_null())
        .filter(pod::Column::Id.is_null())
        .order_by_desc(projects::Column::CreatedAt)
}

/// Fetch the projects missing a pod, in scan order.
pub async fn scan_projects_missing_pods(
    pool: &sqlx::PgPool,
) -> Result<Vec<PodScanRow>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let rows = scan_projects_missing_pods_query()
        .into_model::<PodScanLookupRow>()
        .all(&db)
        .await
        .map_err(into_sqlx)?;
    Ok(rows
        .into_iter()
        .map(|row| PodScanRow {
            id: row.id,
            identifier: row.identifier,
            workspace_id: row.workspace_id,
            project_lead_id: row.project_lead_id,
            default_assignee_id: row.default_assignee_id,
        })
        .collect())
}

/// The `Pod.objects.get_or_create(project, name, defaults=...)` insert.
/// The partial unique index
/// `pod_unique_name_per_project_when_active` ignores soft-deleted rows,
/// exactly like the `get_or_create` lookup under `PodManager`, so
/// `ON CONFLICT ... WHERE deleted_at IS NULL DO NOTHING` resolves races
/// and deleted-name shadows to not-created, like Django's retry-`get`.
/// A single statement is already atomic, matching the per-project
/// `transaction.atomic()` block (`:65-79`).
fn insert_default_pod_stmt(
    id: uuid::Uuid,
    pod_name: &str,
    created_by_id: Option<uuid::Uuid>,
    project: &PodScanRow,
    now: DateTime<Utc>,
) -> sea_query::InsertStatement {
    let mut stmt = Query::insert();
    stmt.into_table(pod::Entity);
    stmt.columns([
        pod::Column::Id,
        pod::Column::Name,
        pod::Column::Description,
        pod::Column::IsDefault,
        pod::Column::DeletedAt,
        pod::Column::CreatedAt,
        pod::Column::UpdatedAt,
        pod::Column::CreatedById,
        pod::Column::WorkspaceId,
        pod::Column::ProjectId,
    ]);
    stmt.values_panic([
        id.into(),
        pod_name.to_owned().into(),
        "Auto-created default pod by ensure_project_pods."
            .to_owned()
            .into(),
        true.into(),
        Expr::val(None::<sea_orm::prelude::DateTimeWithTimeZone>).into(),
        now.into(),
        now.into(),
        created_by_id
            .map(|id| Expr::val(id).into())
            .unwrap_or_else(|| Expr::val(None::<uuid::Uuid>).into()),
        project.workspace_id.into(),
        project.id.into(),
    ]);
    stmt.on_conflict(
        OnConflict::columns([pod::Column::ProjectId, pod::Column::Name])
            .target_cond_where(pod::Column::DeletedAt.is_null())
            .do_nothing()
            .to_owned(),
    );
    stmt.returning_col(pod::Column::Id);
    stmt
}

/// Insert the default pod; `Ok(true)` when this call created it.
pub async fn insert_default_pod(
    pool: &sqlx::PgPool,
    project: &PodScanRow,
    pod_name: &str,
    created_by_id: Option<uuid::Uuid>,
) -> Result<bool, sqlx::Error> {
    let stmt = insert_default_pod_stmt(
        uuid::Uuid::new_v4(),
        pod_name,
        created_by_id,
        project,
        chrono::Utc::now(),
    );
    let db = sea_orm_connection(pool);
    let row: Option<QueryResult> = db
        .query_one(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(into_sqlx)?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// dry_run_scheduler_migration
// ---------------------------------------------------------------------------

/// `information_schema` probe for the pre-migration `cron` column
/// (`dry_run_scheduler_migration.py:74-80`), as a sea-query select over
/// the view (which has no entity): one row iff the column exists.
fn has_cron_column_stmt() -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.column(Alias::new("table_name"));
    stmt.from((Alias::new("information_schema"), Alias::new("columns")));
    stmt.and_where(Expr::col(Alias::new("table_name")).eq("scheduler_bindings"));
    stmt.and_where(Expr::col(Alias::new("column_name")).eq("cron"));
    stmt
}

/// Whether the `cron` column still exists (pre-migration).
pub async fn has_cron_column(pool: &sqlx::PgPool) -> Result<bool, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let rows = db
        .query_all(DbBackend::Postgres.build(&has_cron_column_stmt()))
        .await
        .map_err(into_sqlx)?;
    Ok(!rows.is_empty())
}

/// One dry-run input row: the `.values("id", "workspace_id",
/// "workspace__slug", "created_at", "cron", "enabled")` shape
/// (`:108-117`) read through sea-query (`cron` is not on the
/// `scheduler_bindings` entity — the model no longer carries the
/// pre-migration field, per the comment at `:111-114`: `.values("cron")`
/// raises `FieldError`, the live Django branch crashes; the Rust port
/// implements the evident intent). `workspace` is non-null in practice
/// (`workspace` FK is non-nullable, hence the inner join).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunRow {
    pub id: uuid::Uuid,
    pub workspace_slug: Option<String>,
    pub created_at: DateTime<Utc>,
    pub cron: Option<String>,
    pub enabled: bool,
}

/// The dry-run select: unfiltered rows, or narrowed to one workspace
/// slug (`--workspace`, `:108-109`). The model declares
/// `Meta.ordering = ("-created_at",)`, preserved by `.values()`
/// (`scheduler.py:235`), soft-delete-scoped like every
/// default-manager query.
fn dry_run_rows_stmt(workspace_slug: Option<&str>) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.expr_as(
        Expr::col((scheduler_bindings::Entity, scheduler_bindings::Column::Id)),
        "id",
    );
    stmt.expr_as(
        Expr::col((workspaces::Entity, workspaces::Column::Slug)),
        "workspace_slug",
    );
    stmt.expr_as(
        Expr::col((
            scheduler_bindings::Entity,
            scheduler_bindings::Column::CreatedAt,
        )),
        "created_at",
    );
    stmt.expr_as(Expr::col(Alias::new("cron")), "cron");
    stmt.expr_as(
        Expr::col((
            scheduler_bindings::Entity,
            scheduler_bindings::Column::Enabled,
        )),
        "enabled",
    );
    stmt.from(scheduler_bindings::Entity);
    stmt.join(
        sea_query::JoinType::InnerJoin,
        workspaces::Entity,
        Expr::col((workspaces::Entity, workspaces::Column::Id)).equals((
            scheduler_bindings::Entity,
            scheduler_bindings::Column::WorkspaceId,
        )),
    );
    stmt.and_where(
        Expr::col((
            scheduler_bindings::Entity,
            scheduler_bindings::Column::DeletedAt,
        ))
        .is_null(),
    );
    if let Some(slug) = workspace_slug {
        stmt.and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug));
    }
    stmt.order_by(
        (
            scheduler_bindings::Entity,
            scheduler_bindings::Column::CreatedAt,
        ),
        sea_query::Order::Desc,
    );
    stmt
}

fn map_dry_run_row(row: &QueryResult) -> Result<DryRunRow, sqlx::Error> {
    Ok(DryRunRow {
        id: row.try_get_by("id").map_err(into_sqlx)?,
        workspace_slug: row.try_get_by("workspace_slug").map_err(into_sqlx)?,
        created_at: row.try_get_by("created_at").map_err(into_sqlx)?,
        cron: row.try_get_by("cron").map_err(into_sqlx)?,
        enabled: row.try_get_by("enabled").map_err(into_sqlx)?,
    })
}

/// Fetch the dry-run input rows, optionally limited to one workspace.
pub async fn fetch_dry_run_rows(
    pool: &sqlx::PgPool,
    workspace_slug: Option<&str>,
) -> Result<Vec<DryRunRow>, sqlx::Error> {
    let db = sea_orm_connection(pool);
    let rows = db
        .query_all(DbBackend::Postgres.build(&dry_run_rows_stmt(workspace_slug)))
        .await
        .map_err(into_sqlx)?;
    rows.iter().map(map_dry_run_row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn pod_scan_orders_newest_first_over_active_rows() {
        let sql = scan_projects_missing_pods_query()
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#""projects""#), "{sql}");
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""created_at" DESC"#), "{sql}");
        assert!(sql.contains(r#"LEFT JOIN "pod""#), "{sql}");
        assert!(sql.contains(r#""pod"."id" IS NULL"#), "{sql}");
    }

    #[test]
    fn pod_insert_targets_the_partial_unique_index() {
        let row = PodScanRow {
            id: uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid"),
            identifier: "PROJ".to_owned(),
            workspace_id: uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222")
                .expect("uuid"),
            project_lead_id: None,
            default_assignee_id: None,
        };
        let stmt = insert_default_pod_stmt(
            uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid"),
            "Default",
            None,
            &row,
            chrono::Utc::now(),
        );
        let sql = stmt.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(
                r#"ON CONFLICT ("project_id", "name") WHERE "pod"."deleted_at" IS NULL DO NOTHING"#
            ),
            "{sql}"
        );
        assert!(
            sql.contains("Auto-created default pod by ensure_project_pods."),
            "{sql}"
        );
        assert!(sql.contains(r#"RETURNING "id""#), "{sql}");
    }

    #[test]
    fn dry_run_probe_matches_python() {
        let sql = has_cron_column_stmt().to_string(PostgresQueryBuilder);
        assert!(sql.contains("information_schema"), "{sql}");
        assert!(sql.contains(r#""column_name" = 'cron'"#), "{sql}");
        assert!(
            sql.contains(r#""table_name" = 'scheduler_bindings'"#),
            "{sql}"
        );
    }

    #[test]
    fn dry_run_rows_join_workspace_newest_first() {
        for slug in [None, Some("acme")] {
            let sql = dry_run_rows_stmt(slug).to_string(PostgresQueryBuilder);
            assert!(sql.contains(r#"INNER JOIN "workspaces""#), "{sql}");
            assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
            assert!(sql.contains(r#""created_at" DESC"#), "{sql}");
            assert!(sql.contains(r#""cron""#), "{sql}");
        }
        let filtered = dry_run_rows_stmt(Some("acme")).to_string(PostgresQueryBuilder);
        assert!(filtered.contains(r#""slug" = 'acme'"#), "{filtered}");
    }
}
