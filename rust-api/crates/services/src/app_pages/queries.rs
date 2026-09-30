#![forbid(unsafe_code)]

//! Page read query builders (D-30, stage 5).
//!
//! Ports the page read queries as one topological closure (builders + SQL,
//! same semantics), following the D-27 precedent
//! (`app_cycles/queries.rs`): each builder returns a fragment the caller
//! splices into the statement it executes. Placeholders stay symbolic —
//! `:slug`, `:user`, `:project_id`, `:page_id`, `:pk`, `:archived_at` —
//! exactly the notation the fixtures use; handlers bind them. The one
//! exception is [`archive_cte_sql`], which is verbatim raw SQL passed to
//! `cursor.execute` and therefore keeps the positional `%s` placeholders
//! of the Python source.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/page/base.py:59-72` — `unarchive_archive_page_and_descendants`
//!   recursive CTE.
//! - `app/views/page/base.py:81-127` — `PageViewSet.get_queryset` chain.
//! - `app/views/page/base.py:231-233` — `PageLog` `issue_ids` lookup.
//! - `app/views/page/base.py:308-366` — archive (`:335` CTE call) and
//!   unarchive (`:360-362` detach, `:364` CTE call).
//! - `app/views/page/base.py:421-469` — `summary` aggregate.
//! - `app/views/page/base.py:628-637` — duplicate re-fetch with
//!   `project_ids` annotation.
//! - `app/views/page/version.py:19-31` — version list/detail lookups.
//! - `db/models/page.py:23-28` — `PUBLIC_ACCESS = 0`, `PRIVATE_ACCESS = 1`.
//! - `app/permissions/base.py:13-17` — `GUEST = 5`.
//!
//! Fixture oracles: F30-06 (`queries/get_queryset.sql` + `.rows.json`),
//! F30-07 (`queries/summary.sql` + `.rows.json`), F30-08
//! (`queries/archive_cte.sql` + `.rows.json`), F30-12
//! (`handlers/versions.golden.json`, branches only — shapes live in
//! `shape.rs`). The unit tests below pin the builders against those files
//! so transcription drift fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. Double `order_by` (`:103` then `:105`): the second `order_by`
//!    REPLACES the first in Django, so the request's `?order_by=` value
//!    is silently discarded; the effective order is always
//!    (`-is_favorite`, `-created_at`). Both are ported
//!    ([`DEAD_REQUEST_ORDER_SQL`] is the dead one, [`LIST_ORDER_SQL`] the
//!    effective one).
//! 2. `project_ids` annotation filter `~Q(projects__id=True)` (`:121`):
//!    a UUID column compared to boolean `TRUE` — a type error-or-always-true
//!    no-op on Postgres. Ported as observed in [`project_ids_sql`]; the
//!    `COALESCE(..., '{}')` empty guard is what actually renders `[]`.
//! 3. Archive response builds `str(datetime.now())` AFTER the CTE call
//!    (`:335` vs `:337`) — two `now()` calls, so the body timestamp may
//!    differ from the DB `archived_at` by microseconds. Ported; see
//!    [`ARCHIVE_NOW_CALLS`].
//! 4. Unarchive detach `save(update_fields=["parent"])` (`:360-362`) still
//!    runs the full `save()`, including the `description_stripped`
//!    recompute. Ported; handlers must call the shared strip helper.
//!
//! Out of scope (owned by sibling issues): the `summary`/`list` guest
//! *decisions* live here as the `:451`/`:304` `owned_by` predicate, but
//! the role lookup itself is a guard-layer concern; archive/unarchive
//! owner-or-admin guards (`:317-326`, `:348-357`), the favorites delete
//! (`:328-333`), and every response envelope belong to handlers C/E.

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// `Page.PUBLIC_ACCESS` (`db/models/page.py:24-28`).
pub const PUBLIC_ACCESS: i32 = 0;

/// `Page.PRIVATE_ACCESS` (`db/models/page.py:24-28`).
pub const PRIVATE_ACCESS: i32 = 1;

/// `ROLE.GUEST.value` (`app/permissions/base.py:13-17`), used by the
/// summary (`:446`) and list (`:299`) guest scoping.
pub const GUEST_ROLE: i32 = 5;

/// `entity_type` value the favorite `Exists` matches (`base.py:84`).
pub const FAVORITE_ENTITY_TYPE: &str = "page";

/// `entity_name` value the `PageLog` lookup matches (`base.py:231`).
pub const PAGE_LOG_ISSUE_ENTITY: &str = "issue";

// ---------------------------------------------------------------------------
// get_queryset (base.py:81-127)
// ---------------------------------------------------------------------------

/// R1 base filters, in order (`:91-98`): workspace slug (`:91`),
/// member + active + live project via the projects M2M
/// (`:92-96`, join `pages -> project_pages -> projects ->
/// project_members`), top-level only (`:97`), owner-or-public (`:98`).
pub fn queryset_scope_where() -> String {
    [
        "pages.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)",
        "EXISTS (SELECT 1 FROM project_pages pp_scope JOIN projects ON projects.id = pp_scope.project_id JOIN project_members pm ON pm.project_id = projects.id AND pm.member_id = :user AND pm.is_active = TRUE WHERE pp_scope.page_id = pages.id AND projects.archived_at IS NULL)",
        "pages.parent_id IS NULL",
        "(pages.owned_by_id = :user OR pages.access = 0)",
    ]
    .join(" AND ")
}

/// `is_favorite = EXISTS(...)` over `user_favorites` (`:82-87`, applied
/// `:102`): caller, `entity_type='page'`, bridge to this page
/// (`OuterRef("pk")`), workspace slug.
pub fn favorite_exists_sql() -> String {
    "EXISTS (SELECT 1 FROM user_favorites WHERE user_favorites.user_id = :user AND user_favorites.entity_type = 'page' AND user_favorites.entity_identifier = pages.id AND user_favorites.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug))".to_owned()
}

/// `project = EXISTS(...)` over `project_pages` (`:107-110`): bridge to
/// this page (`OuterRef("id")`) for the URL project.
pub fn project_exists_sql() -> String {
    "EXISTS (SELECT 1 FROM project_pages WHERE project_pages.page_id = pages.id AND project_pages.project_id = :project_id)".to_owned()
}

/// `project` filter (`:125`): only pages linked to the URL project.
pub fn project_filter_sql() -> String {
    format!("({}) = TRUE", project_exists_sql())
}

/// `label_ids = COALESCE(ARRAY_AGG(DISTINCT ...), '{}')` (`:112-119`):
/// distinct label ids, NULLs excluded.
pub fn label_ids_sql() -> String {
    "COALESCE((SELECT ARRAY_AGG(DISTINCT page_labels.label_id) FROM page_labels WHERE page_labels.page_id = pages.id AND page_labels.label_id IS NOT NULL), '{}')".to_owned()
}

/// `project_ids = COALESCE(ARRAY_AGG(DISTINCT ...), '{}')` (`:120-123`).
/// Ported bug 2: the Django filter is `~Q(projects__id=True)` — a UUID
/// column compared to boolean `TRUE`. The `NOT (projects.id = TRUE)`
/// predicate below preserves that shape verbatim; on Postgres it is a
/// type error-or-always-true no-op, and the `COALESCE` empty guard is
/// what actually renders `[]`.
pub fn project_ids_sql() -> String {
    "COALESCE((SELECT ARRAY_AGG(DISTINCT project_pages_2.project_id) FROM project_pages project_pages_2 WHERE project_pages_2.page_id = pages.id AND NOT (project_pages_2.project_id = TRUE)), '{}')".to_owned()
}

/// Dead request order (`:103`):
/// `.order_by(request.GET.get("order_by", "-created_at"))`. Ported bug 1:
/// the `:105` `order_by` below replaces this one in Django, so the
/// request's `?order_by=` value is silently discarded. Kept so the
/// closure reads whole; handlers must NOT apply it.
pub const DEAD_REQUEST_ORDER_SQL: &str = ":order_by /* :103, dead — replaced by LIST_ORDER_SQL */";

/// Effective list order (`:105`): `-is_favorite`, `-created_at`. This is
/// the order clients observe (ported bug 1).
pub const LIST_ORDER_SQL: &str = "is_favorite DESC, pages.created_at DESC";

/// Full representative `get_queryset` SELECT: R1 scope + `is_favorite` /
/// `project` / `label_ids` / `project_ids` annotations + `project` filter
/// + `DISTINCT` (`:126`) + effective order.
///
/// `filter_queryset()` (search `?search=` on `name` via `search_fields`,
/// `:79`) is applied by the handler on top; `prefetch_related` /
/// `select_related` (`:99-101`, `:104`) are fetch hints — same rows with
/// or without them.
pub fn get_queryset_sql() -> String {
    format!(
        "SELECT DISTINCT pages.*, {} AS is_favorite, {} AS project, {} AS label_ids, {} AS project_ids FROM pages WHERE {} AND {} ORDER BY {}",
        favorite_exists_sql(),
        project_exists_sql(),
        label_ids_sql(),
        project_ids_sql(),
        queryset_scope_where(),
        project_filter_sql(),
        LIST_ORDER_SQL,
    )
}

// ---------------------------------------------------------------------------
// summary (base.py:421-469)
// ---------------------------------------------------------------------------

/// Summary scope (`:422-438`): same membership/active/not-archived guards
/// as `get_queryset` but WITHOUT `parent__isnull` (children are counted),
/// WITH owner-or-public (`:430`), WITH the `project` Exists + filter
/// (`:431-437`), `.distinct()` (`:437`).
pub fn summary_scope_where() -> String {
    [
        "pages.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)",
        "EXISTS (SELECT 1 FROM project_pages pp_scope JOIN projects ON projects.id = pp_scope.project_id JOIN project_members pm ON pm.project_id = projects.id AND pm.member_id = :user AND pm.is_active = TRUE WHERE pp_scope.page_id = pages.id AND projects.archived_at IS NULL)",
        "(pages.owned_by_id = :user OR pages.access = 0)",
        project_filter_sql().as_str(),
    ]
    .join(" AND ")
}

/// Guest scoping predicate (`:451`): applied when the caller is an active
/// guest (`role = 5`, `:442-448`) on a project without
/// `guest_view_all_features` (`:449`) — guests then see only their own
/// pages, even public ones owned by someone else.
pub fn guest_owned_only_sql() -> &'static str {
    "pages.owned_by_id = :user"
}

/// Summary aggregates (`:453-467`): conditional counts over the scoped
/// set. Overlap behaviour, ported as observed: archived public/private
/// pages are ALSO counted in `archived_pages` (no mutual exclusion); a
/// non-archived page lands in exactly one of public/private by its
/// `access` value (`PUBLIC_ACCESS = 0`, `PRIVATE_ACCESS = 1`).
pub fn summary_selects_sql() -> String {
    "COUNT(CASE WHEN pages.access = 0 AND pages.archived_at IS NULL THEN 1 END) AS public_pages, COUNT(CASE WHEN pages.access = 1 AND pages.archived_at IS NULL THEN 1 END) AS private_pages, COUNT(CASE WHEN pages.archived_at IS NOT NULL THEN 1 END) AS archived_pages".to_owned()
}

/// Full representative `summary` SELECT. `guest_only = true` appends the
/// `:451` predicate.
pub fn summary_sql(guest_only: bool) -> String {
    let scope = if guest_only {
        format!("{} AND {}", summary_scope_where(), guest_owned_only_sql())
    } else {
        summary_scope_where()
    };
    format!(
        "SELECT {} FROM (SELECT DISTINCT pages.id, pages.access, pages.archived_at FROM pages WHERE {}) scoped",
        summary_selects_sql(),
        scope,
    )
}

/// `summary` response keys in source order (`:453-467`).
pub const SUMMARY_RESPONSE_KEYS: &[&str] = &["public_pages", "private_pages", "archived_pages"];

// ---------------------------------------------------------------------------
// Archive recursive CTE (base.py:59-72, used :335 and :364)
// ---------------------------------------------------------------------------

/// Verbatim CTE (`:61-68`), executed with params `[page_id, archived_at]`
/// (`:72`). `archived_at` is `datetime.now()` on archive (`:335`) and
/// `None` on unarchive (`:364`). Positional `%s` kept: this is raw SQL
/// passed straight to `cursor.execute`, not a Django queryset.
pub fn archive_cte_sql() -> String {
    "WITH RECURSIVE descendants AS (SELECT id FROM pages WHERE id = %s UNION ALL SELECT pages.id FROM pages, descendants WHERE pages.parent_id = descendants.id) UPDATE pages SET archived_at = %s WHERE id IN (SELECT id FROM descendants)".to_owned()
}

/// Number of `datetime.now()` calls on the archive path: the CTE call
/// (`:335`) and the response body (`:337`) each call `now()` separately
/// (ported bug 3) — the body timestamp may differ from the DB value by
/// microseconds.
pub const ARCHIVE_NOW_CALLS: u32 = 2;

/// Unarchive detach predicate (`:360-362`): `page.parent_id` non-null AND
/// the parent still archived — the page is detached (`parent = None`)
/// before the CTE clears its own subtree, so unarchiving a child of an
/// archived parent breaks the hierarchy (the root keeps its timestamp).
pub fn unarchive_detach_where() -> String {
    "pages.parent_id IS NOT NULL AND parent.archived_at IS NOT NULL".to_owned()
}

// ---------------------------------------------------------------------------
// PageLog issue_ids lookup (base.py:231-233)
// ---------------------------------------------------------------------------

/// `PageLog.objects.filter(page_id=page_id, entity_name="issue")`
/// `.values_list("entity_identifier", flat=True)` (`:231-233`): the flat
/// id array merged into the detail body as `issue_ids` (`:235`).
pub fn pagelog_issue_ids_sql() -> String {
    "SELECT entity_identifier FROM page_logs WHERE page_id = :page_id AND entity_name = 'issue'"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Duplicate re-fetch (base.py:628-637)
// ---------------------------------------------------------------------------

/// Re-fetch of the freshly duplicated page (`:628-637`):
/// `Page.objects.filter(pk=page.id)` with the `project_ids`
/// `Coalesce-ArrayAgg` annotation (same no-op filter shape as
/// [`project_ids_sql`], ported bug 2), `.first()`.
pub fn duplicate_refetch_sql() -> String {
    format!(
        "SELECT pages.*, {} AS project_ids FROM pages WHERE pages.id = :page_id LIMIT 1",
        project_ids_sql(),
    )
}

// ---------------------------------------------------------------------------
// Version list/detail lookups (version.py:19-31)
// ---------------------------------------------------------------------------

/// Collection branch (`:28`):
/// `PageVersion.objects.filter(workspace__slug=slug, page_id=page_id)`.
pub fn version_list_where() -> String {
    "page_versions.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug) AND page_versions.page_id = :page_id".to_owned()
}

/// Detail branch (`:23`):
/// `PageVersion.objects.get(workspace__slug=slug, page_id=page_id, pk=pk)`
/// (`DoesNotExist` bubbles to `handle_exception`).
pub fn version_detail_where() -> String {
    format!("{} AND page_versions.id = :pk", version_list_where())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_GET_QUERYSET_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/queries/get_queryset.rows.json"
    );
    const FIXTURE_SUMMARY_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/queries/summary.rows.json"
    );
    const FIXTURE_ARCHIVE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/queries/archive_cte.rows.json"
    );
    const FIXTURE_VERSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_pages/handlers/versions.golden.json"
    );

    fn rows_fixture(path: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(path).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    #[test]
    fn get_queryset_scope_carries_guards_in_source_order() {
        let scope = queryset_scope_where();
        // R1 (:91-98): slug, member/active/live-project, top-level,
        // owner-or-public — in that order.
        let slug = scope.find("slug = :slug").expect("slug guard");
        let member = scope.find("pm.member_id = :user").expect("member guard");
        let parent = scope
            .find("pages.parent_id IS NULL")
            .expect("top-level guard");
        let owner = scope
            .find("(pages.owned_by_id = :user OR pages.access = 0)")
            .expect("owner-or-public");
        assert!(slug < member && member < parent && parent < owner);
        assert!(scope.contains("pm.is_active = TRUE"));
        assert!(scope.contains("projects.archived_at IS NULL"));
    }

    #[test]
    fn get_queryset_annotations_match_fixture_shape() {
        let fav = favorite_exists_sql();
        for needle in [
            "user_favorites.user_id = :user",
            "user_favorites.entity_type = 'page'",
            "user_favorites.entity_identifier = pages.id",
            "slug = :slug",
        ] {
            assert!(fav.contains(needle), "missing {needle}");
        }
        let project = project_exists_sql();
        assert!(project.contains("project_pages.page_id = pages.id"));
        assert!(project.contains("project_pages.project_id = :project_id"));
        assert!(project_filter_sql().ends_with("= TRUE"));
        let labels = label_ids_sql();
        assert!(labels.contains("ARRAY_AGG(DISTINCT page_labels.label_id)"));
        assert!(labels.contains("page_labels.label_id IS NOT NULL"));
        assert!(labels.ends_with(", '{}')"));
        // Ported bug 2: UUID-vs-TRUE no-op kept verbatim.
        let projects = project_ids_sql();
        assert!(projects.contains("ARRAY_AGG(DISTINCT project_pages_2.project_id)"));
        assert!(projects.contains("NOT (project_pages_2.project_id = TRUE)"));
        assert!(projects.ends_with(", '{}')"));
    }

    #[test]
    fn double_order_by_dead_then_effective() {
        // Ported bug 1: the request order (:103) is dead; the effective
        // order (:105) is always (-is_favorite, -created_at).
        assert_eq!(LIST_ORDER_SQL, "is_favorite DESC, pages.created_at DESC");
        let sql = get_queryset_sql();
        assert!(sql.starts_with("SELECT DISTINCT pages.*"));
        assert!(sql.contains("AS is_favorite"));
        assert!(sql.contains("AS project,"));
        assert!(sql.contains("AS label_ids"));
        assert!(sql.contains("AS project_ids"));
        assert!(sql.contains(&project_filter_sql()));
        assert!(sql.ends_with(&format!("ORDER BY {LIST_ORDER_SQL}")));
        assert!(!sql.contains(":order_by"));
    }

    #[test]
    fn get_queryset_rows_match_fixture() {
        // Same annotated row shape and guard behaviour as F30-06.
        let fixture = rows_fixture(FIXTURE_GET_QUERYSET_ROWS);
        assert_eq!(fixture["source"], "app/views/page/base.py:81-127");
        let rows = fixture["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 2);
        for row in rows {
            for key in [
                "access",
                "archived_at",
                "is_favorite",
                "label_ids",
                "name",
                "owned_by",
                "parent",
                "project",
                "project_ids",
            ] {
                assert!(row.get(key).is_some(), "row missing {key}");
            }
            assert_eq!(row["project"], true);
            assert!(row["parent"].is_null());
        }
        assert_eq!(rows[0]["is_favorite"], true);
        assert_eq!(rows[1]["access"], 1);
        // Owner-or-public (:98): the private row is visible because it is
        // owned by the requester.
        assert!(fixture["rows_excluded"]
            .as_array()
            .expect("excluded array")
            .iter()
            .any(|r| r["why"] == "Q(owned_by=user) | Q(access=0)"));
        // Both ported bugs are recorded on the fixture.
        let bugs = fixture["bugs"].as_array().expect("bugs array");
        assert!(bugs
            .iter()
            .any(|b| b.as_str().unwrap_or("").contains("double order_by")));
        assert!(bugs
            .iter()
            .any(|b| b.as_str().unwrap_or("").contains("projects__id=True")));
    }

    #[test]
    fn summary_aggregates_and_overlap_match_fixture() {
        let selects = summary_selects_sql();
        assert!(selects.contains(
            "pages.access = 0 AND pages.archived_at IS NULL THEN 1 END) AS public_pages"
        ));
        assert!(selects.contains(
            "pages.access = 1 AND pages.archived_at IS NULL THEN 1 END) AS private_pages"
        ));
        assert!(selects.contains("pages.archived_at IS NOT NULL THEN 1 END) AS archived_pages"));
        assert_eq!(
            SUMMARY_RESPONSE_KEYS,
            &["public_pages", "private_pages", "archived_pages"]
        );
        // Summary scope has no parent__isnull (children counted) but keeps
        // owner-or-public and the project filter.
        let scope = summary_scope_where();
        assert!(!scope.contains("parent_id IS NULL"));
        assert!(scope.contains("(pages.owned_by_id = :user OR pages.access = 0)"));
        assert!(scope.contains(&project_filter_sql()));
        // Guest scoping (:451) appended only for guests without view-all.
        // (The bare predicate text also occurs inside the owner-or-public
        // clause, so the negative check targets the appended `AND ...`
        // form.)
        assert_eq!(guest_owned_only_sql(), "pages.owned_by_id = :user");
        assert!(!summary_sql(false).contains("AND pages.owned_by_id = :user"));
        assert!(summary_sql(true).contains(&format!("AND {}", guest_owned_only_sql())));
        assert!(summary_sql(false)
            .contains("SELECT DISTINCT pages.id, pages.access, pages.archived_at FROM pages"));
    }

    #[test]
    fn summary_rows_match_fixture() {
        let fixture = rows_fixture(FIXTURE_SUMMARY_ROWS);
        let cases = fixture["cases"].as_array().expect("cases array");
        let member = cases
            .iter()
            .find(|c| c["name"] == "member, mixed set")
            .expect("member case");
        assert_eq!(member["counts"]["public_pages"], 2);
        assert_eq!(member["counts"]["private_pages"], 1);
        assert_eq!(member["counts"]["archived_pages"], 2);
        let guest = cases
            .iter()
            .find(|c| c["name"] == "guest without view-all sees only own")
            .expect("guest case");
        assert_eq!(guest["counts"]["public_pages"], 0);
        assert!(guest["guest_scoping"]
            .as_str()
            .unwrap_or("")
            .contains(":451"));
        assert_eq!(fixture["response_status"], 200);
    }

    #[test]
    fn archive_cte_is_verbatim_and_hierarchy_matches_fixture() {
        let sql = archive_cte_sql();
        // Verbatim shape of base.py:61-68, positional params [page_id,
        // archived_at] (:72).
        assert!(sql.starts_with(
            "WITH RECURSIVE descendants AS (SELECT id FROM pages WHERE id = %s UNION ALL"
        ));
        assert!(sql.contains("pages.parent_id = descendants.id"));
        assert!(sql.ends_with(
            "UPDATE pages SET archived_at = %s WHERE id IN (SELECT id FROM descendants)"
        ));
        assert_eq!(sql.matches("%s").count(), 2);
        // Ported bug 3: two now() calls on archive.
        assert_eq!(ARCHIVE_NOW_CALLS, 2);
        // Detach-when-parent-archived (:360-362).
        let detach = unarchive_detach_where();
        assert!(detach.contains("pages.parent_id IS NOT NULL"));
        assert!(detach.contains("parent.archived_at IS NOT NULL"));
        let fixture = rows_fixture(FIXTURE_ARCHIVE_ROWS);
        assert_eq!(fixture["before"].as_array().expect("before").len(), 3);
        let after = fixture["after_archive_root"]
            .as_array()
            .expect("after archive");
        assert!(after.iter().all(|r| r["archived_at"] == "<now1>"));
        let unarch = fixture["after_unarchive_child_with_archived_parent"]
            .as_array()
            .expect("after unarchive");
        let child = unarch.iter().find(|r| r["id"] == "C").expect("child row");
        assert!(child["archived_at"].is_null());
        assert!(child["parent_id"].is_null());
        assert!(
            unarch.iter().find(|r| r["id"] == "R").expect("root row")["archived_at"] == "<now1>"
        );
    }

    #[test]
    fn pagelog_duplicate_and_version_lookups_match_sources() {
        let pagelog = pagelog_issue_ids_sql();
        assert!(pagelog.contains("FROM page_logs"));
        assert!(pagelog.contains("page_id = :page_id"));
        assert!(pagelog.contains(&format!("entity_name = '{PAGE_LOG_ISSUE_ENTITY}'")));
        // Duplicate re-fetch (:628-637): pk filter + project_ids
        // annotation with the ported no-op shape, single row.
        let dup = duplicate_refetch_sql();
        assert!(dup.contains("pages.id = :page_id"));
        assert!(dup.contains(&project_ids_sql()));
        assert!(dup.ends_with("LIMIT 1"));
        // Versions (version.py:23,28): workspace slug + page id, plus pk
        // on the detail branch.
        assert!(version_list_where().contains("slug = :slug"));
        assert!(version_list_where().contains("page_versions.page_id = :page_id"));
        assert!(version_detail_where().contains(&version_list_where()));
        assert!(version_detail_where().contains("page_versions.id = :pk"));
        // F30-12 branch names line up with the two builders.
        let fixture = rows_fixture(FIXTURE_VERSIONS);
        let branches = fixture["branches"].as_array().expect("branches");
        assert!(branches.iter().any(|b| b["source"]
            .as_str()
            .unwrap_or("")
            .contains("version.py:21-26")));
        assert!(branches.iter().any(|b| b["source"]
            .as_str()
            .unwrap_or("")
            .contains("version.py:28-31")));
    }

    #[test]
    fn access_and_guest_consts_match_models() {
        assert_eq!(PUBLIC_ACCESS, 0);
        assert_eq!(PRIVATE_ACCESS, 1);
        assert_eq!(GUEST_ROLE, 5);
        assert_eq!(FAVORITE_ENTITY_TYPE, "page");
        assert_eq!(PAGE_LOG_ISSUE_ENTITY, "issue");
    }
}
