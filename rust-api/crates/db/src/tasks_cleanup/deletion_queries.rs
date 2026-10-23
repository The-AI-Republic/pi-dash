//! Soft/hard deletion statements and executors.
//!
//! Ports the data-access half of
//! `apps/api/pi_dash/bgtasks/deletion_task.py` (all 193 lines,
//! PIDASHCONV-186). The walk orchestration (relation order, recursion,
//! the ported bug BUG-DEL-2) lives in `pidash-services`; this module owns SQL
//! shapes, the 18-table hard-delete order, the `(app_label, model_name)`
//! entry map, the per-model Collector emulation behind `hard_delete`
//! (PIDASHCONV-928), and the Postgres-catalog discovery that stands in for
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

use std::collections::{HashMap, HashSet};

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
/// Mirrors `apps.get_model(app_label, model_name)`: Django matches
/// `model_name` case-insensitively (the lowercased class name) and
/// `app_label` exactly. Models span the `db`, `license` and `runner`
/// apps. An unknown pair is a loud error at the services layer (the
/// `apps.get_model` `LookupError` equivalent) — never a silent skip.
///
/// Every row but the trailing aliases is generated from `_meta`, in the
/// same maintenance class as [`HARD_DELETE_SWEEP_TABLES`]: a migration
/// adding a soft-delete model must extend it. Regenerate from a Django
/// shell with:
///
/// ```python
/// from django.apps import apps
/// for m in sorted(
///     apps.get_models(include_auto_created=False),
///     key=lambda m: (m._meta.app_label, m._meta.model_name),
/// ):
///     if hasattr(m, "deleted_at"):
///         print(m._meta.app_label, m._meta.model_name, m._meta.db_table)
/// ```
///
/// The trailing rows are not `_meta` soft-delete models: `("db", "user")`
/// is a real model without `deleted_at` (kept for the deliberate entry
/// asymmetry the replay tests pin — Django's task raises `AttributeError`
/// while the Rust worker walks it to null audit FKs); `apilog` and
/// `cldevicecode` are legacy hand aliases nothing enqueues (kept, not
/// extended).
const ENTRY_TABLES: &[(&str, &str, &str)] = &[
    ("db", "analyticview", "analytic_views"),
    ("db", "apiactivitylog", "api_activity_logs"),
    ("db", "apitoken", "api_tokens"),
    ("db", "clidevicecode", "cli_device_codes"),
    ("db", "commentreaction", "comment_reactions"),
    ("db", "cycle", "cycles"),
    ("db", "cycleissue", "cycle_issues"),
    ("db", "cycleuserproperties", "cycle_user_properties"),
    ("db", "deployboard", "deploy_boards"),
    ("db", "description", "descriptions"),
    ("db", "descriptionversion", "description_versions"),
    ("db", "device", "devices"),
    ("db", "devicesession", "device_sessions"),
    ("db", "draftissue", "draft_issues"),
    ("db", "draftissueassignee", "draft_issue_assignees"),
    ("db", "draftissuecycle", "draft_issue_cycles"),
    ("db", "draftissuelabel", "draft_issue_labels"),
    ("db", "draftissuemodule", "draft_issue_modules"),
    ("db", "emailnotificationlog", "email_notification_logs"),
    ("db", "estimate", "estimates"),
    ("db", "estimatepoint", "estimate_points"),
    ("db", "exporterhistory", "exporters"),
    ("db", "fileasset", "file_assets"),
    ("db", "gitcodereviewlink", "git_code_review_links"),
    ("db", "gitcommentsync", "git_comment_syncs"),
    ("db", "githubappinstallation", "github_app_installations"),
    (
        "db",
        "githubappinstallsession",
        "github_app_install_sessions",
    ),
    ("db", "githubcommentsync", "github_comment_syncs"),
    ("db", "githubissuesync", "github_issue_syncs"),
    ("db", "githubpullrequestlink", "github_pull_request_links"),
    ("db", "githubrepository", "github_repositories"),
    ("db", "githubrepositorysync", "github_repository_syncs"),
    ("db", "githubwebhookdelivery", "github_webhook_deliveries"),
    ("db", "gitissuesync", "git_issue_syncs"),
    ("db", "gitprovideraccount", "git_provider_accounts"),
    ("db", "gitrepository", "git_repositories"),
    ("db", "gitrepositorybinding", "git_repository_bindings"),
    ("db", "gitwebhookdelivery", "git_webhook_deliveries"),
    ("db", "gitwebhookregistration", "git_webhook_registrations"),
    ("db", "importer", "importers"),
    ("db", "intake", "intakes"),
    ("db", "intakeissue", "intake_issues"),
    ("db", "integration", "integrations"),
    ("db", "issue", "issues"),
    ("db", "issueactivity", "issue_activities"),
    ("db", "issueagentticker", "issue_agent_ticker"),
    ("db", "issueassignee", "issue_assignees"),
    ("db", "issueattachment", "issue_attachments"),
    ("db", "issueblocker", "issue_blockers"),
    ("db", "issuecomment", "issue_comments"),
    (
        "db",
        "issuedescriptionversion",
        "issue_description_versions",
    ),
    ("db", "issuelabel", "issue_labels"),
    ("db", "issuelink", "issue_links"),
    ("db", "issuemention", "issue_mentions"),
    ("db", "issuereaction", "issue_reactions"),
    ("db", "issuerelation", "issue_relations"),
    ("db", "issuesequence", "issue_sequences"),
    ("db", "issuesubscriber", "issue_subscribers"),
    ("db", "issuetype", "issue_types"),
    ("db", "issueversion", "issue_versions"),
    ("db", "issueview", "issue_views"),
    ("db", "issuevote", "issue_votes"),
    ("db", "label", "labels"),
    ("db", "loopjob", "loop_jobs"),
    ("db", "looptarget", "loop_targets"),
    ("db", "loopuserpreference", "loop_user_preferences"),
    ("db", "module", "modules"),
    ("db", "moduleissue", "module_issues"),
    ("db", "modulelink", "module_links"),
    ("db", "modulemember", "module_members"),
    ("db", "moduleuserproperties", "module_user_properties"),
    ("db", "notification", "notifications"),
    ("db", "page", "pages"),
    ("db", "pagelabel", "page_labels"),
    ("db", "pagelog", "page_logs"),
    ("db", "pageversion", "page_versions"),
    ("db", "project", "projects"),
    ("db", "projectdeployboard", "project_deploy_boards"),
    ("db", "projectidentifier", "project_identifiers"),
    ("db", "projectissuetype", "project_issue_types"),
    ("db", "projectmember", "project_members"),
    ("db", "projectmemberinvite", "project_member_invites"),
    ("db", "projectpage", "project_pages"),
    ("db", "projectpublicmember", "project_public_members"),
    ("db", "projectuserproperty", "project_user_properties"),
    ("db", "projectwebhook", "project_webhooks"),
    ("db", "scheduler", "schedulers"),
    ("db", "schedulerbinding", "scheduler_bindings"),
    ("db", "slackprojectsync", "slack_project_syncs"),
    ("db", "socialloginconnection", "social_login_connections"),
    ("db", "state", "states"),
    ("db", "sticky", "stickies"),
    ("db", "team", "teams"),
    ("db", "userfavorite", "user_favorites"),
    (
        "db",
        "usernotificationpreference",
        "user_notification_preferences",
    ),
    ("db", "userrecentvisit", "user_recent_visits"),
    ("db", "webhook", "webhooks"),
    ("db", "webhooklog", "webhook_logs"),
    ("db", "workspace", "workspaces"),
    (
        "db",
        "workspacehomepreference",
        "workspace_home_preferences",
    ),
    ("db", "workspaceintegration", "workspace_integrations"),
    ("db", "workspacejoinrequest", "workspace_join_requests"),
    ("db", "workspacemember", "workspace_members"),
    ("db", "workspacememberinvite", "workspace_member_invites"),
    ("db", "workspacetheme", "workspace_themes"),
    ("db", "workspaceuserlink", "workspace_user_links"),
    (
        "db",
        "workspaceuserpreference",
        "workspace_user_preferences",
    ),
    ("db", "workspaceuserproperties", "workspace_user_properties"),
    ("license", "changelog", "changelogs"),
    ("license", "instance", "instances"),
    ("license", "instanceadmin", "instance_admins"),
    (
        "license",
        "instanceconfiguration",
        "instance_configurations",
    ),
    ("runner", "pod", "pod"),
    // Legacy aliases (not `_meta` soft-delete models; see above).
    ("db", "apilog", "api_activity_logs"),
    ("db", "cldevicecode", "cli_device_codes"),
    ("db", "user", "users"),
];

/// The `apps.get_models()` sweep order (`deletion_task.py:183-188`):
/// every model carrying `deleted_at`, frozen from `_meta`
/// (PIDASHCONV-928). The Collector absorbs order on the success path, but
/// the per-model failure points do not: a `PROTECT`/`DO_NOTHING` remnant
/// aborts the task with earlier models committed (probed: a due issue with
/// any `issue_activities` fails at `issues`, before `pages` ever runs), so
/// the sweep must visit models in exactly this order. Physical table
/// names; the 18 named tables re-appear here (the Python loop is
/// re-inclusive). Same `_meta`-mirror maintenance class as
/// [`ENTRY_TABLES`]: unknown catalog tables are appended alphabetically by
/// [`sweep_tables_in_django_order`]. Regenerate from a Django shell with:
///
/// ```python
/// from django.apps import apps
/// for m in apps.get_models():
///     if hasattr(m, "deleted_at"):
///         print(m._meta.db_table)
/// ```
pub const HARD_DELETE_SWEEP_TABLES: [&str; 113] = [
    "analytic_views",
    "api_tokens",
    "cli_device_codes",
    "api_activity_logs",
    "file_assets",
    "projects",
    "project_member_invites",
    "project_members",
    "project_identifiers",
    "project_deploy_boards",
    "project_public_members",
    "workspaces",
    "workspace_members",
    "workspace_member_invites",
    "workspace_join_requests",
    "teams",
    "workspace_themes",
    "workspace_user_properties",
    "workspace_user_links",
    "workspace_home_preferences",
    "workspace_user_preferences",
    "descriptions",
    "description_versions",
    "states",
    "issues",
    "issue_blockers",
    "issue_relations",
    "issue_mentions",
    "issue_assignees",
    "issue_links",
    "issue_attachments",
    "issue_activities",
    "issue_comments",
    "issue_labels",
    "issue_sequences",
    "issue_subscribers",
    "issue_reactions",
    "comment_reactions",
    "issue_votes",
    "issue_versions",
    "issue_description_versions",
    "project_user_properties",
    "cycles",
    "cycle_issues",
    "cycle_user_properties",
    "deploy_boards",
    "draft_issues",
    "draft_issue_assignees",
    "draft_issue_labels",
    "draft_issue_modules",
    "draft_issue_cycles",
    "estimates",
    "estimate_points",
    "exporters",
    "importers",
    "intakes",
    "intake_issues",
    "integrations",
    "workspace_integrations",
    "github_repositories",
    "github_repository_syncs",
    "github_issue_syncs",
    "github_comment_syncs",
    "github_app_installations",
    "github_webhook_deliveries",
    "github_app_install_sessions",
    "github_pull_request_links",
    "git_provider_accounts",
    "git_repositories",
    "git_repository_bindings",
    "git_issue_syncs",
    "git_comment_syncs",
    "git_code_review_links",
    "git_webhook_deliveries",
    "git_webhook_registrations",
    "slack_project_syncs",
    "modules",
    "module_members",
    "module_issues",
    "module_links",
    "module_user_properties",
    "notifications",
    "user_notification_preferences",
    "email_notification_logs",
    "pages",
    "page_logs",
    "page_labels",
    "project_pages",
    "page_versions",
    "social_login_connections",
    "issue_views",
    "webhooks",
    "webhook_logs",
    "project_webhooks",
    "user_favorites",
    "issue_types",
    "project_issue_types",
    "user_recent_visits",
    "labels",
    "devices",
    "device_sessions",
    "stickies",
    "issue_agent_ticker",
    "schedulers",
    "scheduler_bindings",
    "loop_jobs",
    "loop_targets",
    "loop_user_preferences",
    "instances",
    "instance_admins",
    "instance_configurations",
    "changelogs",
    "pod",
];

/// Resolve the entry table for `(app_label, model_name)`, or `None` when
/// Django's `apps.get_model` would raise `LookupError`. Like Django,
/// `model_name` matches case-insensitively while `app_label` matches
/// exactly.
pub fn entry_table_for(app_label: &str, model_name: &str) -> Option<&'static str> {
    let lowered = model_name.to_lowercase();
    ENTRY_TABLES
        .iter()
        .find(|(app, model, _)| *app == app_label && *model == lowered)
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
/// `UPDATE child SET fk = NULL WHERE child_pk`. Django's `_save_table`
/// filters the written columns to `update_fields` only, and `pre_save`
/// (which fires `auto_now`) runs only for those fields — so unlike the
/// full `save()` behind [`soft_stamp_sql`], a partial save never bumps
/// `updated_at` (`deletion_task.py:56-57`).
pub fn null_fk_one_to_one_sql(
    child_table: &str,
    child_pk_col: &str,
    child_pk: &RowPk,
    fk_col: &str,
    _now: &str,
    _has_updated_at: bool,
) -> String {
    let mut stmt = Query::update();
    stmt.table(Alias::new(child_table.to_owned()));
    stmt.value(Alias::new(fk_col.to_owned()), None::<&str>);
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

/// Every base table carrying the soft-delete marker, in Django's
/// `apps.get_models()` order ([`HARD_DELETE_SWEEP_TABLES`]).
///
/// Feeds the sweep at `deletion_task.py:183-188`. Each per-model
/// `.delete()` runs the ORM Collector inside one transaction, so order
/// only fixes the failure points (a `PROTECT`/`DO_NOTHING` remnant aborts
/// the task with earlier models committed) — and those must match Django
/// exactly, hence the frozen order rather than a topological sort: even
/// the `loop_jobs` → `loop_targets` parent-first pair and the
/// `projects` ↔ `states` cycle resolve inside the model's own pass
/// (collection + nulling + one joint commit).
///
/// Catalog tables missing from the frozen list (a migration added a
/// soft-delete model) are appended alphabetically: they are still swept,
/// at an approximate position. Tables in the frozen list but absent from
/// the catalog (a migration dropped a model) are skipped.
pub fn sweep_tables_in_django_order(schema: &SchemaInfo) -> Vec<String> {
    let mut ordered: Vec<String> = HARD_DELETE_SWEEP_TABLES
        .iter()
        .filter(|t| schema.has_deleted_at(t))
        .map(ToString::to_string)
        .collect();
    let known: HashSet<&str> = HARD_DELETE_SWEEP_TABLES.iter().copied().collect();
    let mut extra: Vec<String> = schema
        .columns
        .keys()
        .filter(|t| schema.has_deleted_at(t) && !known.contains(t.as_str()))
        .cloned()
        .collect();
    extra.sort();
    ordered.extend(extra);
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
    ("projects", "default_state_id"),
    ("projects", "estimate_id"),
    ("runner", "dev_machine_id"),
    ("scheduler_bindings", "actor_id"),
    ("scheduler_bindings", "last_run_id"),
    ("scheduler_bindings", "pod_id"),
    ("users", "avatar_asset_id"),
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

/// `(child_table, fk_column)` pairs whose Django `on_delete` is
/// `PROTECT`: all 7 in the schema, frozen from `_meta` (PIDASHCONV-928).
/// The Collector raises `ProtectedError` when any row — live, tombstoned,
/// even one collected for deletion itself — references a collected row
/// through one of these, aborting the per-model delete. Includes the two
/// audit FKs that break the audit-column rule below (every other
/// `created_by_id`/`updated_by_id` column, 227 of 229, is `SET_NULL`).
/// Same `_meta`-mirror maintenance class as [`SET_NULL_FKS`]: there is no
/// `RESTRICT`, `SET_DEFAULT`, or `SET()` FK anywhere in the schema, so any
/// future one needs both a mirror entry and an emulation branch in
/// [`run_model_hard_delete`]. Regenerate from a Django shell with:
///
/// ```python
/// from django.apps import apps
/// from django.db import models
/// for m in apps.get_models(include_auto_created=True):
///     for f in m._meta.get_fields(include_hidden=True):
///         if isinstance(f, (models.ForeignKey, models.OneToOneField)) and f.concrete \
///                 and f.remote_field.on_delete.__name__ == "PROTECT":
///             print((m._meta.db_table, f.column))
/// ```
const PROTECT_FKS: &[(&str, &str)] = &[
    ("agent_chat_session", "created_by_id"),
    ("agent_chat_session", "pod_id"),
    ("agent_run", "created_by_id"),
    ("agent_run", "pod_id"),
    ("git_repository_bindings", "provider_account_id"),
    ("issues", "assigned_pod_id"),
    ("runner", "pod_id"),
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
    /// CASCADE branch — including `CASCADE`, `PROTECT` and `SET DEFAULT`,
    /// exactly like the Python `else` (ported as-is).
    ///
    /// The DDL rule alone cannot decide: Django emits `NO ACTION` for
    /// `CASCADE`, `PROTECT`, `SET_NULL` and `DO_NOTHING` alike, so the
    /// maps below are the `_meta` ground truth.
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
                && !self.is_protect())
    }

    /// True for the 7 [`PROTECT_FKS`] pairs. The soft-delete walk ignores
    /// this (Python's `else` stamps through `PROTECT`); the hard-delete
    /// Collector emulation fails the model pass on any referencing row.
    pub fn is_protect(&self) -> bool {
        PROTECT_FKS
            .iter()
            .any(|(table, column)| *table == self.child_table && *column == self.fk_column)
    }

    /// True for genuine `CASCADE` edges: anything the other three
    /// classifiers reject (341 of 620 FKs). A migration adding a
    /// non-`CASCADE` FK without extending the mirrors above silently lands
    /// here — the same maintenance class as [`ENTRY_TABLES`].
    pub fn is_cascade(&self) -> bool {
        !self.is_do_nothing() && !self.is_set_null() && !self.is_protect()
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

/// One catalog foreign key: the Collector emulation's graph edge.
///
/// Unlike [`ReverseRelation`] (one parent's reverse relations for the
/// soft-delete walk), these load once per `hard_delete` run (all 620)
/// and are bucketed by `on_delete` in [`classify_delete_edges`].
/// Every constraint is single-column (verified against the catalog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FkEdge {
    /// Child table holding the FK.
    pub child_table: String,
    /// FK column on the child.
    pub fk_column: String,
    /// Referenced (parent) table.
    pub parent_table: String,
    /// Catalog `DELETE_RULE` (`NO ACTION` for all 620: Django emulates
    /// every `on_delete` in the ORM, so this carries no dispatch signal
    /// beyond an explicit `SET NULL` honored by [`ReverseRelation`]).
    pub delete_rule: String,
}

impl FkEdge {
    fn as_relation(&self, schema: &SchemaInfo) -> ReverseRelation {
        ReverseRelation {
            child_table: self.child_table.clone(),
            fk_column: self.fk_column.clone(),
            child_pk: schema.pk_column(&self.child_table).to_owned(),
            delete_rule: self.delete_rule.clone(),
            one_to_one: false,
        }
    }
}

/// Every FK edge in `public`, deduplicated, ordered by
/// `(child_table, fk_column)` for determinism.
pub async fn load_fk_details(pool: &PgPool) -> Result<Vec<FkEdge>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT DISTINCT kcu.table_name AS child_table, kcu.column_name AS fk_column, \
                ccu.table_name AS parent_table, rc.delete_rule AS delete_rule \
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
        edges.push(FkEdge {
            child_table: row.try_get("child_table")?,
            fk_column: row.try_get("fk_column")?,
            parent_table: row.try_get("parent_table")?,
            delete_rule: row.try_get("delete_rule")?,
        });
    }
    Ok(edges)
}

/// Bucket every edge by Django `on_delete` as `(cascade, set_null,
/// protect)`, using the [`ReverseRelation`] classifiers (the `_meta`
/// mirrors, not the bare `NO ACTION` DDL). `DO_NOTHING` edges are
/// dropped: the Collector never collects through them and the deferred
/// commit check enforces them for free.
pub fn classify_delete_edges(
    edges: &[FkEdge],
    schema: &SchemaInfo,
) -> (Vec<FkEdge>, Vec<FkEdge>, Vec<FkEdge>) {
    let mut cascade = Vec::new();
    let mut set_null = Vec::new();
    let mut protect = Vec::new();
    for edge in edges {
        let relation = edge.as_relation(schema);
        if relation.is_do_nothing() {
            continue;
        } else if relation.is_set_null() {
            set_null.push(edge.clone());
        } else if relation.is_protect() {
            protect.push(edge.clone());
        } else {
            cascade.push(edge.clone());
        }
    }
    (cascade, set_null, protect)
}

/// `SELECT EXISTS(SELECT 1 FROM "<table>" WHERE "<table>"."deleted_at" <
/// '<cutoff>')`: the empty-seed short-circuit. A Collector run over an
/// empty queryset visits no relations and commits nothing, so a model
/// with no due tombstones is a no-op without further queries.
pub fn hard_delete_seed_exists_sql(table: &str, cutoff: &str) -> String {
    format!(
        "SELECT EXISTS(SELECT 1 FROM \"{table}\" WHERE \"{table}\".\"deleted_at\" < '{cutoff}')"
    )
}

/// L1 CASCADE collection: `SELECT "<child_pk>" AS pk FROM "<child>"
/// WHERE "<fk>" IN (SELECT "<parent_pk>" FROM "<parent>" WHERE
/// "<parent>"."deleted_at" < '<cutoff>')`.
///
/// Direct children of a model's due seeds, UNSCOPED — live, tombstoned,
/// triage, future-dated, all collected, exactly like the Collector's
/// `_base_manager` lookup. The due set itself is never materialized:
/// `hard_delete` runs daily, but a backlogged seed could still be large,
/// while the collected fringe (rows outside the tombstone set) stays
/// small. Child tables without `deleted_at` (M2M through tables) collect
/// identically — there is no child-side predicate at all.
pub fn cascade_seed_children_sql(
    child_table: &str,
    child_pk: &str,
    fk_col: &str,
    parent_table: &str,
    parent_pk: &str,
    cutoff: &str,
) -> String {
    format!(
        "SELECT \"{child_pk}\" AS pk FROM \"{child_table}\" \
         WHERE \"{fk_col}\" IN \
         (SELECT \"{parent_pk}\" FROM \"{parent_table}\" \
         WHERE \"{parent_table}\".\"deleted_at\" < '{cutoff}')"
    )
}

/// Transitive CASCADE collection: `SELECT "<child_pk>" AS pk FROM
/// "<child>" WHERE "<fk>" IN (<id literals>)`. `ids` is one chunk of an
/// already-collected parent id set; callers never pass an empty slice
/// (`IN ()` is invalid SQL).
pub fn cascade_collected_children_sql(
    child_table: &str,
    child_pk: &str,
    fk_col: &str,
    ids: &[RowPk],
) -> String {
    let mut stmt = Query::select();
    stmt.expr_as(
        Expr::col(Alias::new(child_pk.to_owned())),
        Alias::new("pk".to_owned()),
    )
    .from(Alias::new(child_table.to_owned()))
    .and_where(
        Expr::col(Alias::new(fk_col.to_owned())).is_in(ids.iter().map(RowPk::as_query_value)),
    );
    stmt.to_string(PostgresQueryBuilder)
}

/// `PROTECT` pre-check over the due seeds: `SELECT EXISTS(SELECT 1 FROM
/// "<child>" WHERE "<fk>" IN (SELECT ... due seeds ...))`. Any hit aborts
/// the model pass before its transaction opens, like the Collector
/// raising during collection.
pub fn protect_seed_check_sql(
    child_table: &str,
    fk_col: &str,
    parent_table: &str,
    parent_pk: &str,
    cutoff: &str,
) -> String {
    format!(
        "SELECT EXISTS(SELECT 1 FROM \"{child_table}\" WHERE \"{fk_col}\" IN \
         (SELECT \"{parent_pk}\" FROM \"{parent_table}\" \
         WHERE \"{parent_table}\".\"deleted_at\" < '{cutoff}'))"
    )
}

/// `PROTECT` pre-check over collected ids: `SELECT EXISTS(SELECT 1 FROM
/// "<child>" WHERE "<fk>" IN (<id literals>))`. No exclusion of rows
/// collected for deletion: `PROTECT` fires on those too (the Collector
/// checks the unscoped related set, not the delete set).
pub fn protect_collected_check_sql(child_table: &str, fk_col: &str, ids: &[RowPk]) -> String {
    let mut stmt = Query::select();
    let mut inner = Query::select();
    inner
        .expr(Expr::val(1))
        .from(Alias::new(child_table.to_owned()))
        .and_where(
            Expr::col(Alias::new(fk_col.to_owned())).is_in(ids.iter().map(RowPk::as_query_value)),
        );
    stmt.expr(Expr::exists(inner));
    stmt.to_string(PostgresQueryBuilder)
}

/// Collector `SET_NULL` over the due seeds: `UPDATE "<child>" SET
/// "<fk>" = NULL WHERE "<fk>" IN (SELECT ... due seeds ...)`, UNSCOPED.
///
/// Deliberately unlike [`null_fk_bulk_sql`]: the soft-delete walk nulls
/// through scoped related managers (`deleted_at IS NULL`,
/// `IssueManager`/`StateManager` guards), but the Collector nulls through
/// `_base_manager` — tombstoned, triage, archived, and draft rows are
/// nulled exactly like live ones, and `updated_at` is untouched (a bulk
/// `QuerySet.update`, never a save).
pub fn null_fk_emulation_seed_sql(
    child_table: &str,
    fk_col: &str,
    parent_table: &str,
    parent_pk: &str,
    cutoff: &str,
) -> String {
    format!(
        "UPDATE \"{child_table}\" SET \"{fk_col}\" = NULL WHERE \"{fk_col}\" IN \
         (SELECT \"{parent_pk}\" FROM \"{parent_table}\" \
         WHERE \"{parent_table}\".\"deleted_at\" < '{cutoff}')"
    )
}

/// Collector `SET_NULL` over collected ids: `UPDATE "<child>" SET
/// "<fk>" = NULL WHERE "<fk>" IN (<id literals>)`, unscoped as above.
pub fn null_fk_emulation_ids_sql(child_table: &str, fk_col: &str, ids: &[RowPk]) -> String {
    let mut stmt = Query::update();
    stmt.table(Alias::new(child_table.to_owned()))
        .value(Alias::new(fk_col.to_owned()), None::<&str>)
        .and_where(
            Expr::col(Alias::new(fk_col.to_owned())).is_in(ids.iter().map(RowPk::as_query_value)),
        );
    stmt.to_string(PostgresQueryBuilder)
}

/// `DELETE FROM "<table>" WHERE "<pk>" IN (<id literals>)`: removes one
/// chunk of CASCADE-collected rows (which may be live, so the
/// `deleted_at` predicate cannot select them).
pub fn delete_ids_sql(table: &str, pk_col: &str, ids: &[RowPk]) -> String {
    let mut stmt = Query::delete();
    stmt.from_table(Alias::new(table.to_owned())).and_where(
        Expr::col(Alias::new(pk_col.to_owned())).is_in(ids.iter().map(RowPk::as_query_value)),
    );
    stmt.to_string(PostgresQueryBuilder)
}

/// Id-list chunk size for collected-id statements: 1000 UUIDs render
/// ~40KB of SQL, far under any statement limit, while the per-run
/// collected fringe is usually a single chunk.
const ID_CHUNK: usize = 1000;

/// What one model's Collector pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelDeleteOutcome {
    /// Rows removed per table (the model's own seeds plus
    /// CASCADE-collected rows in any table).
    pub deleted: Vec<(String, u64)>,
    /// Rows touched by `SET_NULL` nulling.
    pub nulled: u64,
}

/// What one model's Collector pass may fail with, beyond the database.
#[derive(Debug, thiserror::Error)]
pub enum ModelDeleteError {
    /// The `ProtectedError` equivalent: a row still references a
    /// collected row through a `PROTECT` FK. Rendered table-flavored
    /// (model names are `_meta`-only).
    #[error(
        "cannot delete some instances of table \"{parent_table}\" \
         because they are referenced through protected foreign keys: \
         \"{child_table}\".\"{fk_column}\""
    )]
    Protected {
        parent_table: String,
        child_table: String,
        fk_column: String,
    },
    /// Any database failure — including the deferred `DO_NOTHING`
    /// violation (SQLSTATE 23503) surfacing at the pass commit, which
    /// rolls this model's writes back exactly like the Collector's
    /// per-model transaction.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// One model's Collector pass: `Model.all_objects.filter(deleted_at__lt=
/// cutoff).delete()` (`deletion_task.py:138-191`, PIDASHCONV-928).
///
/// The pass mirrors `Collector.collect` + `Collector.delete`
/// (`django/db/models/deletion.py`): the due seeds are collected with
/// their `CASCADE` closure (unscoped — live, tombstoned, triage, and
/// future-dated rows alike), `SET_NULL` targets are nulled unscoped, a
/// remaining `PROTECT` reference aborts before any write, and the writes
/// land in ONE transaction whose deferred FK checks enforce `DO_NOTHING`
/// at commit. Earlier models' commits are untouched by an abort, exactly
/// like the sequential Python `.delete()` calls.
///
/// Collection never materializes the due set itself (L1 selects seed
/// through a subquery); only the collected fringe (rows outside the
/// tombstone set: live/future children and their transitives) accumulates
/// as id sets, in visited-set fixpoint form so `CASCADE` cycles
/// terminate. `DO_NOTHING` edges need no branch: they are never
/// collected, and the commit check fails the pass when remnants exist.
pub async fn run_model_hard_delete(
    pool: &PgPool,
    schema: &SchemaInfo,
    cascade: &[FkEdge],
    set_null: &[FkEdge],
    protect: &[FkEdge],
    table: &str,
    cutoff: &str,
) -> Result<ModelDeleteOutcome, ModelDeleteError> {
    let seed_exists: bool = sqlx::query_scalar(&hard_delete_seed_exists_sql(table, cutoff))
        .fetch_one(pool)
        .await?;
    if !seed_exists {
        return Ok(ModelDeleteOutcome::default());
    }
    let parent_pk = schema.pk_column(table);

    // L1: direct CASCADE children of the due seeds (subquery-seeded).
    let mut collected: HashMap<String, HashSet<RowPk>> = HashMap::new();
    for edge in cascade.iter().filter(|e| e.parent_table == table) {
        let sql = cascade_seed_children_sql(
            &edge.child_table,
            schema.pk_column(&edge.child_table),
            &edge.fk_column,
            table,
            parent_pk,
            cutoff,
        );
        for row in sqlx::query(&sql).fetch_all(pool).await? {
            collected
                .entry(edge.child_table.clone())
                .or_default()
                .insert(decode_child_pk(&row)?);
        }
    }

    // L2+: transitive closure over collected ids (visited-set fixpoint;
    // each round must add a row to continue, so cycles terminate).
    loop {
        let frontier: Vec<(String, Vec<RowPk>)> = collected
            .iter()
            .map(|(t, ids)| (t.clone(), ids.iter().copied().collect()))
            .collect();
        let mut grew = false;
        for edge in cascade.iter() {
            let Some(parent_ids) = frontier
                .iter()
                .find(|(t, _)| t == &edge.parent_table)
                .map(|(_, ids)| ids)
            else {
                continue;
            };
            if parent_ids.is_empty() {
                continue;
            }
            let child_pk_col = schema.pk_column(&edge.child_table);
            for chunk in parent_ids.chunks(ID_CHUNK) {
                let sql = cascade_collected_children_sql(
                    &edge.child_table,
                    child_pk_col,
                    &edge.fk_column,
                    chunk,
                );
                for row in sqlx::query(&sql).fetch_all(pool).await? {
                    if collected
                        .entry(edge.child_table.clone())
                        .or_default()
                        .insert(decode_child_pk(&row)?)
                    {
                        grew = true;
                    }
                }
            }
        }
        if !grew {
            break;
        }
    }

    // PROTECT pre-check, before the transaction opens: seed-subquery form
    // for this model's own seeds, id-list form for collected parents.
    for edge in protect.iter() {
        if edge.parent_table == table {
            let hit: bool = sqlx::query_scalar(&protect_seed_check_sql(
                &edge.child_table,
                &edge.fk_column,
                table,
                parent_pk,
                cutoff,
            ))
            .fetch_one(pool)
            .await?;
            if hit {
                return Err(ModelDeleteError::Protected {
                    parent_table: table.to_owned(),
                    child_table: edge.child_table.clone(),
                    fk_column: edge.fk_column.clone(),
                });
            }
        }
        if let Some(ids) = collected.get(&edge.parent_table) {
            let ids: Vec<RowPk> = ids.iter().copied().collect();
            for chunk in ids.chunks(ID_CHUNK) {
                if chunk.is_empty() {
                    continue;
                }
                let hit: bool = sqlx::query_scalar(&protect_collected_check_sql(
                    &edge.child_table,
                    &edge.fk_column,
                    chunk,
                ))
                .fetch_one(pool)
                .await?;
                if hit {
                    return Err(ModelDeleteError::Protected {
                        parent_table: edge.parent_table.clone(),
                        child_table: edge.child_table.clone(),
                        fk_column: edge.fk_column.clone(),
                    });
                }
            }
        }
    }

    // The Collector's per-model transaction: SET_NULL updates first (as
    // `Collector.delete` orders field updates before deletes), then the
    // seed predicate DELETE (fixture shape) plus id-list DELETEs for
    // collected rows (which may be live). Statement order inside is
    // irrelevant — every constraint is deferred to the commit.
    let mut tx = pool.begin().await?;
    let mut outcome = ModelDeleteOutcome::default();
    for edge in set_null.iter() {
        if edge.parent_table == table {
            let sql = null_fk_emulation_seed_sql(
                &edge.child_table,
                &edge.fk_column,
                table,
                parent_pk,
                cutoff,
            );
            outcome.nulled += sqlx::query(&sql).execute(&mut *tx).await?.rows_affected();
        }
        if let Some(ids) = collected.get(&edge.parent_table) {
            let ids: Vec<RowPk> = ids.iter().copied().collect();
            for chunk in ids.chunks(ID_CHUNK) {
                if chunk.is_empty() {
                    continue;
                }
                let sql = null_fk_emulation_ids_sql(&edge.child_table, &edge.fk_column, chunk);
                outcome.nulled += sqlx::query(&sql).execute(&mut *tx).await?.rows_affected();
            }
        }
    }
    let seeds_removed = sqlx::query(&hard_delete_before_sql(table, cutoff))
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if seeds_removed > 0 {
        outcome.deleted.push((table.to_owned(), seeds_removed));
    }
    for (collected_table, ids) in &collected {
        let ids: Vec<RowPk> = ids.iter().copied().collect();
        let pk_col = schema.pk_column(collected_table);
        let mut removed = 0;
        for chunk in ids.chunks(ID_CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            removed += sqlx::query(&delete_ids_sql(collected_table, pk_col, chunk))
                .execute(&mut *tx)
                .await?
                .rows_affected();
        }
        if removed > 0 {
            outcome.deleted.push((collected_table.clone(), removed));
        }
    }
    tx.commit().await?;
    Ok(outcome)
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
    fn entry_map_matches_model_name_case_insensitively() {
        // `AppConfig.get_model` lowercases `model_name` but matches
        // `app_label` exactly (PIDASHCONV-1293).
        assert_eq!(entry_table_for("db", "DraftIssue"), Some("draft_issues"));
        assert_eq!(entry_table_for("db", "APITOKEN"), Some("api_tokens"));
        assert_eq!(entry_table_for("db", "apiToken"), Some("api_tokens"));
        assert_eq!(entry_table_for("DB", "issue"), None);
    }

    #[test]
    fn entry_map_covers_every_sweep_table() {
        // Every `deleted_at` table the hard-delete sweep visits must be
        // reachable as a soft-delete entry (PIDASHCONV-1293): a missing
        // row fails the job as `UnknownModel` where Django's
        // `apps.get_model` resolves.
        let tables: HashSet<&str> = ENTRY_TABLES.iter().map(|(_, _, table)| *table).collect();
        for table in HARD_DELETE_SWEEP_TABLES {
            assert!(
                tables.contains(table),
                "sweep table {table} has no ENTRY_TABLES row"
            );
        }
    }

    #[test]
    fn entry_map_keys_are_unique_and_lowercase() {
        // The generator emits `(app_label, model_name)` keys sorted and
        // lowercase; a duplicate or mixed-case key means the mirror and
        // the case-insensitive lookup drifted (PIDASHCONV-1293).
        let mut seen = HashSet::new();
        for (app, model, _) in ENTRY_TABLES {
            assert_eq!(*model, model.to_lowercase(), "mixed-case key {app}.{model}");
            assert!(seen.insert((*app, *model)), "duplicate key {app}.{model}");
        }
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
            "UPDATE \"pages\" SET \"workspace_id\" = NULL WHERE \"id\" = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'"
        );
        // `save(update_fields=[fk])` writes only the FK column: no
        // `updated_at` bump whether or not the table has the column.
        assert_eq!(
            null_fk_one_to_one_sql("pages", "id", &RowPk::Uuid(child), "workspace_id", "2026-09-28 06:00:00+00:00", false),
            "UPDATE \"pages\" SET \"workspace_id\" = NULL WHERE \"id\" = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'"
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
        assert!(protect_audit.is_protect() && !protect_audit.is_cascade());
        // Genuine CASCADE edges (the default bucket): the loop pair and an
        // unlisted pair with any non-SET-NULL DDL rule.
        assert!(cascade.is_cascade() && !cascade.is_protect());
        let listed_protect = ReverseRelation {
            child_table: "issues".to_owned(),
            fk_column: "assigned_pod_id".to_owned(),
            ..cascade.clone()
        };
        assert!(listed_protect.is_protect() && !listed_protect.is_cascade());
        assert!(!listed_protect.is_set_null() && !listed_protect.is_do_nothing());
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
            "UPDATE \"project_identifiers\" SET \"project_id\" = NULL WHERE \"id\" = 7"
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

    fn fk_edge(child: &str, column: &str, parent: &str) -> FkEdge {
        FkEdge {
            child_table: child.to_owned(),
            fk_column: column.to_owned(),
            parent_table: parent.to_owned(),
            delete_rule: "NO ACTION".to_owned(),
        }
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
    fn sweep_lists_known_tables_in_django_order() {
        // PIDASHCONV-928: the sweep follows apps.get_models() order, not
        // topology — the per-model pass absorbs the order (collection +
        // nulling + one joint commit), and the order fixes the failure
        // points. `issues` precedes `issue_activities` (a due issue with
        // activities fails the task: BUG-DEL-3); `pod` sorts last.
        let schema = sweep_schema(&[
            "pod",
            "issues",
            "issue_activities",
            "issue_comments",
            "projects",
        ]);
        let ordered = sweep_tables_in_django_order(&schema);
        let position = |table: &str| ordered.iter().position(|t| t == table).unwrap();
        assert!(position("projects") < position("issues"));
        assert!(position("issues") < position("issue_activities"));
        assert!(position("issue_activities") < position("issue_comments"));
        assert_eq!(ordered.last().map(String::as_str), Some("pod"));
        assert_eq!(schema.pk_column("whatever"), "id");
    }

    #[test]
    fn sweep_skips_tables_without_the_marker() {
        let mut schema = sweep_schema(&["projects"]);
        schema
            .columns
            .insert("users".to_owned(), HashSet::from(["id".to_owned()]));
        assert_eq!(
            sweep_tables_in_django_order(&schema),
            vec!["projects".to_owned()]
        );
    }

    #[test]
    fn sweep_appends_unknown_tables_alphabetically() {
        // A migration adding a soft-delete model is still swept, at an
        // approximate (appended) position.
        let schema = sweep_schema(&["zzz_new_model", "projects", "aaa_new_model"]);
        assert_eq!(
            sweep_tables_in_django_order(&schema),
            vec![
                "projects".to_owned(),
                "aaa_new_model".to_owned(),
                "zzz_new_model".to_owned()
            ]
        );
    }

    #[test]
    fn sweep_order_freeze_covers_the_named_tables() {
        assert_eq!(HARD_DELETE_SWEEP_TABLES.len(), 113);
        for table in HARD_DELETE_NAMED_TABLES {
            assert!(
                HARD_DELETE_SWEEP_TABLES.contains(&table),
                "named table {table} missing from the sweep freeze"
            );
        }
        // Failure-point pins (see the probe matrix in the services docs):
        // `issues` before `issue_activities`, provider account before its
        // binding, `pod` last.
        let position = |table: &str| {
            HARD_DELETE_SWEEP_TABLES
                .iter()
                .position(|t| *t == table)
                .unwrap()
        };
        assert!(position("issues") < position("issue_activities"));
        assert!(position("git_provider_accounts") < position("git_repository_bindings"));
        assert_eq!(HARD_DELETE_SWEEP_TABLES[112], "pod");
    }

    #[test]
    fn emulation_builders_match_collector_shapes() {
        // L1 collection: subquery-seeded, no child-side scope — not even
        // `deleted_at IS NULL`.
        assert_eq!(
            cascade_seed_children_sql("loop_targets", "id", "job_id", "loop_jobs", "id", CUTOFF),
            "SELECT \"id\" AS pk FROM \"loop_targets\" WHERE \"job_id\" IN \
             (SELECT \"id\" FROM \"loop_jobs\" WHERE \"loop_jobs\".\"deleted_at\" < \
             '2026-08-29 06:00:00+00:00')"
        );
        // Transitive collection over one chunk of parent ids.
        let one = RowPk::Uuid(uuid("12345678-1234-1234-1234-1234567890ab"));
        let two = RowPk::Int(7);
        assert_eq!(
            cascade_collected_children_sql("loop_targets", "id", "job_id", &[one, two]),
            "SELECT \"id\" AS \"pk\" FROM \"loop_targets\" WHERE \"job_id\" IN \
             ('12345678-1234-1234-1234-1234567890ab', 7)"
        );
        // Empty-seed short-circuit.
        assert_eq!(
            hard_delete_seed_exists_sql("issues", CUTOFF),
            "SELECT EXISTS(SELECT 1 FROM \"issues\" WHERE \"issues\".\"deleted_at\" < \
             '2026-08-29 06:00:00+00:00')"
        );
        // PROTECT pre-checks: seed-subquery + id-list forms.
        assert_eq!(
            protect_seed_check_sql("issues", "assigned_pod_id", "pod", "id", CUTOFF),
            "SELECT EXISTS(SELECT 1 FROM \"issues\" WHERE \"assigned_pod_id\" IN \
             (SELECT \"id\" FROM \"pod\" WHERE \"pod\".\"deleted_at\" < \
             '2026-08-29 06:00:00+00:00'))"
        );
        let probe = protect_collected_check_sql("issues", "assigned_pod_id", &[one]);
        assert!(probe.starts_with("SELECT EXISTS("), "{probe}");
        assert!(
            probe.contains("\"assigned_pod_id\" IN ('12345678-1234-1234-1234-1234567890ab')"),
            "{probe}"
        );
        // Collector SET_NULL: unscoped (no deleted_at / manager guards)
        // and no updated_at touch — unlike null_fk_bulk_sql.
        assert_eq!(
            null_fk_emulation_seed_sql(
                "issues",
                "estimate_point_id",
                "estimate_points",
                "id",
                CUTOFF
            ),
            "UPDATE \"issues\" SET \"estimate_point_id\" = NULL WHERE \"estimate_point_id\" IN \
             (SELECT \"id\" FROM \"estimate_points\" WHERE \"estimate_points\".\"deleted_at\" < \
             '2026-08-29 06:00:00+00:00')"
        );
        assert_eq!(
            null_fk_emulation_ids_sql("issues", "estimate_point_id", &[one]),
            "UPDATE \"issues\" SET \"estimate_point_id\" = NULL WHERE \"estimate_point_id\" IN \
             ('12345678-1234-1234-1234-1234567890ab')"
        );
        // Id-list delete (live collected rows need no deleted_at predicate).
        assert_eq!(
            delete_ids_sql("states", "id", &[one, two]),
            "DELETE FROM \"states\" WHERE \"id\" IN \
             ('12345678-1234-1234-1234-1234567890ab', 7)"
        );
    }

    #[test]
    fn classify_delete_edges_buckets_by_on_delete() {
        let schema = SchemaInfo::default();
        let edges = vec![
            fk_edge("loop_targets", "job_id", "loop_jobs"),
            fk_edge("projects", "default_state_id", "states"),
            fk_edge("issues", "assigned_pod_id", "pod"),
            fk_edge("issue_activities", "issue_id", "issues"),
            fk_edge("webhook_logs", "created_by_id", "users"),
        ];
        let (cascade, set_null, protect) = classify_delete_edges(&edges, &schema);
        assert_eq!(cascade.len(), 1);
        assert_eq!(cascade[0].child_table, "loop_targets");
        assert_eq!(set_null.len(), 2);
        assert_eq!(protect.len(), 1);
        assert_eq!(protect[0].child_table, "issues");
        // DO_NOTHING edges are dropped (never collected, commit-enforced).
        assert_eq!(cascade.len() + set_null.len() + protect.len(), 4);
    }

    #[test]
    fn protected_error_names_table_and_key() {
        let error = ModelDeleteError::Protected {
            parent_table: "pod".to_owned(),
            child_table: "issues".to_owned(),
            fk_column: "assigned_pod_id".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "cannot delete some instances of table \"pod\" because they are referenced \
             through protected foreign keys: \"issues\".\"assigned_pod_id\""
        );
    }
}
