#![forbid(unsafe_code)]

//! Cycle queryset builders (D-27, stage 5).
//!
//! Ports the annotated querysets behind the cycle endpoints to SQL text,
//! following the pilot-2 precedent (`app_issues/ordering.rs`): each builder
//! returns a fragment the caller splices into the statement it executes.
//! The services crate carries no `sea-query` dependency, so placeholders
//! stay symbolic — `:slug`, `:project_id`, `:user`, `:now`, `:cycle_id` —
//! exactly the notation the fixtures use; handlers bind them.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/cycle/base.py:69-182` — `CycleViewSet.get_queryset`
//!   (favorite `Exists` :70-76, group `Count`s :114-151, `status` `Case`
//!   :153-166, `assignee_ids` `ArrayAgg` :168-178, `-is_favorite,name`
//!   ordering :179) + `list()` override :183-268 + retrieve :410-459.
//! - `app/views/cycle/archive.py:41-270` — archived `get_queryset` (archived
//!   filter :117, six group counts :137-207, six `Cast`-`FloatField`
//!   estimate subqueries :49-113 -> :234-266).
//! - `app/views/cycle/issue.py:51-108` — cycle-issue `get_queryset` +
//!   `apply_annotations` (`sub_issues_count` / `cycle_id` / `link_count`
//!   subqueries; `ComplexFilterBackend` + `IssueFilterSet` parity).
//! - `app/views/cycle/base.py:562-570` — favorites `get_queryset`
//!   (workspace-slug + user filter, `select_related` cycle/owned_by).
//! - `utils/cycle_transfer_issues.py:36-479` — `transfer_cycle_issues`
//!   query sequence + `progress_snapshot` write (synchronous — no Celery
//!   task exists in this domain).
//!
//! Fixture oracles: F-C27-03 (`queries_base.sql` + `.rows.json`), F-C27-04
//! (`queries_archive.sql` + `.rows.json`), F-C27-05 (`queries_issue.sql` +
//! `.rows.json`), F-C27-06 (`transfer.json`), F-C27-10 (`misc.json`,
//! favorites queryset rows feed the default list on the
//! user-favorite-cycles GET path). The unit tests below pin the builders
//! against those files so transcription drift fails the build.
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. Base `cancelled_issues` filters `state__group__in=["cancelled"]` —
//!    a single-element `IN`, semantically `=`; the `IN` shape is kept
//!    (`GroupFilter::InSingle`). Archive uses plain `=` — both shapes
//!    are ported, not unified.
//! 2. `list()` re-orders to (`-is_favorite`, `-created_at`) (`:189`), so
//!    the `get_queryset` (`-is_favorite`, `name`) ordering is dead on
//!    list — both orders are ported (`LIST_ORDER_SQL` is the effective
//!    one). Same pattern on the archived list (`archive.py:303`).
//! 3. `cycle_view=current` with zero current cycles returns ALL cycles
//!    (`if data:` on an empty `ValuesQuerySet` is `False`, `:236-237`).
//! 4. The project-timezone round-trip (`:78-88`) is semantically
//!    `timezone.now()` — the value is ported (`:now`), not the detour.
//! 5. Archive counts and `assignee_ids` omit the bridge
//!    (`issue_cycle__deleted_at IS NULL`) guard the base queryset has —
//!    the asymmetry is ported via `bridge_deleted_guard: false`.
//! 6. Transfer to an unknown `new_cycle_id` dereferences `None.end_date`
//!    (`AttributeError` -> 500, `:59-62`) — ported, never guarded.
//!
//! Out of scope (owned by sibling handler issues): the `list()`/`retrieve`
//! envelopes, `create`/`destroy`, date-check, favorite create/destroy,
//! transfer endpoint guard (`new_cycle_id` required), progress/analytics.
//! Their queryset-adjacent constants that ARE in scope here are marked.

// ---------------------------------------------------------------------------
// Shared scope
// ---------------------------------------------------------------------------

/// Membership + tenancy scope shared by every cycle queryset
/// (`base.py:93-99`, `archive.py:114-123`, `issue.py:61-67`):
/// workspace slug, project id, active project membership of the caller,
/// live project. Handlers add the endpoint's own predicates on top.
pub fn tenant_scope_where() -> String {
    [
        "cycles.workspace_slug = :slug",
        "cycles.project_id = :project_id",
        "EXISTS (SELECT 1 FROM project_projectmembers pm WHERE pm.project_id = cycles.project_id AND pm.member_id = :user AND pm.is_active = TRUE)",
        "projects.archived_at IS NULL",
    ]
    .join(" AND ")
}

/// Archived predicate: the list path filters `archived_at IS NULL`
/// (`base.py:185`); the archive path filters `archived_at IS NOT NULL`
/// (`archive.py:117`).
pub fn archived_predicate(archived_only: bool) -> &'static str {
    if archived_only {
        "cycles.archived_at IS NOT NULL"
    } else {
        "cycles.archived_at IS NULL"
    }
}

/// `is_favorite = EXISTS(...)` over `user_favorites`
/// (`base.py:70-76`, `archive.py:42-48`): caller, `entity_type='cycle'`,
/// bridge to this cycle, project, workspace slug.
pub fn favorite_exists_sql() -> String {
    "EXISTS (SELECT 1 FROM user_favorites uf WHERE uf.user_id = :user AND uf.entity_identifier = cycles.id AND uf.entity_type = 'cycle' AND uf.project_id = :project_id AND uf.workspace_slug = :slug)".to_owned()
}

// ---------------------------------------------------------------------------
// Group counts
// ---------------------------------------------------------------------------

/// State-group restriction of a count annotation. `InSingle` keeps the
/// ported single-element-`IN` bug (base `cancelled_issues`, `:139-151`);
/// `Eq` is the plain equality every other count uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupFilter {
    /// No group restriction (`total_issues`, total estimate subquery).
    All,
    /// `state_group = '<group>'`.
    Eq(&'static str),
    /// `state_group IN ('<group>')` — ported single-element-`IN` shape.
    InSingle(&'static str),
}

impl GroupFilter {
    fn predicate(&self) -> String {
        match self {
            GroupFilter::All => String::new(),
            GroupFilter::Eq(group) => format!("AND issue.state_group = '{group}'"),
            GroupFilter::InSingle(group) => format!("AND issue.state_group IN ('{group}')"),
        }
    }
}

/// `COUNT(DISTINCT issue.id)` with the archived / draft / soft-delete
/// guards (`base.py:114-151`, `archive.py:137-207`).
///
/// `bridge_deleted_guard = true` adds `cycle_issues.deleted_at IS NULL`
/// (base queryset); `false` omits it (archive queryset AND the transfer
/// old-cycle counts, which never had it — ported asymmetry, bug 5).
pub fn count_issues_sql(filter: GroupFilter, bridge_deleted_guard: bool) -> String {
    let bridge = if bridge_deleted_guard {
        " AND cycle_issues.deleted_at IS NULL"
    } else {
        ""
    };
    format!(
        "COUNT(DISTINCT issue.id) FILTER (WHERE issue.archived_at IS NULL AND issue.is_draft = FALSE{bridge} AND issue.deleted_at IS NULL {})",
        filter.predicate()
    )
}

/// `status` `CASE` over `:now` (`base.py:153-166`, `archive.py:209-223`).
/// Open-ended cycles fall to `DRAFT` (NULL comparisons are never true);
/// the `default` is `DRAFT` (`:164`). `:now` is `timezone.now()` — the
/// project-tz round-trip in base (`:78-88`) is semantically identical
/// (ported bug 4); archive passes `timezone.now()` directly.
pub fn status_case_sql() -> String {
    "CASE WHEN cycles.start_date <= :now AND cycles.end_date >= :now THEN 'CURRENT' WHEN cycles.start_date > :now THEN 'UPCOMING' WHEN cycles.end_date < :now THEN 'COMPLETED' WHEN cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT' ELSE 'DRAFT' END".to_owned()
}

/// `assignee_ids = COALESCE(ARRAY_AGG(DISTINCT ...), '{}')`
/// (`base.py:168-178`, `archive.py:224-233`). The base path also requires
/// the assignee bridge to be live; archive does not (ported asymmetry,
/// bug 5).
pub fn assignee_ids_sql(bridge_deleted_guard: bool) -> String {
    let bridge = if bridge_deleted_guard {
        " AND issue_assignee.deleted_at IS NULL"
    } else {
        ""
    };
    format!(
        "COALESCE(ARRAY_AGG(DISTINCT issue_assignees.user_id) FILTER (WHERE issue_assignees.user_id IS NOT NULL{bridge}), '{{}}')"
    )
}

// ---------------------------------------------------------------------------
// Ordering + list projection
// ---------------------------------------------------------------------------

/// `get_queryset` tail order (`base.py:179`, `archive.py:267`):
/// `-is_favorite`, `name`. Dead on list — see [`LIST_ORDER_SQL`].
pub const GET_QUERYSET_ORDER_SQL: &str = "is_favorite DESC, cycles.name ASC";

/// Effective list order (`base.py:189`, archived list `archive.py:303`):
/// `-is_favorite`, `-created_at`. This is the order clients observe.
pub const LIST_ORDER_SQL: &str = "is_favorite DESC, cycles.created_at DESC";

/// `cycle_view=current` predicate (`base.py:205`):
/// `start_date <= :now AND end_date >= :now`. When it matches zero rows
/// the view returns ALL cycles instead of `[]` (`if data:`, `:236-237` —
/// ported bug 3); handlers must reproduce that fallthrough.
pub fn current_view_predicate() -> String {
    "cycles.start_date <= :now AND cycles.end_date >= :now".to_owned()
}

/// `list()` `.values()` keys (`base.py:207-232, :239-265`, F-C27-03
/// `list_projection`): annotated counts and `assignee_ids` are projected
/// but started/unstarted/backlog counts are NOT (they render null/absent
/// on list); `version` + `created_by` ARE projected though not serializer
/// fields.
pub const BASE_LIST_VALUES_FIELDS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "start_date",
    "end_date",
    "owned_by_id",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "logo_props",
    "is_favorite",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "assignee_ids",
    "status",
    "version",
    "created_by",
];

/// Retrieve 404 body (`base.py:458-459`).
pub const CYCLE_NOT_FOUND_BODY: &str = "{\"error\":\"Cycle not found\"}";

// ---------------------------------------------------------------------------
// Archive estimates
// ---------------------------------------------------------------------------

/// Per-group estimate subquery (`archive.py:49-113`): correlated over the
/// cycle, restricted to `estimates.type = 'points'`, `SUM` of
/// `CAST(estimate_point.value AS FLOAT)`; `group = None` is the total
/// (no group filter). No archived / draft guards — ported as observed.
/// Handlers wrap it: `COALESCE((<this>), 0.0) AS <group>_estimate_points`.
pub fn estimate_subquery_sql(group: Option<&str>) -> String {
    let group_predicate = match group {
        Some(group) => format!(" AND ie.state_group = '{group}'"),
        None => String::new(),
    };
    format!(
        "SELECT SUM(CAST(estimate_point.value AS FLOAT)) FROM issues ie JOIN estimates e ON e.id = ie.estimate_point_id JOIN cycle_issues ci2 ON ci2.issue_id = ie.id AND ci2.deleted_at IS NULL WHERE ci2.cycle_id = cycles.id AND e.type = 'points'{group_predicate}"
    )
}

/// The six estimate annotation aliases in source order
/// (`archive.py:234-266`).
pub const ESTIMATE_POINT_ALIASES: &[&str] = &[
    "backlog_estimate_points",
    "unstarted_estimate_points",
    "started_estimate_points",
    "cancelled_estimate_points",
    "completed_estimate_points",
    "total_estimate_points",
];

/// `COALESCE((<subquery>), 0.0) AS <alias>` wrapper
/// (`archive.py:234-266`, `Value(0, FloatField())` default).
pub fn estimate_coalesce_sql(group: Option<&str>, alias: &str) -> String {
    format!(
        "COALESCE(({}), 0.0) AS {alias}",
        estimate_subquery_sql(group)
    )
}

/// The six group keys feeding the estimate subqueries, in source order
/// (`archive.py:49-113`); `None` is the total (no group filter).
pub const ESTIMATE_GROUPS: &[Option<&str>] = &[
    Some("backlog"),
    Some("unstarted"),
    Some("started"),
    Some("cancelled"),
    Some("completed"),
    None,
];

// ---------------------------------------------------------------------------
// Cycle-issue queryset + annotations
// ---------------------------------------------------------------------------

/// Declared `filterset_fields` (`issue.py:49`): the only `IssueFilterSet`
/// leaves the cycle-issue endpoint exposes next to `ComplexFilterBackend`
/// (`:43-44`). Leaf resolution itself is the shared F-04
/// `pidash_db::filterset` kernel (pilot-2 `app_issues` pattern) — this
/// const is the parity surface.
pub const CYCLE_ISSUE_FILTERSET_FIELDS: &[&str] = &["issue__labels__id", "issue__assignees__id"];

/// Cycle-issue scope: tenant scope plus the cycle (`issue.py:61-68`).
/// `select_related` project/workspace/cycle/issue/issue__state/
/// issue__project (`:69-72`) and `prefetch` issue__assignees/labels
/// (`:73`) are fetch hints — same rows with or without them.
pub fn cycle_issue_scope_where() -> String {
    format!(
        "{} AND cycle_issues.cycle_id = :cycle_id",
        tenant_scope_where()
    )
}

/// `sub_issues_count` on the `CycleIssue` queryset (`issue.py:55-60`):
/// children of the bridge's issue. Rendered via
/// `.annotate(count=Count(id)).values("count")` — NULL when zero
/// children, not zero.
pub fn cycle_issue_sub_issues_count_sql() -> String {
    "SELECT COUNT(*) FROM issues child WHERE child.parent_id = cycle_issues.issue_id".to_owned()
}

/// `cycle_id` in `apply_annotations` (`issue.py:80-82`): the live bridge
/// for the issue, applied to the ISSUE queryset in `list()`, not to the
/// `CycleIssue` queryset above.
pub fn annotated_cycle_id_sql() -> String {
    "SELECT cycle_id FROM cycle_issues WHERE issue_id = issues.id AND deleted_at IS NULL LIMIT 1"
        .to_owned()
}

/// `link_count` in `apply_annotations` (`issue.py:84-89`).
pub fn link_count_sql() -> String {
    "SELECT COUNT(*) FROM issue_links WHERE issue_id = issues.id".to_owned()
}

/// `FileAsset.EntityTypeContext.ISSUE_ATTACHMENT` value
/// (`db/models/asset.py:34`) used by the `attachment_count` annotation
/// (`issue.py:90-98`).
pub const ATTACHMENT_ENTITY_TYPE: &str = "ISSUE_ATTACHMENT";

/// `attachment_count` in `apply_annotations` (`issue.py:90-98`).
pub fn attachment_count_sql() -> String {
    format!(
        "SELECT COUNT(*) FROM file_assets WHERE issue_id = issues.id AND entity_type = '{ATTACHMENT_ENTITY_TYPE}'"
    )
}

/// `sub_issues_count` in `apply_annotations` (`issue.py:99-104`):
/// children of the issue itself (vs [`cycle_issue_sub_issues_count_sql`],
/// children of the bridge's issue — same shape, different outer row).
pub fn issue_sub_issues_count_sql() -> String {
    "SELECT COUNT(*) FROM issues WHERE parent_id = issues.id".to_owned()
}

/// Group/sub-group clash error (`issue.py:146-150`).
pub const GROUP_CLASH_BODY: &str =
    "{\"error\":\"Group by and sub group by cannot have same parameters\"}";

// ---------------------------------------------------------------------------
// Favorites queryset
// ---------------------------------------------------------------------------

/// `CycleFavoriteViewSet.get_queryset` (`base.py:562-570`): favorites in
/// this workspace belonging to the caller; `select_related("cycle",
/// "cycle__owned_by")` is a fetch hint. F-C27-10 favorites rows feed the
/// default list on the user-favorite-cycles GET path.
pub fn favorites_scope_where() -> String {
    "user_favorites.workspace_slug = :slug AND user_favorites.user_id = :user".to_owned()
}

// ---------------------------------------------------------------------------
// Transfer query sequence
// ---------------------------------------------------------------------------

/// Movable groups: `OPEN_STATE_GROUPS = STATE_GROUP_ORDER[:-2]`
/// (`utils/constants.py:77-86`): backlog, unstarted, started, review,
/// test. Completed/cancelled bridges stay (`:435-459`).
pub const OPEN_STATE_GROUPS: &[&str] = &["backlog", "unstarted", "started", "review", "test"];

/// Completed-destination refusal (`cycle_transfer_issues.py:59-66`,
/// surfaced as HTTP 400 by `base.py:615-620`): fires when
/// `new_cycle.end_date is not None and new_cycle.end_date < now`.
pub const DESTINATION_COMPLETED_ERROR: &str =
    "The cycle where the issues are transferred is already completed";

/// Missing source cycle (`cycle_transfer_issues.py:145-149`).
pub const SOURCE_CYCLE_MISSING_ERROR: &str = "Source cycle not found";

/// Missing `new_cycle_id` on the endpoint (`base.py:599-603`, HTTP 400).
/// Endpoint-owned, recorded here so the transfer contract reads whole.
pub const NEW_CYCLE_ID_REQUIRED_BODY: &str = "{\"error\":\"New Cycle Id is required\"}";

/// Transfer success body (`base.py:594-622`).
pub const TRANSFER_SUCCESS_BODY: &str = "{\"message\":\"Success\"}";

/// Old-cycle count annotation (`cycle_transfer_issues.py:72-143`):
/// non-distinct `COUNT` over the bridge (`Count("issue_cycle")`) with the
/// archived / draft / bridge-deleted / issue-deleted guards.
/// `group = None` is `total_issues`; otherwise `state__group = '<group>'`
/// (plain equality everywhere here — no single-element-`IN`).
pub fn old_cycle_count_sql(group: Option<&str>) -> String {
    let group_predicate = match group {
        Some(group) => format!(" AND issue.state_group = '{group}'"),
        None => String::new(),
    };
    format!(
        "COUNT(cycle_issues.id) FILTER (WHERE issue.archived_at IS NULL AND issue.is_draft = FALSE AND cycle_issues.deleted_at IS NULL AND issue.deleted_at IS NULL{group_predicate})"
    )
}

/// Move filter: open-group bridges of this cycle
/// (`cycle_transfer_issues.py:436-443`). Completed, cancelled, draft and
/// archived issues are untouched.
pub fn transfer_move_where() -> String {
    format!(
        "cycle_issues.cycle_id = :cycle_id AND cycle_issues.project_id = :project_id AND cycle_issues.workspace_slug = :slug AND issue.archived_at IS NULL AND issue.is_draft = FALSE AND issue.state_group IN ({})",
        OPEN_STATE_GROUPS
            .iter()
            .map(|group| format!("'{group}'"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `bulk_update(updated_cycles, ["cycle_id"], batch_size=100)` (`:459`).
pub const TRANSFER_BULK_UPDATE_FIELDS: &[&str] = &["cycle_id"];
/// Batch size of the transfer `bulk_update` (`:459`).
pub const TRANSFER_BULK_BATCH_SIZE: i32 = 100;

/// `save(update_fields=["progress_snapshot"])` (`:433`).
pub const SNAPSHOT_UPDATE_FIELDS: &[&str] = &["progress_snapshot"];

/// Snapshot top-level keys in write order (`:411-431`).
pub const SNAPSHOT_KEYS: &[&str] = &[
    "total_issues",
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "distribution",
    "estimate_distribution",
];

/// Snapshot `distribution` keys in write order (`:423-427`).
pub const SNAPSHOT_DISTRIBUTION_KEYS: &[&str] = &["labels", "assignees", "completion_chart"];

/// Assignee/label distribution row keys in serialization order
/// (`:219-229` assignee estimates, `:318-329` issue counts — same key
/// order family: name, id, avatar/color, total, completed, pending).
pub const ASSIGNEE_ROW_KEYS: &[&str] = &[
    "display_name",
    "assignee_id",
    "avatar_url",
    "total_issues",
    "completed_issues",
    "pending_issues",
];
/// Estimate-flavoured assignee row keys (`:214-222`).
pub const ASSIGNEE_ESTIMATE_ROW_KEYS: &[&str] = &[
    "display_name",
    "assignee_id",
    "avatar_url",
    "total_estimates",
    "completed_estimates",
    "pending_estimates",
];
/// Label distribution row keys (`:354-363`).
pub const LABEL_ROW_KEYS: &[&str] = &[
    "label_name",
    "color",
    "label_id",
    "total_issues",
    "completed_issues",
    "pending_issues",
];
/// Estimate-flavoured label row keys (`:269-277`).
pub const LABEL_ESTIMATE_ROW_KEYS: &[&str] = &[
    "label_name",
    "color",
    "label_id",
    "total_estimates",
    "completed_estimates",
    "pending_estimates",
];

/// `avatar_url` `Case` (`:176-196`, repeated `:300-315`): the asset URL
/// when `avatar_asset` is set, else the raw `avatar` field, else NULL.
pub fn avatar_url_case_sql() -> String {
    "CASE WHEN assignees.avatar_asset IS NOT NULL THEN CONCAT('/api/assets/v2/static/', assignees.avatar_asset, '/') WHEN assignees.avatar_asset IS NULL THEN assignees.avatar ELSE NULL END".to_owned()
}

/// Estimate-distribution sums (`:203-217`, `:246-260`): total plus
/// completed (`completed_at IS NOT NULL`) / pending
/// (`completed_at IS NULL`) splits, each over live non-draft issues.
pub fn estimate_sum_sql(completed: Option<bool>) -> String {
    let split = match completed {
        Some(true) => " AND issues.completed_at IS NOT NULL",
        Some(false) => " AND issues.completed_at IS NULL",
        None => "",
    };
    format!(
        "SUM(CAST(estimate_point.value AS FLOAT)) FILTER (WHERE issues.archived_at IS NULL AND issues.is_draft = FALSE{split})"
    )
}

/// Issue-count distribution (`:318-329`, `:344-355`): same splits as
/// [`estimate_sum_sql`] but `COUNT(id)`.
pub fn distribution_count_sql(completed: Option<bool>) -> String {
    let split = match completed {
        Some(true) => " AND issues.completed_at IS NOT NULL",
        Some(false) => " AND issues.completed_at IS NULL",
        None => "",
    };
    format!("COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.is_draft = FALSE{split})")
}

/// Transfer scope shared by the four distribution queries
/// (`:164-167`, `:238-244`, `:288-294`, `:337-343`): this cycle's live
/// bridges, tenant-scoped.
pub fn transfer_distribution_scope_where() -> String {
    "issues.issue_cycle_cycle_id = :cycle_id AND issues.issue_cycle_deleted_at IS NULL AND issues.workspace_slug = :slug AND issues.project_id = :project_id".to_owned()
}

/// Activity recorded after the move (`:462-477`):
/// `type="cycle.activity.created"`, `requested_data` is ALWAYS
/// `{"cycles_list": []}` on this path, `current_instance` carries
/// `updated_cycle_issues: [{old_cycle_id, new_cycle_id, issue_id}]` and
/// an empty `created_cycle_issues`.
pub const TRANSFER_ACTIVITY_TYPE: &str = "cycle.activity.created";
/// `requested_data` on the transfer path — always empty
/// (`json.dumps({"cycles_list": []})`, `:466`).
pub const TRANSFER_ACTIVITY_REQUESTED_DATA: &str = "{\"cycles_list\": []}";
/// `current_instance` keys in write order (`:468-473`).
pub const TRANSFER_ACTIVITY_INSTANCE_KEYS: &[&str] =
    &["updated_cycle_issues", "created_cycle_issues"];
/// Per-moved-bridge activity row keys in write order (`:448-454`).
pub const TRANSFER_ACTIVITY_ROW_KEYS: &[&str] = &["old_cycle_id", "new_cycle_id", "issue_id"];

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_BASE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_cycles/queries_base.rows.json"
    );
    const FIXTURE_ARCHIVE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_cycles/queries_archive.rows.json"
    );
    const FIXTURE_ISSUE_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_cycles/queries_issue.rows.json"
    );
    const FIXTURE_TRANSFER: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_cycles/transfer.json"
    );

    fn rows_fixture(path: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(path).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    #[test]
    fn tenant_scope_carries_membership_and_live_project() {
        let scope = tenant_scope_where();
        assert!(scope.contains("cycles.workspace_slug = :slug"));
        assert!(scope.contains("cycles.project_id = :project_id"));
        assert!(scope.contains("pm.member_id = :user"));
        assert!(scope.contains("pm.is_active = TRUE"));
        assert!(scope.contains("projects.archived_at IS NULL"));
        assert!(scope.contains("project_projectmembers"));
    }

    #[test]
    fn archived_predicate_flips_per_endpoint() {
        assert_eq!(archived_predicate(false), "cycles.archived_at IS NULL");
        assert_eq!(archived_predicate(true), "cycles.archived_at IS NOT NULL");
    }

    #[test]
    fn favorite_exists_matches_django_lookup() {
        let sql = favorite_exists_sql();
        assert!(sql.starts_with("EXISTS (SELECT 1 FROM user_favorites"));
        for needle in [
            "uf.user_id = :user",
            "uf.entity_identifier = cycles.id",
            "uf.entity_type = 'cycle'",
            "uf.project_id = :project_id",
            "uf.workspace_slug = :slug",
        ] {
            assert!(sql.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn base_counts_keep_guards_and_cancelled_in_shape() {
        let total = count_issues_sql(GroupFilter::All, true);
        assert!(total.contains("COUNT(DISTINCT issue.id)"));
        assert!(total.contains("cycle_issues.deleted_at IS NULL"));
        assert!(!total.contains("state_group"));
        let completed = count_issues_sql(GroupFilter::Eq("completed"), true);
        assert!(completed.contains("issue.state_group = 'completed'"));
        assert!(!completed.contains("IN ("));
        // Ported bug 1: single-element IN, semantically `= 'cancelled'`.
        let cancelled = count_issues_sql(GroupFilter::InSingle("cancelled"), true);
        assert!(cancelled.contains("issue.state_group IN ('cancelled')"));
    }

    #[test]
    fn archive_counts_drop_bridge_guard() {
        // Ported bug 5: the archive queryset omits the bridge deleted_at
        // guard the base queryset has.
        for group in ["completed", "cancelled", "started", "unstarted", "backlog"] {
            let sql = count_issues_sql(GroupFilter::Eq(group), false);
            assert!(!sql.contains("cycle_issues.deleted_at IS NULL"), "{group}");
            assert!(sql.contains(&format!("issue.state_group = '{group}'")));
        }
        // Archive cancelled is plain `=`, not the base `IN` shape.
        let cancelled = count_issues_sql(GroupFilter::Eq("cancelled"), false);
        assert!(!cancelled.contains("IN ("));
    }

    #[test]
    fn status_case_covers_all_four_branches() {
        let sql = status_case_sql();
        assert!(sql.contains("THEN 'CURRENT'"));
        assert!(sql.contains("THEN 'UPCOMING'"));
        assert!(sql.contains("THEN 'COMPLETED'"));
        assert!(sql.contains("cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT'"));
        assert!(sql.ends_with("ELSE 'DRAFT' END"));
        assert_eq!(sql.matches(":now").count(), 4);
    }

    #[test]
    fn assignee_ids_coalesce_empty_and_guard_asymmetry() {
        let base = assignee_ids_sql(true);
        assert!(base.contains("COALESCE(ARRAY_AGG(DISTINCT issue_assignees.user_id)"));
        assert!(base.contains("issue_assignees.user_id IS NOT NULL"));
        assert!(base.contains("issue_assignee.deleted_at IS NULL"));
        assert!(base.ends_with(", '{}')"));
        let archive = assignee_ids_sql(false);
        assert!(!archive.contains("issue_assignee.deleted_at IS NULL"));
    }

    #[test]
    fn effective_list_order_is_not_the_queryset_order() {
        // Ported bug 2: the queryset `-is_favorite,name` order is dead on
        // list; `-is_favorite,-created_at` is what clients observe.
        assert_eq!(GET_QUERYSET_ORDER_SQL, "is_favorite DESC, cycles.name ASC");
        assert_eq!(LIST_ORDER_SQL, "is_favorite DESC, cycles.created_at DESC");
        assert_ne!(GET_QUERYSET_ORDER_SQL, LIST_ORDER_SQL);
    }

    #[test]
    fn list_projection_matches_fixture_keys_in_order() {
        let fixture = rows_fixture(FIXTURE_BASE_ROWS);
        let expected: Vec<&str> = fixture["list_projection"]["values_keys"]
            .as_array()
            .expect("F-C27-03 list_projection.values_keys")
            .iter()
            .map(|value| value.as_str().expect("key is a string"))
            .collect();
        assert_eq!(BASE_LIST_VALUES_FIELDS, expected.as_slice());
        assert!(
            fixture["retrieve_extra"]["missing"]["body"]["error"].as_str()
                == Some("Cycle not found")
        );
        assert_eq!(CYCLE_NOT_FOUND_BODY, "{\"error\":\"Cycle not found\"}");
    }

    #[test]
    fn estimate_subqueries_use_points_cast_float() {
        for (group, alias) in ESTIMATE_GROUPS.iter().zip(ESTIMATE_POINT_ALIASES.iter()) {
            let sub = estimate_subquery_sql(*group);
            assert!(
                sub.contains("SUM(CAST(estimate_point.value AS FLOAT))"),
                "{alias}"
            );
            assert!(sub.contains("e.type = 'points'"), "{alias}");
            assert!(sub.contains("ci2.cycle_id = cycles.id"), "{alias}");
            assert!(sub.contains("ci2.deleted_at IS NULL"), "{alias}");
            match group {
                Some(group) => assert!(
                    sub.contains(&format!("ie.state_group = '{group}'")),
                    "{alias}"
                ),
                None => assert!(!sub.contains("ie.state_group"), "{alias}"),
            }
            let wrapped = estimate_coalesce_sql(*group, alias);
            assert!(wrapped.starts_with("COALESCE(("));
            assert!(wrapped.ends_with(&format!("), 0.0) AS {alias}")));
        }
        assert_eq!(ESTIMATE_POINT_ALIASES.len(), 6);
    }

    #[test]
    fn cycle_issue_scope_and_annotations() {
        let scope = cycle_issue_scope_where();
        assert!(scope.contains("cycle_issues.cycle_id = :cycle_id"));
        assert!(scope.contains("cycles.workspace_slug = :slug"));
        assert_eq!(
            CYCLE_ISSUE_FILTERSET_FIELDS,
            &["issue__labels__id", "issue__assignees__id"]
        );
        assert_eq!(
            cycle_issue_sub_issues_count_sql(),
            "SELECT COUNT(*) FROM issues child WHERE child.parent_id = cycle_issues.issue_id"
        );
        assert!(annotated_cycle_id_sql().contains("LIMIT 1"));
        assert!(annotated_cycle_id_sql().contains("deleted_at IS NULL"));
        assert_eq!(
            link_count_sql(),
            "SELECT COUNT(*) FROM issue_links WHERE issue_id = issues.id"
        );
        assert!(attachment_count_sql().contains("entity_type = 'ISSUE_ATTACHMENT'"));
        assert_eq!(
            issue_sub_issues_count_sql(),
            "SELECT COUNT(*) FROM issues WHERE parent_id = issues.id"
        );
        let _ = rows_fixture(FIXTURE_ISSUE_ROWS);
    }

    #[test]
    fn favorites_scope_is_workspace_plus_user() {
        assert_eq!(
            favorites_scope_where(),
            "user_favorites.workspace_slug = :slug AND user_favorites.user_id = :user"
        );
    }

    #[test]
    fn transfer_groups_and_refusals_match_fixture() {
        let fixture = rows_fixture(FIXTURE_TRANSFER);
        let groups: Vec<&str> = fixture["move_filter"]["groups"]
            .as_array()
            .expect("F-C27-06 move_filter.groups")
            .iter()
            .map(|value| value.as_str().expect("group is a string"))
            .collect();
        assert_eq!(OPEN_STATE_GROUPS, groups.as_slice());
        assert_eq!(
            DESTINATION_COMPLETED_ERROR,
            fixture["refusals"]["completed_destination"]["error"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            SOURCE_CYCLE_MISSING_ERROR,
            fixture["refusals"]["source_cycle_missing"]["error"]
                .as_str()
                .unwrap()
        );
        assert_eq!(TRANSFER_BULK_UPDATE_FIELDS, &["cycle_id"]);
        assert_eq!(TRANSFER_BULK_BATCH_SIZE, 100);
        assert_eq!(SNAPSHOT_UPDATE_FIELDS, &["progress_snapshot"]);
        // F-C27-06 abbreviates the shape (no backlog_issues row) but every
        // data key it records must be a snapshot key; the source-order
        // list carries the full eight Python keys (:411-431).
        let snapshot = &fixture["progress_snapshot_shape"];
        for key in SNAPSHOT_KEYS {
            if key != &"backlog_issues" {
                assert!(snapshot.get(key).is_some(), "snapshot has {key}");
            }
        }
        assert_eq!(
            SNAPSHOT_KEYS,
            &[
                "total_issues",
                "completed_issues",
                "cancelled_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues",
                "distribution",
                "estimate_distribution",
            ]
        );
        let _ = rows_fixture(FIXTURE_ARCHIVE_ROWS);
    }

    #[test]
    fn transfer_move_where_covers_scope_and_open_groups() {
        let sql = transfer_move_where();
        assert!(sql.contains("cycle_issues.cycle_id = :cycle_id"));
        assert!(sql.contains("cycle_issues.project_id = :project_id"));
        assert!(sql.contains("cycle_issues.workspace_slug = :slug"));
        assert!(sql.contains("issue.archived_at IS NULL"));
        assert!(sql.contains("issue.is_draft = FALSE"));
        for group in OPEN_STATE_GROUPS {
            assert!(sql.contains(&format!("'{group}'")), "moves {group}");
        }
        assert!(!sql.contains("'completed'"));
        assert!(!sql.contains("'cancelled'"));
    }

    #[test]
    fn transfer_activity_contract() {
        assert_eq!(TRANSFER_ACTIVITY_TYPE, "cycle.activity.created");
        assert_eq!(TRANSFER_ACTIVITY_REQUESTED_DATA, "{\"cycles_list\": []}");
        assert_eq!(
            TRANSFER_ACTIVITY_INSTANCE_KEYS,
            &["updated_cycle_issues", "created_cycle_issues"]
        );
        assert_eq!(
            TRANSFER_ACTIVITY_ROW_KEYS,
            &["old_cycle_id", "new_cycle_id", "issue_id"]
        );
    }

    #[test]
    fn distribution_builders_split_completed_pending() {
        assert!(estimate_sum_sql(None).contains("SUM(CAST(estimate_point.value AS FLOAT))"));
        assert!(!estimate_sum_sql(None).contains("completed_at"));
        assert!(estimate_sum_sql(Some(true)).contains("issues.completed_at IS NOT NULL"));
        assert!(estimate_sum_sql(Some(false)).contains("issues.completed_at IS NULL"));
        assert!(distribution_count_sql(Some(true)).contains("COUNT(issues.id)"));
        assert!(avatar_url_case_sql().contains("CONCAT('/api/assets/v2/static/'"));
        assert!(avatar_url_case_sql().contains("THEN assignees.avatar ELSE NULL END"));
        let scope = transfer_distribution_scope_where();
        assert!(scope.contains(":cycle_id"));
        assert!(scope.contains("issue_cycle_deleted_at IS NULL"));
    }
}
