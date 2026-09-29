//! View queryset + annotation SQL builders (D-29, stage 5).
//!
//! Port of the five query units in
//! `apps/api/pi_dash/app/views/view/base.py`: `WorkspaceViewViewSet.get_queryset`
//! (`:60-69`), `IssueViewViewSet.get_queryset` (`:263-287`),
//! `IssueViewFavoriteViewSet.get_queryset` (`:404-411`),
//! `WorkspaceViewIssuesViewSet` (`:142-162` permission Q, `:164-210`
//! `apply_annotations`, `:212-213` `get_queryset`) and the view-issues list
//! pipeline (`:217-253`: `filter_queryset` + `issue_filters` +
//! `order_issue_queryset` + `paginate`).
//!
//! Recorded in `rust-api/fixtures/app_views_search/` (`FX-VIEW-ISSUES.sql`,
//! the queryset halves of `FX-VIEW-CRUD.json` and `FX-FAV.json`).
//! Builders return SQL text with PostgreSQL `$N` binds; execution owns to
//! the `db` layer (`sqlx::query` at runtime — no `query!` macros, there is
//! no build-time database, same as the merged `db` queries precedent).
//! The services crate holds no `sea-query` dependency, so — like the D-26
//! `OrderSpec` precedent in `crate::app_issues::ordering` — dynamic clauses
//! arrive as caller-supplied fragments and ordering reuses
//! [`crate::app_issues::ordering::order_sql`] verbatim; the F-04 filter
//! kernels (`pidash_db::{filter, filterset, issue_filters}`) and the F-07
//! paginator kernel (`pidash_api::paginator`) likewise apply verbatim and
//! are referenced, never re-ported.
//!
//! SQL semantics are Django's, quirks included (translate, don't redesign):
//!
//! * Every table reads through the soft-deletion manager
//!   (`db/mixins.py:56-58` — `deleted_at IS NULL`), including the joined
//!   `workspaces` / `projects` rows (merged D-33 precedent) and every
//!   annotation / prefetch subquery.
//! * `Issue.issue_objects` adds four exclusions (`db/models/issue.py:95-104`,
//!   pinned as `issue_ref::*` consts in
//!   `pidash_db::app_views_search::models`): triage state group, archived,
//!   project-archived, drafts. The sub-issue count re-applies them because
//!   it queries through `issue_objects` (`base.py:187`).
//! * `ROLE`: admin 20, member 15, guest 5
//!   (`app/permissions/base.py:13-16`).
//! * `FileAsset.EntityTypeContext.ISSUE_ATTACHMENT` stores `"ISSUE_ATTACHMENT"`
//!   (`db/models/asset.py:34`); the count subquery matches the merged D-26
//!   shape. (`FX-VIEW-ISSUES.sql` prose renders this literal lowercase;
//!   the Python value and the merged code are uppercase — the prose is
//!   illustrative, the value is normative.)
//! * Count annotations render `NULL` when empty: Django's grouped
//!   `Subquery(...annotate(count=Count()).values("count"))` has no
//!   `Coalesce`, so `NULLIF(COUNT(*), 0)` reproduces the null-on-empty
//!   shape (merged D-26 precedent, `api/src/app_issues/mod.rs`).
//! * This view's `apply_annotations` carries **no** assignee/module/label
//!   array subqueries: the id lists come from three `prefetch_related`
//!   queries (`base.py:192-209`), each a separate `SELECT ... WHERE
//!   issue_id = ANY($1)` with the manager scope and no join.
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * B1 (`base.py:102-112`): `WorkspaceViewViewSet.retrieve` serializes
//!   `.first()` unconditionally — unknown pk returns HTTP 200 with JSON
//!   `null`, not 404. The queryset builder is shared; the handler port
//!   must keep the null body.
//! * B1b (`base.py:317-327`): `IssueViewViewSet.retrieve` dereferences
//!   `issue_view.owned_by` after `.first()`; unknown pk plus a guest
//!   without `guest_view_all_features` raises `AttributeError` (500),
//!   not 404. The handler port must keep the crash order.
//! * B4 (`base.py:404-411`): `IssueViewFavoriteViewSet.get_queryset` calls
//!   `.select_related("view")`, but `UserFavorite` has no FK named `view`
//!   — Django raises `FieldError` on every list request. Ported as
//!   [`favorite_list_sql`], which always returns
//!   [`FavoriteListError::InvalidSelectRelated`]; the handler maps it to
//!   the Django 500.
//! * B2 (`app/serializers/base.py:12-18`, `?fields=` silently ignored) is
//!   serializer-owned and already ported in [`super::serializers`].

use crate::app_issues::ordering::{order_sql, OrderSpec};
use pidash_db::app_views_search::models::{issue_ref, issue_view, user_favorite};

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// `issue_views` table (`pidash_db::app_views_search::models::issue_view`).
pub const ISSUE_VIEW_TABLE: &str = issue_view::TABLE;
/// `user_favorites` table (`...::models::user_favorite`).
pub const FAVORITE_TABLE: &str = user_favorite::TABLE;
/// `issues` table (`...::models::issue_ref`).
pub const ISSUE_TABLE: &str = issue_ref::TABLE;
/// Tenant / membership tables (merged `db` consts agree on these names).
pub const WORKSPACE_TABLE: &str = "workspaces";
/// Projects table.
pub const PROJECT_TABLE: &str = "projects";
/// Project membership table.
pub const PROJECT_MEMBER_TABLE: &str = "project_members";
/// Workspace membership table.
pub const WORKSPACE_MEMBER_TABLE: &str = "workspace_members";
/// Lifecycle-state table.
pub const STATE_TABLE: &str = "states";
/// `CycleIssue` link table.
pub const CYCLE_ISSUE_TABLE: &str = "cycle_issues";
/// `IssueLink` table.
pub const ISSUE_LINK_TABLE: &str = "issue_links";
/// `FileAsset` table.
pub const FILE_ASSET_TABLE: &str = "file_assets";
/// `IssueAssignee` prefetch table.
pub const ISSUE_ASSIGNEE_TABLE: &str = "issue_assignees";
/// `IssueLabel` prefetch table.
pub const ISSUE_LABEL_TABLE: &str = "issue_labels";
/// `ModuleIssue` prefetch table.
pub const MODULE_ISSUE_TABLE: &str = "module_issues";

/// Guest role (`ROLE.GUEST`, `app/permissions/base.py:16`).
pub const ROLE_GUEST: i32 = 5;
/// Public-view marker (`IssueView.access = 1`, `db/models/view.py:66`).
pub const VIEW_ACCESS_PUBLIC: i16 = 1;
/// Favorite entity type for views (`base.py:267,417`).
pub const ENTITY_TYPE_VIEW: &str = "view";
/// Attachment entity literal (`db/models/asset.py:34`).
pub const ENTITY_ISSUE_ATTACHMENT: &str = "ISSUE_ATTACHMENT";

/// Default `order_by` for the view-issues list (`base.py:223`).
pub const DEFAULT_ORDER_BY: &str = "-created_at";
/// `BasePaginator.get_per_page` default (`utils/paginator.py:642-652`).
pub const DEFAULT_PER_PAGE: i64 = 1000;
/// `BasePaginator.get_per_page` ceiling (same source).
pub const MAX_PER_PAGE: i64 = 1000;

/// Default cursor when the request carries none
/// (`utils/paginator.py`: `f"{per_page}:0:0"`).
pub fn default_cursor(per_page: i64) -> String {
    format!("{per_page}:0:0")
}

/// Paginator envelope keys, exact (`utils/paginator.py:654-694`; pinned by
/// `PAGINATED_KEYS` in `contract-tests/app_views_search/test_global_views.py`).
/// Owned by the F-07 paginator kernel — repeated here so the list-pipeline
/// stage can assert the contract without depending on `pidash-api`.
pub const PAGINATOR_ENVELOPE_KEYS: &[&str] = &[
    "grouped_by",
    "sub_grouped_by",
    "total_count",
    "next_cursor",
    "prev_cursor",
    "next_page_results",
    "prev_page_results",
    "count",
    "total_pages",
    "total_results",
    "extra_stats",
    "results",
];

/// The view-issues list stages in order (`base.py:217-253`):
/// `get_queryset` → `filter_queryset` (ComplexFilterBackend + IssueFilterSet)
/// → legacy `issue_filters` → project permission Q → deepcopy `.only("id")`
/// for the count → `apply_annotations` → `order_issue_queryset` → `paginate`.
pub const LIST_PIPELINE_STAGES: &[&str] = &[
    "get_queryset",
    "filter_queryset",
    "issue_filters",
    "project_permission_filters",
    "total_count_queryset",
    "apply_annotations",
    "order_issue_queryset",
    "paginate",
];

/// `SELECT` every `issue_views` column, table-qualified, in Django `_meta`
/// order (`issue_view::COLUMNS`).
fn select_view_columns(alias: &str) -> String {
    issue_view::COLUMNS
        .iter()
        .map(|col| format!("{alias}.{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `Issue.issue_objects` scope (`db/models/issue.py:95-104`) as a `WHERE`
/// conjunction over `issue_alias` (the issue row) and `project_alias` (its
/// project row): soft-delete base plus the triage / archived /
/// project-archived / draft exclusions. The triage literal compiles from
/// [`issue_ref::MANAGER_EXCLUDE_STATE_GROUP`] so the scope cannot drift
/// from the models port.
pub fn issue_manager_scope(issue_alias: &str, project_alias: &str) -> String {
    format!(
        "{issue_alias}.deleted_at IS NULL \
         AND (state.\"group\" IS NULL OR NOT (state.\"group\" = '{}')) \
         AND {issue_alias}.archived_at IS NULL \
         AND {project_alias}.archived_at IS NULL \
         AND {issue_alias}.is_draft = FALSE",
        issue_ref::MANAGER_EXCLUDE_STATE_GROUP
    )
}

// ---------------------------------------------------------------------------
// Unit 1 — WorkspaceView queryset (base.py:60-69)
// ---------------------------------------------------------------------------

/// `WorkspaceViewViewSet.get_queryset` (`base.py:60-69`):
/// `filter(workspace__slug).filter(project__isnull=True)`
/// `.filter(Q(owned_by=user) | Q(access=1))`
/// `.order_by(GET order_by default -created_at).distinct()`.
///
/// Binds: `$1` workspace slug, `$2` user id. `order_by_sql` is the raw
/// `ORDER BY` fragment — Django splices the `order_by` query param
/// verbatim (default `-created_at`); validation owns to the handler.
/// `filter_queryset` is a pass-through here: the viewset declares no
/// `filter_backends` / `filterset_class`, so the default backends filter
/// on `filterset_fields = []` and change nothing.
pub fn workspace_view_list_sql(order_by_sql: &str) -> String {
    format!(
        "SELECT DISTINCT {cols} FROM {view} AS v \
         JOIN {ws} AS w ON w.id = v.workspace_id AND w.deleted_at IS NULL \
         WHERE w.slug = ($1) \
         AND v.project_id IS NULL \
         AND (v.owned_by_id = ($2) OR v.access = {public}) \
         AND v.deleted_at IS NULL \
         ORDER BY {order_by_sql}",
        cols = select_view_columns("v"),
        view = ISSUE_VIEW_TABLE,
        ws = WORKSPACE_TABLE,
        public = VIEW_ACCESS_PUBLIC,
    )
}

// ---------------------------------------------------------------------------
// Unit 2 — IssueView queryset (base.py:263-287)
// ---------------------------------------------------------------------------

/// `IssueViewViewSet.get_queryset` (`base.py:263-287`): workspace + project
/// scoping, active project membership, project-not-archived,
/// creator-or-public, `select_related("project")` / `("workspace")` (row
/// neutral — the serializer reads FK ids, and the project join below
/// already fetches the filtered row), `Exists(UserFavorite …)` as
/// `is_favorite`, `.order_by("-is_favorite", "name").distinct()`.
///
/// Binds: `$1` workspace slug, `$2` project id, `$3` user id.
pub fn project_view_list_sql() -> String {
    format!(
        "SELECT DISTINCT {cols}, \
         EXISTS(SELECT 1 FROM {fav} AS uf \
          JOIN {ws} AS fws ON fws.id = uf.workspace_id AND fws.deleted_at IS NULL \
          WHERE uf.user_id = ($3) \
            AND uf.entity_identifier = v.id \
            AND uf.entity_type = '{entity}' \
            AND uf.project_id = ($2) \
            AND fws.slug = ($1) \
            AND uf.deleted_at IS NULL) AS is_favorite \
         FROM {view} AS v \
         JOIN {ws} AS w ON w.id = v.workspace_id AND w.deleted_at IS NULL \
         JOIN {proj} AS p ON p.id = v.project_id AND p.deleted_at IS NULL \
         JOIN {pm} AS pm ON pm.project_id = v.project_id \
            AND pm.member_id = ($3) \
            AND pm.is_active \
            AND pm.deleted_at IS NULL \
         WHERE w.slug = ($1) \
         AND v.project_id = ($2) \
         AND p.archived_at IS NULL \
         AND (v.owned_by_id = ($3) OR v.access = {public}) \
         AND v.deleted_at IS NULL \
         ORDER BY is_favorite DESC, v.name ASC",
        cols = select_view_columns("v"),
        fav = FAVORITE_TABLE,
        ws = WORKSPACE_TABLE,
        entity = ENTITY_TYPE_VIEW,
        view = ISSUE_VIEW_TABLE,
        proj = PROJECT_TABLE,
        pm = PROJECT_MEMBER_TABLE,
        public = VIEW_ACCESS_PUBLIC,
    )
}

// ---------------------------------------------------------------------------
// Unit 3 — Favorite queryset (base.py:404-411, ported bug B4)
// ---------------------------------------------------------------------------

/// Error for the favorite-list read.
///
/// B4 (`base.py:404-411`): `.select_related("view")` names an FK that does
/// not exist on `UserFavorite`, so Django raises `FieldError` before any
/// row is returned. The handler maps this to the Django 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FavoriteListError {
    /// `FieldError('Invalid field name(s) given in select_related: view')`.
    InvalidSelectRelated {
        /// The offending field name (`"view"`).
        field: &'static str,
    },
}

/// `IssueViewFavoriteViewSet.get_queryset` (`base.py:404-411`).
///
/// Always returns [`FavoriteListError::InvalidSelectRelated`] — ported bug
/// B4, translate don't redesign. The `WHERE` the queryset *would* run
/// (`workspace__slug`, `user`) is documented in `FX-FAV.json`
/// (`list_shape.shape_if_valid`); no SQL is emitted because Django emits
/// none either — evaluation raises first.
pub fn favorite_list_sql() -> Result<String, FavoriteListError> {
    Err(FavoriteListError::InvalidSelectRelated { field: "view" })
}

// ---------------------------------------------------------------------------
// Unit 4 — View-issues queryset
// ---------------------------------------------------------------------------

/// `_get_project_permission_filters` (`base.py:142-162`) as a SQL fragment
/// over the `project_members` alias `pm`, the project alias `p` and the
/// issue alias `i`: guest with full access sees everything, guest without
/// sees only own-created rows, higher roles see everything — always scoped
/// to the caller's active membership.
///
/// `user_bind` is the `$N` placeholder for the user id (e.g. `"$2"`), so
/// the fragment composes into statements with their own numbering.
pub fn project_permission_filter(user_bind: &str) -> String {
    format!(
        "(((pm.role = {guest} AND p.guest_view_all_features = TRUE) \
         OR (pm.role = {guest} AND p.guest_view_all_features = FALSE \
             AND i.created_by_id = ({user_bind})) \
         OR (pm.role > {guest})) \
         AND pm.member_id = ({user_bind}) \
         AND pm.is_active = TRUE)",
        guest = ROLE_GUEST,
    )
}

/// `apply_annotations` scalar subqueries (`base.py:164-191`).
///
/// Shapes are the merged D-26 fragments verbatim: `cycle_id` is the first
/// live cycle row; the three counts are `NULLIF(COUNT(*), 0)` (null on
/// empty — no `Coalesce` in Django); link/attachment counts carry the
/// soft-delete manager scope; `sub_issues_count` re-applies the full
/// `issue_objects` scope because it queries through that manager
/// (`base.py:187`). No assignee/module/label arrays — this view serves
/// those lists from prefetches (see [`PrefetchRelation`]).
pub fn view_issue_annotation_selects() -> String {
    format!(
        "(SELECT ci.cycle_id FROM {ci} AS ci \
          WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1) AS cycle_id, \
        (SELECT NULLIF(COUNT(*), 0) FROM {il} AS il \
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS link_count, \
        (SELECT NULLIF(COUNT(*), 0) FROM {fa} AS fa \
          WHERE fa.issue_id = issue.id AND fa.entity_type = '{attach}' \
            AND fa.deleted_at IS NULL) AS attachment_count, \
        (SELECT NULLIF(COUNT(*), 0) FROM {issue} AS c \
           LEFT JOIN {state} AS cs ON cs.id = c.state_id AND cs.deleted_at IS NULL \
           JOIN {proj} AS cp ON cp.id = c.project_id AND cp.deleted_at IS NULL \
          WHERE c.parent_id = issue.id AND c.deleted_at IS NULL \
            AND (cs.\"group\" IS NULL OR NOT (cs.\"group\" = '{triage}')) \
            AND c.archived_at IS NULL AND cp.archived_at IS NULL \
            AND c.is_draft = FALSE) AS sub_issues_count",
        ci = CYCLE_ISSUE_TABLE,
        il = ISSUE_LINK_TABLE,
        fa = FILE_ASSET_TABLE,
        attach = ENTITY_ISSUE_ATTACHMENT,
        issue = ISSUE_TABLE,
        state = STATE_TABLE,
        proj = PROJECT_TABLE,
        triage = issue_ref::MANAGER_EXCLUDE_STATE_GROUP,
    )
}

/// The three `prefetch_related` relations (`base.py:192-209`), each a
/// separate query (no join) over the plain manager — deleted scope only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefetchRelation {
    /// `issue_assignee` → `IssueAssignee` (`base.py:193-197`).
    Assignees,
    /// `label_issue` → `IssueLabel` (`base.py:198-203`).
    Labels,
    /// `issue_module` → `ModuleIssue` (`base.py:204-209`).
    Modules,
}

/// Table behind a [`PrefetchRelation`].
pub fn prefetch_table(which: PrefetchRelation) -> &'static str {
    match which {
        PrefetchRelation::Assignees => ISSUE_ASSIGNEE_TABLE,
        PrefetchRelation::Labels => ISSUE_LABEL_TABLE,
        PrefetchRelation::Modules => MODULE_ISSUE_TABLE,
    }
}

/// One prefetch query: every column (`Prefetch(...objects.all())` selects
/// the full rows) for the given issue ids. Binds: `$1` `uuid[]` of issue
/// ids — the executable shape of Django's `WHERE issue_id IN (...)`.
pub fn prefetch_sql(which: PrefetchRelation) -> String {
    format!(
        "SELECT * FROM {table} WHERE issue_id = ANY(($1)) AND deleted_at IS NULL",
        table = prefetch_table(which),
    )
}

/// `WorkspaceViewIssuesViewSet.get_queryset` (`base.py:212-213`) plus the
/// `IssueManager` scope: issues in the workspace slug with the four
/// manager exclusions. Binds: `$1` workspace slug.
pub fn view_issues_base_sql() -> String {
    format!(
        "SELECT issue.id FROM {issue} AS issue \
         JOIN {proj} AS p ON p.id = issue.project_id AND p.deleted_at IS NULL \
         JOIN {ws} AS w ON w.id = issue.workspace_id AND w.deleted_at IS NULL \
         LEFT JOIN {state} AS state ON state.id = issue.state_id \
            AND state.deleted_at IS NULL \
         WHERE w.slug = ($1) AND {scope}",
        issue = ISSUE_TABLE,
        proj = PROJECT_TABLE,
        ws = WORKSPACE_TABLE,
        state = STATE_TABLE,
        scope = issue_manager_scope("issue", "p"),
    )
}

/// AND-combine the two filter stages of the list pipeline: the
/// `filter_queryset` fragment (ComplexFilterBackend `filter=` tree over the
/// F-04 kernels + `IssueFilterSet` leaves) and the legacy
/// `issue_filters(params, "GET")` fragment (`base.py:221-227`). `None`
/// stages drop out; both `None` yields `None` (no extra `WHERE`).
pub fn combine_filters(complex_where: Option<&str>, legacy_where: Option<&str>) -> Option<String> {
    match (complex_where, legacy_where) {
        (None, None) => None,
        (Some(only), None) | (None, Some(only)) => Some(only.to_owned()),
        (Some(left), Some(right)) => Some(format!("({left}) AND ({right})")),
    }
}

// ---------------------------------------------------------------------------
// Unit 5 — View-issues list pipeline (base.py:217-253)
// ---------------------------------------------------------------------------

/// Issue row columns the `ViewIssueListSerializer` reads
/// (`app/serializers/view.py:14-53`; pinned by `VIEW_ISSUE_KEYS`): the
/// D-26 flat-list columns verbatim — `estimate_point` renders
/// `estimate_point_id`, `created_by` / `updated_by` render `*_id`, and
/// `state__group` comes from the state join.
pub const VIEW_ISSUE_ROW_COLUMNS: &str = "issue.id, issue.name, issue.state_id, \
    issue.sort_order, issue.completed_at, issue.estimate_point_id AS estimate_point, \
    issue.priority, issue.start_date, issue.target_date, issue.sequence_id, \
    issue.project_id, issue.parent_id, issue.created_at, issue.updated_at, \
    issue.created_by_id AS created_by, issue.updated_by_id AS updated_by, \
    issue.is_draft, issue.archived_at, state.\"group\" AS \"state__group\"";

/// Full view-issues list statement: base scope + caller filter fragments +
/// permission Q, then annotations, then the caller's `ORDER BY` fragment
/// (`base.py:218-244`).
///
/// Binds: `$1` workspace slug, `$2` user id. `order_by_sql` is the fragment
/// from [`view_issues_order_sql`]; `complex_where` / `legacy_where` are the
/// `filter_queryset` / `issue_filters` fragments (see [`combine_filters`]).
pub fn view_issues_list_sql(
    complex_where: Option<&str>,
    legacy_where: Option<&str>,
    order_by_sql: &str,
) -> String {
    let mut sql = format!(
        "SELECT {rows}, {annotations} \
         FROM {issue} AS issue \
         JOIN {proj} AS p ON p.id = issue.project_id AND p.deleted_at IS NULL \
         JOIN {ws} AS w ON w.id = issue.workspace_id AND w.deleted_at IS NULL \
         LEFT JOIN {state} AS state ON state.id = issue.state_id \
            AND state.deleted_at IS NULL \
         JOIN {pm} AS pm ON pm.project_id = issue.project_id \
            AND pm.deleted_at IS NULL \
         WHERE w.slug = ($1) AND {scope} AND {perm}",
        rows = VIEW_ISSUE_ROW_COLUMNS,
        annotations = view_issue_annotation_selects(),
        issue = ISSUE_TABLE,
        proj = PROJECT_TABLE,
        ws = WORKSPACE_TABLE,
        state = STATE_TABLE,
        pm = PROJECT_MEMBER_TABLE,
        scope = issue_manager_scope("issue", "p"),
        perm = project_permission_filter("$2"),
    );
    if let Some(extra) = combine_filters(complex_where, legacy_where) {
        sql.push_str(&format!(" AND ({extra})"));
    }
    sql.push_str(&format!(" ORDER BY {order_by_sql}"));
    sql
}

/// Count statement for the paginator total (`base.py:235-236`): a deepcopy
/// of the filtered queryset with `.only("id")`, i.e. `COUNT(*)` over the
/// pre-annotation query. Same binds as [`view_issues_list_sql`].
pub fn view_issues_count_sql(complex_where: Option<&str>, legacy_where: Option<&str>) -> String {
    let mut sql = format!(
        "SELECT COUNT(*) FROM {issue} AS issue \
         JOIN {proj} AS p ON p.id = issue.project_id AND p.deleted_at IS NULL \
         JOIN {ws} AS w ON w.id = issue.workspace_id AND w.deleted_at IS NULL \
         LEFT JOIN {state} AS state ON state.id = issue.state_id \
            AND state.deleted_at IS NULL \
         JOIN {pm} AS pm ON pm.project_id = issue.project_id \
            AND pm.deleted_at IS NULL \
         WHERE w.slug = ($1) AND {scope} AND {perm}",
        issue = ISSUE_TABLE,
        proj = PROJECT_TABLE,
        ws = WORKSPACE_TABLE,
        state = STATE_TABLE,
        pm = PROJECT_MEMBER_TABLE,
        scope = issue_manager_scope("issue", "p"),
        perm = project_permission_filter("$2"),
    );
    if let Some(extra) = combine_filters(complex_where, legacy_where) {
        sql.push_str(&format!(" AND ({extra})"));
    }
    sql
}

/// `order_issue_queryset(issue_queryset, order_by_param)` (`base.py:242-244`)
/// via the D-26 kernel verbatim: all four branches (priority `CASE`,
/// state-group `CASE` with the dead `[::-1]` branch, min-aggregation over
/// the label/assignee/module relations, default with the `created_at`
/// substring tiebreak rule) plus the rewritten `order_by` param the
/// paginator echoes. The min aggregations correlate to the outer `issue`
/// alias; the state column is the list-query state join.
pub fn view_issues_order_sql(order_by_param: &str) -> OrderSpec {
    order_sql(
        order_by_param,
        "state.\"group\"",
        |stripped| match stripped {
            "labels__name" => "(SELECT MIN(l.name) FROM issue_labels il \
             JOIN labels l ON l.id = il.label_id AND l.deleted_at IS NULL \
             WHERE il.issue_id = issue.id AND il.deleted_at IS NULL)"
                .to_owned(),
            "assignees__first_name" => "(SELECT MIN(u.first_name) FROM issue_assignees ia \
             JOIN users u ON u.id = ia.assignee_id \
             WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL)"
                .to_owned(),
            _ => "(SELECT MIN(m.name) FROM module_issues mi \
             JOIN modules m ON m.id = mi.module_id AND m.deleted_at IS NULL \
             WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL)"
                .to_owned(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // FX-VIEW-CRUD.json `workspace_view.get_queryset`
    // (base.py:60-69): tenant + project-null + creator-or-public.
    #[test]
    fn workspace_queryset_scopes_tenant_creator_and_public() {
        let sql = workspace_view_list_sql("-created_at");
        assert!(sql.contains("FROM issue_views AS v"), "{sql}");
        assert!(sql.contains("w.slug = ($1)"), "{sql}");
        assert!(sql.contains("v.project_id IS NULL"), "{sql}");
        assert!(
            sql.contains("v.owned_by_id = ($2) OR v.access = 1"),
            "{sql}"
        );
        assert!(sql.contains("v.deleted_at IS NULL"), "{sql}");
        assert!(sql.contains("w.deleted_at IS NULL"), "{sql}");
        assert!(sql.starts_with("SELECT DISTINCT"), "{sql}");
        assert!(sql.ends_with("ORDER BY -created_at"), "{sql}");
        // Full view column list, `_meta` order.
        for col in issue_view::COLUMNS {
            assert!(sql.contains(&format!("v.{col}")), "{sql}");
        }
    }

    // FX-VIEW-CRUD.json `project_view.get_queryset` (base.py:263-287):
    // membership + not-archived + creator-or-public + is_favorite first.
    #[test]
    fn project_queryset_scopes_membership_and_favorites_first() {
        let sql = project_view_list_sql();
        assert!(sql.contains("w.slug = ($1)"), "{sql}");
        assert!(sql.contains("v.project_id = ($2)"), "{sql}");
        assert!(sql.contains("pm.member_id = ($3)"), "{sql}");
        assert!(sql.contains("pm.is_active"), "{sql}");
        assert!(sql.contains("p.archived_at IS NULL"), "{sql}");
        assert!(
            sql.contains("v.owned_by_id = ($3) OR v.access = 1"),
            "{sql}"
        );
        assert!(sql.contains("uf.entity_identifier = v.id"), "{sql}");
        assert!(sql.contains("uf.entity_type = 'view'"), "{sql}");
        assert!(sql.contains("uf.project_id = ($2)"), "{sql}");
        assert!(sql.contains("uf.user_id = ($3)"), "{sql}");
        assert!(
            sql.contains("ORDER BY is_favorite DESC, v.name ASC"),
            "{sql}"
        );
    }

    // FX-FAV.json `bugs.B4`: every list evaluation raises FieldError.
    #[test]
    fn favorite_queryset_raises_invalid_select_related() {
        assert_eq!(
            favorite_list_sql(),
            Err(FavoriteListError::InvalidSelectRelated { field: "view" })
        );
    }

    // FX-VIEW-ISSUES.sql §2 (base.py:142-162): the three guest/role
    // branches plus the active-membership conjunction.
    #[test]
    fn permission_filter_has_guest_and_role_branches() {
        let frag = project_permission_filter("$2");
        assert!(
            frag.contains("pm.role = 5 AND p.guest_view_all_features = TRUE"),
            "{frag}"
        );
        assert!(
            frag.contains(
                "pm.role = 5 AND p.guest_view_all_features = FALSE \
                 AND i.created_by_id = ($2)"
            ),
            "{frag}"
        );
        assert!(frag.contains("pm.role > 5"), "{frag}");
        assert!(frag.contains("pm.member_id = ($2)"), "{frag}");
        assert!(frag.contains("pm.is_active = TRUE"), "{frag}");
    }

    // FX-VIEW-ISSUES.sql §1 + manager scope (issue.py:95-104): the four
    // exclusions compile from the models consts.
    #[test]
    fn manager_scope_excludes_triage_archived_and_drafts() {
        let scope = issue_manager_scope("issue", "p");
        assert!(scope.contains("issue.deleted_at IS NULL"), "{scope}");
        assert!(
            scope.contains("NOT (state.\"group\" = 'triage')"),
            "{scope}"
        );
        assert!(scope.contains("issue.archived_at IS NULL"), "{scope}");
        assert!(scope.contains("p.archived_at IS NULL"), "{scope}");
        assert!(scope.contains("issue.is_draft = FALSE"), "{scope}");
        assert_eq!(issue_ref::MANAGER_EXCLUDE_STATE_GROUP, "triage");
    }

    // FX-VIEW-ISSUES.sql §4 (base.py:164-191): four subqueries, null on
    // empty, no array guards on this view.
    #[test]
    fn annotations_match_apply_annotations_shapes() {
        let sel = view_issue_annotation_selects();
        assert!(sel.contains("LIMIT 1) AS cycle_id"), "{sel}");
        assert!(sel.contains("ci.deleted_at IS NULL"), "{sel}");
        assert!(sel.contains("NULLIF(COUNT(*), 0)"), "{sel}");
        assert_eq!(sel.matches("NULLIF(COUNT(*), 0)").count(), 3, "{sel}");
        assert!(sel.contains("AS link_count"), "{sel}");
        assert!(sel.contains("AS attachment_count"), "{sel}");
        assert!(sel.contains("AS sub_issues_count"), "{sel}");
        assert!(sel.contains("entity_type = 'ISSUE_ATTACHMENT'"), "{sel}");
        assert!(sel.contains("c.parent_id = issue.id"), "{sel}");
        assert!(sel.contains("NOT (cs.\"group\" = 'triage')"), "{sel}");
        assert!(!sel.contains("ARRAY_AGG"), "{sel}");
        assert!(!sel.contains("assignee_ids"), "{sel}");
    }

    // FX-VIEW-ISSUES.sql §4 prefetches (base.py:192-209): separate
    // queries, manager scope, no join.
    #[test]
    fn prefetches_are_separate_scoped_queries() {
        for which in [
            PrefetchRelation::Assignees,
            PrefetchRelation::Labels,
            PrefetchRelation::Modules,
        ] {
            let sql = prefetch_sql(which);
            assert!(sql.contains("WHERE issue_id = ANY(($1))"), "{sql}");
            assert!(sql.contains("deleted_at IS NULL"), "{sql}");
            assert!(!sql.contains("JOIN"), "{sql}");
        }
        assert_eq!(
            prefetch_table(PrefetchRelation::Assignees),
            "issue_assignees"
        );
        assert_eq!(prefetch_table(PrefetchRelation::Labels), "issue_labels");
        assert_eq!(prefetch_table(PrefetchRelation::Modules), "module_issues");
    }

    // List pipeline stage order (base.py:217-253).
    #[test]
    fn pipeline_stages_match_list_order() {
        assert_eq!(
            LIST_PIPELINE_STAGES,
            &[
                "get_queryset",
                "filter_queryset",
                "issue_filters",
                "project_permission_filters",
                "total_count_queryset",
                "apply_annotations",
                "order_issue_queryset",
                "paginate",
            ]
        );
    }

    // Filter-stage combiner: absent stages drop out, both present AND.
    #[test]
    fn filter_combiner_ands_present_stages() {
        assert_eq!(combine_filters(None, None), None);
        assert_eq!(
            combine_filters(Some("a = 1"), None),
            Some("a = 1".to_owned())
        );
        assert_eq!(
            combine_filters(None, Some("b = 2")),
            Some("b = 2".to_owned())
        );
        assert_eq!(
            combine_filters(Some("a = 1"), Some("b = 2")),
            Some("(a = 1) AND (b = 2)".to_owned())
        );
    }

    // Full list statement: scope + permission + annotations + order;
    // count statement: same filters, no annotations, COUNT(*).
    #[test]
    fn list_and_count_share_filters_but_not_annotations() {
        let order = view_issues_order_sql(DEFAULT_ORDER_BY);
        let list = view_issues_list_sql(
            Some("complex = TRUE"),
            Some("legacy = TRUE"),
            &order.order_by_sql,
        );
        assert!(list.contains("w.slug = ($1)"), "{list}");
        assert!(list.contains("pm.member_id = ($2)"), "{list}");
        assert!(
            list.contains("(complex = TRUE) AND (legacy = TRUE)"),
            "{list}"
        );
        assert!(list.contains("AS cycle_id"), "{list}");
        assert!(list.contains("AS sub_issues_count"), "{list}");
        assert!(list.ends_with("ORDER BY -created_at"), "{list}");

        let count = view_issues_count_sql(Some("complex = TRUE"), Some("legacy = TRUE"));
        assert!(count.starts_with("SELECT COUNT(*)"), "{count}");
        assert!(count.contains("w.slug = ($1)"), "{count}");
        assert!(count.contains("pm.member_id = ($2)"), "{count}");
        assert!(
            count.contains("(complex = TRUE) AND (legacy = TRUE)"),
            "{count}"
        );
        assert!(!count.contains("AS cycle_id"), "{count}");
        assert!(!count.contains("ORDER BY"), "{count}");
    }

    // Ordering reuses the D-26 kernel: default passes through, the
    // rewritten param is echoed by the paginator.
    #[test]
    fn ordering_defaults_to_created_at_desc() {
        assert_eq!(DEFAULT_ORDER_BY, "-created_at");
        let spec = view_issues_order_sql("-created_at");
        assert_eq!(spec.order_by_sql, "-created_at");
        assert_eq!(spec.out_param, "-created_at");
        let priority = view_issues_order_sql("-priority");
        assert!(priority.order_by_sql.contains("created_at DESC"));
        assert_eq!(priority.out_param, "priority_order");
    }

    // Paginator contract (utils/paginator.py:642-694): per-page rule,
    // cursor default, 12-key envelope.
    #[test]
    fn paginator_contract_values() {
        assert_eq!((DEFAULT_PER_PAGE, MAX_PER_PAGE), (1000, 1000));
        assert_eq!(default_cursor(1000), "1000:0:0");
        assert_eq!(
            PAGINATOR_ENVELOPE_KEYS,
            &[
                "grouped_by",
                "sub_grouped_by",
                "total_count",
                "next_cursor",
                "prev_cursor",
                "next_page_results",
                "prev_page_results",
                "count",
                "total_pages",
                "total_results",
                "extra_stats",
                "results",
            ]
        );
    }
}
