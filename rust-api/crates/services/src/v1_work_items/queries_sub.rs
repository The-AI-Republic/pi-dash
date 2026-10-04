#![forbid(unsafe_code)]

//! Subresource read query builders (D-18 queries B, PIDASHCONV-669).
//!
//! Ports the subresource querysets of `apps/api/pi_dash/api/views/issue.py`
//! (drift baseline `01a93e17`) plus the PR/review-link querysets in
//! `api/views/github_pr.py` and `api/views/git_code_review.py`, following
//! the D-30 precedent (`app_pages::queries`): each builder returns a
//! fragment the caller splices into the statement it executes.
//! Placeholders stay symbolic — `:slug`, `:user`, `:project_id`,
//! `:issue_id`, `:pk`, `:issues`, `:actual` — handlers bind them.
//!
//! | Unit | Python source | Builders |
//! | --- | --- | --- |
//! | Label list/detail | `views/issue.py:1322-1336` (detail inherits it, `:1440`; `get`/`patch`/`delete` add `.get(pk)`, `:1466/:1492/:1535`; patch conflict check `:1495-1506`) | [`label_scope_where`], [`label_list_sql`], [`label_detail_where`], [`label_external_dedupe_where`] |
//! | Link list/detail | `:1557-1569` / `:1662-1675` (detail `get` pk-`None` branch `:1704-1712`, else `.get(pk)` `:1713`; `patch`/`delete` direct `.get` `:1746`/`:1795`) | [`link_scope_where`], [`link_list_sql`], [`link_detail_sql`], [`link_detail_where`], [`link_direct_lookup_where`] |
//! | Comment list/detail | `:1805-1828` / `:1961-1984` (`get` `.get(pk)` `:2006`; `patch`/`delete` direct `.get` `:2035`/`:2117`; patch conflict check `:2041-2055`) | [`comment_scope_where`], [`comment_is_member_sql`], [`comment_list_sql`], [`comment_detail_where`], [`comment_direct_lookup_where`], [`comment_external_dedupe_where`] |
//! | Activity list/detail | `:2156-2165` / `:2211-2224` (inline in `get`, no `get_queryset`; detail `.first()` + 404 body) | [`activity_list_where`], [`activity_list_sql`], [`activity_detail_where`], [`ACTIVITY_NOT_FOUND_BODY`] |
//! | Attachment list/detail | `:2235-2653` (list filter `:2438-2444`; post dedupe `:2354-2375`; detail `.get`s `:2491`/`:2562`/`:2640`) | [`attachment_list_where`], [`attachment_list_sql`], [`attachment_dedupe_where`], [`attachment_detail_where`], [`attachment_issue_where`] |
//! | Relation grouped read | `:2965-3008` aggregate + `:3010-3019` merge | [`relation_scope_where`], [`relation_aggregate_sql`], [`RELATION_RESPONSE_KEYS`], [`union_ids`] |
//! | Relation create map/refetch | mapper `utils/issue_relation_mapper.py:19-32`, reverse set `:3077`, refetch `:3111-3132` | [`actual_relation`], [`is_reverse_relation`], [`relation_refetch_where`] |
//! | Workpad read/lock | get_queryset `:3269-3273`, `get` `:3275-3279`, patch lock `:3307-3321` | [`workpad_scope_where`], [`workpad_list_sql`], [`workpad_get_where`], [`workpad_lock_sql`] |
//! | PR links list/detail | `github_pr.py:37-49` / `:86-95` | [`pr_list_sql`], [`pr_detail_sql`], [`pr_detail_where`] |
//! | Review links list/detail | `git_code_review.py:32-44` / `:86-95` | [`review_list_sql`], [`review_detail_sql`], [`review_detail_where`] |
//!
//! Fixture oracle: F18-07
//! (`rust-api/fixtures/v1_work_items/queries/F18-07.subresources.json`).
//! The tests below replay it: source-line pointers, predicate needles in
//! source order, order defaults, row counts/keys, the aggregate result,
//! and the relation mapping table.
//!
//! SQL-form notes (same rows as Django, D-30 style):
//!
//! * The member/archived guards are `EXISTS` subqueries correlated on
//!   `{table}.project_id` ([`member_guard_sql`], [`project_live_sql`])
//!   instead of Django's `INNER JOIN project_members` + `DISTINCT`
//!   fanout guard. No fanout is possible through `EXISTS`, so the
//!   representative selects keep `DISTINCT` only where the Python chain
//!   calls `.distinct()`, as a source-fidelity marker.
//! * `select_related` (label `:1331-1333`, comment `:1814`/`:1970`,
//!   activity `:2164`/`:2220`) is a fetch hint — same rows with or
//!   without it — so the selects project `{table}.*` plus real
//!   annotations only ([`comment_is_member_sql`]).
//! * Every model here orders `Meta.ordering = ("-created_at",)`; the
//!   chains without an explicit `.order_by` (attachments, workpad,
//!   PR/review detail) inherit it, exactly as the fixture SQL shows.
//! * `.get(pk)` executes with `LIMIT 21` (Django `MAX_GET_RESULTS`);
//!   `.first()` with `LIMIT 1`. Only [`workpad_lock_sql`] renders the
//!   executed form (the fixture captured it); the other detail builders
//!   return the queryset form plus the pk predicate.
//!
//! Ported bugs (translate, don't redesign):
//!
//! 1. Attachment reads skip the member/archived guards every sibling
//!    applies (list `:2438-2444`, all detail `.get`s; the endpoints
//!    declare no `permission_classes`).
//! 2. The relation grouped read ignores `project_id`
//!    (`:2984-2987`) — cross-project rows aggregate into the response.
//! 3. `duplicate`/`relates_to` merge via `set()` (`:3015-3016`) —
//!    direction-union order is unstable. [`union_ids`] keeps the union
//!    membership with first-seen order.
//! 4. Link/comment `patch`/`delete` use a direct `.get()` bypassing the
//!    queryset's member/archived guards (`:1746`/`:1795`, `:2035`/`:2117`).
//! 5. PR/review-link detail drops the archived guard, the order and the
//!    distinct (`github_pr.py:86-95`, `git_code_review.py:86-95`) —
//!    detach works on archived projects.
//! 6. Link detail `get` with `pk is None` paginates the whole list
//!    (`:1704-1712`); comment detail has no such branch. Handler-owned,
//!    noted so the mapping reads total.
//! 7. The eight `kwargs.get("order_by", "-created_at")` chains always
//!    take the default on the wire (no URL conf sets an `order_by`
//!    kwarg); only activity reads live `request.GET` — and defaults to
//!    ascending `created_at`.
//! 8. Comment patch compares `external_id` in Python before the dedupe
//!    `exists()` (`:2042-2043`); label patch excludes the pk in SQL
//!    (`:1504`). Both shapes ported as observed.
//! 9. A raw `blocking` row matches NO aggregate arm — the grouped read
//!    only knows stored types (fixture seed note).
//!
//! Out of scope (sibling issues): pagination envelopes and `fields=`/
//! `expand=` shaping (handlers B/C/D/G/H: PIDASHCONV-674/675/676/679/680),
//! permission gates (PIDASHCONV-671), activity/task enqueues
//! (PIDASHCONV-672), the agent relation endpoints (`_IssueRelationAgentBase`,
//! PIDASHCONV-676), and every write path.

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// `StateGroup.TRIAGE.value` (`db/models/state.py:22`), excluded by
/// `IssueManager` (`db/models/issue.py:95-103`).
pub const TRIAGE_GROUP: &str = "triage";

/// The `.order_by(self.kwargs.get("order_by", "-created_at"))` default
/// carried by the eight queryset chains (label, link ×2, comment ×2,
/// PR/review list). Always taken on the wire — ported bug 7.
pub const QUERYSET_ORDER_DEFAULT: &str = "-created_at";

/// The activity `.order_by(request.GET.get("order_by", "created_at"))`
/// default (`:2165`, `:2223`) — ascending, unlike every queryset chain.
pub const ACTIVITY_ORDER_DEFAULT: &str = "created_at";

/// Fields the activity reads exclude (`~Q(field__in=[...])`, `:2159`,
/// `:2215`).
pub const ACTIVITY_EXCLUDED_FIELDS: &[&str] = &["comment", "vote", "reaction", "draft"];

/// `FileAsset.EntityTypeContext.ISSUE_ATTACHMENT`
/// (`db/models/asset.py:34`).
pub const ISSUE_ATTACHMENT_ENTITY: &str = "ISSUE_ATTACHMENT";

/// Activity detail 404 body (`:2226`, DRF compact separators, key order
/// as constructed).
pub const ACTIVITY_NOT_FOUND_BODY: &str =
    "{\"message\":\"Activity not found.\",\"code\":\"NOT_FOUND\"}";

/// Workpad patch missing-`body` error message (`:3289-3292`).
pub const WORKPAD_MISSING_BODY_MESSAGE: &str =
    "PATCH requires a `body` field in the request payload.";

/// Workpad patch missing-`body` status (`:3291`).
pub const WORKPAD_MISSING_BODY_STATUS: u16 = 400;

// ---------------------------------------------------------------------------
// Order
// ---------------------------------------------------------------------------

/// A resolved `.order_by(...)` argument: the column text plus direction.
/// A leading `-` selects descending, exactly like Django; the column
/// text (including an empty or unknown column) passes through untouched
/// and fails at the database like Django's `FieldError`-at-evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubOrder {
    /// Column text after stripping one leading `-`.
    pub column: String,
    /// True when the raw value had a leading `-`.
    pub descending: bool,
}

/// Parse one `.order_by(...)` / `request.GET.get("order_by", ...)`
/// argument. `raw` is `None` when the kwarg/param is absent (→ `default`).
pub fn parse_order(raw: Option<&str>, default: &str) -> SubOrder {
    let text = raw.unwrap_or(default);
    match text.strip_prefix('-') {
        Some(column) => SubOrder {
            column: column.to_owned(),
            descending: true,
        },
        None => SubOrder {
            column: text.to_owned(),
            descending: false,
        },
    }
}

impl SubOrder {
    /// Render the `ORDER BY` fragment (without the keywords) against
    /// `table`, e.g. `labels.created_at DESC`.
    pub fn sql(&self, table: &str) -> String {
        format!(
            "{table}.{} {}",
            self.column,
            if self.descending { "DESC" } else { "ASC" }
        )
    }
}

// ---------------------------------------------------------------------------
// Shared guards
// ---------------------------------------------------------------------------

/// `workspace__slug=slug` (`:1323`, `:1558`, …): every table here carries
/// its own `workspace_id` FK.
pub fn workspace_slug_sql(table: &str) -> String {
    format!("{table}.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)")
}

/// `project__project_projectmember__member=user, __is_active=True`
/// (`:1326-1329`, `:1561-1564`, …) as an `EXISTS` correlated on
/// `{table}.project_id`.
///
/// No `deleted_at` guard: the fixture SQL shows the join-span form
/// carries none (contrast [`comment_is_member_sql`], whose explicit
/// `ProjectMember.objects` manager adds one).
pub fn member_guard_sql(table: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = {table}.project_id \
         AND pm.member_id = :user AND pm.is_active = TRUE)"
    )
}

/// `project__archived_at__isnull=True` (`:1330`, `:1565`, …) as an
/// `EXISTS` correlated on `{table}.project_id`.
pub fn project_live_sql(table: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM projects WHERE projects.id = {table}.project_id \
         AND projects.archived_at IS NULL)"
    )
}

/// `SoftDeletionManager` (`db/mixins.py:56-58`): every queryset here
/// starts from `deleted_at IS NULL`.
pub fn live_row_sql(table: &str) -> String {
    format!("{table}.deleted_at IS NULL")
}

// ---------------------------------------------------------------------------
// Labels (views/issue.py:1322-1336, detail :1440+)
// ---------------------------------------------------------------------------

/// Label scope in source order (`:1323-1330`): slug, project, member,
/// live project. Detail reuses it via inheritance (`:1440`).
pub fn label_scope_where() -> String {
    [
        live_row_sql("labels"),
        workspace_slug_sql("labels"),
        "labels.project_id = :project_id".to_owned(),
        member_guard_sql("labels"),
        project_live_sql("labels"),
    ]
    .join(" AND ")
}

/// Full representative label list/detail `SELECT`: scope + `DISTINCT`
/// (`:1334`) + kwargs order (`:1335`). `select_related("project",
/// "workspace", "parent")` (`:1331-1333`) is a fetch hint — same rows.
pub fn label_list_sql(order: &SubOrder) -> String {
    format!(
        "SELECT DISTINCT labels.* FROM labels WHERE {} ORDER BY {}",
        label_scope_where(),
        order.sql("labels"),
    )
}

/// Detail predicate (`:1466`, `:1492`, `:1535`): scope + pk.
pub fn label_detail_where() -> String {
    format!("{} AND labels.id = :pk", label_scope_where())
}

/// Label patch external-dedupe `exists()` (`:1495-1506`): same
/// project/slug/source/id, excluding this pk → 409 `{"error": "Label
/// with the same external id and external source already exists",
/// "id": ...}`. Ported asymmetry 8: the exclusion lives in SQL here.
pub fn label_external_dedupe_where() -> String {
    [
        live_row_sql("labels"),
        "labels.project_id = :project_id".to_owned(),
        workspace_slug_sql("labels"),
        "labels.external_source = :external_source".to_owned(),
        "labels.external_id = :external_id".to_owned(),
        "labels.id != :pk".to_owned(),
    ]
    .join(" AND ")
}

// ---------------------------------------------------------------------------
// Links (views/issue.py:1557-1569 list, :1662-1675 detail)
// ---------------------------------------------------------------------------

/// Link scope in source order (`:1558-1565`, `:1663-1670`): slug,
/// project, issue, member, live project. Both chains are identical.
pub fn link_scope_where() -> String {
    [
        live_row_sql("issue_links"),
        workspace_slug_sql("issue_links"),
        "issue_links.project_id = :project_id".to_owned(),
        "issue_links.issue_id = :issue_id".to_owned(),
        member_guard_sql("issue_links"),
        project_live_sql("issue_links"),
    ]
    .join(" AND ")
}

/// Full representative link list `SELECT`: scope + kwargs order
/// (`:1566`) + `DISTINCT` (`:1567`). (Order-before-distinct in source
/// is immaterial — same rows as the label chain's order.)
pub fn link_list_sql(order: &SubOrder) -> String {
    format!(
        "SELECT DISTINCT issue_links.* FROM issue_links WHERE {} ORDER BY {}",
        link_scope_where(),
        order.sql("issue_links"),
    )
}

/// Full representative link detail `SELECT` (`:1662-1675`): identical
/// chain to the list (fixture `link_detail_queryset` pins the same SQL).
/// Detail `get` either paginates this (ported bug 6: `pk is None`,
/// `:1704-1712`) or appends the pk ([`link_detail_where`], `:1713`).
pub fn link_detail_sql(order: &SubOrder) -> String {
    link_list_sql(order)
}

/// Detail predicate (`:1713`): scope + pk.
pub fn link_detail_where() -> String {
    format!("{} AND issue_links.id = :pk", link_scope_where())
}

/// Link `patch`/`delete` direct lookup (`:1746`, `:1795`):
/// `IssueLink.objects.get(workspace__slug, project_id, issue_id, pk)` —
/// ported bug 4: bypasses the queryset's member/archived guards.
pub fn link_direct_lookup_where() -> String {
    [
        live_row_sql("issue_links"),
        workspace_slug_sql("issue_links"),
        "issue_links.project_id = :project_id".to_owned(),
        "issue_links.issue_id = :issue_id".to_owned(),
        "issue_links.id = :pk".to_owned(),
    ]
    .join(" AND ")
}

// ---------------------------------------------------------------------------
// Comments (views/issue.py:1805-1828 list, :1961-1984 detail)
// ---------------------------------------------------------------------------

/// Comment scope in source order (`:1806-1813`, `:1962-1969`): slug,
/// project, issue, member, live project. Both chains are identical.
pub fn comment_scope_where() -> String {
    [
        live_row_sql("issue_comments"),
        workspace_slug_sql("issue_comments"),
        "issue_comments.project_id = :project_id".to_owned(),
        "issue_comments.issue_id = :issue_id".to_owned(),
        member_guard_sql("issue_comments"),
        project_live_sql("issue_comments"),
    ]
    .join(" AND ")
}

/// `is_member=Exists(...)` annotation (`:1815-1823`, `:1971-1979`) in
/// fixture predicate order: live row, active, member, project, slug.
/// Unlike [`member_guard_sql`], this explicit `ProjectMember.objects`
/// filter carries the manager's `deleted_at IS NULL`.
pub fn comment_is_member_sql() -> String {
    "EXISTS (SELECT 1 FROM project_members WHERE project_members.deleted_at IS NULL \
     AND project_members.is_active = TRUE AND project_members.member_id = :user \
     AND project_members.project_id = :project_id \
     AND project_members.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug))"
        .to_owned()
}

/// Full representative comment list/detail `SELECT`: scope +
/// `is_member` annotation + kwargs order (`:1824`) + `DISTINCT`
/// (`:1825`). `select_related("workspace", "project", "issue", "actor")`
/// (`:1814`, `:1970`) is a fetch hint — same rows.
pub fn comment_list_sql(order: &SubOrder) -> String {
    format!(
        "SELECT DISTINCT issue_comments.*, {} AS is_member FROM issue_comments WHERE {} ORDER BY {}",
        comment_is_member_sql(),
        comment_scope_where(),
        order.sql("issue_comments"),
    )
}

/// Detail predicate (`:2006`): scope + pk (the annotation still
/// applies — the detail chain annotates identically).
pub fn comment_detail_where() -> String {
    format!("{} AND issue_comments.id = :pk", comment_scope_where())
}

/// Comment `patch`/`delete` direct lookup (`:2035`, `:2117`):
/// `IssueComment.objects.get(workspace__slug, project_id, issue_id, pk)`
/// — ported bug 4: bypasses the queryset's member/archived guards
/// (and the `is_member` annotation).
pub fn comment_direct_lookup_where() -> String {
    [
        live_row_sql("issue_comments"),
        workspace_slug_sql("issue_comments"),
        "issue_comments.project_id = :project_id".to_owned(),
        "issue_comments.issue_id = :issue_id".to_owned(),
        "issue_comments.id = :pk".to_owned(),
    ]
    .join(" AND ")
}

/// Comment patch external-dedupe `exists()` (`:2044-2050`): same
/// project/slug/source/id → 409 `{"error": "Work item comment with the
/// same external id and external source already exists", "id": ...}`.
/// Ported asymmetry 8: no pk exclusion in SQL — the caller compares
/// `external_id` in Python first (`:2042-2043`).
pub fn comment_external_dedupe_where() -> String {
    [
        live_row_sql("issue_comments"),
        "issue_comments.project_id = :project_id".to_owned(),
        workspace_slug_sql("issue_comments"),
        "issue_comments.external_source = :external_source".to_owned(),
        "issue_comments.external_id = :external_id".to_owned(),
    ]
    .join(" AND ")
}

// ---------------------------------------------------------------------------
// Activities (views/issue.py:2156-2165 list, :2211-2224 detail)
// ---------------------------------------------------------------------------

/// `~Q(field__in=["comment", "vote", "reaction", "draft"])` (`:2159`,
/// `:2215`), rendered the way Django negates an `__in` over a nullable
/// column — fixture-verbatim, including the null guard.
pub fn activity_exclusion_sql() -> String {
    "NOT (issue_activities.field IN ('comment', 'vote', 'reaction', 'draft') \
     AND issue_activities.field IS NOT NULL)"
        .to_owned()
}

/// Activity list predicates in source order (`:2157-2163`): issue,
/// slug, project, field exclusion, member, live project.
pub fn activity_list_where() -> String {
    [
        live_row_sql("issue_activities"),
        "issue_activities.issue_id = :issue_id".to_owned(),
        workspace_slug_sql("issue_activities"),
        "issue_activities.project_id = :project_id".to_owned(),
        activity_exclusion_sql(),
        member_guard_sql("issue_activities"),
        project_live_sql("issue_activities"),
    ]
    .join(" AND ")
}

/// Full representative activity list `SELECT`: predicates + live
/// `request.GET` order (`:2165`, default ascending) and NO `DISTINCT`
/// — the only list chain here without one.
/// `select_related("actor", "workspace", "issue", "project")` (`:2164`)
/// is a fetch hint — same rows.
pub fn activity_list_sql(order: &SubOrder) -> String {
    format!(
        "SELECT issue_activities.* FROM issue_activities WHERE {} ORDER BY {}",
        activity_list_where(),
        order.sql("issue_activities"),
    )
}

/// Activity detail predicates (`:2212-2219`): list predicates + pk,
/// executed with `.order_by(...).first()` (`:2222-2223`, `LIMIT 1`);
/// empty renders [`ACTIVITY_NOT_FOUND_BODY`] (`:2226`).
pub fn activity_detail_where() -> String {
    format!("{} AND issue_activities.id = :pk", activity_list_where())
}

// ---------------------------------------------------------------------------
// Attachments (views/issue.py:2235-2653; no get_queryset anywhere)
// ---------------------------------------------------------------------------

/// Attachment list filter in source order (`:2438-2444`): issue,
/// entity type, slug, project, uploaded. Ported bug 1: NO member or
/// archived guard, unlike every sibling chain — and no `.order_by` or
/// `.distinct()`, so `Meta.ordering` (`-created_at`) applies.
/// The view returns an un-paginated `Response(serializer.data)`.
pub fn attachment_list_where() -> String {
    [
        live_row_sql("file_assets"),
        "file_assets.issue_id = :issue_id".to_owned(),
        format!("file_assets.entity_type = '{ISSUE_ATTACHMENT_ENTITY}'"),
        workspace_slug_sql("file_assets"),
        "file_assets.project_id = :project_id".to_owned(),
        "file_assets.is_uploaded = TRUE".to_owned(),
    ]
    .join(" AND ")
}

/// Effective attachment list order: `Meta.ordering = ("-created_at",)`
/// (`db/models/asset.py:68`) — the source calls no `.order_by`.
pub const ATTACHMENT_LIST_ORDER_SQL: &str = "file_assets.created_at DESC";

/// Full representative attachment list `SELECT`.
pub fn attachment_list_sql() -> String {
    format!(
        "SELECT file_assets.* FROM file_assets WHERE {} ORDER BY {ATTACHMENT_LIST_ORDER_SQL}",
        attachment_list_where(),
    )
}

/// Attachment post external-dedupe `exists()` (`:2359-2368`): same
/// project/slug/source/id/issue/entity → 409 `{"error": "Issue with
/// the same external id and external source already exists",
/// "id": ...}` (`:2377-2383`).
pub fn attachment_dedupe_where() -> String {
    [
        live_row_sql("file_assets"),
        "file_assets.project_id = :project_id".to_owned(),
        workspace_slug_sql("file_assets"),
        "file_assets.external_source = :external_source".to_owned(),
        "file_assets.external_id = :external_id".to_owned(),
        "file_assets.issue_id = :issue_id".to_owned(),
        format!("file_assets.entity_type = '{ISSUE_ATTACHMENT_ENTITY}'"),
    ]
    .join(" AND ")
}

/// Attachment detail lookup (`:2491`, `:2562`, `:2640`):
/// `FileAsset.objects.get(pk, workspace__slug, project_id)`.
pub fn attachment_detail_where() -> String {
    [
        live_row_sql("file_assets"),
        "file_assets.id = :pk".to_owned(),
        workspace_slug_sql("file_assets"),
        "file_assets.project_id = :project_id".to_owned(),
    ]
    .join(" AND ")
}

/// Attachment issue load for the permission check (`:2308`, `:2472`,
/// `:2626`): `Issue.objects.get(pk, workspace__slug, project_id)` —
/// note the plain manager (triage/archived/draft rows load here).
pub fn attachment_issue_where() -> String {
    [
        live_row_sql("issues"),
        "issues.id = :issue_id".to_owned(),
        workspace_slug_sql("issues"),
        "issues.project_id = :project_id".to_owned(),
    ]
    .join(" AND ")
}

// ---------------------------------------------------------------------------
// Relations: grouped read (views/issue.py:2965-3008) + merge (:3010-3019)
// ---------------------------------------------------------------------------

/// Grouped-read scope (`:2984-2987`): live rows mentioning the issue on
/// either side, in this workspace. Ported bug 2: NO `project_id`
/// predicate — cross-project rows aggregate into the response.
pub fn relation_scope_where() -> String {
    [
        live_row_sql("issue_relations"),
        "(issue_relations.issue_id = :issue_id OR issue_relations.related_issue_id = :issue_id)"
            .to_owned(),
        workspace_slug_sql("issue_relations"),
    ]
    .join(" AND ")
}

/// One `_agg_ids` arm (`:2974-2979`): distinct ids where the stored
/// type and the fixed side match, `COALESCE`d to the empty uuid array
/// (`:2971`).
pub fn relation_aggregate_arm(
    field: &str,
    relation_type: &str,
    fixed_side: &str,
    alias: &str,
) -> String {
    format!(
        "COALESCE(ARRAY_AGG(DISTINCT issue_relations.{field}) FILTER \
         (WHERE issue_relations.relation_type = '{relation_type}' \
         AND issue_relations.{fixed_side} = :issue_id), '{{}}'::uuid[]) AS {alias}"
    )
}

/// Full representative grouped aggregate `SELECT` (`:2989-3008`): all
/// ten arms in source order over the scope.
pub fn relation_aggregate_sql() -> String {
    let arms = [
        ("issue_id", "blocked_by", "related_issue_id", "blocking_ids"),
        (
            "related_issue_id",
            "blocked_by",
            "issue_id",
            "blocked_by_ids",
        ),
        ("related_issue_id", "duplicate", "issue_id", "duplicate_ids"),
        (
            "issue_id",
            "duplicate",
            "related_issue_id",
            "duplicate_ids_related",
        ),
        (
            "related_issue_id",
            "relates_to",
            "issue_id",
            "relates_to_ids",
        ),
        (
            "issue_id",
            "relates_to",
            "related_issue_id",
            "relates_to_ids_related",
        ),
        (
            "issue_id",
            "start_before",
            "related_issue_id",
            "start_after_ids",
        ),
        (
            "related_issue_id",
            "start_before",
            "issue_id",
            "start_before_ids",
        ),
        (
            "issue_id",
            "finish_before",
            "related_issue_id",
            "finish_after_ids",
        ),
        (
            "related_issue_id",
            "finish_before",
            "issue_id",
            "finish_before_ids",
        ),
    ]
    .into_iter()
    .map(|(field, relation_type, fixed_side, alias)| {
        relation_aggregate_arm(field, relation_type, fixed_side, alias)
    })
    .collect::<Vec<_>>()
    .join(", ");
    format!(
        "SELECT {arms} FROM issue_relations WHERE {}",
        relation_scope_where()
    )
}

/// Grouped-response keys in source order (`:3010-3019`).
pub const RELATION_RESPONSE_KEYS: &[&str] = &[
    "blocking",
    "blocked_by",
    "duplicate",
    "relates_to",
    "start_after",
    "start_before",
    "finish_after",
    "finish_before",
];

/// `list(set(forward + related))` (`:3015-3016`, ported bug 3):
/// direction-union membership with first-seen order. Python's
/// `set()` order is hash-based and unstable; only the membership is
/// contractual, and the fixture pins exactly that.
pub fn union_ids(first: &[uuid::Uuid], second: &[uuid::Uuid]) -> Vec<uuid::Uuid> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(first.len() + second.len());
    for id in first.iter().chain(second.iter()) {
        if seen.insert(*id) {
            out.push(*id);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Relations: create mapping + refetch (views/issue.py:3073-3132)
// ---------------------------------------------------------------------------

/// `get_actual_relation` (`utils/issue_relation_mapper.py:19-32`): the
/// stored type for a requested wire type. Unknown wires (including the
/// symmetric `duplicate`/`relates_to`) fall through unchanged
/// (`.get(relation_type, relation_type)`).
pub fn actual_relation(requested: &str) -> &str {
    match requested {
        "start_after" | "start_before" => "start_before",
        "finish_after" | "finish_before" => "finish_before",
        "blocking" | "blocked_by" => "blocked_by",
        "implemented_by" | "implements" => "implemented_by",
        other => other,
    }
}

/// The reverse-wire set (`:3077`): these swap the issue/related sides
/// on write (`:3082-3083`) and refetch (`:3113-3118`).
pub fn is_reverse_relation(requested: &str) -> bool {
    matches!(requested, "blocking" | "start_after" | "finish_after")
}

/// Post-create refetch filter (`:3111-3128`): the written pairs (sides
/// per `reverse`) + stored type + workspace slug, with
/// `select_related("issue__state", "related_issue__state")` (`:3127`;
/// fetch hint). Reverse renders `RelatedIssueSerializer`, forward
/// `IssueRelationSerializer` (`:3130`). (`bulk_create(...,
/// batch_size=10, ignore_conflicts=True)` `:3079-3092` is handler-owned.)
pub fn relation_refetch_where(reverse: bool) -> String {
    let pairs = if reverse {
        "issue_relations.issue_id IN (:issues) AND issue_relations.related_issue_id = :issue_id"
    } else {
        "issue_relations.issue_id = :issue_id AND issue_relations.related_issue_id IN (:issues)"
    };
    [
        live_row_sql("issue_relations"),
        pairs.to_owned(),
        "issue_relations.relation_type = :actual".to_owned(),
        workspace_slug_sql("issue_relations"),
    ]
    .join(" AND ")
}

// ---------------------------------------------------------------------------
// Workpad (views/issue.py:3269-3280 read, :3307-3321 patch lock)
// ---------------------------------------------------------------------------

/// `IssueManager.get_queryset` (`db/models/issue.py:95-103`): the four
/// excludes, rendered the way Django negates them — fixture-verbatim.
/// The triage exclude is why the workpad selects carry a `LEFT OUTER
/// JOIN states` (nullable FK) while every other unit here joins inward.
pub fn issue_manager_where() -> String {
    [
        "NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL)".to_owned(),
        "NOT (issues.archived_at IS NOT NULL)".to_owned(),
        "NOT (projects.archived_at IS NOT NULL)".to_owned(),
        "NOT (issues.is_draft)".to_owned(),
    ]
    .join(" AND ")
}

/// Workpad scope in fixture SQL order (`:3269-3273`): live row, manager
/// excludes, project, slug. (The source calls `filter(slug, project)`
/// — Django renders the project predicate first; the slug form is
/// [`workspace_slug_sql`].)
pub fn workpad_scope_where() -> String {
    [
        live_row_sql("issues"),
        issue_manager_where(),
        "issues.project_id = :project_id".to_owned(),
        workspace_slug_sql("issues"),
    ]
    .join(" AND ")
}

/// The manager join spine the scope predicates require, in fixture
/// order: nullable state (outer), project, workspace (inner).
pub fn workpad_joins_sql() -> String {
    "FROM issues LEFT OUTER JOIN states ON (issues.state_id = states.id) \
     INNER JOIN projects ON (issues.project_id = projects.id) \
     INNER JOIN workspaces ON (issues.workspace_id = workspaces.id)"
        .to_owned()
}

/// Effective workpad order: `Meta.ordering = ("-created_at",)`
/// (`db/models/issue.py:254`) — the queryset calls no `.order_by`.
pub const WORKPAD_LIST_ORDER_SQL: &str = "issues.created_at DESC";

/// Full representative workpad `SELECT` (unevaluated queryset form,
/// fixture `workpad_get_queryset`): joins + scope + Meta order.
pub fn workpad_list_sql() -> String {
    format!(
        "SELECT issues.* {} WHERE {} ORDER BY {WORKPAD_LIST_ORDER_SQL}",
        workpad_joins_sql(),
        workpad_scope_where(),
    )
}

/// Workpad `get` predicate (`:3276-3278`): scope + pk.
pub fn workpad_get_where() -> String {
    format!("{} AND issues.id = :pk", workpad_scope_where())
}

/// Executed patch-lock `SELECT` (`:3315-3317`, fixture
/// `workpad_patch_lock`): the `get` form plus `LIMIT 21` (executed
/// `.get()`) plus `FOR UPDATE OF issues` (`select_for_update(of=
/// ("self",))`). The `OF issues` scope is load-bearing, not cosmetic:
/// a bare `FOR UPDATE` would try to lock the nullable side of the
/// state outer join and Postgres rejects it — see the source comment
/// `:3298-3306`.
pub fn workpad_lock_sql() -> String {
    format!(
        "SELECT issues.* {} WHERE {} LIMIT 21 FOR UPDATE OF issues",
        workpad_joins_sql(),
        workpad_get_where(),
    )
}

// ---------------------------------------------------------------------------
// PR links (views/github_pr.py:37-49 list, :86-95 detail)
// ---------------------------------------------------------------------------

/// PR-link list scope in source order (`:38-45`): slug, project,
/// issue, member, live project.
pub fn pr_scope_where() -> String {
    [
        live_row_sql("github_pull_request_links"),
        workspace_slug_sql("github_pull_request_links"),
        "github_pull_request_links.project_id = :project_id".to_owned(),
        "github_pull_request_links.issue_id = :issue_id".to_owned(),
        member_guard_sql("github_pull_request_links"),
        project_live_sql("github_pull_request_links"),
    ]
    .join(" AND ")
}

/// Full representative PR-link list `SELECT`: scope + kwargs order
/// (`:46`) + `DISTINCT` (`:47`).
pub fn pr_list_sql(order: &SubOrder) -> String {
    format!(
        "SELECT DISTINCT github_pull_request_links.* FROM github_pull_request_links WHERE {} ORDER BY {}",
        pr_scope_where(),
        order.sql("github_pull_request_links"),
    )
}

/// PR-link detail scope (`:87-94`, ported bug 5): slug, project,
/// issue, member — NO archived guard, and the chain calls neither
/// `.order_by` nor `.distinct()`, so `Meta.ordering` applies.
pub fn pr_detail_scope_where() -> String {
    [
        live_row_sql("github_pull_request_links"),
        workspace_slug_sql("github_pull_request_links"),
        "github_pull_request_links.project_id = :project_id".to_owned(),
        "github_pull_request_links.issue_id = :issue_id".to_owned(),
        member_guard_sql("github_pull_request_links"),
    ]
    .join(" AND ")
}

/// Effective PR-link detail order: `Meta.ordering = ("-created_at",)`
/// (`db/models/integration/github.py:252`).
pub const PR_DETAIL_ORDER_SQL: &str = "github_pull_request_links.created_at DESC";

/// Full representative PR-link detail `SELECT` (queryset form, fixture
/// `github_pr_detail_queryset`): scope + Meta order, no `DISTINCT`.
/// `delete` executes `.get(pk)` (`:95`) on top.
pub fn pr_detail_sql() -> String {
    format!(
        "SELECT github_pull_request_links.* FROM github_pull_request_links WHERE {} ORDER BY {PR_DETAIL_ORDER_SQL}",
        pr_detail_scope_where(),
    )
}

/// PR-link `delete` predicate (`:95`): detail scope + pk.
pub fn pr_detail_where() -> String {
    format!(
        "{} AND github_pull_request_links.id = :pk",
        pr_detail_scope_where()
    )
}

// ---------------------------------------------------------------------------
// Review links (views/git_code_review.py:32-44 list, :86-95 detail)
// ---------------------------------------------------------------------------

/// Review-link list scope in source order (`:33-40`): slug, project,
/// issue, member, live project.
pub fn review_scope_where() -> String {
    [
        live_row_sql("git_code_review_links"),
        workspace_slug_sql("git_code_review_links"),
        "git_code_review_links.project_id = :project_id".to_owned(),
        "git_code_review_links.issue_id = :issue_id".to_owned(),
        member_guard_sql("git_code_review_links"),
        project_live_sql("git_code_review_links"),
    ]
    .join(" AND ")
}

/// Full representative review-link list `SELECT`: scope + kwargs order
/// (`:41`) + `DISTINCT` (`:42`).
pub fn review_list_sql(order: &SubOrder) -> String {
    format!(
        "SELECT DISTINCT git_code_review_links.* FROM git_code_review_links WHERE {} ORDER BY {}",
        review_scope_where(),
        order.sql("git_code_review_links"),
    )
}

/// Review-link detail scope (`:87-94`, ported bug 5): slug, project,
/// issue, member — NO archived guard, and the chain calls neither
/// `.order_by` nor `.distinct()`, so `Meta.ordering` applies.
pub fn review_detail_scope_where() -> String {
    [
        live_row_sql("git_code_review_links"),
        workspace_slug_sql("git_code_review_links"),
        "git_code_review_links.project_id = :project_id".to_owned(),
        "git_code_review_links.issue_id = :issue_id".to_owned(),
        member_guard_sql("git_code_review_links"),
    ]
    .join(" AND ")
}

/// Effective review-link detail order: `Meta.ordering =
/// ("-created_at",)` (`db/models/integration/git.py:259`).
pub const REVIEW_DETAIL_ORDER_SQL: &str = "git_code_review_links.created_at DESC";

/// Full representative review-link detail `SELECT` (queryset form,
/// fixture `code_review_detail_queryset`): scope + Meta order, no
/// `DISTINCT`. `delete` executes `.get(pk)` (`:95`) on top.
pub fn review_detail_sql() -> String {
    format!(
        "SELECT git_code_review_links.* FROM git_code_review_links WHERE {} ORDER BY {REVIEW_DETAIL_ORDER_SQL}",
        review_detail_scope_where(),
    )
}

/// Review-link `delete` predicate (`:95`): detail scope + pk.
pub fn review_detail_where() -> String {
    format!(
        "{} AND git_code_review_links.id = :pk",
        review_detail_scope_where()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/queries/F18-07.subresources.json"
    );

    fn fixture() -> serde_json::Value {
        let raw = std::fs::read_to_string(FIXTURE).expect("F18-07 fixture exists");
        serde_json::from_str(&raw).expect("F18-07 is valid JSON")
    }

    fn unit(fixture: &serde_json::Value, key: &str) -> serde_json::Value {
        fixture["units"][key].clone()
    }

    fn unit_str(fixture: &serde_json::Value, key: &str, field: &str) -> String {
        fixture["units"][key][field]
            .as_str()
            .unwrap_or_else(|| panic!("{key}.{field} is a string"))
            .to_owned()
    }

    fn default_order() -> SubOrder {
        parse_order(None, QUERYSET_ORDER_DEFAULT)
    }

    #[test]
    fn fixture_source_pointers_match_ported_lines() {
        let fixture = fixture();
        assert_eq!(fixture["fixture"], "F18-07");
        // Every unit names the exact Python lines this module ports; if a
        // re-record moves them, the builders' line pointers are stale too.
        let pointers = [
            ("label_list_queryset", "api/views/issue.py:1322-1336"),
            ("link_list_queryset", "api/views/issue.py:1557-1569"),
            ("link_detail_queryset", "api/views/issue.py:1662-1680"),
            ("comment_list_queryset", "api/views/issue.py:1805-1828"),
            ("activity_list_queryset", "api/views/issue.py:2156-2165"),
            ("attachment_list_queryset", "api/views/issue.py:2438-2444"),
            (
                "relation_grouped_aggregation",
                "api/views/issue.py:2971-3008",
            ),
            (
                "relation_type_mapping",
                "utils/issue_relation_mapper.py:19-32",
            ),
            ("workpad_get_queryset", "api/views/issue.py:3269-3273"),
            ("workpad_patch_lock", "api/views/issue.py:3307-3321"),
            ("github_pr_list_queryset", "api/views/github_pr.py:37-49"),
            ("github_pr_detail_queryset", "api/views/github_pr.py:86-95"),
            (
                "code_review_list_queryset",
                "api/views/git_code_review.py:32-44",
            ),
            (
                "code_review_detail_queryset",
                "api/views/git_code_review.py:86-95",
            ),
        ];
        for (key, needle) in pointers {
            let source = unit_str(&fixture, key, "source");
            assert!(source.contains(needle), "{key} moved: {source}");
        }
    }

    #[test]
    fn order_parse_dash_default_and_passthrough() {
        // Absent kwarg/param → default.
        assert_eq!(
            parse_order(None, QUERYSET_ORDER_DEFAULT),
            SubOrder {
                column: "created_at".to_owned(),
                descending: true,
            }
        );
        assert_eq!(
            parse_order(None, ACTIVITY_ORDER_DEFAULT),
            SubOrder {
                column: "created_at".to_owned(),
                descending: false,
            }
        );
        // Leading dash selects descending, exactly like Django.
        assert_eq!(default_order().sql("labels"), "labels.created_at DESC");
        let asc = parse_order(Some("created_at"), QUERYSET_ORDER_DEFAULT);
        assert_eq!(
            asc.sql("issue_activities"),
            "issue_activities.created_at ASC"
        );
        // Unknown/empty columns pass through untouched (FieldError parity).
        let weird = parse_order(Some("-nope"), QUERYSET_ORDER_DEFAULT);
        assert_eq!(weird.sql("labels"), "labels.nope DESC");
        let empty = parse_order(Some(""), QUERYSET_ORDER_DEFAULT);
        assert!(!empty.descending);
    }

    #[test]
    fn label_scope_guards_in_source_order() {
        let scope = label_scope_where();
        let live = scope.find("labels.deleted_at IS NULL").expect("manager");
        let slug = scope.find("slug = :slug").expect("slug");
        let project = scope
            .find("labels.project_id = :project_id")
            .expect("project");
        let member = scope.find("pm.member_id = :user").expect("member");
        let archived = scope
            .find("projects.archived_at IS NULL")
            .expect("archived");
        assert!(live < slug && slug < project && project < member && member < archived);
        assert!(scope.contains("pm.is_active = TRUE"));
        // Join-span guard carries no deleted_at (fixture-verbatim).
        assert!(!member_guard_sql("labels").contains("deleted_at"));

        let sql = label_list_sql(&default_order());
        assert!(sql.starts_with("SELECT DISTINCT labels.*"));
        assert!(sql.ends_with("ORDER BY labels.created_at DESC"));
        assert!(label_detail_where().ends_with("AND labels.id = :pk"));

        // Fixture SQL agrees: DISTINCT + DESC + archived guard.
        let fixture = fixture();
        let recorded = unit_str(&fixture, "label_list_queryset", "sql");
        assert!(recorded.starts_with("SELECT DISTINCT"));
        assert!(recorded.contains("ORDER BY \"labels\".\"created_at\" DESC"));
        assert!(recorded.contains("\"projects\".\"archived_at\" IS NULL"));
    }

    #[test]
    fn link_chains_identical_and_direct_lookup_skips_guards() {
        let order = default_order();
        assert_eq!(link_list_sql(&order), link_detail_sql(&order));
        assert!(link_detail_where().ends_with("AND issue_links.id = :pk"));

        // Ported bug 4: patch/delete direct .get() skips member + archived.
        let direct = link_direct_lookup_where();
        assert!(!direct.contains("project_members"));
        assert!(!direct.contains("archived_at"));
        assert!(direct.contains("issue_links.issue_id = :issue_id"));

        let fixture = fixture();
        let list = unit_str(&fixture, "link_list_queryset", "sql");
        let detail = unit_str(&fixture, "link_detail_queryset", "sql");
        assert_eq!(list, detail);
        assert!(list.starts_with("SELECT DISTINCT"));
    }

    #[test]
    fn comment_annotation_lists_member_guards() {
        let member = comment_is_member_sql();
        for needle in [
            "project_members.deleted_at IS NULL",
            "project_members.is_active = TRUE",
            "project_members.member_id = :user",
            "project_members.project_id = :project_id",
            "slug = :slug",
        ] {
            assert!(member.contains(needle), "missing {needle}");
        }
        let sql = comment_list_sql(&default_order());
        assert!(sql.contains("AS is_member"));
        assert!(sql.starts_with("SELECT DISTINCT issue_comments.*"));
        assert!(sql.ends_with("ORDER BY issue_comments.created_at DESC"));

        // Ported bug 4 also holds for comments.
        let direct = comment_direct_lookup_where();
        assert!(!direct.contains("project_members"));
        assert!(!direct.contains("archived_at"));

        let fixture = fixture();
        let recorded = unit_str(&fixture, "comment_list_queryset", "sql");
        assert!(recorded.contains("AS \"is_member\""));
        assert!(recorded.starts_with("SELECT DISTINCT"));
    }

    #[test]
    fn activity_exclusion_order_and_no_distinct() {
        // Fixture-verbatim negation incl. the null guard.
        assert_eq!(
            activity_exclusion_sql(),
            "NOT (issue_activities.field IN ('comment', 'vote', 'reaction', 'draft') \
             AND issue_activities.field IS NOT NULL)"
        );
        assert_eq!(
            ACTIVITY_EXCLUDED_FIELDS,
            &["comment", "vote", "reaction", "draft"]
        );
        let sql = activity_list_sql(&parse_order(None, ACTIVITY_ORDER_DEFAULT));
        assert!(sql.starts_with("SELECT issue_activities.*"));
        assert!(!sql.contains("DISTINCT"));
        assert!(sql.ends_with("ORDER BY issue_activities.created_at ASC"));
        assert!(activity_detail_where().ends_with("AND issue_activities.id = :pk"));
        assert_eq!(
            ACTIVITY_NOT_FOUND_BODY,
            "{\"message\":\"Activity not found.\",\"code\":\"NOT_FOUND\"}"
        );

        let fixture = fixture();
        let recorded = unit_str(&fixture, "activity_list_queryset", "sql");
        assert!(recorded.starts_with("SELECT \"issue_activities\""));
        assert!(recorded.contains("ORDER BY \"issue_activities\".\"created_at\" ASC"));
        assert!(recorded
            .contains("NOT (\"issue_activities\".\"field\" IN (comment, vote, reaction, draft)"));
    }

    #[test]
    fn attachment_skips_guards_and_uses_meta_order() {
        // Ported bug 1: no member/archived guard anywhere on the list.
        let scope = attachment_list_where();
        assert!(!scope.contains("project_members"));
        assert!(!scope.contains("archived_at"));
        assert!(scope.contains("file_assets.entity_type = 'ISSUE_ATTACHMENT'"));
        assert!(scope.contains("file_assets.is_uploaded = TRUE"));
        assert_eq!(ATTACHMENT_LIST_ORDER_SQL, "file_assets.created_at DESC");
        let sql = attachment_list_sql();
        assert!(!sql.contains("DISTINCT"));
        assert!(sql.ends_with("ORDER BY file_assets.created_at DESC"));

        let dedupe = attachment_dedupe_where();
        assert!(dedupe.contains("file_assets.external_source = :external_source"));
        assert!(dedupe.contains("file_assets.external_id = :external_id"));
        assert!(dedupe.contains("file_assets.issue_id = :issue_id"));
        let detail = attachment_detail_where();
        assert!(detail.contains("file_assets.id = :pk"));
        // Plain manager on the issue load (no triage/archived/draft guard).
        let issue = attachment_issue_where();
        assert!(!issue.contains("states."));
        assert!(!issue.contains("is_draft"));

        let fixture = fixture();
        let recorded = unit_str(&fixture, "attachment_list_queryset", "sql");
        assert!(!recorded.contains("project_members"));
        assert!(!recorded.contains("archived_at"));
        assert!(recorded.contains("ORDER BY \"file_assets\".\"created_at\" DESC"));
    }

    #[test]
    fn relation_aggregate_arms_scope_and_keys() {
        // Ported bug 2: scope has no project predicate.
        let scope = relation_scope_where();
        assert!(!scope.contains("project_id"));
        assert!(scope.contains(
            "(issue_relations.issue_id = :issue_id OR issue_relations.related_issue_id = :issue_id)"
        ));

        let sql = relation_aggregate_sql();
        let aliases = [
            "blocking_ids",
            "blocked_by_ids",
            "duplicate_ids",
            "duplicate_ids_related",
            "relates_to_ids",
            "relates_to_ids_related",
            "start_after_ids",
            "start_before_ids",
            "finish_after_ids",
            "finish_before_ids",
        ];
        let mut cursor = 0;
        for alias in aliases {
            let at = sql[cursor..]
                .find(alias)
                .unwrap_or_else(|| panic!("arm {alias}"));
            cursor += at + alias.len();
        }
        assert!(sql.contains("ARRAY_AGG(DISTINCT issue_relations.issue_id)"));
        assert!(sql.contains("'{}'::uuid[]"));
        // Ported bug 9: the raw wire type matches no arm.
        assert!(!sql.contains("relation_type = 'blocking'"));

        assert_eq!(
            RELATION_RESPONSE_KEYS,
            &[
                "blocking",
                "blocked_by",
                "duplicate",
                "relates_to",
                "start_after",
                "start_before",
                "finish_after",
                "finish_before",
            ]
        );

        let fixture = fixture();
        let recorded = unit_str(&fixture, "relation_grouped_aggregation", "sql");
        for alias in aliases {
            assert!(recorded.contains(alias), "fixture arm {alias}");
        }
        assert!(!recorded.contains("project_id"));
    }

    #[test]
    fn relation_mapping_and_refetch_match_fixture() {
        let fixture = fixture();
        let mapping = unit(&fixture, "relation_type_mapping");
        for (wire, stored) in [
            ("blocking", "blocked_by"),
            ("blocked_by", "blocked_by"),
            ("duplicate", "duplicate"),
            ("relates_to", "relates_to"),
            ("start_before", "start_before"),
            ("start_after", "start_before"),
            ("finish_before", "finish_before"),
            ("finish_after", "finish_before"),
            ("implemented_by", "implemented_by"),
            ("implements", "implemented_by"),
        ] {
            assert_eq!(mapping["mapping"][wire], stored, "fixture {wire}");
            assert_eq!(actual_relation(wire), stored, "builder {wire}");
        }
        // Unknown wires fall through (.get default).
        assert_eq!(actual_relation("relates_to"), "relates_to");
        assert_eq!(actual_relation("whatever"), "whatever");

        let reverse: Vec<&str> = mapping["is_reverse"]
            .as_array()
            .expect("reverse array")
            .iter()
            .map(|v| v.as_str().expect("wire str"))
            .collect();
        assert_eq!(reverse, vec!["blocking", "start_after", "finish_after"]);
        for wire in ["blocking", "start_after", "finish_after"] {
            assert!(is_reverse_relation(wire));
        }
        for wire in ["blocked_by", "duplicate", "start_before", "finish_before"] {
            assert!(!is_reverse_relation(wire));
        }

        let fwd = relation_refetch_where(false);
        assert!(fwd.contains("issue_relations.issue_id = :issue_id"));
        assert!(fwd.contains("issue_relations.related_issue_id IN (:issues)"));
        assert!(fwd.contains("issue_relations.relation_type = :actual"));
        let rev = relation_refetch_where(true);
        assert!(rev.contains("issue_relations.issue_id IN (:issues)"));
        assert!(rev.contains("issue_relations.related_issue_id = :issue_id"));
    }

    #[test]
    fn union_ids_dedupes_membership() {
        let a = uuid::Uuid::parse_str("25ace52e-c64d-4043-a700-911b1e42bffc").expect("uuid");
        let b = uuid::Uuid::parse_str("2143bf91-62b6-45b6-8fe0-702122483419").expect("uuid");
        let merged = union_ids(&[a, b], &[b, a]);
        assert_eq!(merged.len(), 2);
        assert!(merged.contains(&a) && merged.contains(&b));
        assert!(union_ids(&[], &[]).is_empty());
    }

    #[test]
    fn workpad_manager_scope_lock_and_bodies() {
        let manager = issue_manager_where();
        assert!(manager.contains("states.\"group\" = 'triage'"));
        assert!(manager.contains("NOT (issues.archived_at IS NOT NULL)"));
        assert!(manager.contains("NOT (projects.archived_at IS NOT NULL)"));
        assert!(manager.contains("NOT (issues.is_draft)"));

        let joins = workpad_joins_sql();
        assert!(joins.contains("LEFT OUTER JOIN states ON (issues.state_id = states.id)"));
        assert!(joins.contains("INNER JOIN projects ON (issues.project_id = projects.id)"));
        assert!(joins.contains("INNER JOIN workspaces ON (issues.workspace_id = workspaces.id)"));

        let list = workpad_list_sql();
        assert!(list.ends_with("ORDER BY issues.created_at DESC"));
        assert!(!list.contains("DISTINCT"));
        assert!(workpad_get_where().ends_with("AND issues.id = :pk"));

        let lock = workpad_lock_sql();
        assert!(lock.ends_with("LIMIT 21 FOR UPDATE OF issues"));
        assert!(lock.contains("issues.id = :pk"));

        assert_eq!(
            WORKPAD_MISSING_BODY_MESSAGE,
            "PATCH requires a `body` field in the request payload."
        );
        assert_eq!(WORKPAD_MISSING_BODY_STATUS, 400);

        let fixture = fixture();
        let recorded = unit_str(&fixture, "workpad_get_queryset", "sql");
        assert!(recorded.contains("LEFT OUTER JOIN \"states\""));
        assert!(recorded.contains("ORDER BY \"issues\".\"created_at\" DESC"));
        let lock_sql = unit_str(&fixture, "workpad_patch_lock", "lock_sql");
        assert!(lock_sql.ends_with("LIMIT 21 FOR UPDATE OF \"issues\""));
    }

    #[test]
    fn pr_and_review_list_detail_shapes() {
        let order = default_order();
        for (table, list_sql, detail_sql, detail_where, detail_order) in [
            (
                "github_pull_request_links",
                pr_list_sql(&order),
                pr_detail_sql(),
                pr_detail_where(),
                PR_DETAIL_ORDER_SQL,
            ),
            (
                "git_code_review_links",
                review_list_sql(&order),
                review_detail_sql(),
                review_detail_where(),
                REVIEW_DETAIL_ORDER_SQL,
            ),
        ] {
            // List: archived guard + DISTINCT + kwargs order.
            assert!(list_sql.starts_with(&format!("SELECT DISTINCT {table}.*")));
            assert!(list_sql.contains("projects.archived_at IS NULL"));
            assert!(list_sql.ends_with(&format!("ORDER BY {table}.created_at DESC")));
            // Detail (ported bug 5): no archived guard, no DISTINCT, Meta order.
            assert!(!detail_sql.contains("DISTINCT"));
            assert!(!detail_sql.contains("archived_at"));
            assert!(detail_sql.ends_with(&format!("ORDER BY {detail_order}")));
            assert!(detail_sql.contains("pm.member_id = :user"));
            assert!(detail_where.ends_with(&format!("AND {table}.id = :pk")));
        }
        assert_eq!(
            PR_DETAIL_ORDER_SQL,
            "github_pull_request_links.created_at DESC"
        );
        assert_eq!(
            REVIEW_DETAIL_ORDER_SQL,
            "git_code_review_links.created_at DESC"
        );

        let fixture = fixture();
        let pr_list = unit_str(&fixture, "github_pr_list_queryset", "sql");
        assert!(pr_list.starts_with("SELECT DISTINCT"));
        assert!(pr_list.contains("\"projects\".\"archived_at\" IS NULL"));
        let pr_detail = unit_str(&fixture, "github_pr_detail_queryset", "sql");
        assert!(pr_detail.starts_with("SELECT \"github_pull_request_links\""));
        assert!(!pr_detail.contains("archived_at"));
        let review_list = unit_str(&fixture, "code_review_list_queryset", "sql");
        assert!(review_list.starts_with("SELECT DISTINCT"));
        let review_detail = unit_str(&fixture, "code_review_detail_queryset", "sql");
        assert!(!review_detail.contains("archived_at"));
    }

    #[test]
    fn fixture_rows_shape_and_counts() {
        let fixture = fixture();
        // (unit key, expected rows, keys every row carries)
        let shapes = [
            ("label_list_queryset", 2, vec!["id", "name", "color"]),
            ("link_list_queryset", 2, vec!["id", "title", "url"]),
            ("link_detail_queryset", 2, vec!["id", "title"]),
            (
                "comment_list_queryset",
                1,
                vec!["id", "comment_stripped", "is_member"],
            ),
            ("activity_list_queryset", 5, vec!["id", "field", "verb"]),
            (
                "attachment_list_queryset",
                5,
                vec!["id", "asset", "is_uploaded"],
            ),
            ("workpad_get_queryset", 5, vec!["id", "name"]),
            (
                "github_pr_list_queryset",
                1,
                vec!["id", "repo_owner", "repo_name", "pr_number"],
            ),
            (
                "code_review_list_queryset",
                1,
                vec!["id", "provider", "url"],
            ),
        ];
        for (key, count, keys) in shapes {
            let rows = unit(&fixture, key)["rows"]
                .as_array()
                .unwrap_or_else(|| panic!("{key} rows array"))
                .clone();
            assert_eq!(rows.len(), count, "{key} row count");
            for row in &rows {
                for k in &keys {
                    assert!(row.get(k).is_some(), "{key} row missing {k}");
                }
            }
        }
        // Detail querysets for PR/review record SQL only (no rows).
        assert!(unit(&fixture, "github_pr_detail_queryset")
            .get("rows")
            .is_none());
        assert!(unit(&fixture, "code_review_detail_queryset")
            .get("rows")
            .is_none());

        // Aggregate result: one blocking id, one relates_to id, rest empty.
        let agg = unit(&fixture, "relation_grouped_aggregation")["aggregate_result"].clone();
        assert_eq!(agg["blocking_ids"].as_array().expect("array").len(), 1);
        assert_eq!(agg["relates_to_ids"].as_array().expect("array").len(), 1);
        for key in [
            "blocked_by_ids",
            "duplicate_ids",
            "duplicate_ids_related",
            "relates_to_ids_related",
            "start_after_ids",
            "start_before_ids",
            "finish_after_ids",
            "finish_before_ids",
        ] {
            assert!(
                agg[key].as_array().expect("array").is_empty(),
                "{key} empty"
            );
        }
        // Comment row carries the is_member annotation value.
        let comment_rows = unit(&fixture, "comment_list_queryset")["rows"]
            .as_array()
            .expect("comment rows")
            .clone();
        assert_eq!(comment_rows[0]["is_member"], "True");
    }

    #[test]
    fn dedupe_wheres_keep_the_exclusion_asymmetry() {
        // Ported asymmetry 8: label excludes the pk in SQL, comment does not.
        assert!(label_external_dedupe_where().contains("labels.id != :pk"));
        assert!(!comment_external_dedupe_where().contains(":pk"));
        assert!(!comment_external_dedupe_where().contains("!="));
    }
}
