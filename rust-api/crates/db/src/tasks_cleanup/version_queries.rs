//! D-09 version-task query units (stage 5, PIDASHCONV-188).
//!
//! Ports the SQL behind the four version files
//! (`apps/api/pi_dash/bgtasks/issue_version_sync.py`,
//! `issue_description_version_sync.py`, `issue_description_version_task.py`,
//! `page_version_task.py`; drift baseline `01a93e17216faea7bfc156b0f864cbbe420d1c52`).
//!
//! Static statements are string constants; statements with a dynamic arity
//! (`IN` lists, bulk `INSERT` rows, the coalesce `UPDATE`) are builder
//! functions emitting the same text Django's compiler emits, with Postgres
//! `$N` placeholders where Django renders `%s`. Execution uses runtime
//! `sqlx::query` (no `query!` macros): there is no build-time database, per
//! the merged precedent (`db/src/license/queries.rs`).
//!
//! Column lists follow `rust-api/fixtures/tasks_cleanup/columns.json`
//! (Django `_meta` definition order); the tests assert they match.
//!
//! Wiring note: the crate root declares `pub mod tasks_cleanup;` and this
//! module's parent declares `pub mod version_queries;` (foundation changes,
//! per the merged layer-PR precedent); these files are new-files-only.
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * Every model here rides a `SoftDeletionManager`: all reads carry
//!   `WHERE "t"."deleted_at" IS NULL`, verified by compiling each statement
//!   with Django (`str(qs.query)` under `pi_dash.settings.test`).
//! * `select_related("workspace", "project")` in both sync batches emits
//!   `INNER JOIN`s on non-nullable FKs (Django would render `LEFT OUTER
//!   JOIN` otherwise), so the joins filter nothing; no joined column is
//!   consumed downstream (only the issue's own `workspace_id`/`project_id`),
//!   so the port selects from `issues` alone — same rows, same consumed
//!   values, documented here.
//! * `.get(id=)` renders `LIMIT 21` plus the inert `Meta.ordering`
//!   `ORDER BY`; the port uses a bare `LIMIT 1`: a primary-key lookup
//!   cannot return two rows, so both the guard and the single-row ordering
//!   are unobservable. Same for `Page.objects.get`.
//! * Narrowed projections (`member_id` only for the admin fallback;
//!   `(id, issue_id, value)` for related data; owner `username`/`email`
//!   for the bug comparison) drop columns no call site consumes; row
//!   selection and order match Django exactly.
//! * The cycle map keeps Django's `Meta.ordering` (`created_at DESC`):
//!   the dict comprehension's last-wins duplicate rule is order-dependent,
//!   and the port's `HashMap::insert` replays it over the same order.
//! * `bulk_create` with no `batch_size` (description sync) is one multi-row
//!   `INSERT`; with `batch_size=1000` (issue sync) it chunks —
//!   [`ISSUE_VERSION_BULK_BATCH_SIZE`] names the chunk size, chunking itself
//!   happens at the call site over [`issue_versions_insert_sql`].
//! * `log_issue_version` (`db/models/issue.py:863-904`) writes
//!   `properties={}` / `meta={}` (empty literals, NOT the issue's — and the
//!   live `Issue` model has no `properties`/`meta` columns at all, so the
//!   sync path's `getattr(issue, "properties", {})` also always yields
//!   `{}`); `owned_by` is the passed user, not `get_owner_id`; single-issue
//!   sub-queries, not the bulk `related_data`.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `issue_task` coalesces on `str(issue_version.owned_by) == str(user_id)`
//!   (`issue_version_sync.py:49`) — `owned_by` is the related `User`
//!   *object*, whose `__str__` is `"{username} <{email}>"`
//!   (`db/models/user.py:139-140`), so the comparison against a user-id
//!   string is effectively never true and every detected change mints a new
//!   `IssueVersion`. [`LATEST_ISSUE_VERSION_OWNER_SQL`] fetches the owner's
//!   `username`/`email` so the decision logic renders the identical string.
//! * `track_page_version` always writes `sub_pages_data={}` (fresh literal,
//!   never carried over) and its update path saves `update_fields=[...,
//!   "updated_at"]` — `updated_at`, NOT `last_saved_at`
//!   (`page_version_task.py:48-56`). [`PAGE_VERSION_UPDATE_COLUMNS`].
//! * `PageVersion.save` / `Page.save` recompute
//!   `description_stripped = strip_tags(description_html)` (`None` when the
//!   html is empty/`None`) on every save (`db/models/page.py:70-80,175-181`),
//!   so the stripped value the task copies is always derived — the services
//!   layer re-derives it identically instead of trusting a stale column.
//! * `should_update_existing_version` returns bare `None` (not `False`)
//!   when there is no version (`issue_description_version_task.py:18-19`);
//!   the services layer preserves the `None`.

use sea_query::SimpleExpr;
use sea_query::{
    Alias, Asterisk, Expr, Func, InsertStatement, JoinType, Order, Query, UpdateStatement,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Column lists (Django `_meta` order, cf. `columns.json`)
// ---------------------------------------------------------------------------

/// `issues` concrete columns in `_meta` order, used by the full-row selects
/// (`columns.json`: the trailing `assignees`/`labels` entries are
/// `ManyToManyField`s with no physical column and are excluded, exactly as
/// Django's own `SELECT` does).
pub const ISSUE_COLUMNS: [&str; 34] = [
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "project_id",
    "workspace_id",
    "parent_id",
    "state_id",
    "point",
    "estimate_point_id",
    "name",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "type_id",
    "git_work_branch",
    "workpad",
    "created_via",
    "assigned_pod_id",
    "agent_executor",
];

/// The nine `.only()` columns of the description backfill, in the order
/// Django's compiler emits them (`_meta` order, not the call-site order at
/// `issue_description_version_sync.py:57-67`).
pub const ISSUE_DESCRIPTION_ONLY_COLUMNS: [&str; 9] = [
    "created_by_id",
    "updated_by_id",
    "id",
    "project_id",
    "workspace_id",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
];

/// `issue_versions` columns in `_meta` order (bulk `INSERT` order).
pub const ISSUE_VERSION_COLUMNS: [&str; 33] = [
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "project_id",
    "workspace_id",
    "parent",
    "state",
    "estimate_point",
    "name",
    "priority",
    "start_date",
    "target_date",
    "assignees",
    "sequence_id",
    "labels",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "type",
    "cycle",
    "modules",
    "properties",
    "meta",
    "last_saved_at",
    "issue_id",
    "activity_id",
    "owned_by_id",
];

/// `issue_description_versions` columns in `_meta` order.
pub const ISSUE_DESCRIPTION_VERSION_COLUMNS: [&str; 15] = [
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "project_id",
    "workspace_id",
    "issue_id",
    "description_binary",
    "description_html",
    "description_stripped",
    "description_json",
    "last_saved_at",
    "owned_by_id",
];

/// `page_versions` columns in `_meta` order.
pub const PAGE_VERSION_COLUMNS: [&str; 15] = [
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "workspace_id",
    "page_id",
    "last_saved_at",
    "owned_by_id",
    "description_binary",
    "description_html",
    "description_stripped",
    "description_json",
    "sub_pages_data",
];

/// `pages` concrete columns in `_meta` order, used by the page fetch
/// (`columns.json`: `labels`/`projects` are `ManyToManyField`s, excluded).
pub const PAGE_COLUMNS: [&str; 26] = [
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
    "id",
    "workspace_id",
    "name",
    "description_json",
    "description_binary",
    "description_html",
    "description_stripped",
    "owned_by_id",
    "access",
    "color",
    "parent_id",
    "archived_at",
    "is_locked",
    "view_props",
    "logo_props",
    "is_global",
    "moved_to_page",
    "moved_to_project",
    "sort_order",
    "external_id",
    "external_source",
];

/// Fixed `update_fields` of `update_existing_version`
/// (`issue_description_version_task.py:32-39`).
pub const DESCRIPTION_UPDATE_COLUMNS: [&str; 5] = [
    "description_json",
    "description_html",
    "description_binary",
    "description_stripped",
    "last_saved_at",
];

/// Fixed `update_fields` of the page-version update path — note the last
/// entry is `updated_at`, NOT `last_saved_at` (`page_version_task.py:48-56`).
pub const PAGE_VERSION_UPDATE_COLUMNS: [&str; 6] = [
    "description_html",
    "description_binary",
    "description_json",
    "description_stripped",
    "sub_pages_data",
    "updated_at",
];

/// `bulk_create(..., batch_size=1000)` chunk size of `sync_issue_version`
/// (`issue_version_sync.py:214`).
pub const ISSUE_VERSION_BULK_BATCH_SIZE: usize = 1000;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn table_alias(table: &str) -> Alias {
    Alias::new(table.to_owned())
}

fn col_exprs(table: &str, columns: &[&str]) -> Vec<SimpleExpr> {
    columns
        .iter()
        .map(|c| Expr::col((table_alias(table), Alias::new(c.to_owned()))).into())
        .collect()
}

fn iden_aliases(columns: &[&str]) -> Vec<Alias> {
    columns.iter().map(|c| Alias::new(c.to_owned())).collect()
}

// ---------------------------------------------------------------------------
// Count + batched reads
// ---------------------------------------------------------------------------

/// `Issue.objects.count()` — soft-delete-scoped
/// (`issue_version_sync.py:187`, `issue_description_version_sync.py:45`).
pub fn issue_count_stmt() -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.expr_as(
        Func::count::<SimpleExpr>(Expr::col(Asterisk).into()),
        "__count",
    );
    stmt.from(Alias::new("issues".to_owned())).and_where(
        Expr::col((
            Alias::new("issues".to_owned()),
            Alias::new("deleted_at".to_owned()),
        ))
        .is_null(),
    );
    stmt
}

/// Full-row batch: `order_by("created_at")` slice `[offset:end_offset]`
/// with `select_related` joins omitted (see module docs).
/// Django renders `LIMIT <n> OFFSET <m>` as literals.
pub fn issues_batch_stmt(limit: i64, offset: i64) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.exprs(col_exprs("issues", &ISSUE_COLUMNS));
    stmt.from(Alias::new("issues".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("issues".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .order_by(Alias::new("created_at".to_owned()), Order::Asc)
        .limit(limit as u64)
        .offset(offset as u64);
    stmt
}

/// `.only(...)` batch of the description backfill
/// (`issue_description_version_sync.py:54-68`).
pub fn issues_batch_limited_stmt(limit: i64, offset: i64) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.exprs(col_exprs("issues", &ISSUE_DESCRIPTION_ONLY_COLUMNS));
    stmt.from(Alias::new("issues".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("issues".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .order_by(Alias::new("created_at".to_owned()), Order::Asc)
        .limit(limit as u64)
        .offset(offset as u64);
    stmt
}

/// Latest version rows: `filter(fk=).order_by("-last_saved_at").first()`
/// under the soft-delete scope.
pub fn latest_version_stmt(
    table: &str,
    fk: &str,
    columns: &[&str],
    fk_value: Uuid,
) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.exprs(col_exprs(table, columns));
    stmt.from(table_alias(table))
        .and_where(Expr::col((table_alias(table), Alias::new("deleted_at".to_owned()))).is_null())
        .and_where(Expr::col((table_alias(table), Alias::new(fk.to_owned()))).eq(fk_value))
        .order_by(Alias::new("last_saved_at".to_owned()), Order::Desc)
        .limit(1);
    stmt
}

/// Latest `IssueVersion` for one issue (`issue_version_sync.py:45`).
pub fn latest_issue_version_stmt(fk_value: Uuid) -> sea_query::SelectStatement {
    latest_version_stmt(
        "issue_versions",
        "issue_id",
        &ISSUE_VERSION_COLUMNS,
        fk_value,
    )
}

/// Latest `IssueDescriptionVersion` (`issue_description_version_task.py:58-60`).
pub fn latest_description_version_stmt(fk_value: Uuid) -> sea_query::SelectStatement {
    latest_version_stmt(
        "issue_description_versions",
        "issue_id",
        &ISSUE_DESCRIPTION_VERSION_COLUMNS,
        fk_value,
    )
}

/// Latest `PageVersion` (`page_version_task.py:35`).
pub fn latest_page_version_stmt(fk_value: Uuid) -> sea_query::SelectStatement {
    latest_version_stmt("page_versions", "page_id", &PAGE_VERSION_COLUMNS, fk_value)
}

/// Owner rendering for the `issue_task` coalesce bug: the latest version's
/// id, its `owned_by` user's `username`/`email` and its `last_saved_at`,
/// so the caller can compare `"{username} <{email}>" == user_id` and the
/// 600s window exactly as Python's `str(version.owned_by)` comparison does.
pub fn latest_issue_version_owner_stmt(issue_id: Uuid) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.exprs(vec![
        Expr::col((
            Alias::new("issue_versions".to_owned()),
            Alias::new("id".to_owned()),
        )),
        Expr::col((
            Alias::new("users".to_owned()),
            Alias::new("username".to_owned()),
        )),
        Expr::col((
            Alias::new("users".to_owned()),
            Alias::new("email".to_owned()),
        )),
        Expr::col((
            Alias::new("issue_versions".to_owned()),
            Alias::new("last_saved_at".to_owned()),
        )),
    ]);
    stmt.from(Alias::new("issue_versions".to_owned()))
        .join(
            JoinType::InnerJoin,
            Alias::new("users".to_owned()),
            Expr::col((
                Alias::new("issue_versions".to_owned()),
                Alias::new("owned_by_id".to_owned()),
            ))
            .equals((Alias::new("users".to_owned()), Alias::new("id".to_owned()))),
        )
        .and_where(
            Expr::col((
                Alias::new("issue_versions".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(
            Expr::col((
                Alias::new("issue_versions".to_owned()),
                Alias::new("issue_id".to_owned()),
            ))
            .eq(issue_id),
        )
        .order_by(Alias::new("last_saved_at".to_owned()), Order::Desc)
        .limit(1);
    stmt
}

/// Project-admin fallback of both `get_owner_id`s
/// (`issue_version_sync.py:77-80`, `issue_description_version_sync.py:31-34`):
/// `filter(project_id=, role=20).first()` — the `Meta.ordering`
/// (`-created_at`) applies since there is no explicit `order_by`.
pub fn admin_fallback_stmt(project_id: Uuid) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.expr(Expr::col((
        Alias::new("project_members".to_owned()),
        Alias::new("member_id".to_owned()),
    )));
    stmt.from(Alias::new("project_members".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("project_members".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(
            Expr::col((
                Alias::new("project_members".to_owned()),
                Alias::new("project_id".to_owned()),
            ))
            .eq(project_id),
        )
        .and_where(
            Expr::col((
                Alias::new("project_members".to_owned()),
                Alias::new("role".to_owned()),
            ))
            .eq(20),
        )
        .order_by(Alias::new("created_at".to_owned()), Order::Desc)
        .limit(1);
    stmt
}

// ---------------------------------------------------------------------------
// Related-data queries (`get_related_data`, `issue_version_sync.py:85-128`)
// ---------------------------------------------------------------------------

/// `IN`-list select with `ORDER BY <id> ASC` (assignees/labels/modules),
/// under the soft-delete scope.
pub fn related_in_stmt(
    table: &str,
    id_col: &str,
    value_col: &str,
    ids: &[Uuid],
) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.exprs(vec![
        Expr::col((table_alias(table), Alias::new("issue_id".to_owned()))),
        Expr::col((table_alias(table), Alias::new(value_col.to_owned()))),
    ]);
    stmt.from(table_alias(table))
        .and_where(Expr::col((table_alias(table), Alias::new("deleted_at".to_owned()))).is_null())
        .and_where(
            Expr::col((table_alias(table), Alias::new(id_col.to_owned())))
                .is_in(ids.iter().cloned()),
        )
        .order_by(Alias::new("issue_id".to_owned()), Order::Asc);
    stmt
}

/// `CycleIssue` map (`issue_version_sync.py:88`): the `Meta.ordering`
/// (`-created_at`) governs iteration, and the dict comprehension keeps the
/// LAST row per issue — the port inserts over the same order.
pub fn cycle_issues_stmt(ids: &[Uuid]) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.exprs(vec![
        Expr::col((
            Alias::new("cycle_issues".to_owned()),
            Alias::new("issue_id".to_owned()),
        )),
        Expr::col((
            Alias::new("cycle_issues".to_owned()),
            Alias::new("cycle_id".to_owned()),
        )),
    ]);
    stmt.from(Alias::new("cycle_issues".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("cycle_issues".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(
            Expr::col((
                Alias::new("cycle_issues".to_owned()),
                Alias::new("issue_id".to_owned()),
            ))
            .is_in(ids.iter().cloned()),
        )
        .order_by(Alias::new("created_at".to_owned()), Order::Desc);
    stmt
}

/// `IssueActivity` latest-per-issue (`issue_version_sync.py:116`):
/// `order_by("issue_id", "-created_at")`, first per group in Python.
pub fn activities_stmt(ids: &[Uuid]) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.exprs(vec![
        Expr::col((
            Alias::new("issue_activities".to_owned()),
            Alias::new("id".to_owned()),
        )),
        Expr::col((
            Alias::new("issue_activities".to_owned()),
            Alias::new("issue_id".to_owned()),
        )),
        Expr::col((
            Alias::new("issue_activities".to_owned()),
            Alias::new("created_at".to_owned()),
        )),
    ]);
    stmt.from(Alias::new("issue_activities".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("issue_activities".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(
            Expr::col((
                Alias::new("issue_activities".to_owned()),
                Alias::new("issue_id".to_owned()),
            ))
            .is_in(ids.iter().cloned()),
        )
        .order_by(Alias::new("issue_id".to_owned()), Order::Asc)
        .order_by(Alias::new("created_at".to_owned()), Order::Desc);
    stmt
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// Single-row `INSERT` with the `_meta`-order columns and `rows` value
/// groups (each row aligned with `columns`).
pub fn insert_stmt(table: &str, columns: &[&str], rows: Vec<Vec<SimpleExpr>>) -> InsertStatement {
    let mut stmt = Query::insert();
    stmt.into_table(table_alias(table));
    stmt.columns(iden_aliases(columns));
    for row in rows {
        stmt.values_panic(row);
    }
    stmt
}

/// Multi-row `INSERT` for `bulk_create` (`rows` row groups).
pub fn bulk_insert_stmt(
    table: &str,
    columns: &[&str],
    rows: Vec<Vec<SimpleExpr>>,
) -> InsertStatement {
    insert_stmt(table, columns, rows)
}

/// `save(update_fields=[...])`: `UPDATE ... SET "c"=$i ... WHERE id=$N`.
pub fn update_fields_stmt(
    table: &str,
    columns: &[&str],
    values: Vec<SimpleExpr>,
    id: Uuid,
) -> UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(table_alias(table));
    for (col, val) in columns.iter().zip(values) {
        stmt.value(Alias::new(col.to_owned()), val);
    }
    stmt.and_where(Expr::col((table_alias(table), Alias::new("id".to_owned()))).eq(id));
    stmt
}

/// `PageVersion` count per page (`page_version_task.py:72`):
/// `.filter(page_id=).count()` under the soft-delete scope.
pub fn page_version_count_stmt(page_id: Uuid) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.expr_as(
        Func::count::<SimpleExpr>(Expr::col(Asterisk).into()),
        "__count",
    );
    stmt.from(Alias::new("page_versions".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("page_versions".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(
            Expr::col((
                Alias::new("page_versions".to_owned()),
                Alias::new("page_id".to_owned()),
            ))
            .eq(page_id),
        );
    stmt
}

/// Oldest version id per page for the one-row prune
/// (`page_version_task.py:74`).
pub fn oldest_page_version_id_stmt(page_id: Uuid) -> sea_query::SelectStatement {
    let mut stmt = Query::select();
    stmt.expr(Expr::col((
        Alias::new("page_versions".to_owned()),
        Alias::new("id".to_owned()),
    )));
    stmt.from(Alias::new("page_versions".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("page_versions".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(
            Expr::col((
                Alias::new("page_versions".to_owned()),
                Alias::new("page_id".to_owned()),
            ))
            .eq(page_id),
        )
        .order_by(Alias::new("last_saved_at".to_owned()), Order::Asc)
        .limit(1);
    stmt
}

/// Prune by primary key (`.first().delete()` on a `SoftDeleteModel`:
/// `update(deleted_at=now())`, never a row removal).
pub fn prune_page_version_by_id_stmt(
    now: chrono::DateTime<chrono::Utc>,
    id: Uuid,
) -> UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new("page_versions".to_owned()));
    stmt.value(Alias::new("deleted_at".to_owned()), now);
    stmt.and_where(
        Expr::col((
            Alias::new("page_versions".to_owned()),
            Alias::new("id".to_owned()),
        ))
        .eq(id),
    );
    stmt.and_where(
        Expr::col((
            Alias::new("page_versions".to_owned()),
            Alias::new("deleted_at".to_owned()),
        ))
        .is_null(),
    );
    stmt
}

/// Read one `issues` row by id into a [`sea_orm::QueryResult`] (caller
/// maps columns by name): `Issue.objects.get(id=)` under the soft-delete
/// scope (see module docs for the `LIMIT 1` choice).
pub async fn fetch_issue_row(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Option<sea_orm::QueryResult>, sqlx::Error> {
    use sea_orm::{ConnectionTrait, DbBackend};
    let mut stmt = Query::select();
    stmt.exprs(col_exprs("issues", &ISSUE_COLUMNS));
    stmt.from(Alias::new("issues".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("issues".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(Expr::col((Alias::new("issues".to_owned()), Alias::new("id".to_owned()))).eq(id))
        .limit(1);
    let db = crate::pool::sea_orm_connection(pool);
    db.query_one(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(crate::orm_tx::into_sqlx)
}

/// Read one `pages` row by id into a [`sea_orm::QueryResult`]
/// (`Page.objects.get(id=)`, same scope and limit notes).
pub async fn fetch_page_row(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Option<sea_orm::QueryResult>, sqlx::Error> {
    use sea_orm::{ConnectionTrait, DbBackend};
    let mut stmt = Query::select();
    stmt.exprs(col_exprs("pages", &PAGE_COLUMNS));
    stmt.from(Alias::new("pages".to_owned()))
        .and_where(
            Expr::col((
                Alias::new("pages".to_owned()),
                Alias::new("deleted_at".to_owned()),
            ))
            .is_null(),
        )
        .and_where(Expr::col((Alias::new("pages".to_owned()), Alias::new("id".to_owned()))).eq(id))
        .limit(1);
    let db = crate::pool::sea_orm_connection(pool);
    db.query_one(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(crate::orm_tx::into_sqlx)
}

/// Helper: pull an optional UUID column out of a row.
pub fn opt_uuid(row: &sea_orm::QueryResult, column: &str) -> Result<Option<Uuid>, sqlx::Error> {
    row.try_get_by(column).map_err(crate::orm_tx::into_sqlx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;
    use std::collections::HashSet;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tasks_cleanup")
    }

    fn columns_fixture() -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_dir().join("columns.json"))
            .expect("read columns.json");
        serde_json::from_str(&body).expect("columns.json is valid JSON")
    }

    /// Copy a const into an owned vec (clippy denies asserting on constants).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Concrete (physical) columns from the fixture: every field except
    /// `ManyToManyField`s, which have no column. The fixture object is
    /// alphabetically ordered, so this is a set-membership check; order
    /// against Django's `_meta` emission is pinned by the SQL-text tests
    /// below (verified with `str(qs.query)` under
    /// `pi_dash.settings.test`).
    fn concrete_sorted(table: &serde_json::Value) -> Vec<String> {
        let mut cols: Vec<String> = table["fields"]
            .as_object()
            .expect("fields object")
            .iter()
            .filter(|(_, v)| v.as_str() != Some("ManyToManyField"))
            .map(|(k, _)| k.clone())
            .collect();
        cols.sort();
        cols
    }

    #[test]
    fn column_lists_match_columns_fixture() {
        let fx = columns_fixture();
        let tables = &fx["tables"];
        for (table, cols) in [
            ("issues", &ISSUE_COLUMNS[..]),
            ("issue_versions", &ISSUE_VERSION_COLUMNS[..]),
            (
                "issue_description_versions",
                &ISSUE_DESCRIPTION_VERSION_COLUMNS[..],
            ),
            ("page_versions", &PAGE_VERSION_COLUMNS[..]),
            ("pages", &PAGE_COLUMNS[..]),
        ] {
            let mut ours = owned(cols);
            ours.sort();
            assert_eq!(
                ours,
                concrete_sorted(&tables[table]),
                "column set for {table}"
            );
        }
    }

    #[test]
    fn only_columns_match_django_emission_order() {
        // `.only(...)` at `issue_description_version_sync.py:57-67`;
        // Django's compiler reorders to `_meta` order (verified via
        // `str(qs.query)`), not the call-site order.
        assert_eq!(
            ISSUE_DESCRIPTION_ONLY_COLUMNS,
            [
                "created_by_id",
                "updated_by_id",
                "id",
                "project_id",
                "workspace_id",
                "description_json",
                "description_html",
                "description_stripped",
                "description_binary",
            ]
        );
    }

    #[test]
    fn update_field_lists_match_sources() {
        // `issue_description_version_task.py:32-39`.
        assert_eq!(
            DESCRIPTION_UPDATE_COLUMNS,
            [
                "description_json",
                "description_html",
                "description_binary",
                "description_stripped",
                "last_saved_at",
            ]
        );
        // `page_version_task.py:48-56` — `updated_at`, NOT `last_saved_at`.
        assert_eq!(
            PAGE_VERSION_UPDATE_COLUMNS,
            [
                "description_html",
                "description_binary",
                "description_json",
                "description_stripped",
                "sub_pages_data",
                "updated_at",
            ]
        );
    }

    #[test]
    fn static_statements_match_django_rendering() {
        let id = uuid::Uuid::new_v4();
        let rendered = issue_count_stmt().to_string(PostgresQueryBuilder);
        assert!(rendered.contains("COUNT(*)"), "{rendered}");
        assert!(rendered.contains("\"issues\""), "{rendered}");
        assert!(rendered.contains("\"deleted_at\" IS NULL"), "{rendered}");
        // `Meta.ordering` (`-created_at`) applies: no explicit `order_by`.
        let rendered = admin_fallback_stmt(id).to_string(PostgresQueryBuilder);
        assert!(
            rendered.contains("\"project_members\".\"member_id\""),
            "{rendered}"
        );
        assert!(rendered.contains("\"deleted_at\" IS NULL"), "{rendered}");
        assert!(rendered.contains("\"role\" = 20"), "{rendered}");
        assert!(
            rendered.contains("ORDER BY \"created_at\" DESC"),
            "{rendered}"
        );
        assert!(rendered.ends_with("LIMIT 1"), "{rendered}");
        let rendered = page_version_count_stmt(id).to_string(PostgresQueryBuilder);
        assert!(rendered.contains("COUNT(*)"), "{rendered}");
        assert!(rendered.contains("\"page_versions\""), "{rendered}");
        let rendered = oldest_page_version_id_stmt(id).to_string(PostgresQueryBuilder);
        assert!(rendered.contains("\"page_versions\".\"id\""), "{rendered}");
        assert!(
            rendered.contains("ORDER BY \"last_saved_at\" ASC"),
            "{rendered}"
        );
        // Owner join carries the version id, the username/email pair for
        // the `str(owned_by)` bug-compatibility comparison, and the stamp
        // for the 600s window.
        let rendered = latest_issue_version_owner_stmt(id).to_string(PostgresQueryBuilder);
        assert!(rendered.contains("INNER JOIN"), "{rendered}");
        assert!(rendered.contains("\"users\""), "{rendered}");
        assert!(rendered.contains("\"issue_versions\".\"id\""), "{rendered}");
        assert!(
            rendered.contains("\"issue_versions\".\"last_saved_at\""),
            "{rendered}"
        );
        assert!(
            rendered.contains("ORDER BY \"last_saved_at\" DESC"),
            "{rendered}"
        );
    }

    #[test]
    fn batch_sql_renders_django_slice() {
        // `[offset:end_offset]` with `end-offset = 5000`: soft-delete
        // scope, literals, ASC.
        let sql = issues_batch_stmt(5000, 0).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("SELECT"));
        assert!(sql.contains("\"issues\".\"created_at\""));
        assert!(sql.contains("\"deleted_at\" IS NULL"));
        assert!(sql.ends_with("ORDER BY \"created_at\" ASC LIMIT 5000 OFFSET 0"));
        // Second page.
        assert!(issues_batch_stmt(5000, 5000)
            .to_string(PostgresQueryBuilder)
            .ends_with("LIMIT 5000 OFFSET 5000"));
        // Short tail batch.
        assert!(issues_batch_limited_stmt(7, 10000)
            .to_string(PostgresQueryBuilder)
            .ends_with("LIMIT 7 OFFSET 10000"));
        // `.only()` selects exactly the nine columns in `_meta` order.
        let limited = issues_batch_limited_stmt(1, 0).to_string(PostgresQueryBuilder);
        let mut positions = Vec::new();
        for col in ISSUE_DESCRIPTION_ONLY_COLUMNS {
            let needle = format!("\"{col}\"");
            positions.push(limited.find(&needle).unwrap_or(usize::MAX));
        }
        assert!(positions.iter().all(|p| *p != usize::MAX));
        let mut ordered = positions.clone();
        ordered.sort();
        assert_eq!(positions, ordered, "only() columns in _meta order");
        assert!(!limited.contains("\"name\""));
    }

    #[test]
    fn related_queries_match_grouping_protocol() {
        // Assignees/labels/modules: soft-delete scope, IN list, ordered by
        // issue_id (groupby).
        let ids = vec![uuid::Uuid::new_v4(); 3];
        let assignees = related_in_stmt("issue_assignees", "issue_id", "assignee_id", &ids)
            .to_string(PostgresQueryBuilder);
        assert!(
            assignees.contains("\"issue_assignees\".\"issue_id\""),
            "{assignees}"
        );
        assert!(
            assignees.contains("\"issue_assignees\".\"assignee_id\""),
            "{assignees}"
        );
        assert!(assignees.contains("IN"), "{assignees}");
        assert!(
            assignees.contains("ORDER BY \"issue_id\" ASC"),
            "{assignees}"
        );
        // Cycle map: `Meta.ordering` (`created_at DESC`); the dict
        // comprehension keeps the last row per issue over this order.
        let cycles = cycle_issues_stmt(&ids[..2]).to_string(PostgresQueryBuilder);
        assert!(cycles.contains("IN"), "{cycles}");
        assert!(cycles.ends_with("ORDER BY \"created_at\" DESC"));
        // Activities: dual ordering, first-per-group taken in Python.
        let activities = activities_stmt(&ids[..1]).to_string(PostgresQueryBuilder);
        assert!(
            activities.contains("\"deleted_at\" IS NULL"),
            "{activities}"
        );
        assert!(
            activities.contains("ORDER BY \"issue_id\" ASC"),
            "{activities}"
        );
        assert!(activities.contains("\"created_at\" DESC"), "{activities}");
    }

    #[test]
    fn write_builders_shape() {
        let rendered = insert_stmt(
            "issue_versions",
            &ISSUE_VERSION_COLUMNS,
            vec![vec![Expr::value(0); ISSUE_VERSION_COLUMNS.len()]],
        )
        .to_string(PostgresQueryBuilder);
        assert!(rendered.starts_with("INSERT INTO \"issue_versions\" (\"created_at\""));
        assert!(rendered.ends_with("\") VALUES (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)"));
        // Bulk: two row groups.
        let bulk = insert_stmt(
            "issue_description_versions",
            &ISSUE_DESCRIPTION_VERSION_COLUMNS,
            vec![
                vec![Expr::value(0); ISSUE_DESCRIPTION_VERSION_COLUMNS.len()],
                vec![Expr::value(0); ISSUE_DESCRIPTION_VERSION_COLUMNS.len()],
            ],
        )
        .to_string(PostgresQueryBuilder);
        assert!(bulk.contains("), ("));
        // Coalesce update: SET list then PK placeholder last.
        let id = uuid::Uuid::new_v4();
        let upd = update_fields_stmt(
            "issue_description_versions",
            &DESCRIPTION_UPDATE_COLUMNS,
            vec![
                Expr::value("x"),
                Expr::value("y"),
                Expr::value(None::<Vec<u8>>),
                Expr::value(None::<String>),
                Expr::value(chrono::Utc::now()),
            ],
            id,
        )
        .to_string(PostgresQueryBuilder);
        assert!(upd.starts_with("UPDATE \"issue_description_versions\" SET"));
        assert!(upd.contains("\"last_saved_at\" ="));
        assert!(upd.contains("\"issue_description_versions\".\"id\" ="));
        let page_upd = update_fields_stmt(
            "page_versions",
            &PAGE_VERSION_UPDATE_COLUMNS,
            vec![
                Expr::value("a"),
                Expr::value(None::<Vec<u8>>),
                Expr::value("c"),
                Expr::value(None::<String>),
                Expr::value("e"),
                Expr::value(chrono::Utc::now()),
            ],
            id,
        )
        .to_string(PostgresQueryBuilder);
        assert!(page_upd.contains("\"updated_at\" ="));
        assert!(page_upd.contains("\"page_versions\".\"id\" ="));
    }

    #[test]
    fn latest_version_sql_orders_by_last_saved_desc() {
        let id = uuid::Uuid::new_v4();
        for (sql, table, fk) in [
            (
                latest_issue_version_stmt(id).to_string(PostgresQueryBuilder),
                "issue_versions",
                "issue_id",
            ),
            (
                latest_description_version_stmt(id).to_string(PostgresQueryBuilder),
                "issue_description_versions",
                "issue_id",
            ),
            (
                latest_page_version_stmt(id).to_string(PostgresQueryBuilder),
                "page_versions",
                "page_id",
            ),
        ] {
            assert!(sql.contains(&format!("\"{table}\""),), "{table}");
            assert!(sql.contains(&format!("\"{fk}\" ="),), "{table}");
            assert!(
                sql.ends_with("ORDER BY \"last_saved_at\" DESC LIMIT 1"),
                "{table}"
            );
        }
    }

    #[test]
    fn column_consts_have_no_duplicates() {
        for (name, cols) in [
            ("issues", &ISSUE_COLUMNS[..]),
            ("issue_versions", &ISSUE_VERSION_COLUMNS[..]),
            (
                "issue_description_versions",
                &ISSUE_DESCRIPTION_VERSION_COLUMNS[..],
            ),
            ("page_versions", &PAGE_VERSION_COLUMNS[..]),
            ("pages", &PAGE_COLUMNS[..]),
        ] {
            let set: HashSet<&&str> = cols.iter().collect();
            assert_eq!(set.len(), cols.len(), "duplicates in {name}");
        }
    }
}
