//! Soft/hard deletion statements and executors.
//!
//! Ports the data-access half of
//! `apps/api/pi_dash/bgtasks/deletion_task.py` (all 193 lines,
//! PIDASHCONV-186). The walk orchestration (relation order, recursion,
//! the ported bug BUG-DEL-2) lives in `pidash-services`; this module owns SQL
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
//! the builder), primary keys are typed (`RowPk`: parsed `Uuid` or
//! catalog-decoded `i64`) before use, and the cutoff is formatted by
//! `chrono`. No caller input reaches SQL raw.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, Order, PostgresQueryBuilder, Query, SimpleExpr};
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
    ("db", "loopjob", "loop_jobs"),
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
    pk: &RowPk,
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
    stmt.and_where(Expr::col(Alias::new(pk_col.to_owned())).eq(pk.as_query_value()));
    stmt.to_string(PostgresQueryBuilder)
}

/// `save(update_fields=[fk])` on a reverse one-to-one:
/// `UPDATE child SET fk = NULL [+ updated_at] WHERE child_pk`
/// (`auto_now` still bumps `updated_at` on a partial save).
pub fn null_fk_one_to_one_sql(
    child_table: &str,
    child_pk_col: &str,
    child_pk: &RowPk,
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
    stmt.and_where(Expr::col(Alias::new(child_pk_col.to_owned())).eq(child_pk.as_query_value()));
    stmt.to_string(PostgresQueryBuilder)
}

/// `related_queryset.update(fk=None)`: `UPDATE child SET fk = NULL WHERE
/// fk = parent_pk [AND deleted_at IS NULL]`. A queryset `update()` never
/// touches `updated_at`, and the related manager is scoped to live rows,
/// hence the `deleted_at IS NULL` guard on tables that have the column.
///
/// `issues` children additionally carry the `IssueManager` guards
/// (`db/models/issue.py:82-97`): the reverse related manager is built from
/// the *child* model's default manager
/// (`related_descriptors.py:597-602`), and `Issue`'s default manager
/// excludes triage-state, archived, draft, and archived-project rows. The
/// join-shaped predicates use `NOT EXISTS` subqueries so nullable FKs keep
/// Django's `exclude()` NULL-preserving semantics (a bare `NOT (group =
/// 'triage')` over a left join would drop NULL-state rows; `NOT EXISTS`
/// retains them).
///
/// `states` children carry the `StateManager` guard
/// (`db/models/state.py:79-83`): `.exclude(group=TRIAGE)`. `group` is NOT
/// NULL with no `null=True`, so a plain `<> 'triage'` is exact — there are
/// no NULL rows to preserve.
pub fn null_fk_bulk_sql(
    child_table: &str,
    fk_col: &str,
    parent_pk: &RowPk,
    scope_live: bool,
) -> String {
    let mut stmt = Query::update();
    stmt.table(Alias::new(child_table.to_owned()))
        .value(Alias::new(fk_col.to_owned()), None::<&str>)
        .and_where(Expr::col(Alias::new(fk_col.to_owned())).eq(parent_pk.as_query_value()));
    if scope_live {
        stmt.and_where(Expr::col(Alias::new(DELETED_AT.to_owned())).is_null());
    }
    if child_table == "issues" {
        // `IssueManager.get_queryset()`: `.exclude(archived_at__isnull=False)`
        // and `.exclude(is_draft=True)` — plain column predicates.
        //
        // SET_NULL-only, deliberately: the to-many null path inherits the
        // child *default* manager (`IssueManager`), but the CASCADE
        // to-many path names `(manager="objects")` — and `Issue.objects`
        // is the inherited `SoftDeletionManager` (live-only; `Issue` is
        // the one model whose `objects` differs from its default
        // manager). A live Django run stamps triage-state issues through
        // `issues.project_id` CASCADE, so the fetch carries no `issues`
        // guard (PIDASHCONV-900 run 3 parity probe).
        stmt.and_where(
            Expr::col((
                Alias::new(child_table.to_owned()),
                Alias::new("archived_at".to_owned()),
            ))
            .is_null(),
        );
        stmt.and_where(
            Expr::col((
                Alias::new(child_table.to_owned()),
                Alias::new("is_draft".to_owned()),
            ))
            .eq(false),
        );
        // `.exclude(state__group=TRIAGE)`: NULL-state rows are retained.
        let mut triage = Query::select();
        triage
            .expr(Expr::val(1))
            .from(Alias::new("states".to_owned()))
            .and_where(
                Expr::col((Alias::new("states".to_owned()), Alias::new("id".to_owned()))).equals((
                    Alias::new(child_table.to_owned()),
                    Alias::new("state_id".to_owned()),
                )),
            )
            .and_where(
                Expr::col((
                    Alias::new("states".to_owned()),
                    Alias::new("group".to_owned()),
                ))
                .eq("triage"),
            );
        stmt.and_where(Expr::exists(triage).not());
        // `.exclude(project__archived_at__isnull=False)`: same
        // NULL-preserving shape for the nullable project FK.
        let mut archived_project = Query::select();
        archived_project
            .expr(Expr::val(1))
            .from(Alias::new("projects".to_owned()))
            .and_where(
                Expr::col((
                    Alias::new("projects".to_owned()),
                    Alias::new("id".to_owned()),
                ))
                .equals((
                    Alias::new(child_table.to_owned()),
                    Alias::new("project_id".to_owned()),
                )),
            )
            .and_where(
                Expr::col((
                    Alias::new("projects".to_owned()),
                    Alias::new("archived_at".to_owned()),
                ))
                .is_not_null(),
            );
        stmt.and_where(Expr::exists(archived_project).not());
    }
    if child_table == "states" {
        stmt.and_where(states_scope_predicate(child_table));
    }
    stmt.to_string(PostgresQueryBuilder)
}

/// `StateManager.get_queryset()` as one predicate
/// (`db/models/state.py:79-83`): `.exclude(group=TRIAGE)`. Shared by the
/// SET_NULL bulk null and the CASCADE to-many fetch — `State.objects`
/// *is* `StateManager` (unlike `Issue.objects`), so both paths scope
/// triage out and the two call sites must never drift.
fn states_scope_predicate(child_table: &str) -> SimpleExpr {
    Expr::col((
        Alias::new(child_table.to_owned()),
        Alias::new("group".to_owned()),
    ))
    .ne("triage")
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

/// Every base table carrying the soft-delete marker, children before
/// parents (topological order over `edges`, alphabetical tie-break).
///
/// Feeds the `apps.get_models()` sweep (`deletion_task.py:183-188`).
/// Python iterates the app registry with the Collector absorbing the
/// order (each per-model `.delete()` cascades inside one transaction);
/// Rust issues one raw `DELETE` per table with the deferred FK check
/// landing at each statement's own commit, so a tombstoned parent must
/// sort after its children or the `DELETE` fails and aborts the whole
/// task (PIDASHCONV-928: `loop_jobs` sorts before `loop_targets`
/// alphabetically). Tables in FK cycles (e.g. `projects` ↔ `states`
/// via `default_state_id`) have no valid order and are appended
/// alphabetically; the joint retry in [`run_retry_hard_deletes`]
/// deletes them together. Self-loops need no ordering: one statement
/// removes the whole table's tombstones atomically. `edges` must hold
/// distinct pairs ([`load_fk_edges`] deduplicates).
pub fn sweep_tables(schema: &SchemaInfo, edges: &[(String, String)]) -> Vec<String> {
    let mut tables: Vec<String> = schema
        .columns
        .keys()
        .filter(|t| schema.has_deleted_at(t))
        .cloned()
        .collect();
    tables.sort();
    // Kahn's algorithm: a table is releasable once every sweep child
    // (tables holding an FK into it) has been emitted, so children sort
    // before parents. Edges outside the sweep set and self-loops are
    // ignored: a parent that is never swept (or the table itself) cannot
    // block a per-table `DELETE`.
    let mut parents_of: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut child_count: HashMap<&str, usize> = HashMap::new();
    for table in &tables {
        child_count.insert(table.as_str(), 0);
    }
    for (child, parent) in edges {
        if child == parent || !schema.has_deleted_at(child) || !schema.has_deleted_at(parent) {
            continue;
        }
        parents_of
            .entry(child.as_str())
            .or_default()
            .push(parent.as_str());
        *child_count.entry(parent.as_str()).or_default() += 1;
    }
    let mut releasable: BTreeSet<&str> = tables
        .iter()
        .map(String::as_str)
        .filter(|t| child_count[t] == 0)
        .collect();
    let mut ordered = Vec::with_capacity(tables.len());
    let mut emitted: HashSet<&str> = HashSet::new();
    while let Some(table) = releasable.pop_first() {
        emitted.insert(table);
        ordered.push(table.to_owned());
        if let Some(parents) = parents_of.get(table) {
            for parent in parents {
                if emitted.contains(parent) {
                    continue;
                }
                if let Some(count) = child_count.get_mut(parent) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        releasable.insert(parent);
                    }
                }
            }
        }
    }
    if ordered.len() < tables.len() {
        // Cycle remainder: no valid order exists; append alphabetically
        // (the joint retry in `run_retry_hard_deletes` cracks these).
        let mut rest: Vec<&str> = tables
            .iter()
            .map(String::as_str)
            .filter(|t| !emitted.contains(t))
            .collect();
        rest.sort();
        ordered.extend(rest.iter().map(ToString::to_string));
    }
    ordered
}

/// `(child_table, fk_column)` pairs whose Django `on_delete` is
/// `SET_NULL`, beyond the audit-column rule in
/// [`ReverseRelation::is_set_null`]: 43 pairs across every app, frozen
/// from `_meta` (PIDASHCONV-900). Django emits bare FK constraints (`NO
/// ACTION`) for every `on_delete`, so the catalog cannot drive the
/// walk's branch dispatch — this table is the `_meta` mirror, in the same
/// maintenance class as [`ENTRY_TABLES`]: a migration adding a
/// non-`CASCADE` FK must extend it. Regenerate from a Django shell with:
///
/// ```python
/// from django.apps import apps
/// from django.db import models
/// for m in apps.get_models(include_auto_created=False):
///     for f in m._meta.get_fields():
///         if isinstance(f, (models.ForeignKey, models.OneToOneField)) and f.concrete \
///                 and f.remote_field.on_delete.__name__ == "SET_NULL" \
///                 and f.column not in ("created_by_id", "updated_by_id"):
///             print((m._meta.db_table, f.column))
/// ```
const SET_NULL_FKS: &[(&str, &str)] = &[
    ("agent_chat_approval", "decided_by_id"),
    ("agent_chat_event", "message_id"),
    ("agent_run", "owner_id"),
    ("agent_run", "parent_run_id"),
    ("agent_run", "pinned_runner_id"),
    ("agent_run", "runner_id"),
    ("agent_run", "scheduler_binding_id"),
    ("agent_run", "work_item_id"),
    ("agent_run_approval", "decided_by_id"),
    ("assistant_thread", "active_turn_id"),
    ("assistant_turn", "user_message_id"),
    ("deploy_boards", "intake_id"),
    ("draft_issues", "estimate_point_id"),
    ("draft_issues", "type_id"),
    ("git_provider_accounts", "workspace_integration_id"),
    ("git_webhook_deliveries", "repository_id"),
    ("github_repository_syncs", "label_id"),
    ("instance_admins", "user_id"),
    ("intake_issues", "duplicate_to_id"),
    ("issue_activities", "actor_id"),
    ("issue_agent_ticker", "pending_entry_actor_id"),
    ("issue_agent_ticker", "resume_parent_run_id"),
    ("issue_sequences", "issue_id"),
    ("issue_versions", "activity_id"),
    ("issues", "estimate_point_id"),
    ("issues", "type_id"),
    ("loop_targets", "last_run_id"),
    ("loop_targets", "thread_id"),
    ("machine_token", "dev_machine_id"),
    ("modules", "lead_id"),
    ("notifications", "triggered_by_id"),
    ("project_deploy_boards", "intake_id"),
    ("projects", "cover_image_asset_id"),
    ("projects", "default_state_id"),
    ("projects", "estimate_id"),
    ("runner", "dev_machine_id"),
    ("scheduler_bindings", "actor_id"),
    ("scheduler_bindings", "last_run_id"),
    ("scheduler_bindings", "pod_id"),
    ("users", "avatar_asset_id"),
    ("users", "cover_image_asset_id"),
    ("workspace_join_requests", "responded_by_id"),
    ("workspaces", "logo_asset_id"),
];

/// `(child_table, fk_column)` pairs whose Django `on_delete` is
/// `DO_NOTHING` (the walk's only skip branch): both live on
/// `issue_activities` (`db/models/issue.py:515,525`). Same `_meta`-mirror
/// maintenance class as [`SET_NULL_FKS`].
const DO_NOTHING_FKS: &[(&str, &str)] = &[
    ("issue_activities", "issue_comment_id"),
    ("issue_activities", "issue_id"),
];

/// Audit FKs that break the audit-column rule: both are `PROTECT`, so the
/// Python `else` stamps rather than nulling. Every other
/// `created_by_id`/`updated_by_id` column (227 of 229) is `SET_NULL`
/// (`db/mixins.py` `UserAuditModel`).
const PROTECT_AUDIT_FKS: &[(&str, &str)] = &[
    ("agent_chat_session", "created_by_id"),
    ("agent_run", "created_by_id"),
];

/// `(child_table, fk_column)` pairs whose reverse relation is hidden
/// (`related_name='+'`): Python's `instance._meta.get_fields()` defaults to
/// `include_hidden=False`, so `soft_delete_related_objects` never
/// enumerates them — whatever their `on_delete`. The catalog walk sees
/// every one as a real FK, so the services walk skips these
/// before dispatch (PIDASHCONV-900 review defect 2).
///
/// All 7 are `SET_NULL` in `_meta` and stay listed in [`SET_NULL_FKS`]
/// (that map is the `on_delete` ground truth); this set models the
/// separate `get_fields()` visibility rule. Same `_meta`-mirror
/// maintenance class: a migration adding a `related_name='+'` FK must
/// extend it. Regenerate from a Django shell with:
///
/// ```python
/// from django.apps import apps
/// from django.db import models
/// for m in apps.get_models(include_auto_created=False):
///     for f in m._meta.get_fields(include_hidden=True):
///         if isinstance(f, (models.ForeignKey, models.OneToOneField)) and f.concrete \
///                 and (f.remote_field.related_name or "").endswith("+"):
///             print((m._meta.db_table, f.column))
/// ```
const HIDDEN_REVERSE_FKS: &[(&str, &str)] = &[
    ("assistant_thread", "active_turn_id"),
    ("assistant_turn", "user_message_id"),
    ("issue_agent_ticker", "pending_entry_actor_id"),
    ("issue_agent_ticker", "resume_parent_run_id"),
    ("loop_targets", "last_run_id"),
    ("loop_targets", "thread_id"),
    ("scheduler_bindings", "last_run_id"),
];

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
    /// Catalog `DELETE_RULE`. Django emulates every `on_delete` in the
    /// ORM and emits bare FK constraints, so this reads `NO ACTION` for
    /// all 620 constraints — it carries no dispatch signal (an explicit
    /// `SET NULL` is still honored if one ever appears).
    pub delete_rule: String,
    /// True when the FK column carries a single-column UNIQUE (or PK):
    /// the catalog form of a reverse `OneToOneRel`.
    pub one_to_one: bool,
}

impl ReverseRelation {
    /// `relation.on_delete.__name__` classifier, mirroring the Python
    /// branches at `deletion_task.py:44-64`: `DO_NOTHING` skips (exactly
    /// [`DO_NOTHING_FKS`]); `SET_NULL` nulls; everything else takes the
    /// CASCADE branch — including `CASCADE`, `PROTECT`, `RESTRICT` and
    /// `SET DEFAULT`, exactly like the Python `else` (ported as-is).
    ///
    /// The DDL rule alone cannot decide: Django emits `NO ACTION` for
    /// `CASCADE`, `PROTECT`, `SET_NULL` and `DO_NOTHING` alike, so the
    /// map below is the `_meta` ground truth.
    pub fn is_do_nothing(&self) -> bool {
        DO_NOTHING_FKS
            .iter()
            .any(|(table, column)| *table == self.child_table && *column == self.fk_column)
    }

    pub fn is_set_null(&self) -> bool {
        self.delete_rule == "SET NULL"
            || SET_NULL_FKS
                .iter()
                .any(|(table, column)| *table == self.child_table && *column == self.fk_column)
            || ((self.fk_column == "created_by_id" || self.fk_column == "updated_by_id")
                && !PROTECT_AUDIT_FKS
                    .iter()
                    .any(|(table, column)| *table == self.child_table && *column == self.fk_column))
    }

    /// True when Python never enumerates this relation at all
    /// (`related_name='+'`, invisible to the default `get_fields()`).
    /// Checked before [`Self::is_do_nothing`] /
    /// [`Self::is_set_null`] — hidden wins over `on_delete`.
    pub fn is_hidden(&self) -> bool {
        HIDDEN_REVERSE_FKS
            .iter()
            .any(|(table, column)| *table == self.child_table && *column == self.fk_column)
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

/// A walked row's primary key: `UUID` on 112 of the 113 `deleted_at`
/// tables, `BIGINT` on `project_identifiers` (a `BigAutoField`, the only
/// non-UUID soft-delete PK in the schema — PIDASHCONV-900 review defect
/// 3). Django stamps identifier rows through the project O2O and the
/// workspace to-many CASCADEs like any other child; decoding the child
/// pk as `Uuid`-only turned those relations into error-skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowPk {
    Uuid(uuid::Uuid),
    Int(i64),
}

impl std::fmt::Display for RowPk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RowPk::Uuid(pk) => write!(f, "{pk}"),
            RowPk::Int(pk) => write!(f, "{pk}"),
        }
    }
}

impl RowPk {
    /// The pk as a query value (`UUID` / `BIGINT`).
    fn as_query_value(&self) -> sea_query::Value {
        match self {
            RowPk::Uuid(pk) => (*pk).into(),
            RowPk::Int(pk) => (*pk).into(),
        }
    }
}

/// Decode a fetched child pk: UUID first (112 tables), then 64-bit, then
/// 32-bit ints (only `project_identifiers` is non-UUID today; the `i32`
/// arm keeps a future `AutoField` child working). A pk of any other type
/// surfaces as the decode error — an honest failure, never a skip.
fn decode_child_pk(row: &sqlx::postgres::PgRow) -> Result<RowPk, sqlx::Error> {
    if let Ok(pk) = row.try_get::<uuid::Uuid, _>("pk") {
        return Ok(RowPk::Uuid(pk));
    }
    if let Ok(pk) = row.try_get::<i64, _>("pk") {
        return Ok(RowPk::Int(pk));
    }
    row.try_get::<i32, _>("pk")
        .map(|pk| RowPk::Int(i64::from(pk)))
}

/// The `model_class.all_objects.get(pk)` snapshot: unscoped (sees already
/// soft-deleted rows), `None` when the row is gone (the `DoesNotExist`
/// silent return at `:25-28`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowState {
    pub pk: RowPk,
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
            pk: RowPk::Uuid(*pk),
            deleted_at: if schema.has_deleted_at(table) {
                row.try_get("deleted_at")?
            } else {
                None
            },
        })),
    }
}

/// Render the reverse one-to-one SELECT (split from
/// [`fetch_one_to_one`] so the unscoped shape is unit-pinned, like
/// [`live_to_many_select`]).
/// The catalog form of the reverse-O2O `getattr`
/// (`deletion_task.py:54,68`): Django's `ReverseOneToOneDescriptor`
/// reads through the child `_base_manager`
/// (`related_descriptors.py:427`), which is the plain unscoped
/// `Manager` on every model here (no `Meta.base_manager_name`
/// anywhere) — so there is deliberately NO `deleted_at IS NULL`
/// guard. `deleted_at` is still selected when the table has it: the
/// services CASCADE branch needs it for the `if not deleted_at` skip
/// (`:70-71`), while the SET_NULL branch nulls whatever is found
/// (`:54-57`, no `deleted_at` check).
fn one_to_one_select(
    relation: &ReverseRelation,
    parent_pk: &RowPk,
    select_deleted_at: bool,
) -> String {
    let mut stmt = Query::select();
    stmt.expr_as(
        Expr::col(Alias::new(relation.child_pk.clone())),
        Alias::new("pk".to_owned()),
    )
    .from(Alias::new(relation.child_table.clone()))
    .and_where(Expr::col(Alias::new(relation.fk_column.clone())).eq(parent_pk.as_query_value()));
    if select_deleted_at {
        stmt.expr(Expr::col(Alias::new(DELETED_AT.to_owned())));
    }
    stmt.limit(1);
    stmt.to_string(PostgresQueryBuilder)
}

/// Fetch the related row behind a reverse one-to-one: the catalog form
/// of the descriptor `getattr` — unscoped, like `_base_manager`
/// (soft-deleted rows are found, not missing). Branching on
/// `deleted_at` is the services walk's job (SET_NULL nulls
/// unconditionally; CASCADE skips stamped rows), exactly like the
/// Python branches.
pub async fn fetch_one_to_one(
    pool: &PgPool,
    schema: &SchemaInfo,
    relation: &ReverseRelation,
    parent_pk: &RowPk,
) -> Result<Option<RowState>, sqlx::Error> {
    // The builder lives and drops inside the pure call above (no
    // await inside), so no non-`Send` `SeaRc` handle is alive at the
    // `.await` below and the walk future stays `Send`.
    let select = one_to_one_select(
        relation,
        parent_pk,
        schema.has_deleted_at(&relation.child_table),
    );
    let row = sqlx::query(&select).fetch_optional(pool).await?;
    match row {
        None => Ok(None),
        Some(row) => Ok(Some(RowState {
            pk: decode_child_pk(&row)?,
            deleted_at: if schema.has_deleted_at(&relation.child_table) {
                row.try_get("deleted_at")?
            } else {
                None
            },
        })),
    }
}

/// Render the to-many live-children SELECT (split from
/// [`fetch_live_to_many`] so the `states` scoping below is unit-pinned).
/// The `(manager="objects").all()` queryset is `objects`-scoped — for
/// `states` that is `StateManager` (triage excluded), for every other
/// model the plain live scope (`Issue.objects` is the inherited
/// `SoftDeletionManager`, so `issues` deliberately carry no
/// `IssueManager` guard here — unlike the SET_NULL path, which inherits
/// the child *default* manager).
fn live_to_many_select(
    child_table: &str,
    child_pk: &str,
    fk_col: &str,
    parent_pk: &RowPk,
) -> String {
    let mut stmt = Query::select();
    stmt.expr_as(
        Expr::col(Alias::new(child_pk.to_owned())),
        Alias::new("pk".to_owned()),
    )
    .expr(Expr::col(Alias::new(DELETED_AT.to_owned())))
    .from(Alias::new(child_table.to_owned()))
    .and_where(Expr::col(Alias::new(fk_col.to_owned())).eq(parent_pk.as_query_value()))
    .and_where(Expr::col(Alias::new(DELETED_AT.to_owned())).is_null());
    if child_table == "states" {
        stmt.and_where(states_scope_predicate(child_table));
    }
    stmt.order_by(Alias::new(child_pk.to_owned()), Order::Asc)
        .to_string(PostgresQueryBuilder)
}

/// Fetch every LIVE row behind a to-many CASCADE: the catalog form of the
/// `(manager="objects").all()` queryset (`deletion_task.py:83`).
/// Reverse-FK related managers ARE callable (`RelatedManager.__call__`
/// rebuilds with the named manager class), so Django iterates the
/// `objects`-scoped children — live rows only on soft-delete models,
/// further scoped by the child `objects` manager (`StateManager` for
/// `states`; every other model inherits the plain live scope — see
/// `live_to_many_select`).
/// The catalog equivalent is the `deleted_at IS NULL` guard on tables
/// carrying the column. Tables without `deleted_at` yield no rows:
/// Django's per-row `hasattr(related_obj, "deleted_at")` guard (`:86`)
/// skips the stamp AND the recursion for every such child, and the guard
/// is table-level (it cannot vary row to row). Ordered by pk for a
/// deterministic walk; Python's queryset order is likewise fixed but not
/// meaningful, since every child is stamped and recursed into.
pub async fn fetch_live_to_many(
    pool: &PgPool,
    schema: &SchemaInfo,
    relation: &ReverseRelation,
    parent_pk: &RowPk,
) -> Result<Vec<RowState>, sqlx::Error> {
    if !schema.has_deleted_at(&relation.child_table) {
        return Ok(Vec::new());
    }
    let select = live_to_many_select(
        &relation.child_table,
        &relation.child_pk,
        &relation.fk_column,
        parent_pk,
    );
    let mut out = Vec::new();
    for row in sqlx::query(&select).fetch_all(pool).await? {
        out.push(RowState {
            pk: decode_child_pk(&row)?,
            deleted_at: row.try_get("deleted_at")?,
        });
    }
    Ok(out)
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
    pk: &RowPk,
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
    child_pk: &RowPk,
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
    parent_pk: &RowPk,
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

/// Every `(child_table, parent_table)` FK edge in `public`, deduplicated.
///
/// Drives the sweep's topological order (children before parents).
/// Django emits bare `NO ACTION` constraints for every `on_delete`, so
/// the edge *rules* carry no signal — only the graph shape matters here.
pub async fn load_fk_edges(pool: &PgPool) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT DISTINCT kcu.table_name AS child_table, ccu.table_name AS parent_table \
         FROM information_schema.referential_constraints rc \
         JOIN information_schema.key_column_usage kcu \
           ON kcu.constraint_name = rc.constraint_name \
          AND kcu.constraint_schema = rc.constraint_schema \
         JOIN information_schema.constraint_column_usage ccu \
           ON ccu.constraint_name = rc.unique_constraint_name \
          AND ccu.constraint_schema = rc.unique_constraint_schema \
         WHERE rc.constraint_schema = 'public' \
           AND ccu.table_schema = 'public' \
         ORDER BY 1, 2",
    )
    .fetch_all(pool)
    .await?;
    let mut edges = Vec::with_capacity(rows.len());
    for row in rows {
        edges.push((row.try_get("child_table")?, row.try_get("parent_table")?));
    }
    Ok(edges)
}

/// `true` for a Postgres foreign-key violation (`23503`) — a tombstoned
/// parent whose children are still present. Hard-delete passes record
/// the table as blocked and continue; anything else aborts the task.
pub fn is_fk_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23503"))
}

/// What one hard-delete pass did over its tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HardDeletePass {
    /// Rows removed by the pass.
    pub deleted: u64,
    /// Tables whose `DELETE` hit an FK violation (SQLSTATE 23503): the
    /// statement rolled back and the table is retried jointly in
    /// [`run_retry_hard_deletes`]. Any other error aborts the pass.
    pub blocked: Vec<String>,
}

/// The 18 named deletes, in fixture order. FK-blocked tables are
/// recorded, not fatal: a named parent (e.g. `projects`) sorts before
/// its sweep children (`project_members`), so the pass cannot be
/// order-safe on its own — the joint retry finishes those tables.
pub async fn run_named_hard_deletes(
    pool: &PgPool,
    cutoff: &str,
) -> Result<HardDeletePass, sqlx::Error> {
    let mut pass = HardDeletePass::default();
    for table in HARD_DELETE_NAMED_TABLES {
        match hard_delete_table(pool, table, cutoff).await {
            Ok(affected) => pass.deleted += affected,
            Err(error) if is_fk_violation(&error) => pass.blocked.push(table.to_owned()),
            Err(error) => return Err(error),
        }
    }
    Ok(pass)
}

/// The `apps.get_models()` sweep: every `deleted_at` base table in
/// topological order, re-inclusive of the 18 named ones, exactly like
/// the Python loop at `:183-188` (the re-deletes match no rows unless
/// the named pass left them blocked; kept for structural parity).
pub async fn run_sweep_hard_deletes(
    pool: &PgPool,
    schema: &SchemaInfo,
    edges: &[(String, String)],
    cutoff: &str,
) -> Result<HardDeletePass, sqlx::Error> {
    let mut pass = HardDeletePass::default();
    for table in sweep_tables(schema, edges) {
        match hard_delete_table(pool, &table, cutoff).await {
            Ok(affected) => pass.deleted += affected,
            Err(error) if is_fk_violation(&error) => pass.blocked.push(table),
            Err(error) => return Err(error),
        }
    }
    Ok(pass)
}

/// Joint retry for blocked tables: one transaction, deferred FK checks.
///
/// Cracks FK cycles (no table order deletes `projects` + `states`
/// tombstones with separate per-table `DELETE`s) and cross-phase
/// stragglers (a named parent whose sweep children deleted late in the
/// pass). Every constraint is `DEFERRABLE INITIALLY DEFERRED`, so a
/// consistent tombstoned set commits cleanly whatever the statement
/// order; a tombstoned parent with *live* children still fails loudly
/// with the FK violation (the pinned PIDASHCONV-928 divergence from the
/// Collector, which would delete the live children). Returns per-table
/// row counts for phase attribution; any error rolls the whole retry
/// back and propagates.
pub async fn run_retry_hard_deletes(
    pool: &PgPool,
    tables: &[String],
    cutoff: &str,
) -> Result<Vec<(String, u64)>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut per_table = Vec::with_capacity(tables.len());
    for table in tables {
        let affected = sqlx::query(&hard_delete_before_sql(table, cutoff))
            .execute(&mut *tx)
            .await?
            .rows_affected();
        per_table.push((table.clone(), affected));
    }
    tx.commit().await?;
    Ok(per_table)
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
        assert_eq!(entry_table_for("db", "loopjob"), Some("loop_jobs"));
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
            soft_stamp_sql("issues", "id", &RowPk::Uuid(pk), "2026-09-28 06:00:00+00:00", true),
            "UPDATE \"issues\" SET \"deleted_at\" = '2026-09-28 06:00:00+00:00', \"updated_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = '12345678-1234-1234-1234-1234567890ab'"
        );
        assert_eq!(
            soft_stamp_sql("issues", "id", &RowPk::Uuid(pk), "2026-09-28 06:00:00+00:00", false),
            "UPDATE \"issues\" SET \"deleted_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = '12345678-1234-1234-1234-1234567890ab'"
        );
    }

    #[test]
    fn set_null_shapes_match_python_branches() {
        let parent = uuid("12345678-1234-1234-1234-1234567890ab");
        let child = uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(
            null_fk_one_to_one_sql("pages", "id", &RowPk::Uuid(child), "workspace_id", "2026-09-28 06:00:00+00:00", true),
            "UPDATE \"pages\" SET \"workspace_id\" = NULL, \"updated_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'"
        );
        assert_eq!(
            null_fk_bulk_sql("issue_comments", "issue_id", &RowPk::Uuid(parent), true),
            "UPDATE \"issue_comments\" SET \"issue_id\" = NULL WHERE \"issue_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL"
        );
        assert_eq!(
            null_fk_bulk_sql("issue_comments", "issue_id", &RowPk::Uuid(parent), false),
            "UPDATE \"issue_comments\" SET \"issue_id\" = NULL WHERE \"issue_id\" = '12345678-1234-1234-1234-1234567890ab'"
        );
    }

    #[test]
    fn issues_bulk_null_carries_issue_manager_scoping() {
        // D1: the reverse related manager inherits the child default
        // manager, and `Issue`'s is `IssueManager` (triage / archived /
        // draft / archived-project exclusions). Soft-deleting an
        // `EstimatePoint` or `IssueType` must leave those rows' FKs alone.
        let parent = uuid("12345678-1234-1234-1234-1234567890ab");
        assert_eq!(
            null_fk_bulk_sql("issues", "estimate_point_id", &RowPk::Uuid(parent), true),
            "UPDATE \"issues\" SET \"estimate_point_id\" = NULL WHERE \"estimate_point_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL AND \"issues\".\"archived_at\" IS NULL AND \"issues\".\"is_draft\" = FALSE AND (NOT EXISTS(SELECT 1 FROM \"states\" WHERE \"states\".\"id\" = \"issues\".\"state_id\" AND \"states\".\"group\" = 'triage')) AND (NOT EXISTS(SELECT 1 FROM \"projects\" WHERE \"projects\".\"id\" = \"issues\".\"project_id\" AND \"projects\".\"archived_at\" IS NOT NULL))"
        );
        assert_eq!(
            null_fk_bulk_sql("issues", "type_id", &RowPk::Uuid(parent), true),
            "UPDATE \"issues\" SET \"type_id\" = NULL WHERE \"type_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL AND \"issues\".\"archived_at\" IS NULL AND \"issues\".\"is_draft\" = FALSE AND (NOT EXISTS(SELECT 1 FROM \"states\" WHERE \"states\".\"id\" = \"issues\".\"state_id\" AND \"states\".\"group\" = 'triage')) AND (NOT EXISTS(SELECT 1 FROM \"projects\" WHERE \"projects\".\"id\" = \"issues\".\"project_id\" AND \"projects\".\"archived_at\" IS NOT NULL))"
        );
        // The triage / project guards retain NULL-FK rows (Django
        // `exclude()` left-join semantics): no bare `NOT (group = ...)`
        // that three-valued logic would turn against NULLs.
        let sql = null_fk_bulk_sql("issues", "estimate_point_id", &RowPk::Uuid(parent), true);
        assert!(sql.contains("NOT EXISTS"), "{sql}");
        assert!(!sql.contains("NOT (\"states\""), "{sql}");
    }

    #[test]
    fn states_bulk_null_excludes_triage() {
        // D1 (PIDASHCONV-900 review): the reverse related manager
        // inherits the child default manager, and `State`'s is
        // `StateManager` (excludes `group='triage'`). Soft-deleting a
        // user must leave triage states' audit FKs alone.
        let parent = uuid("12345678-1234-1234-1234-1234567890ab");
        assert_eq!(
            null_fk_bulk_sql("states", "created_by_id", &RowPk::Uuid(parent), true),
            "UPDATE \"states\" SET \"created_by_id\" = NULL WHERE \"created_by_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL AND \"states\".\"group\" <> 'triage'"
        );
    }

    #[test]
    fn live_to_many_select_carries_objects_manager_scoping() {
        // D1 (PIDASHCONV-900 review): the CASCADE to-many fetch is the
        // `(manager="objects").all()` queryset — `StateManager`-scoped
        // for `states` (triage rows never stamped or recursed into;
        // `group` is NOT NULL, hence the plain predicate). `issues`
        // deliberately carry NO `IssueManager` guard: `Issue.objects`
        // is the inherited live-only manager (the `IssueManager`
        // scoping lives on the SET_NULL path, which inherits the child
        // *default* manager) — a live Django run stamps triage-state
        // issues through `issues.project_id` CASCADE.
        let parent = uuid("12345678-1234-1234-1234-1234567890ab");
        assert_eq!(
            live_to_many_select("states", "id", "project_id", &RowPk::Uuid(parent)),
            "SELECT \"id\" AS \"pk\", \"deleted_at\" FROM \"states\" WHERE \"project_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL AND \"states\".\"group\" <> 'triage' ORDER BY \"id\" ASC"
        );
        assert_eq!(
            live_to_many_select("issues", "id", "project_id", &RowPk::Uuid(parent)),
            "SELECT \"id\" AS \"pk\", \"deleted_at\" FROM \"issues\" WHERE \"project_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL ORDER BY \"id\" ASC"
        );
        assert_eq!(
            live_to_many_select("loop_targets", "id", "job_id", &RowPk::Uuid(parent)),
            "SELECT \"id\" AS \"pk\", \"deleted_at\" FROM \"loop_targets\" WHERE \"job_id\" = '12345678-1234-1234-1234-1234567890ab' AND \"deleted_at\" IS NULL ORDER BY \"id\" ASC"
        );
    }

    #[test]
    fn hidden_reverse_relations_skip_before_dispatch() {
        // D2 (PIDASHCONV-900 review): `related_name='+'` relations are
        // invisible to Python's `get_fields()`, so the walk never sees
        // them whatever their `on_delete`. All 7 are SET_NULL in
        // `_meta` — they stay listed there as ground truth — but the
        // services walk skips them before dispatch.
        assert_eq!(HIDDEN_REVERSE_FKS.len(), 7);
        for (table, column) in HIDDEN_REVERSE_FKS {
            let relation = ReverseRelation {
                child_table: (*table).to_owned(),
                fk_column: (*column).to_owned(),
                child_pk: "id".to_owned(),
                delete_rule: "NO ACTION".to_owned(),
                one_to_one: false,
            };
            assert!(relation.is_hidden(), "{table}.{column}");
            assert!(relation.is_set_null(), "{table}.{column}");
        }
        let visible = ReverseRelation {
            child_table: "webhook_logs".to_owned(),
            fk_column: "created_by_id".to_owned(),
            child_pk: "id".to_owned(),
            delete_rule: "NO ACTION".to_owned(),
            one_to_one: false,
        };
        assert!(!visible.is_hidden());
        assert!(visible.is_set_null());
    }

    #[test]
    fn on_delete_classifier_mirrors_python_branches() {
        // Bare NO ACTION carries no signal (Django emits it for every
        // on_delete): an unlisted pair takes the CASCADE branch.
        let cascade = ReverseRelation {
            child_table: "loop_targets".to_owned(),
            fk_column: "job_id".to_owned(),
            child_pk: "id".to_owned(),
            delete_rule: "NO ACTION".to_owned(),
            one_to_one: false,
        };
        assert!(!cascade.is_do_nothing() && !cascade.is_set_null());
        // An explicit DDL action still dispatches directly.
        let ddl_null = ReverseRelation {
            delete_rule: "SET NULL".to_owned(),
            ..cascade.clone()
        };
        assert!(ddl_null.is_set_null() && !ddl_null.is_do_nothing());
        // Audit columns null via the UserAuditModel rule ...
        let audit = ReverseRelation {
            child_table: "issues".to_owned(),
            fk_column: "created_by_id".to_owned(),
            ..cascade.clone()
        };
        assert!(audit.is_set_null() && !audit.is_do_nothing());
        // ... except the two PROTECT audit FKs, which stamp via the else.
        let protect_audit = ReverseRelation {
            child_table: "agent_run".to_owned(),
            fk_column: "created_by_id".to_owned(),
            ..cascade.clone()
        };
        assert!(!protect_audit.is_set_null() && !protect_audit.is_do_nothing());
        // A listed SET_NULL pair nulls despite NO ACTION DDL.
        let listed_null = ReverseRelation {
            child_table: "loop_targets".to_owned(),
            fk_column: "thread_id".to_owned(),
            ..cascade.clone()
        };
        assert!(listed_null.is_set_null() && !listed_null.is_do_nothing());
        // The only skip branch: the two DO_NOTHING pairs.
        let skip = ReverseRelation {
            child_table: "issue_activities".to_owned(),
            fk_column: "issue_id".to_owned(),
            ..cascade.clone()
        };
        assert!(skip.is_do_nothing() && !skip.is_set_null());
        for rule in ["CASCADE", "RESTRICT", "SET DEFAULT"] {
            let other = ReverseRelation {
                delete_rule: rule.to_owned(),
                ..cascade.clone()
            };
            assert!(!other.is_do_nothing() && !other.is_set_null(), "{rule}");
        }
    }

    #[test]
    fn int_pks_render_as_bare_bigint_literals() {
        // D3 (PIDASHCONV-900 review): `project_identifiers` rows stamp
        // and null through the same builders with a bare BIGINT
        // literal — never a quoted UUID — and stringify plainly for
        // the walk's visited set.
        let pk = RowPk::Int(7);
        assert_eq!(pk.to_string(), "7");
        assert_eq!(
            RowPk::Uuid(uuid("12345678-1234-1234-1234-1234567890ab")).to_string(),
            "12345678-1234-1234-1234-1234567890ab"
        );
        assert_eq!(
            soft_stamp_sql(
                "project_identifiers",
                "id",
                &pk,
                "2026-09-28 06:00:00+00:00",
                true
            ),
            "UPDATE \"project_identifiers\" SET \"deleted_at\" = '2026-09-28 06:00:00+00:00', \"updated_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = 7"
        );
        assert_eq!(
            null_fk_one_to_one_sql(
                "project_identifiers",
                "id",
                &pk,
                "project_id",
                "2026-09-28 06:00:00+00:00",
                true
            ),
            "UPDATE \"project_identifiers\" SET \"project_id\" = NULL, \"updated_at\" = '2026-09-28 06:00:00+00:00' WHERE \"id\" = 7"
        );
        assert_eq!(
            null_fk_bulk_sql("project_identifiers", "workspace_id", &pk, true),
            "UPDATE \"project_identifiers\" SET \"workspace_id\" = NULL WHERE \"workspace_id\" = 7 AND \"deleted_at\" IS NULL"
        );
        assert_eq!(
            live_to_many_select("project_identifiers", "id", "workspace_id", &pk),
            "SELECT \"id\" AS \"pk\", \"deleted_at\" FROM \"project_identifiers\" WHERE \"workspace_id\" = 7 AND \"deleted_at\" IS NULL ORDER BY \"id\" ASC"
        );
    }

    fn sweep_schema(tables: &[&str]) -> SchemaInfo {
        let mut schema = SchemaInfo::default();
        for table in tables {
            schema.columns.insert(
                (*table).to_owned(),
                HashSet::from(["deleted_at".to_owned()]),
            );
        }
        schema
    }

    fn edges(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(child, parent)| ((*child).to_owned(), (*parent).to_owned()))
            .collect()
    }

    #[test]
    fn one_to_one_select_is_unscoped_base_manager_read() {
        // PIDASHCONV-936: the reverse-O2O `getattr`
        // (`deletion_task.py:54,68`) reads through the child
        // `_base_manager` (`related_descriptors.py:427`), the plain
        // unscoped `Manager` on all 153 models — so the SELECT
        // carries no `deleted_at IS NULL` guard: soft-deleted
        // children are found. `deleted_at` is still selected when
        // present: the services CASCADE branch skips on it
        // (`:70-71`), while SET_NULL nulls unconditionally (`:54-57`).
        let parent = uuid("12345678-1234-1234-1234-1234567890ab");
        let relation = ReverseRelation {
            child_table: "project_identifiers".to_owned(),
            fk_column: "project_id".to_owned(),
            child_pk: "id".to_owned(),
            delete_rule: "NO ACTION".to_owned(),
            one_to_one: true,
        };
        let unscoped_shape = one_to_one_select(&relation, &RowPk::Uuid(parent), true);
        assert_eq!(
            unscoped_shape,
            "SELECT \"id\" AS \"pk\", \"deleted_at\" FROM \"project_identifiers\" WHERE \"project_id\" = '12345678-1234-1234-1234-1234567890ab' LIMIT 1"
        );
        assert!(
            !unscoped_shape.contains("IS NULL"),
            "O2O read must be unscoped: {unscoped_shape}"
        );
        assert_eq!(
            one_to_one_select(&relation, &RowPk::Uuid(parent), false),
            "SELECT \"id\" AS \"pk\" FROM \"project_identifiers\" WHERE \"project_id\" = '12345678-1234-1234-1234-1234567890ab' LIMIT 1"
        );
        // Int parents render bare (O2O children decode either way).
        assert_eq!(
            one_to_one_select(&relation, &RowPk::Int(7), true),
            "SELECT \"id\" AS \"pk\", \"deleted_at\" FROM \"project_identifiers\" WHERE \"project_id\" = 7 LIMIT 1"
        );
    }

    #[test]
    fn sweep_lists_deleted_tables_sorted() {
        let mut schema = sweep_schema(&["b_table", "a_table"]);
        schema
            .columns
            .insert("users".to_owned(), HashSet::from(["id".to_owned()]));
        assert_eq!(
            sweep_tables(&schema, &[]),
            vec!["a_table".to_owned(), "b_table".to_owned()]
        );
        assert_eq!(schema.pk_column("whatever"), "id");
    }

    #[test]
    fn sweep_orders_children_before_parents() {
        // PIDASHCONV-928: the loop pair sorts parent-first
        // alphabetically; the sweep must still delete the children first.
        let schema = sweep_schema(&[
            "loop_jobs",
            "loop_targets",
            "loop_user_preferences",
            "unrelated",
        ]);
        let fk = edges(&[
            ("loop_targets", "loop_jobs"),
            ("loop_user_preferences", "loop_jobs"),
        ]);
        let ordered = sweep_tables(&schema, &fk);
        let position = |table: &str| ordered.iter().position(|t| t == table).unwrap();
        assert!(position("loop_targets") < position("loop_jobs"));
        assert!(position("loop_user_preferences") < position("loop_jobs"));
        // Alphabetical tie-break among the releasable tables.
        assert_eq!(ordered[0], "loop_targets");
        assert_eq!(ordered[1], "loop_user_preferences");
        // A chain emits leaf-first.
        let chain = sweep_schema(&["grandparent", "parent", "child"]);
        let chain_fk = edges(&[("child", "parent"), ("parent", "grandparent")]);
        assert_eq!(
            sweep_tables(&chain, &chain_fk),
            vec![
                "child".to_owned(),
                "parent".to_owned(),
                "grandparent".to_owned()
            ]
        );
    }

    #[test]
    fn sweep_appends_cycles_and_ignores_foreign_edges() {
        let mut schema = sweep_schema(&["projects", "states", "issues"]);
        schema
            .columns
            .insert("users".to_owned(), HashSet::from(["id".to_owned()]));
        let fk = edges(&[
            ("projects", "states"),
            ("states", "projects"),
            ("issues", "projects"),
            ("issues", "users"),
            ("issues", "issues"),
        ]);
        // `issues` has no sweep children, so it sorts first; the
        // `projects` ↔ `states` cycle has no valid order and is appended
        // alphabetically for the joint retry to crack.
        assert_eq!(
            sweep_tables(&schema, &fk),
            vec![
                "issues".to_owned(),
                "projects".to_owned(),
                "states".to_owned()
            ]
        );
    }

    #[derive(Debug)]
    struct CodedDbError {
        message: String,
        code: Option<String>,
    }

    impl std::fmt::Display for CodedDbError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl std::error::Error for CodedDbError {}

    impl sqlx::error::DatabaseError for CodedDbError {
        fn message(&self) -> &str {
            &self.message
        }

        fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
            self.code.as_deref().map(std::borrow::Cow::Borrowed)
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::ForeignKeyViolation
        }
    }

    #[test]
    fn fk_violation_matches_only_23503() {
        let fk = sqlx::Error::Database(Box::new(CodedDbError {
            message: "violates foreign key constraint".to_owned(),
            code: Some("23503".to_owned()),
        }));
        assert!(is_fk_violation(&fk));
        let unique = sqlx::Error::Database(Box::new(CodedDbError {
            message: "duplicate key".to_owned(),
            code: Some("23505".to_owned()),
        }));
        assert!(!is_fk_violation(&unique));
        let codeless = sqlx::Error::Database(Box::new(CodedDbError {
            message: "boom".to_owned(),
            code: None,
        }));
        assert!(!is_fk_violation(&codeless));
        assert!(!is_fk_violation(&sqlx::Error::RowNotFound));
    }
}
