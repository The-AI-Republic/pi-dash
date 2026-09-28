//! Soft/hard deletion statements and executors.
//!
//! Ports the data-access half of
//! `apps/api/pi_dash/bgtasks/deletion_task.py` (all 193 lines,
//! PIDASHCONV-186). The walk orchestration (relation order, recursion,
//! the ported bugs) lives in `pidash-services`; this module owns SQL
//! shapes, the 18-table hard-delete order, the `(app_label, model_name)`
//! entry map, and the Postgres-catalog discovery that stands in for
//! Django's `_meta` reverse-relation graph.
//!
//! Fixture pins: `rust-api/fixtures/tasks_cleanup/deletion.json`
//! (`hard_delete.first_statements` / `all_statements`,
//! `soft_delete_walk.*`) and `columns.json` (column lists).
//!
//! Query-building uses `sea-query` with every literal inlined, executed
//! through `sqlx::query(&sql)`. Inlining is safe here: table and column
//! names come from internal constants or the live catalog (both quoted by
//! the builder), primary keys are parsed as `Uuid` before use, and the
//! cutoff is formatted by `chrono`. No caller input reaches SQL raw.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
use sqlx::{PgPool, Row};

/// `deleted_at` marker column, as on every `SoftDeleteModel`.
pub const DELETED_AT: &str = "deleted_at";
/// Server-stamped write column (`TimeAuditModel`, `auto_now=True`).
pub const UPDATED_AT: &str = "updated_at";
/// Default primary-key column (every fixture table uses it).
pub const PK_COLUMN: &str = "id";

/// The 18 models `hard_delete` sweeps first, in source order
/// (`deletion_task.py:124-160`, fixture `hard_delete.named_models_in_order`).
/// Values are physical table names, matching the fixture's
/// `first_statements` (`DELETE FROM "workspaces" ...` first).
pub const HARD_DELETE_NAMED_TABLES: [&str; 18] = [
    "workspaces",
    "projects",
    "cycles",
    "modules",
    "issues",
    "pages",
    "issue_views",
    "labels",
    "states",
    "issue_activities",
    "issue_comments",
    "issue_links",
    "issue_reactions",
    "user_favorites",
    "module_issues",
    "cycle_issues",
    "estimates",
    "estimate_points",
];

/// `(app_label, model_name)` entry map for `soft_delete_related_objects`.
///
/// Mirrors `apps.get_model(app_label, model_name)`: Django's `model_name`
/// is the lowercased class name. Every Pi Dash model ported here lives in
/// the `db` app (`pi_dash/db/apps.py` has no explicit label, so Django
/// derives `db`). An unknown pair is a loud error at the services layer
/// (the `apps.get_model` `LookupError` equivalent) — never a silent skip.
const ENTRY_TABLES: &[(&str, &str, &str)] = &[
    ("db", "apiToken", "api_tokens"),
    ("db", "apilog", "api_activity_logs"),
    ("db", "apiactivitylog", "api_activity_logs"),
    ("db", "apitoken", "api_tokens"),
    ("db", "analyticview", "analytic_views"),
    ("db", "cldevicecode", "cli_device_codes"),
    ("db", "clidevicecode", "cli_device_codes"),
    ("db", "cycle", "cycles"),
    ("db", "cycleissue", "cycle_issues"),
    ("db", "cycleuserproperty", "cycle_user_properties"),
    ("db", "dashboard", "dashboards"),
    ("db", "emailnotificationlog", "email_notification_logs"),
    ("db", "estimate", "estimates"),
    ("db", "estimatepoint", "estimate_points"),
    ("db", "exporter", "exporters"),
    ("db", "exporterhistory", "exporters"),
    ("db", "fileasset", "file_assets"),
    ("db", "intake", "intakes"),
    ("db", "intakeissue", "intake_issues"),
    ("db", "issue", "issues"),
    ("db", "issueactivity", "issue_activities"),
    ("db", "issueassignee", "issue_assignees"),
    ("db", "issuelabel", "issue_labels"),
    ("db", "issuerelation", "issue_relations"),
    ("db", "issuesequence", "issue_sequences"),
    ("db", "issuesubscriber", "issue_subscribers"),
    ("db", "issueversion", "issue_versions"),
    ("db", "issuecomment", "issue_comments"),
    (
        "db",
        "issuedescriptionversion",
        "issue_description_versions",
    ),
    ("db", "issuelink", "issue_links"),
    ("db", "issuereaction", "issue_reactions"),
    ("db", "issueview", "issue_views"),
    ("db", "label", "labels"),
    ("db", "module", "modules"),
    ("db", "moduleissue", "module_issues"),
    ("db", "moduleuserproperty", "module_user_properties"),
    ("db", "page", "pages"),
    ("db", "pagelabel", "page_labels"),
    ("db", "pageversion", "page_versions"),
    ("db", "project", "projects"),
    ("db", "projectdeployboard", "project_deploy_boards"),
    ("db", "projectidentifier", "project_identifiers"),
    ("db", "projectmember", "project_members"),
    ("db", "projectmemberinvite", "project_member_invites"),
    ("db", "projectpage", "project_pages"),
    ("db", "projectpublicmember", "project_public_members"),
    ("db", "projectuserproperty", "project_user_properties"),
    ("db", "state", "states"),
    ("db", "user", "users"),
    ("db", "userfavorite", "user_favorites"),
    ("db", "webhooklog", "webhook_logs"),
    ("db", "workspace", "workspaces"),
    ("db", "workspacemember", "workspace_members"),
    ("db", "workspacememberinvite", "workspace_member_invites"),
];

/// Resolve the entry table for `(app_label, model_name)`, or `None` when
/// Django's `apps.get_model` would raise `LookupError`.
pub fn entry_table_for(app_label: &str, model_name: &str) -> Option<&'static str> {
    ENTRY_TABLES
        .iter()
        .find(|(app, model, _)| *app == app_label && *model == model_name)
        .map(|(_, _, table)| *table)
}

/// Format a cutoff the way Django renders a `timestamptz` literal:
/// `2026-08-29 06:00:00+00:00` (fixture `hard_delete.first_statements`).
pub fn format_cutoff(cutoff: DateTime<Utc>) -> String {
    cutoff.format("%Y-%m-%d %H:%M:%S%:z").to_string()
}

/// `cutoff = now - days`, formatted for the hard-delete predicate.
/// `days` is `settings.HARD_DELETE_AFTER_DAYS`
/// (`deletion_task.py:121`; Rust default 60, same as Django).
pub fn cutoff_for(days: i64, now: DateTime<Utc>) -> String {
    format_cutoff(now - chrono::Duration::days(days))
}

/// `DELETE FROM "<table>" WHERE "<table>"."deleted_at" < '<cutoff>'`
/// (`model.all_objects.filter(deleted_at__lt=cutoff).delete()`:
/// `all_objects` is unscoped, so no `deleted_at IS NULL` guard).
pub fn hard_delete_before_sql(table: &str, cutoff: &str) -> String {
    Query::delete()
        .from_table(Alias::new(table.to_owned()))
        .and_where(
            Expr::col((
                Alias::new(table.to_owned()),
                Alias::new(DELETED_AT.to_owned()),
            ))
            .lt(cutoff),
        )
        .to_string(PostgresQueryBuilder)
}

/// Full-save stamp: `UPDATE "<table>" SET deleted_at + updated_at WHERE pk`
/// (instance `save()`: `deleted_at = now()` plus the `auto_now`
/// `updated_at` bump; `updated_at` only when the table has it).
pub fn soft_stamp_sql(
    table: &str,
    pk_col: &str,
    pk: &uuid::Uuid,
    now: &str,
    has_updated_at: bool,
) -> String {
    let mut stmt = Query::update();
    stmt.table(Alias::new(table.to_owned()));
    if has_updated_at {
        stmt.values([
            (Alias::new(DELETED_AT.to_owned()), now.into()),
            (Alias::new(UPDATED_AT.to_owned()), now.into()),
        ]);
    } else {
        stmt.value(Alias::new(DELETED_AT.to_owned()), now);
    }
    stmt.and_where(Expr::col(Alias::new(pk_col.to_owned())).eq(*pk));
    stmt.to_string(PostgresQueryBuilder)
}

/// `save(update_fields=[fk])` on a reverse one-to-one:
/// `UPDATE child SET fk = NULL [+ updated_at] WHERE child_pk`
/// (`auto_now` still bumps `updated_at` on a partial save).
pub fn null_fk_one_to_one_sql(
    child_table: &str,
    child_pk_col: &str,
    child_pk: &uuid::Uuid,
    fk_col: &str,
    now: &str,
    has_updated_at: bool,
) -> String {
    let mut stmt = Query::update();
    stmt.table(Alias::new(child_table.to_owned()));
    if has_updated_at {
        stmt.values([
            (Alias::new(fk_col.to_owned()), None::<&str>.into()),
            (Alias::new(UPDATED_AT.to_owned()), now.into()),
        ]);
    } else {
        stmt.value(Alias::new(fk_col.to_owned()), None::<&str>);
    }
    stmt.and_where(Expr::col(Alias::new(child_pk_col.to_owned())).eq(*child_pk));
    stmt.to_string(PostgresQueryBuilder)
}

/// `related_queryset.update(fk=None)`: `UPDATE child SET fk = NULL WHERE
/// fk = parent_pk [AND deleted_at IS NULL]`. A queryset `update()` never
/// touches `updated_at`, and the related manager is scoped to live rows,
/// hence the `deleted_at IS NULL` guard on tables that have the column.
pub fn null_fk_bulk_sql(
    child_table: &str,
    fk_col: &str,
    parent_pk: &uuid::Uuid,
    scope_live: bool,
) -> String {
    let mut stmt = Query::update();
    stmt.table(Alias::new(child_table.to_owned()))
        .value(Alias::new(fk_col.to_owned()), None::<&str>)
        .and_where(Expr::col(Alias::new(fk_col.to_owned())).eq(*parent_pk));
    if scope_live {
        stmt.and_where(Expr::col(Alias::new(DELETED_AT.to_owned())).is_null());
    }
    stmt.to_string(PostgresQueryBuilder)
}

/// The ported-bug line for the to-many CASCADE branch
/// (`deletion_task.py:83`): the code calls the related manager as a
/// function — `getattr(instance, related_name)(manager="objects").all()`
/// — but a RelatedManager is not callable, so every to-many CASCADE
/// relation raises `TypeError`, lands in the `except Exception` at `:97`,
/// prints `Error handling relation <name>: ...`, and is skipped. Net
/// effect: to-many CASCADE children are NEVER soft-deleted. Ported as-is
/// (BUG-DEL-1); the services walk reports this text as a warning and
/// continues without touching a row.
pub fn cascade_to_many_message(accessor: &str) -> String {
    format!("Error handling relation {accessor}: 'RelatedManager' object is not callable")
}

/// Live-schema snapshot for one task run: every base table's columns plus
/// each table's single-column primary key. The column sets answer
/// `hasattr(obj, "deleted_at")` / `hasattr(obj, "updated_at")`; views
/// (including the `<table>_active` read views) are excluded, as Django's
/// `apps.get_models()` never sees them.
#[derive(Debug, Clone, Default)]
pub struct SchemaInfo {
    columns: HashMap<String, HashSet<String>>,
    pk_column: HashMap<String, String>,
}

impl SchemaInfo {
    pub fn has_column(&self, table: &str, column: &str) -> bool {
        self.columns
            .get(table)
            .is_some_and(|cols| cols.contains(column))
    }

    pub fn has_deleted_at(&self, table: &str) -> bool {
        self.has_column(table, DELETED_AT)
    }

    /// Primary-key column, defaulting to `id` (every fixture table uses it).
    pub fn pk_column(&self, table: &str) -> &str {
        self.pk_column
            .get(table)
            .map(String::as_str)
            .unwrap_or(PK_COLUMN)
    }
}

/// Load [`SchemaInfo`] from the Postgres catalog (two round trips).
pub async fn load_schema_info(pool: &PgPool) -> Result<SchemaInfo, sqlx::Error> {
    let columns = sqlx::query(
        "SELECT t.table_name, c.column_name \
         FROM information_schema.tables t \
         JOIN information_schema.columns c \
           ON c.table_name = t.table_name AND c.table_schema = t.table_schema \
         WHERE t.table_schema = 'public' AND t.table_type = 'BASE TABLE'",
    )
    .fetch_all(pool)
    .await?;
    let mut info = SchemaInfo::default();
    for row in columns {
        let table: String = row.try_get("table_name")?;
        let column: String = row.try_get("column_name")?;
        info.columns.entry(table).or_default().insert(column);
    }
    let pks = sqlx::query(
        "SELECT kcu.table_name, kcu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON kcu.constraint_name = tc.constraint_name \
          AND kcu.constraint_schema = tc.constraint_schema \
         WHERE tc.constraint_schema = 'public' AND tc.constraint_type = 'PRIMARY KEY'",
    )
    .fetch_all(pool)
    .await?;
    for row in pks {
        let table: String = row.try_get("table_name")?;
        let column: String = row.try_get("column_name")?;
        info.pk_column.entry(table).or_insert(column);
    }
    Ok(info)
}

/// Every base table carrying the soft-delete marker, alphabetically.
/// Feeds the `apps.get_models()` sweep (`deletion_task.py:183-188`):
/// Python iterates the app registry, Rust iterates the catalog — the
/// statement order differs, but each `DELETE` is independent so the
/// before/after DB state is identical.
pub fn sweep_tables(schema: &SchemaInfo) -> Vec<String> {
    let mut tables: Vec<String> = schema
        .columns
        .keys()
        .filter(|t| schema.has_deleted_at(t))
        .cloned()
        .collect();
    tables.sort();
    tables
}

/// One reverse foreign key into `parent_table`: the catalog form of a
/// Django reverse relation (`instance._meta.get_fields()` filtered to
/// `one_to_many or one_to_one`, `auto_created`, not concrete).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReverseRelation {
    /// Child table holding the FK (the related model's table).
    pub child_table: String,
    /// FK column on the child (`relation.remote_field.name`).
    pub fk_column: String,
    /// Child primary-key column (for single-row one-to-one writes).
    pub child_pk: String,
    /// Catalog `DELETE_RULE`: CASCADE / SET NULL / NO ACTION / RESTRICT /
    /// SET DEFAULT — the DDL spelling of `on_delete`.
    pub delete_rule: String,
    /// True when the FK column carries a single-column UNIQUE (or PK):
    /// the catalog form of a reverse `OneToOneRel`.
    pub one_to_one: bool,
}

impl ReverseRelation {
    /// `relation.on_delete.__name__` classifier:
    /// `DO_NOTHING` (DDL `NO ACTION`) skips; `SET_NULL` (DDL `SET NULL`)
    /// nulls; everything else takes the CASCADE branch — including
    /// `RESTRICT`/`SET DEFAULT`, exactly like the Python `else` at `:64`
    /// (ported as-is).
    pub fn is_do_nothing(&self) -> bool {
        self.delete_rule == "NO ACTION"
    }

    pub fn is_set_null(&self) -> bool {
        self.delete_rule == "SET NULL"
    }
}

/// Discover [`ReverseRelation`]s into `parent_table` from the catalog
/// (one round trip). Ordered by `(child_table, fk_column)` for
/// determinism; Python walks `_meta` order, which is likewise fixed per
/// model but not meaningful — each relation's writes are independent.
pub async fn reverse_relations(
    pool: &PgPool,
    parent_table: &str,
) -> Result<Vec<ReverseRelation>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT kcu.table_name AS child_table, kcu.column_name AS fk_column, \
                rc.delete_rule AS delete_rule, \
                EXISTS ( \
                  SELECT 1 \
                  FROM information_schema.table_constraints tc2 \
                  JOIN information_schema.key_column_usage kcu2 \
                    ON kcu2.constraint_name = tc2.constraint_name \
                   AND kcu2.constraint_schema = tc2.constraint_schema \
                   AND kcu2.table_name = kcu.table_name \
                   AND kcu2.column_name = kcu.column_name \
                  WHERE tc2.constraint_schema = 'public' \
                    AND tc2.table_name = kcu.table_name \
                    AND tc2.constraint_type IN ('UNIQUE', 'PRIMARY KEY') \
                    AND (SELECT count(*) \
                         FROM information_schema.key_column_usage kcu3 \
                         WHERE kcu3.constraint_name = tc2.constraint_name \
                           AND kcu3.constraint_schema = tc2.constraint_schema) = 1 \
                ) AS is_unique, \
                (SELECT kcu4.column_name \
                 FROM information_schema.table_constraints tc4 \
                 JOIN information_schema.key_column_usage kcu4 \
                   ON kcu4.constraint_name = tc4.constraint_name \
                  AND kcu4.constraint_schema = tc4.constraint_schema \
                 WHERE tc4.constraint_schema = 'public' \
                   AND tc4.table_name = kcu.table_name \
                   AND tc4.constraint_type = 'PRIMARY KEY' \
                 LIMIT 1) AS child_pk \
         FROM information_schema.referential_constraints rc \
         JOIN information_schema.key_column_usage kcu \
           ON kcu.constraint_name = rc.constraint_name \
          AND kcu.constraint_schema = rc.constraint_schema \
         JOIN information_schema.constraint_column_usage ccu \
           ON ccu.constraint_name = rc.unique_constraint_name \
          AND ccu.constraint_schema = rc.unique_constraint_schema \
         WHERE rc.constraint_schema = 'public' \
           AND ccu.table_schema = 'public' \
           AND ccu.table_name = $1 \
         ORDER BY kcu.table_name, kcu.column_name",
    )
    .bind(parent_table)
    .fetch_all(pool)
    .await?;
    let mut relations = Vec::with_capacity(rows.len());
    for row in rows {
        relations.push(ReverseRelation {
            child_table: row.try_get("child_table")?,
            fk_column: row.try_get("fk_column")?,
            child_pk: row
                .try_get::<Option<String>, _>("child_pk")?
                .unwrap_or_else(|| PK_COLUMN.to_owned()),
            delete_rule: row.try_get("delete_rule")?,
            one_to_one: row.try_get("is_unique")?,
        });
    }
    Ok(relations)
}

/// Lookup failures for task entry points.
#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    /// The `instance_pk` is not a UUID (Django raises `ValidationError`;
    /// the task fails loudly).
    #[error("bad primary key {0:?}: not a UUID")]
    BadPk(String),
    /// Any catalog/read failure.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Parse an entry primary key (every fixture table uses a UUID pk).
pub fn parse_pk(raw: &str) -> Result<uuid::Uuid, LookupError> {
    uuid::Uuid::parse_str(raw).map_err(|_| LookupError::BadPk(raw.to_owned()))
}

/// One `timezone.now()` stamp, Django-literal shaped.
pub fn now_stamp() -> String {
    format_cutoff(chrono::Utc::now())
}

/// The `model_class.all_objects.get(pk)` snapshot: unscoped (sees already
/// soft-deleted rows), `None` when the row is gone (the `DoesNotExist`
/// silent return at `:25-28`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowState {
    pub pk: uuid::Uuid,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Fetch [`RowState`] without any `deleted_at` scope.
pub async fn fetch_row(
    pool: &PgPool,
    schema: &SchemaInfo,
    table: &str,
    pk: &uuid::Uuid,
) -> Result<Option<RowState>, sqlx::Error> {
    let pk_col = schema.pk_column(table);
    let select = if schema.has_deleted_at(table) {
        format!("SELECT \"{pk_col}\" AS pk, \"deleted_at\" FROM \"{table}\" WHERE \"{pk_col}\" = '{pk}'")
    } else {
        format!("SELECT \"{pk_col}\" AS pk FROM \"{table}\" WHERE \"{pk_col}\" = '{pk}'")
    };
    let row = sqlx::query(&select).fetch_optional(pool).await?;
    match row {
        None => Ok(None),
        Some(row) => Ok(Some(RowState {
            pk: *pk,
            deleted_at: if schema.has_deleted_at(table) {
                row.try_get("deleted_at")?
            } else {
                None
            },
        })),
    }
}

/// Fetch one LIVE related row for a reverse one-to-one (`getattr` through
/// the scoped default manager: soft-deleted rows read as missing).
pub async fn fetch_live_one_to_one(
    pool: &PgPool,
    schema: &SchemaInfo,
    relation: &ReverseRelation,
    parent_pk: &uuid::Uuid,
) -> Result<Option<RowState>, sqlx::Error> {
    let select = if schema.has_deleted_at(&relation.child_table) {
        format!(
            "SELECT \"{pk}\" AS pk, \"deleted_at\" FROM \"{child}\" WHERE \"{fk}\" = '{parent}' AND \"deleted_at\" IS NULL LIMIT 1",
            pk = relation.child_pk,
            child = relation.child_table,
            fk = relation.fk_column,
            parent = parent_pk,
        )
    } else {
        format!(
            "SELECT \"{pk}\" AS pk FROM \"{child}\" WHERE \"{fk}\" = '{parent}' LIMIT 1",
            pk = relation.child_pk,
            child = relation.child_table,
            fk = relation.fk_column,
            parent = parent_pk,
        )
    };
    let row = sqlx::query(&select).fetch_optional(pool).await?;
    match row {
        None => Ok(None),
        Some(row) => Ok(Some(RowState {
            pk: row.try_get("pk")?,
            deleted_at: if schema.has_deleted_at(&relation.child_table) {
                row.try_get("deleted_at")?
            } else {
                None
            },
        })),
    }
}

/// Execute a rendered statement; returns affected rows.
async fn execute(pool: &PgPool, sql: &str) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(sql).execute(pool).await?.rows_affected())
}

/// Stamp one row (full-save shape): returns affected rows.
pub async fn stamp_row(
    pool: &PgPool,
    schema: &SchemaInfo,
    table: &str,
    pk: &uuid::Uuid,
    now: &str,
) -> Result<u64, sqlx::Error> {
    let sql = soft_stamp_sql(
        table,
        schema.pk_column(table),
        pk,
        now,
        schema.has_column(table, UPDATED_AT),
    );
    execute(pool, &sql).await
}

/// Null one reverse one-to-one (`save(update_fields=[fk])` shape).
pub async fn null_one_to_one(
    pool: &PgPool,
    schema: &SchemaInfo,
    relation: &ReverseRelation,
    child_pk: &uuid::Uuid,
    now: &str,
) -> Result<u64, sqlx::Error> {
    let sql = null_fk_one_to_one_sql(
        &relation.child_table,
        &relation.child_pk,
        child_pk,
        &relation.fk_column,
        now,
        schema.has_column(&relation.child_table, UPDATED_AT),
    );
    execute(pool, &sql).await
}

/// Null every live row behind a to-many `SET_NULL` (queryset-update shape).
pub async fn null_bulk(
    pool: &PgPool,
    schema: &SchemaInfo,
    relation: &ReverseRelation,
    parent_pk: &uuid::Uuid,
) -> Result<u64, sqlx::Error> {
    let sql = null_fk_bulk_sql(
        &relation.child_table,
        &relation.fk_column,
        parent_pk,
        schema.has_deleted_at(&relation.child_table),
    );
    execute(pool, &sql).await
}

/// Hard-delete tombstones from one table; returns affected rows.
pub async fn hard_delete_table(
    pool: &PgPool,
    table: &str,
    cutoff: &str,
) -> Result<u64, sqlx::Error> {
    execute(pool, &hard_delete_before_sql(table, cutoff)).await
}

/// The 18 named deletes, in fixture order.
pub async fn run_named_hard_deletes(pool: &PgPool, cutoff: &str) -> Result<u64, sqlx::Error> {
    let mut affected = 0;
    for table in HARD_DELETE_NAMED_TABLES {
        affected += hard_delete_table(pool, table, cutoff).await?;
    }
    Ok(affected)
}

/// The `apps.get_models()` sweep: every `deleted_at` base table,
/// re-inclusive of the 18 named ones, exactly like the Python loop at
/// `:183-188` (the re-deletes match no rows; kept for structural parity).
pub async fn run_sweep_hard_deletes(
    pool: &PgPool,
    schema: &SchemaInfo,
    cutoff: &str,
) -> Result<u64, sqlx::Error> {
    let mut affected = 0;
    for table in sweep_tables(schema) {
        affected += hard_delete_table(pool, &table, cutoff).await?;
    }
    Ok(affected)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: &str = "2026-08-29 06:00:00+00:00";

    fn uuid(s: &str) -> uuid::Uuid {
        uuid::Uuid::parse_str(s).expect("valid test uuid")
    }

    #[test]
    fn hard_delete_sql_matches_fixture_first_statements() {
        let expected = [
            "DELETE FROM \"workspaces\" WHERE \"workspaces\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"projects\" WHERE \"projects\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"cycles\" WHERE \"cycles\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"modules\" WHERE \"modules\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"issues\" WHERE \"issues\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"pages\" WHERE \"pages\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"issue_views\" WHERE \"issue_views\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"labels\" WHERE \"labels\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"states\" WHERE \"states\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"issue_activities\" WHERE \"issue_activities\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"issue_comments\" WHERE \"issue_comments\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"issue_links\" WHERE \"issue_links\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"issue_reactions\" WHERE \"issue_reactions\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"user_favorites\" WHERE \"user_favorites\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"module_issues\" WHERE \"module_issues\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"cycle_issues\" WHERE \"cycle_issues\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"estimates\" WHERE \"estimates\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
            "DELETE FROM \"estimate_points\" WHERE \"estimate_points\".\"deleted_at\" < '2026-08-29 06:00:00+00:00'",
        ];
        assert_eq!(HARD_DELETE_NAMED_TABLES.len(), expected.len());
        for (table, sql) in HARD_DELETE_NAMED_TABLES.iter().zip(expected.iter()) {
            assert_eq!(&hard_delete_before_sql(table, CUTOFF), sql);
        }
    }

    #[test]
    fn cutoff_format_matches_django_literal_shape() {
        let now = DateTime::parse_from_rfc3339("2026-08-29T06:00:00+00:00")
            .expect("valid")
            .with_timezone(&Utc);
        assert_eq!(format_cutoff(now), "2026-08-29 06:00:00+00:00");
        assert_eq!(cutoff_for(30, now), "2026-07-30 06:00:00+00:00");
    }

    #[test]
    fn entry_map_resolves_fixture_models() {
        assert_eq!(entry_table_for("db", "workspace"), Some("workspaces"));
        assert_eq!(entry_table_for("db", "issue"), Some("issues"));
        assert_eq!(entry_table_for("db", "issuelink"), Some("issue_links"));
        assert_eq!(
            entry_table_for("db", "apiactivitylog"),
            Some("api_activity_logs")
        );
        assert_eq!(entry_table_for("db", "nosuchmodel"), None);
        assert_eq!(entry_table_for("other", "issue"), None);
    }

    #[test]
    fn soft_stamp_sets_both_markers() {
        let pk = uuid("12345678-1234-1234-1234-1234567890ab");
        assert_eq!(
            soft_stamp_sql("issues", "id", &pk, "2026-09-28 06:00:00+00:00", true),
            "UPDATE \"issues\" SET \"deleted_at\" = '2026-09-28 06:00:00+00:00', \"updated_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = '12345678-1234-1234-1234-1234567890ab'"
        );
        assert_eq!(
            soft_stamp_sql("issues", "id", &pk, "2026-09-28 06:00:00+00:00", false),
            "UPDATE \"issues\" SET \"deleted_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = '12345678-1234-1234-1234-1234567890ab'"
        );
    }

    #[test]
    fn set_null_shapes_match_python_branches() {
        let parent = uuid("12345678-1234-1234-1234-1234567890ab");
        let child = uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(
            null_fk_one_to_one_sql("pages", "id", &child, "workspace_id", "2026-09-28 06:00:00+00:00", true),
            "UPDATE \"pages\" SET \"workspace_id\" = NULL, \"updated_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'"
        );
        assert_eq!(
            null_fk_bulk_sql("issues", "project_id", &parent, true),
            "UPDATE \"issues\" SET \"project_id\" = NULL WHERE \"project_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL"
        );
        assert_eq!(
            null_fk_bulk_sql("issues", "project_id", &parent, false),
            "UPDATE \"issues\" SET \"project_id\" = NULL WHERE \"project_id\" = '12345678-1234-1234-1234-1234567890ab'"
        );
    }

    #[test]
    fn on_delete_classifier_mirrors_python_branches() {
        let base = ReverseRelation {
            child_table: "issues".to_owned(),
            fk_column: "project_id".to_owned(),
            child_pk: "id".to_owned(),
            delete_rule: "NO ACTION".to_owned(),
            one_to_one: false,
        };
        assert!(base.is_do_nothing() && !base.is_set_null());
        let set_null = ReverseRelation {
            delete_rule: "SET NULL".to_owned(),
            ..base.clone()
        };
        assert!(set_null.is_set_null() && !set_null.is_do_nothing());
        for rule in ["CASCADE", "RESTRICT", "SET DEFAULT"] {
            let other = ReverseRelation {
                delete_rule: rule.to_owned(),
                ..base.clone()
            };
            assert!(!other.is_do_nothing() && !other.is_set_null(), "{rule}");
        }
    }

    #[test]
    fn cascade_bug_message_names_the_relation() {
        let message = cascade_to_many_message("issue_project");
        assert!(message.starts_with("Error handling relation issue_project: "));
        assert!(message.contains("not callable"));
    }

    #[test]
    fn sweep_lists_deleted_tables_sorted() {
        let mut schema = SchemaInfo::default();
        schema.columns.insert(
            "b_table".to_owned(),
            HashSet::from(["deleted_at".to_owned()]),
        );
        schema.columns.insert(
            "a_table".to_owned(),
            HashSet::from(["deleted_at".to_owned()]),
        );
        schema
            .columns
            .insert("users".to_owned(), HashSet::from(["id".to_owned()]));
        assert_eq!(
            sweep_tables(&schema),
            vec!["a_table".to_owned(), "b_table".to_owned()]
        );
        assert_eq!(schema.pk_column("whatever"), "id");
    }
}
