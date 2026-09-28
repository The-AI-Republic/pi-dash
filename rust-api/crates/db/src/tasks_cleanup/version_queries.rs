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

use sqlx::postgres::PgRow;
use sqlx::Row;
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

fn quoted_list(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|c| format!("\"{table}\".\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Count + batched reads
// ---------------------------------------------------------------------------

/// `Issue.objects.count()` — soft-delete-scoped
/// (`issue_version_sync.py:187`, `issue_description_version_sync.py:45`).
pub const ISSUE_COUNT_SQL: &str =
    "SELECT COUNT(*) AS \"__count\" FROM \"issues\" WHERE \"issues\".\"deleted_at\" IS NULL";

/// Full-row batch: `order_by("created_at")` slice `[offset:end_offset]`
/// with `select_related` joins omitted (see module docs).
/// Django renders `LIMIT <n> OFFSET <m>` as literals.
pub fn issues_batch_sql(limit: i64, offset: i64) -> String {
    format!(
        "SELECT {} FROM \"issues\" WHERE \"issues\".\"deleted_at\" IS NULL ORDER BY \"issues\".\"created_at\" ASC LIMIT {limit} OFFSET {offset}",
        quoted_list("issues", &ISSUE_COLUMNS)
    )
}

/// `.only(...)` batch of the description backfill
/// (`issue_description_version_sync.py:54-68`).
pub fn issues_batch_limited_sql(limit: i64, offset: i64) -> String {
    format!(
        "SELECT {} FROM \"issues\" WHERE \"issues\".\"deleted_at\" IS NULL ORDER BY \"issues\".\"created_at\" ASC LIMIT {limit} OFFSET {offset}",
        quoted_list("issues", &ISSUE_DESCRIPTION_ONLY_COLUMNS)
    )
}

/// Latest version rows: `filter(fk=).order_by("-last_saved_at").first()`
/// under the soft-delete scope.
pub fn latest_version_sql(table: &str, fk: &str, columns: &[&str]) -> String {
    format!(
        "SELECT {} FROM \"{table}\" WHERE (\"{table}\".\"deleted_at\" IS NULL AND \"{table}\".\"{fk}\" = $1) ORDER BY \"{table}\".\"last_saved_at\" DESC LIMIT 1",
        quoted_list(table, columns)
    )
}

/// Latest `IssueVersion` for one issue (`issue_version_sync.py:45`).
pub fn latest_issue_version_sql() -> String {
    latest_version_sql("issue_versions", "issue_id", &ISSUE_VERSION_COLUMNS)
}

/// Latest `IssueDescriptionVersion` (`issue_description_version_task.py:58-60`).
pub fn latest_description_version_sql() -> String {
    latest_version_sql(
        "issue_description_versions",
        "issue_id",
        &ISSUE_DESCRIPTION_VERSION_COLUMNS,
    )
}

/// Latest `PageVersion` (`page_version_task.py:35`).
pub fn latest_page_version_sql() -> String {
    latest_version_sql("page_versions", "page_id", &PAGE_VERSION_COLUMNS)
}

/// Owner rendering for the `issue_task` coalesce bug: the latest version's
/// id, its `owned_by` user's `username`/`email` and its `last_saved_at`,
/// so the caller can compare `"{username} <{email}>" == user_id` and the
/// 600s window exactly as Python's `str(version.owned_by)` comparison does.
pub const LATEST_ISSUE_VERSION_OWNER_SQL: &str = "SELECT \"issue_versions\".\"id\", \"users\".\"username\", \"users\".\"email\", \"issue_versions\".\"last_saved_at\" FROM \"issue_versions\" INNER JOIN \"users\" ON (\"issue_versions\".\"owned_by_id\" = \"users\".\"id\") WHERE (\"issue_versions\".\"deleted_at\" IS NULL AND \"issue_versions\".\"issue_id\" = $1) ORDER BY \"issue_versions\".\"last_saved_at\" DESC LIMIT 1";

/// Project-admin fallback of both `get_owner_id`s
/// (`issue_version_sync.py:77-80`, `issue_description_version_sync.py:31-34`):
/// `filter(project_id=, role=20).first()` — the `Meta.ordering`
/// (`-created_at`) applies since there is no explicit `order_by`.
pub const ADMIN_FALLBACK_SQL: &str = "SELECT \"project_members\".\"member_id\" FROM \"project_members\" WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"project_id\" = $1 AND \"project_members\".\"role\" = 20) ORDER BY \"project_members\".\"created_at\" DESC LIMIT 1";

// ---------------------------------------------------------------------------
// Related-data queries (`get_related_data`, `issue_version_sync.py:85-128`)
// ---------------------------------------------------------------------------

/// `IN`-list select with `ORDER BY <id> ASC` (assignees/labels/modules),
/// under the soft-delete scope.
pub fn related_in_sql(table: &str, id_col: &str, value_col: &str, n: usize) -> String {
    let placeholders = (1..=n)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT \"{table}\".\"issue_id\", \"{table}\".\"{value_col}\" FROM \"{table}\" WHERE (\"{table}\".\"deleted_at\" IS NULL AND \"{table}\".\"{id_col}\" IN ({placeholders})) ORDER BY \"{table}\".\"issue_id\" ASC"
    )
}

/// `CycleIssue` map (`issue_version_sync.py:88`): the `Meta.ordering`
/// (`-created_at`) governs iteration, and the dict comprehension keeps the
/// LAST row per issue — the port inserts over the same order.
pub fn cycle_issues_sql(n: usize) -> String {
    let placeholders = (1..=n)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT \"cycle_issues\".\"issue_id\", \"cycle_issues\".\"cycle_id\" FROM \"cycle_issues\" WHERE (\"cycle_issues\".\"deleted_at\" IS NULL AND \"cycle_issues\".\"issue_id\" IN ({placeholders})) ORDER BY \"cycle_issues\".\"created_at\" DESC"
    )
}

/// `IssueActivity` latest-per-issue (`issue_version_sync.py:116`):
/// `order_by("issue_id", "-created_at")`, first per group in Python.
pub fn activities_sql(n: usize) -> String {
    let placeholders = (1..=n)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT \"issue_activities\".\"id\", \"issue_activities\".\"issue_id\", \"issue_activities\".\"created_at\" FROM \"issue_activities\" WHERE (\"issue_activities\".\"deleted_at\" IS NULL AND \"issue_activities\".\"issue_id\" IN ({placeholders})) ORDER BY \"issue_activities\".\"issue_id\" ASC, \"issue_activities\".\"created_at\" DESC"
    )
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// Single-row `INSERT` with the `_meta`-order columns.
pub fn insert_sql(table: &str, columns: &[&str]) -> String {
    let names = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let values = (1..=columns.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO \"{table}\" ({names}) VALUES ({values})")
}

/// Multi-row `INSERT` for `bulk_create` (`rows` row groups).
pub fn bulk_insert_sql(table: &str, columns: &[&str], rows: usize) -> String {
    let names = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut groups = Vec::with_capacity(rows);
    for r in 0..rows {
        let base = r * columns.len();
        let row = (1..=columns.len())
            .map(|i| format!("${}", base + i))
            .collect::<Vec<_>>()
            .join(", ");
        groups.push(format!("({row})"));
    }
    format!(
        "INSERT INTO \"{table}\" ({names}) VALUES {}",
        groups.join(", ")
    )
}

/// `save(update_fields=[...])`: `UPDATE ... SET "c"=$i ... WHERE id=$N`.
pub fn update_fields_sql(table: &str, columns: &[&str]) -> String {
    let set = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE \"{table}\" SET {set} WHERE \"{table}\".\"id\" = ${}",
        columns.len() + 1
    )
}

/// `PageVersion` count per page (`page_version_task.py:72`):
/// `.filter(page_id=).count()` under the soft-delete scope.
pub const PAGE_VERSION_COUNT_SQL: &str = "SELECT COUNT(*) AS \"__count\" FROM \"page_versions\" WHERE (\"page_versions\".\"deleted_at\" IS NULL AND \"page_versions\".\"page_id\" = $1)";

/// Oldest version id per page for the one-row prune
/// (`page_version_task.py:74`).
pub const OLDEST_PAGE_VERSION_ID_SQL: &str = "SELECT \"page_versions\".\"id\" FROM \"page_versions\" WHERE (\"page_versions\".\"deleted_at\" IS NULL AND \"page_versions\".\"page_id\" = $1) ORDER BY \"page_versions\".\"last_saved_at\" ASC LIMIT 1";

/// Prune delete by primary key (`.first().delete()`).
pub const DELETE_PAGE_VERSION_BY_ID_SQL: &str =
    "DELETE FROM \"page_versions\" WHERE \"page_versions\".\"id\" = $1";

/// Read one `issues` row by id into a [`PgRow`] (caller maps columns):
/// `Issue.objects.get(id=)` under the soft-delete scope (see module docs
/// for the `LIMIT 1` choice).
pub async fn fetch_issue_row<'e, E>(executor: E, id: Uuid) -> Result<Option<PgRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = format!(
        "SELECT {} FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1) LIMIT 1",
        quoted_list("issues", &ISSUE_COLUMNS)
    );
    sqlx::query(&sql).bind(id).fetch_optional(executor).await
}

/// Read one `pages` row by id into a [`PgRow`] (`Page.objects.get(id=)`,
/// same scope and limit notes).
pub async fn fetch_page_row<'e, E>(executor: E, id: Uuid) -> Result<Option<PgRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let sql = format!(
        "SELECT {} FROM \"pages\" WHERE (\"pages\".\"deleted_at\" IS NULL AND \"pages\".\"id\" = $1) LIMIT 1",
        quoted_list("pages", &PAGE_COLUMNS)
    );
    sqlx::query(&sql).bind(id).fetch_optional(executor).await
}

/// Helper: pull an optional UUID column out of a row.
pub fn opt_uuid(row: &PgRow, column: &str) -> Result<Option<Uuid>, sqlx::Error> {
    row.try_get(column)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(
            ISSUE_COUNT_SQL,
            "SELECT COUNT(*) AS \"__count\" FROM \"issues\" WHERE \"issues\".\"deleted_at\" IS NULL"
        );
        // `Meta.ordering` (`-created_at`) applies: no explicit `order_by`.
        assert_eq!(
            ADMIN_FALLBACK_SQL,
            "SELECT \"project_members\".\"member_id\" FROM \"project_members\" WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"project_id\" = $1 AND \"project_members\".\"role\" = 20) ORDER BY \"project_members\".\"created_at\" DESC LIMIT 1"
        );
        assert_eq!(
            PAGE_VERSION_COUNT_SQL,
            "SELECT COUNT(*) AS \"__count\" FROM \"page_versions\" WHERE (\"page_versions\".\"deleted_at\" IS NULL AND \"page_versions\".\"page_id\" = $1)"
        );
        assert_eq!(
            OLDEST_PAGE_VERSION_ID_SQL,
            "SELECT \"page_versions\".\"id\" FROM \"page_versions\" WHERE (\"page_versions\".\"deleted_at\" IS NULL AND \"page_versions\".\"page_id\" = $1) ORDER BY \"page_versions\".\"last_saved_at\" ASC LIMIT 1"
        );
        assert_eq!(
            DELETE_PAGE_VERSION_BY_ID_SQL,
            "DELETE FROM \"page_versions\" WHERE \"page_versions\".\"id\" = $1"
        );
        // Owner join carries the version id, the username/email pair for
        // the `str(owned_by)` bug-compatibility comparison, and the stamp
        // for the 600s window.
        assert!(LATEST_ISSUE_VERSION_OWNER_SQL.contains(
            "INNER JOIN \"users\" ON (\"issue_versions\".\"owned_by_id\" = \"users\".\"id\")"
        ));
        assert!(LATEST_ISSUE_VERSION_OWNER_SQL.contains("\"issue_versions\".\"id\""));
        assert!(LATEST_ISSUE_VERSION_OWNER_SQL.contains("\"issue_versions\".\"last_saved_at\""));
        assert!(LATEST_ISSUE_VERSION_OWNER_SQL
            .ends_with("ORDER BY \"issue_versions\".\"last_saved_at\" DESC LIMIT 1"));
    }

    #[test]
    fn batch_sql_renders_django_slice() {
        // `[offset:end_offset]` with `end-offset = 5000`: soft-delete
        // scope, literals, ASC.
        let sql = issues_batch_sql(5000, 0);
        assert!(sql.starts_with("SELECT \"issues\".\"created_at\""));
        assert!(sql.contains("FROM \"issues\" WHERE \"issues\".\"deleted_at\" IS NULL"));
        assert!(sql.ends_with("ORDER BY \"issues\".\"created_at\" ASC LIMIT 5000 OFFSET 0"));
        // Second page.
        assert!(issues_batch_sql(5000, 5000).ends_with("LIMIT 5000 OFFSET 5000"));
        // Short tail batch.
        assert!(issues_batch_limited_sql(7, 10000).ends_with("LIMIT 7 OFFSET 10000"));
        // `.only()` selects exactly the nine columns in `_meta` order.
        let limited = issues_batch_limited_sql(1, 0);
        let mut positions = Vec::new();
        for col in ISSUE_DESCRIPTION_ONLY_COLUMNS {
            let needle = format!("\"issues\".\"{col}\"");
            positions.push(limited.find(&needle).unwrap_or(usize::MAX));
        }
        assert!(positions.iter().all(|p| *p != usize::MAX));
        let mut ordered = positions.clone();
        ordered.sort();
        assert_eq!(positions, ordered, "only() columns in _meta order");
        assert!(!limited.contains("\"issues\".\"name\""));
    }

    #[test]
    fn related_queries_match_grouping_protocol() {
        // Assignees/labels/modules: soft-delete scope, IN list, ordered by
        // issue_id (groupby).
        let assignees = related_in_sql("issue_assignees", "issue_id", "assignee_id", 3);
        assert_eq!(
            assignees,
            "SELECT \"issue_assignees\".\"issue_id\", \"issue_assignees\".\"assignee_id\" FROM \"issue_assignees\" WHERE (\"issue_assignees\".\"deleted_at\" IS NULL AND \"issue_assignees\".\"issue_id\" IN ($1, $2, $3)) ORDER BY \"issue_assignees\".\"issue_id\" ASC"
        );
        // Cycle map: `Meta.ordering` (`created_at DESC`); the dict
        // comprehension keeps the last row per issue over this order.
        let cycles = cycle_issues_sql(2);
        assert!(cycles.contains("IN ($1, $2)"));
        assert!(cycles.ends_with("ORDER BY \"cycle_issues\".\"created_at\" DESC"));
        // Activities: dual ordering, first-per-group taken in Python.
        let activities = activities_sql(1);
        assert!(activities.contains(
            "WHERE (\"issue_activities\".\"deleted_at\" IS NULL AND \"issue_activities\".\"issue_id\" IN ($1))"
        ));
        assert!(activities.ends_with(
            "ORDER BY \"issue_activities\".\"issue_id\" ASC, \"issue_activities\".\"created_at\" DESC"
        ));
    }

    #[test]
    fn write_builders_shape() {
        let one = insert_sql("issue_versions", &ISSUE_VERSION_COLUMNS);
        assert!(one.starts_with("INSERT INTO \"issue_versions\" (\"created_at\""));
        assert!(one.ends_with("\"owned_by_id\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33)"));
        // Bulk: placeholder numbering continues across row groups.
        let bulk = bulk_insert_sql(
            "issue_description_versions",
            &ISSUE_DESCRIPTION_VERSION_COLUMNS,
            2,
        );
        assert!(bulk.contains("($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15), ($16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30)"));
        // Coalesce update: SET list then PK placeholder last.
        let upd = update_fields_sql("issue_description_versions", &DESCRIPTION_UPDATE_COLUMNS);
        assert_eq!(
            upd,
            "UPDATE \"issue_description_versions\" SET \"description_json\" = $1, \"description_html\" = $2, \"description_binary\" = $3, \"description_stripped\" = $4, \"last_saved_at\" = $5 WHERE \"issue_description_versions\".\"id\" = $6"
        );
        let page_upd = update_fields_sql("page_versions", &PAGE_VERSION_UPDATE_COLUMNS);
        assert!(page_upd.ends_with("\"updated_at\" = $6 WHERE \"page_versions\".\"id\" = $7"));
    }

    #[test]
    fn latest_version_sql_orders_by_last_saved_desc() {
        for (sql, table, fk) in [
            (latest_issue_version_sql(), "issue_versions", "issue_id"),
            (
                latest_description_version_sql(),
                "issue_description_versions",
                "issue_id",
            ),
            (latest_page_version_sql(), "page_versions", "page_id"),
        ] {
            assert!(
                sql.ends_with(&format!(
                    "WHERE (\"{table}\".\"deleted_at\" IS NULL AND \"{table}\".\"{fk}\" = $1) ORDER BY \"{table}\".\"last_saved_at\" DESC LIMIT 1"
                )),
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
