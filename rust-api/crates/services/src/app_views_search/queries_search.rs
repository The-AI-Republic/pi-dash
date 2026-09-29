//! D-29 search query builders: global, entity and issue-search SQL.
//!
//! Port of the query construction in
//! `apps/api/pi_dash/app/views/search/base.py` (`GlobalSearchEndpoint`
//! `:54-253`, `SearchEndpoint.get` `:293-691`) and
//! `apps/api/pi_dash/app/views/search/issue.py` (`IssueSearchEndpoint`
//! `:24-166`). The FTS predicate itself lives in [`super::fts`]; this
//! module scopes it per endpoint, adds the per-entity `icontains` filters,
//! the guest `created_by` scoping and the caps.
//!
//! Output is SQL text with `$N` bind placeholders plus an ordered
//! [`SqlParam`] list (the `ORDER BY` precedent is
//! `app_issues::ordering::OrderSpec`: fragments the handler splices and
//! binds). Table/column names are the Django physical names verified
//! against Django 4.2.30's own compiler output for every section below.
//!
//! Faithfulness notes (all verified by compiling the real querysets):
//!
//! * Every section orders by `<root>.created_at DESC`, including the global
//!   issue section — that ordering comes from the model `Meta`, not from an
//!   explicit `order_by()` call, but it is in the emitted SQL all the same.
//! * `DISTINCT` collapses: repeated `.distinct()` calls set one flag.
//!   Extra `created_at` selects ride along for `DISTINCT`+`ORDER BY`
//!   compliance; `values()` dicts only carry the documented `*_KEYS` — row
//!   shaping (handler 276) drops the extra column.
//! * Traversal scopes (`project__project_projectmember__…`) do NOT pick up
//!   the member table's soft-delete predicate — only queries rooted
//!   directly on a member model (`user_mention` branches) filter
//!   `<members>.deleted_at IS NULL`. Likewise the entity-project branch has
//!   neither `is_active` nor a member soft-delete arm (source has neither).
//! * `icontains` renders as `… ILIKE '%' || <p> || '%' ESCAPE '\'` with the
//!   pattern bound through `escape_icontains` — the crate's single LIKE
//!   spelling (see [`super::fts`]).
//!
//! Ported bugs (translate, don't redesign — listed in the PR):
//!
//! * B5 (`views/search/issue.py:77-80`): `filter_root_issues_only` reads
//!   `issue.parent` outside the `if issue:` guard, so an unknown `issue_id`
//!   raises `AttributeError` → 500. [`IssueSearchError::UnknownIssue`]
//!   marks that path so the handler 500s instead of returning root issues.
//! * B7 (`views/search/base.py:230`): `filter_intakes` uses `Issue.objects`
//!   instead of `Issue.issue_objects`, so triage/draft/archived issues can
//!   appear in the intake section. [`global_intakes`] keeps the soft-delete
//!   arm only.
//! * B8 (`views/search/base.py:297`): `count = int(…)` is unguarded —
//!   non-numeric (or negative-slice) counts 500 instead of 400.
//!   [`parse_count`] accepts exactly Python `int()` inputs and the handler
//!   maps `Err` (and negative values) to 500.
//! * B9 (`views/search/base.py:174-189`): the pages `ArrayAgg` filter
//!   `~Q(projects__id=True)` coerces `True` to `uuid(int=1)`, i.e.
//!   `FILTER (WHERE NOT (project_id = '00000000-…-000000000001'))` — a
//!   no-op for real data. [`global_pages`] emits the coerced literal.

use super::fts::{escape_like, search_applies, search_filter_sql, sequence_tokens};

// ---------------------------------------------------------------------------
// Bindings
// ---------------------------------------------------------------------------

/// A bound parameter, in `$N` order.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlParam {
    /// Text bind (`slug`, raw FTS query, escaped `LIKE` pattern, timestamps).
    Text(String),
    /// Integer bind (`status`, counts).
    Integer(i64),
    /// Integer-array bind (sequence-id tokens, `UNNEST($N::int[])`).
    IntegerArray(Vec<i64>),
    /// UUID bind (user, project, module ids).
    Uuid(String),
}

/// Threads `$N` placeholders and collects [`SqlParam`]s in order.
#[derive(Debug, Default)]
pub struct Builder {
    params: Vec<SqlParam>,
}

impl Builder {
    /// New empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a text value, returning its placeholder.
    pub fn text(&mut self, value: impl Into<String>) -> String {
        self.params.push(SqlParam::Text(value.into()));
        self.placeholder()
    }

    /// Bind an integer value, returning its placeholder.
    pub fn int(&mut self, value: i64) -> String {
        self.params.push(SqlParam::Integer(value));
        self.placeholder()
    }

    /// Bind an integer-array value, returning its placeholder.
    pub fn ints(&mut self, values: Vec<i64>) -> String {
        self.params.push(SqlParam::IntegerArray(values));
        self.placeholder()
    }

    /// Bind a UUID value, returning its placeholder.
    pub fn uuid(&mut self, value: impl Into<String>) -> String {
        self.params.push(SqlParam::Uuid(value.into()));
        self.placeholder()
    }

    fn placeholder(&self) -> String {
        format!("${}", self.params.len())
    }

    /// Finish `sql` with the collected params, in `$N` order.
    pub fn finish(self, sql: String) -> BuiltQuery {
        BuiltQuery {
            sql,
            params: self.params,
        }
    }
}

/// A complete query: SQL text plus ordered bind params.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltQuery {
    /// SQL with `$N` placeholders.
    pub sql: String,
    /// Bind values in placeholder order.
    pub params: Vec<SqlParam>,
}

// ---------------------------------------------------------------------------
// Param parsing (query-string kernels)
// ---------------------------------------------------------------------------

/// Python truthiness for optional query params: `None` (absent/`False`
/// default) or `""` both mean "not given". Every `if <param>:` gate in the
/// three views funnels through here.
pub fn opt(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

/// Flag params (`parent`, `issue_relation`, `sub_issue`, `cycle`): only the
/// string `"true"` activates (`issue.py:128-137`).
pub fn parse_flag(value: &str) -> bool {
    value == "true"
}

/// `target_date` activates only on the string `"none"` (`issue.py:97-102`;
/// the default is boolean `True`, which never equals `"none"`).
pub fn parse_target_date_none(value: Option<&str>) -> bool {
    value == Some("none")
}

/// Project narrow shared by the global sections and the issue pipeline:
/// only when `workspace_search == "false"` AND `project_id` is truthy
/// (`base.py:99-100`, `issue.py:122-123`).
pub fn project_narrow<'a>(workspace_search: &str, project_id: Option<&'a str>) -> Option<&'a str> {
    if workspace_search == "false" {
        opt(project_id)
    } else {
        None
    }
}

/// Error for [`parse_count`]; the handler maps it to 500 (B8), never 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountError {
    /// `int()` raised `ValueError`.
    Invalid,
}

/// `count = int(query_params.get("count", 5))` (`base.py:297`), B8 port:
/// accepts exactly what CPython `int()` accepts (surrounding whitespace,
/// optional sign, underscores between digits) and rejects everything else.
/// A negative value parses fine here — Django then 500s with
/// `AssertionError: Negative indexing is not supported` on `qs[:count]` —
/// so the handler must 500 on `Ok(n) if n < 0` as well.
pub fn parse_count(raw: Option<&str>) -> Result<i64, CountError> {
    let text = raw.unwrap_or("5");
    py_int(text).ok_or(CountError::Invalid)
}

/// CPython `int()` over a string: strip whitespace, optional `+`/`-`,
/// ASCII digits with single underscores between digits (leading, trailing
/// or doubled underscores fail; empty digit runs fail).
fn py_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (sign, digits) = if let Some(rest) = trimmed.strip_prefix('+') {
        (false, rest)
    } else if let Some(rest) = trimmed.strip_prefix('-') {
        (true, rest)
    } else {
        (false, trimmed)
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    let mut prev_underscore = true; // leading '_' fails
    let mut any_digit = false;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            any_digit = true;
            prev_underscore = false;
            value = value
                .checked_mul(10)?
                .checked_add((ch as i64) - ('0' as i64))?;
        } else {
            return None;
        }
    }
    if prev_underscore || !any_digit {
        return None;
    }
    Some(if sign { -value } else { value })
}

/// `query_type` param (`base.py:294-296`): comma-split, stripped; unknown
/// types are silently ignored by the caller (no key emitted).
pub fn split_query_types(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("user_mention")
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Global-search entities (`base.py:261-277`): comma list filtered to known
/// mapper keys (unknown names silently dropped; all-unknown yields
/// `{'results': {}}`); default is all 8 mapper keys in mapper order.
/// Request order wins (`:273-275` iterate the request list, so
/// `?entities=intake,issue` emits `intake` first); repeats collapse —
/// Python overwrites the same `results` key, which is output-identical.
pub fn requested_entities(raw: Option<&str>) -> Vec<&'static str> {
    const MAPPER: &[&str] = &[
        "workspace",
        "project",
        "issue",
        "cycle",
        "module",
        "issue_view",
        "page",
        "intake",
    ];
    match opt(raw) {
        None => MAPPER.to_vec(),
        Some(list) => {
            let mut out: Vec<&'static str> = Vec::new();
            for part in list.split(',').map(str::trim) {
                if part.is_empty() {
                    continue;
                }
                if let Some(key) = MAPPER.iter().copied().find(|key| *key == part) {
                    if !out.contains(&key) {
                        out.push(key);
                    }
                }
            }
            out
        }
    }
}

// ---------------------------------------------------------------------------
// FTS application
// ---------------------------------------------------------------------------

/// Bind an issue-text search and return its `WHERE` arm, or `None` when the
/// query is absent/empty (the queryset passes through unchanged —
/// `search/issue.py:161-162`, `search_issues:201-207`).
///
/// Binds three params in order: the raw query (`$fts`), the
/// `sequence_tokens(query)` list (`$seq`), the `escape_like(query)` pattern
/// (`$like`).
pub fn issue_fts_arm(
    builder: &mut Builder,
    query: Option<&str>,
    include_comments: bool,
) -> Option<String> {
    let query = opt(query)?;
    if !search_applies(query) {
        return None;
    }
    let fts = builder.text(query);
    let seq = builder.ints(sequence_tokens(query));
    let like = builder.text(escape_like(query));
    Some(search_filter_sql(include_comments, &fts, &seq, &like))
}

/// `field__icontains` OR-chain over `fields` (`base.py:57-59` pattern:
/// empty `Q()` when the query is falsy matches all rows, so no arm).
/// `like` binds `escape_like(query)`.
pub fn icontains_arm(table: &str, fields: &[&str], like: &str) -> Option<String> {
    if fields.is_empty() {
        return None;
    }
    let arms: Vec<String> = fields
        .iter()
        .map(|field| format!("{table}.{field} ILIKE '%' || {like} || '%' ESCAPE '\\'"))
        .collect();
    Some(format!("({})", arms.join(" OR ")))
}

// ---------------------------------------------------------------------------
// Shared scopes
// ---------------------------------------------------------------------------

/// Soft-delete arm for a directly-queried root table.
fn deleted_arm(table: &str) -> String {
    format!("{table}.deleted_at IS NULL")
}

/// `Issue.issue_objects` exclusions (`db/models/issue.py:95-104`): soft
/// delete, triage-state group (in the odd `NOT (group = triage AND group IS
/// NOT NULL)` Django form — kept verbatim), archived issue, archived
/// project, drafts. Requires the `LEFT JOIN states` + `INNER JOIN projects`
/// in the `FROM` clause.
fn issue_manager_arms() -> Vec<String> {
    vec![
        "issues.deleted_at IS NULL".to_owned(),
        "NOT (states.group = 'triage' AND states.group IS NOT NULL)".to_owned(),
        "NOT (issues.archived_at IS NOT NULL)".to_owned(),
        "NOT (projects.archived_at IS NOT NULL)".to_owned(),
        "NOT (issues.is_draft)".to_owned(),
    ]
}

/// Membership + tenancy scope shared by the traversal sections
/// (`project__project_projectmember__member/is_active`,
/// `project__archived_at__isnull`, `workspace__slug`).
fn member_scope_arms(user: &str, slug: &str) -> Vec<String> {
    vec![
        "projects.archived_at IS NULL".to_owned(),
        "project_members.is_active".to_owned(),
        format!("project_members.member_id = {user}"),
        format!("workspaces.slug = {slug}"),
    ]
}

// ---------------------------------------------------------------------------
// Global search sections (GlobalSearchEndpoint :54-253)
// ---------------------------------------------------------------------------

/// Scope for the global sections: the requesting user, the URL workspace
/// slug, and the resolved project narrow (see [`project_narrow`]).
pub struct GlobalScope<'a> {
    /// `project_members.member_id` / `workspace_members.member_id` bind.
    pub user_id: &'a str,
    /// `workspaces.slug` bind.
    pub workspace_slug: &'a str,
    /// Resolved narrow (`Some` only when `workspace_search == "false"` and
    /// `project_id` is truthy).
    pub narrow_project_id: Option<&'a str>,
}

/// `filter_workspaces` (`base.py:54-65`): `name` icontains over every
/// workspace the user belongs to — NOT scoped by the URL slug.
pub fn global_workspaces(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let mut wheres = vec![
        deleted_arm("workspaces"),
        format!("workspace_members.member_id = {user}"),
    ];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm("workspaces", &["name"], &like) {
            wheres.push(arm);
        }
    }
    let select: Vec<String> = GLOBAL_WORKSPACE_KEYS
        .iter()
        .map(|key| format!("workspaces.{key}"))
        .chain(std::iter::once("workspaces.created_at".to_owned()))
        .collect();
    builder.finish(format!(
        "SELECT DISTINCT {} FROM workspaces \
         INNER JOIN workspace_members ON (workspaces.id = workspace_members.workspace_id) \
         WHERE ({}) ORDER BY workspaces.created_at DESC",
        select.join(", "),
        wheres.join(" AND ")
    ))
}

/// Row keys of the global workspace section (`values("name", "id", "slug")`,
/// `base.py:64`).
pub const GLOBAL_WORKSPACE_KEYS: &[&str] = &["name", "id", "slug"];

/// `filter_projects` (`base.py:67-84`): `name` OR `identifier` icontains;
/// membership-active, archived-null, URL-slug scope. No project narrow —
/// the source never filters this section by `project_id`.
pub fn global_projects(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = vec![deleted_arm("projects")];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm("projects", &["name", "identifier"], &like) {
            wheres.push(arm);
        }
    }
    wheres.extend(member_scope_arms(&user, &slug));
    builder.finish(format!(
        "SELECT DISTINCT projects.name, projects.id, projects.identifier, workspaces.slug, projects.created_at \
         FROM projects \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (projects.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY projects.created_at DESC",
        wheres.join(" AND ")
    ))
}

/// Row keys of the global project section (`values("name", "id",
/// "identifier", "workspace__slug")`, `base.py:83`).
pub const GLOBAL_PROJECT_KEYS: &[&str] = &["name", "id", "identifier", "workspace__slug"];

/// `filter_issues` (`base.py:86-109`): `issue_search_queryset` with
/// `include_comments=True` (comment match returns the parent issue row,
/// `:94-96`), optional project narrow, `DISTINCT`, `[:100]`.
///
/// The `-created_at` ordering is the model `Meta` default, not an explicit
/// call — but it is in the emitted SQL all the same.
pub fn global_issues(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = issue_manager_arms();
    wheres.extend(member_scope_arms(&user, &slug));
    if let Some(arm) = issue_fts_arm(&mut builder, query, true) {
        wheres.push(arm);
    }
    if let Some(pid) = scope.narrow_project_id {
        let narrow = builder.uuid(pid);
        wheres.push(format!("issues.project_id = {narrow}"));
    }
    builder.finish(format!(
        "SELECT DISTINCT issues.name, issues.id, issues.sequence_id, projects.identifier, \
         issues.project_id, workspaces.slug, issues.created_at \
         FROM issues \
         LEFT OUTER JOIN states ON (issues.state_id = states.id) \
         INNER JOIN projects ON (issues.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY issues.created_at DESC LIMIT 100",
        wheres.join(" AND ")
    ))
}

/// Row keys of the global issue section (`base.py:102-109`).
pub const GLOBAL_ISSUE_KEYS: &[&str] = &[
    "name",
    "id",
    "sequence_id",
    "project__identifier",
    "project_id",
    "workspace__slug",
];

/// `filter_cycles` (`base.py:111-133`) and `filter_modules`
/// (`base.py:135-157`): `name` icontains, membership scope, optional
/// project narrow, `-created_at`, `DISTINCT`.
fn global_named_in_project(
    mut builder: Builder,
    table: &str,
    select: &str,
    scope: &GlobalScope<'_>,
    query: Option<&str>,
) -> BuiltQuery {
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = vec![deleted_arm(table)];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm(table, &["name"], &like) {
            wheres.push(arm);
        }
    }
    wheres.extend(member_scope_arms(&user, &slug));
    if let Some(pid) = scope.narrow_project_id {
        let narrow = builder.uuid(pid);
        wheres.push(format!("{table}.project_id = {narrow}"));
    }
    builder.finish(format!(
        "SELECT DISTINCT {select}, {table}.created_at FROM {table} \
         INNER JOIN projects ON ({table}.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON ({table}.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY {table}.created_at DESC",
        wheres.join(" AND ")
    ))
}

/// `filter_cycles` (`base.py:111-133`).
pub fn global_cycles(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let builder = Builder::new();
    global_named_in_project(
        builder,
        "cycles",
        "cycles.name, cycles.id, cycles.project_id, projects.identifier, workspaces.slug",
        scope,
        query,
    )
}

/// Row keys of the global cycle section (`base.py:132`).
pub const GLOBAL_CYCLE_KEYS: &[&str] = &[
    "name",
    "id",
    "project_id",
    "project__identifier",
    "workspace__slug",
];

/// `filter_modules` (`base.py:135-157`).
pub fn global_modules(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let builder = Builder::new();
    global_named_in_project(
        builder,
        "modules",
        "modules.name, modules.id, modules.project_id, projects.identifier, workspaces.slug",
        scope,
        query,
    )
}

/// Row keys of the global module section (`base.py:156`).
pub const GLOBAL_MODULE_KEYS: &[&str] = &[
    "name",
    "id",
    "project_id",
    "project__identifier",
    "workspace__slug",
];

/// `filter_views` (`base.py:205-227`).
pub fn global_views(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let builder = Builder::new();
    global_named_in_project(
        builder,
        "issue_views",
        "issue_views.name, issue_views.id, issue_views.project_id, projects.identifier, workspaces.slug",
        scope,
        query,
    )
}

/// Row keys of the global view section (`base.py:226`).
pub const GLOBAL_VIEW_KEYS: &[&str] = &[
    "name",
    "id",
    "project_id",
    "project__identifier",
    "workspace__slug",
];

/// `~Q(projects__id=True)` as Django renders it (`base.py:176,185`): the
/// boolean coerces to `uuid(int=1)`, so the `ArrayAgg` filter keeps every
/// row whose project is not the impossible zero-plus-one UUID — a no-op
/// for real data. Ported as-is (B9).
pub const PAGE_AGG_EXCLUDED_PROJECT_ID: &str = "00000000-0000-0000-0000-000000000001";

/// `filter_pages` (`base.py:159-203`): `name` icontains over pages whose
/// projects the user actively belongs to, with the `project_ids` /
/// `project_identifiers` `ArrayAgg` annotations; the project narrow goes
/// through the `ProjectPage` subquery annotate+filter (`:192-197`).
pub fn global_pages(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = vec![deleted_arm("pages")];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm("pages", &["name"], &like) {
            wheres.push(arm);
        }
    }
    wheres.extend([
        "projects.archived_at IS NULL".to_owned(),
        "project_members.is_active".to_owned(),
        format!("project_members.member_id = {user}"),
        format!("workspaces.slug = {slug}"),
    ]);
    if let Some(pid) = scope.narrow_project_id {
        let narrow = builder.uuid(pid);
        // `pages.annotate(project_id=Subquery(...)).filter(project_id=…)`
        // (`:192-197`): the subquery carries the m2m model's default
        // `-created_at` ordering and `LIMIT 1` verbatim.
        wheres.push(format!(
            "(SELECT project_id FROM project_pages \
             WHERE deleted_at IS NULL AND page_id = pages.id AND project_id = {narrow} \
             ORDER BY created_at DESC LIMIT 1) = {narrow}"
        ));
    }
    let agg_filter = format!("NOT (project_pages.project_id = '{PAGE_AGG_EXCLUDED_PROJECT_ID}')");
    builder.finish(format!(
        "SELECT DISTINCT pages.name, pages.id, \
         COALESCE(ARRAY_AGG(DISTINCT project_pages.project_id) FILTER (WHERE {agg_filter}), \
         '{{}}'::uuid[]) AS project_ids, \
         COALESCE(ARRAY_AGG(DISTINCT projects.identifier) FILTER (WHERE {agg_filter}), \
         '{{}}'::varchar[]) AS project_identifiers, \
         workspaces.slug, pages.created_at \
         FROM pages \
         INNER JOIN project_pages ON (pages.id = project_pages.page_id) \
         INNER JOIN projects ON (project_pages.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (pages.workspace_id = workspaces.id) \
         WHERE ({}) GROUP BY pages.id, workspaces.slug ORDER BY pages.created_at DESC",
        wheres.join(" AND ")
    ))
}

/// Row keys of the global page section (`base.py:202`).
pub const GLOBAL_PAGE_KEYS: &[&str] = &[
    "name",
    "id",
    "project_ids",
    "project_identifiers",
    "workspace__slug",
];

/// `filter_intakes` (`base.py:229-253`): issues in the intake states
/// (`intake_issues.status IN (0, -2)`), FTS with `include_comments=False`,
/// optional project narrow, `-created_at`, `DISTINCT`, `[:100]`.
///
/// B7 port: the root manager is `Issue.objects`, not
/// `Issue.issue_objects` — only the soft-delete arm applies, so
/// triage/draft/archived issues can surface here.
pub fn global_intakes(scope: &GlobalScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = vec![
        deleted_arm("issues"),
        "projects.archived_at IS NULL".to_owned(),
        "project_members.is_active".to_owned(),
        format!("project_members.member_id = {user}"),
        format!("workspaces.slug = {slug}"),
        "(intake_issues.status = 0 OR intake_issues.status = -2)".to_owned(),
    ];
    if let Some(arm) = issue_fts_arm(&mut builder, query, false) {
        wheres.push(arm);
    }
    if let Some(pid) = scope.narrow_project_id {
        let narrow = builder.uuid(pid);
        wheres.push(format!("issues.project_id = {narrow}"));
    }
    builder.finish(format!(
        "SELECT DISTINCT issues.name, issues.id, issues.sequence_id, projects.identifier, \
         issues.project_id, workspaces.slug, issues.created_at \
         FROM issues \
         INNER JOIN projects ON (issues.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
         INNER JOIN intake_issues ON (issues.id = intake_issues.issue_id) \
         WHERE ({}) ORDER BY issues.created_at DESC LIMIT 100",
        wheres.join(" AND ")
    ))
}

/// Row keys of the global intake section (`base.py:245-252`).
pub const GLOBAL_INTAKE_KEYS: &[&str] = &[
    "name",
    "id",
    "sequence_id",
    "project__identifier",
    "project_id",
    "workspace__slug",
];

// ---------------------------------------------------------------------------
// Entity search branches (SearchEndpoint.get :293-691)
// ---------------------------------------------------------------------------

/// Scope for one entity-search branch: `project_id = Some` selects the
/// project branch (`:302-501`), `None` (param absent or `""`) the workspace
/// branch (`:503-691`). `count` is the parsed `[:count]` limit (B8: the
/// handler 500s on parse failure and on negative values).
pub struct EntityScope<'a> {
    /// Member bind.
    pub user_id: &'a str,
    /// `workspaces.slug` bind.
    pub workspace_slug: &'a str,
    /// URL `project_id`, if truthy.
    pub project_id: Option<&'a str>,
    /// `LIMIT` value (`>= 0`; negative is a handler 500).
    pub count: i64,
}

/// `member__avatar_url` annotation (`base.py:324-341,523-540`): the asset
/// UUID renders raw into the `/api/assets/v2/static/<uuid>/` path when an
/// avatar asset exists, else the plain avatar URL, else `NULL`. Django's
/// nested-`CONCAT` casts kept verbatim.
pub fn avatar_url_case() -> String {
    "CASE WHEN users.avatar_asset_id IS NOT NULL \
     THEN CONCAT(('/api/assets/v2/static/')::text, \
     (CONCAT((users.avatar_asset_id)::text, ('/')::text))::text) \
     WHEN users.avatar_asset_id IS NULL THEN users.avatar \
     ELSE NULL END AS member__avatar_url"
        .to_owned()
}

/// Cycle `status` annotation (`base.py:412-430,604-621`). Django evaluates
/// `timezone.now()` once per `When`, so the handler binds `now` four times
/// (same value four times is semantically identical).
pub fn cycle_status_case(builder: &mut Builder, now: &str) -> String {
    let at1 = builder.text(now);
    let at2 = builder.text(now);
    let at3 = builder.text(now);
    let at4 = builder.text(now);
    format!(
        "CASE WHEN (cycles.start_date <= {at1} AND cycles.end_date >= {at2}) THEN 'CURRENT' \
         WHEN cycles.start_date > {at3} THEN 'UPCOMING' \
         WHEN cycles.end_date < {at4} THEN 'COMPLETED' \
         WHEN (cycles.start_date IS NULL AND cycles.end_date IS NULL) THEN 'DRAFT' \
         ELSE 'DRAFT' END AS status"
    )
}

/// `user_mention` branch (`base.py:304-351,505-544`): name-field icontains
/// over active non-bot memberships with the avatar annotation.
///
/// Asymmetry ported as-is: the project branch queries `ProjectMember`
/// (`DISTINCT`, `LEFT JOIN users`, `LIMIT` applied in Python after
/// `values()` — same emitted `LIMIT`), the workspace branch queries
/// `WorkspaceMember` (no `DISTINCT`, `INNER JOIN users`, in-DB `[:count]`).
pub fn entity_user_mention(scope: &EntityScope<'_>, query: Option<&str>) -> BuiltQuery {
    // `member__first_name` etc. traverse the member join; the SQL columns
    // live on `users`.
    const FIELDS: &[&str] = &["first_name", "last_name", "display_name"];
    let mut builder = Builder::new();
    let slug = builder.text(scope.workspace_slug);
    let avatar = avatar_url_case();
    if let Some(pid) = scope.project_id {
        let project = builder.uuid(pid);
        let mut wheres = vec![deleted_arm("project_members")];
        if let Some(q) = opt(query) {
            let like = builder.text(escape_like(q));
            if let Some(arm) = icontains_arm("users", FIELDS, &like) {
                wheres.push(arm);
            }
        }
        wheres.extend([
            "project_members.is_active".to_owned(),
            "NOT users.is_bot".to_owned(),
            format!("project_members.project_id = {project}"),
            format!("workspaces.slug = {slug}"),
        ]);
        builder.finish(format!(
            "SELECT DISTINCT users.display_name, project_members.member_id, {avatar}, \
             project_members.created_at FROM project_members \
             LEFT OUTER JOIN users ON (project_members.member_id = users.id) \
             INNER JOIN workspaces ON (project_members.workspace_id = workspaces.id) \
             WHERE ({}) ORDER BY project_members.created_at DESC LIMIT {}",
            wheres.join(" AND "),
            scope.count
        ))
    } else {
        let mut wheres = vec![deleted_arm("workspace_members")];
        if let Some(q) = opt(query) {
            let like = builder.text(escape_like(q));
            if let Some(arm) = icontains_arm("users", FIELDS, &like) {
                wheres.push(arm);
            }
        }
        wheres.extend([
            "workspace_members.is_active".to_owned(),
            "NOT users.is_bot".to_owned(),
            format!("workspaces.slug = {slug}"),
        ]);
        builder.finish(format!(
            "SELECT users.display_name, workspace_members.member_id, {avatar} \
             FROM workspace_members \
             INNER JOIN users ON (workspace_members.member_id = users.id) \
             INNER JOIN workspaces ON (workspace_members.workspace_id = workspaces.id) \
             WHERE ({}) ORDER BY workspace_members.created_at DESC LIMIT {}",
            wheres.join(" AND "),
            scope.count
        ))
    }
}

/// Row keys of the `user_mention` branch (`base.py:345-349,542`).
pub const ENTITY_USER_MENTION_KEYS: &[&str] =
    &["member__avatar_url", "member__display_name", "member__id"];

/// `project` branch (`base.py:353-370,546-563`): `name` OR `identifier`
/// icontains; member-or-public (`network = 2`) scope; URL-slug scope. Both
/// source branches are identical, so this one builder serves both.
pub fn entity_project(scope: &EntityScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = vec![deleted_arm("projects")];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm("projects", &["name", "identifier"], &like) {
            wheres.push(arm);
        }
    }
    wheres.extend([
        format!("(project_members.member_id = {user} OR projects.network = 2)"),
        format!("workspaces.slug = {slug}"),
    ]);
    builder.finish(format!(
        "SELECT DISTINCT projects.name, projects.id, projects.identifier, projects.logo_props, \
         workspaces.slug, projects.created_at FROM projects \
         LEFT OUTER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (projects.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY projects.created_at DESC LIMIT {}",
        wheres.join(" AND "),
        scope.count
    ))
}

/// Row keys of the `project` branch (`base.py:368,561`).
pub const ENTITY_PROJECT_KEYS: &[&str] =
    &["name", "id", "identifier", "logo_props", "workspace__slug"];

/// `issue` branch (`base.py:372-394,565-586`): `issue_objects` scope plus
/// the project pin on the project branch, FTS with `include_comments=False`,
/// `-created_at`, `DISTINCT`, `values` + `[:count]`.
pub fn entity_issue(scope: &EntityScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = issue_manager_arms();
    wheres.extend(member_scope_arms(&user, &slug));
    if let Some(pid) = scope.project_id {
        let project = builder.uuid(pid);
        wheres.push(format!("issues.project_id = {project}"));
    }
    if let Some(arm) = issue_fts_arm(&mut builder, query, false) {
        wheres.push(arm);
    }
    builder.finish(format!(
        "SELECT DISTINCT issues.name, issues.id, issues.sequence_id, projects.identifier, \
         issues.project_id, issues.priority, issues.state_id, issues.type_id, issues.created_at \
         FROM issues \
         LEFT OUTER JOIN states ON (issues.state_id = states.id) \
         INNER JOIN projects ON (issues.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY issues.created_at DESC LIMIT {}",
        wheres.join(" AND "),
        scope.count
    ))
}

/// Row keys of the `issue` branch (`base.py:383-392,575-584`).
pub const ENTITY_ISSUE_KEYS: &[&str] = &[
    "name",
    "id",
    "sequence_id",
    "project__identifier",
    "project_id",
    "priority",
    "state_id",
    "type_id",
];

/// `cycle` branch (`base.py:396-442,588-633`): `name` icontains, membership
/// scope, project pin on the project branch, the `status` annotation,
/// `-created_at`, `DISTINCT`, `values` + `[:count]`. `now` is the
/// `timezone.now()` value, bound four times (see [`cycle_status_case`]).
pub fn entity_cycle(scope: &EntityScope<'_>, query: Option<&str>, now: &str) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let status = cycle_status_case(&mut builder, now);
    let mut wheres = vec![deleted_arm("cycles")];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm("cycles", &["name"], &like) {
            wheres.push(arm);
        }
    }
    wheres.extend(member_scope_arms(&user, &slug));
    if let Some(pid) = scope.project_id {
        let project = builder.uuid(pid);
        wheres.push(format!("cycles.project_id = {project}"));
    }
    builder.finish(format!(
        "SELECT DISTINCT cycles.name, cycles.id, cycles.project_id, projects.identifier, \
         {status}, workspaces.slug, cycles.created_at FROM cycles \
         INNER JOIN projects ON (cycles.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (cycles.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY cycles.created_at DESC LIMIT {}",
        wheres.join(" AND "),
        scope.count
    ))
}

/// Row keys of the `cycle` branch (`base.py:433-440,624-631`).
pub const ENTITY_CYCLE_KEYS: &[&str] = &[
    "name",
    "id",
    "project_id",
    "project__identifier",
    "status",
    "workspace__slug",
];

/// `module` branch (`base.py:444-471,635-661`): same shape as `cycle`
/// except `status` is a plain column (no `Case`).
pub fn entity_module(scope: &EntityScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = vec![deleted_arm("modules")];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm("modules", &["name"], &like) {
            wheres.push(arm);
        }
    }
    wheres.extend(member_scope_arms(&user, &slug));
    if let Some(pid) = scope.project_id {
        let project = builder.uuid(pid);
        wheres.push(format!("modules.project_id = {project}"));
    }
    builder.finish(format!(
        "SELECT DISTINCT modules.name, modules.id, modules.project_id, projects.identifier, \
         modules.status, workspaces.slug, modules.created_at FROM modules \
         INNER JOIN projects ON (modules.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (modules.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY modules.created_at DESC LIMIT {}",
        wheres.join(" AND "),
        scope.count
    ))
}

/// Row keys of the `module` branch (`base.py:462-470,653-660`).
pub const ENTITY_MODULE_KEYS: &[&str] = &[
    "name",
    "id",
    "project_id",
    "project__identifier",
    "status",
    "workspace__slug",
];

/// `page` branch (`base.py:473-500,663-690`): `name` icontains over the
/// m2m project scope with `access = 0`; the project branch additionally
/// pins `projects__id`, the workspace branch requires `is_global`.
pub fn entity_page(scope: &EntityScope<'_>, query: Option<&str>) -> BuiltQuery {
    let mut builder = Builder::new();
    let user = builder.uuid(scope.user_id);
    let slug = builder.text(scope.workspace_slug);
    let mut wheres = vec![deleted_arm("pages")];
    if let Some(q) = opt(query) {
        let like = builder.text(escape_like(q));
        if let Some(arm) = icontains_arm("pages", &["name"], &like) {
            wheres.push(arm);
        }
    }
    wheres.extend([
        "project_members.is_active".to_owned(),
        format!("project_members.member_id = {user}"),
        format!("workspaces.slug = {slug}"),
        "pages.access = 0".to_owned(),
    ]);
    if let Some(pid) = scope.project_id {
        let project = builder.uuid(pid);
        wheres.push(format!("project_pages.project_id = {project}"));
    } else {
        wheres.push("pages.is_global".to_owned());
    }
    builder.finish(format!(
        "SELECT DISTINCT pages.name, pages.id, pages.logo_props, project_pages.project_id, \
         workspaces.slug, pages.created_at FROM pages \
         INNER JOIN project_pages ON (pages.id = project_pages.page_id) \
         INNER JOIN projects ON (project_pages.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (pages.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY pages.created_at DESC LIMIT {}",
        wheres.join(" AND "),
        scope.count
    ))
}

/// Row keys of the `page` branch (`base.py:492-499,682-689`; identical on
/// both branches).
pub const ENTITY_PAGE_KEYS: &[&str] = &[
    "name",
    "id",
    "logo_props",
    "projects__id",
    "workspace__slug",
];

// ---------------------------------------------------------------------------
// Issue search pipeline (IssueSearchEndpoint :24-166)
// ---------------------------------------------------------------------------

/// Failure modes of [`build_issue_search`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSearchError {
    /// B5 (`issue.py:77-80`): `sub_issue == "true"` with an unknown
    /// `issue_id` — `issue.parent` is read outside the `if issue:` guard, so
    /// Django raises `AttributeError` and the endpoint 500s. The handler
    /// must 500 when it sees this, never fall back to root issues.
    UnknownIssue,
}

/// Input to [`build_issue_search`], mirroring `get()`'s parsed params
/// (`issue.py:104-149`). Flags are pre-parsed (`"true"`-only); optional
/// strings are `None` when absent, `False`-defaulted or empty. The
/// DB-resolved fields (`issue_parent_id`, `related_ids`, `guest`) come from
/// handler lookups against the primary.
pub struct IssueSearchInput<'a> {
    /// URL workspace slug.
    pub slug: &'a str,
    /// URL project id (the `project_id` path arg, always present).
    pub url_project_id: &'a str,
    /// Requesting user id.
    pub user_id: &'a str,
    /// `search` param (`None` = `False` default/empty).
    pub query: Option<&'a str>,
    /// `workspace_search` param (`"false"` default).
    pub workspace_search: &'a str,
    /// `parent == "true"`.
    pub parent: bool,
    /// `issue_relation == "true"`.
    pub issue_relation: bool,
    /// `sub_issue == "true"`.
    pub sub_issue: bool,
    /// `cycle == "true"`.
    pub cycle: bool,
    /// `module` param (`None` = `False` default/empty; else a module UUID).
    pub module: Option<&'a str>,
    /// `target_date == "none"` (the default `True` never activates).
    pub target_date_none: bool,
    /// `issue_id` param (`None` = `False` default/empty).
    pub issue_id: Option<&'a str>,
    /// The `issue_id` row's `parent_id`, when the row exists: `None` =
    /// unknown `issue_id` (`.first()` is `None`); `Some(None)` = known root;
    /// `Some(Some(pid))` = known with parent `pid`.
    pub issue_parent_id: Option<Option<&'a str>>,
    /// `IssueRelation` id pairs touching `issue_id`, flattened, with
    /// `issue_id` appended — exactly what the handler collects at
    /// `issue.py:58-65` (the append happens before the `if issue:` guard).
    pub related_ids: Vec<String>,
    /// A `ProjectMember(role=5, is_active)` row exists for this user on the
    /// URL project (`issue.py:146-149`).
    pub guest: bool,
}

/// `IssueSearchEndpoint.get` (`issue.py:104-166`) as one composed query:
/// base scope (`:115-120`), optional project pin (`:122-123`), the FTS
/// `search_issues` wrapper (`:125-126`, `include_comments=False` +
/// `DISTINCT`), the parent/related/root/cycle/module/target-date filters
/// (`:128-144`), guest `created_by` scoping (`:146-149`) and the 11-key
/// `values(…)` projection with `[:100]` (`:151-164`).
pub fn build_issue_search(input: &IssueSearchInput<'_>) -> Result<BuiltQuery, IssueSearchError> {
    let mut builder = Builder::new();
    let user = builder.uuid(input.user_id);
    let slug = builder.text(input.slug);
    let mut wheres = issue_manager_arms();
    wheres.extend(member_scope_arms(&user, &slug));

    // `filter_issues_by_project` (:24-31), gated by `project_narrow`.
    if project_narrow(input.workspace_search, Some(input.url_project_id)).is_some() {
        let project = builder.uuid(input.url_project_id);
        wheres.push(format!("issues.project_id = {project}"));
    }

    // `search_issues_by_query` (:33-40).
    if let Some(arm) = issue_fts_arm(&mut builder, input.query, false) {
        wheres.push(arm);
    }

    let known = input.issue_parent_id.is_some();
    let issue_id = opt(input.issue_id);

    // `search_issues_and_excluding_parent` (:42-50): self, parent, children.
    // A `None` parent renders Django's `NOT (id IS NULL)` tautology for that
    // arm, which is a no-op — skipped here with identical effect.
    if input.parent {
        if let Some(xid) = issue_id {
            if let Some(parent) = input.issue_parent_id {
                let x = builder.uuid(xid);
                wheres.push(format!("NOT (issues.id = {x})"));
                if let Some(pid) = parent {
                    let p = builder.uuid(pid);
                    wheres.push(format!("NOT (issues.id = {p})"));
                }
                wheres.push(format!("NOT (issues.parent_id = {x})"));
            }
        }
    }

    // `filter_issues_excluding_related_issues` (:52-70): unknown `issue_id`
    // leaves the queryset unchanged (`if issue:` guard, `:67`).
    if input.issue_relation && issue_id.is_some() && known {
        let placeholders: Vec<String> = input
            .related_ids
            .iter()
            .map(|id| builder.uuid(id))
            .collect();
        if !placeholders.is_empty() {
            wheres.push(format!("issues.id NOT IN ({})", placeholders.join(", ")));
        }
    }

    // `filter_root_issues_only` (:72-81), B5 port: unknown `issue_id`
    // errors (the handler 500s) instead of filtering.
    if input.sub_issue && issue_id.is_some() {
        match input.issue_parent_id {
            None => return Err(IssueSearchError::UnknownIssue),
            Some(parent) => {
                let x = builder.uuid(issue_id.unwrap_or_default());
                wheres.push(format!("issues.id <> {x}"));
                wheres.push("issues.parent_id IS NULL".to_owned());
                if let Some(pid) = parent {
                    let p = builder.uuid(pid);
                    wheres.push(format!("issues.id <> {p}"));
                }
            }
        }
    }

    // `exclude_issues_in_cycles` (:83-88). Django renders this as
    // `NOT (EXISTS(any cycle row) AND EXISTS(non-deleted cycle row))`;
    // a non-deleted row implies a row, so the truth table collapses to
    // "no non-deleted cycle row" in all three cases (no rows, only deleted
    // rows, live rows) — the single `NOT EXISTS` below is exactly
    // equivalent, without the double-`EXISTS` noise.
    if input.cycle {
        wheres.push(
            "NOT EXISTS (SELECT 1 FROM cycle_issues \
             WHERE issue_id = issues.id AND deleted_at IS NULL)"
                .to_owned(),
        );
    }

    // `exclude_issues_in_module` (:90-95): same collapse with the module
    // pinned. `if module:` — empty was normalised to `None` by the caller.
    if let Some(mid) = opt(input.module) {
        let module = builder.uuid(mid);
        wheres.push(format!(
            "NOT EXISTS (SELECT 1 FROM module_issues \
             WHERE issue_id = issues.id AND module_id = {module} AND deleted_at IS NULL)"
        ));
    }

    // `filter_issues_without_target_date` (:97-102).
    if input.target_date_none {
        wheres.push("issues.target_date IS NULL".to_owned());
    }

    // Guest scoping (:146-149): `role=5` channel — `created_by` only.
    if input.guest {
        let creator = builder.uuid(input.user_id);
        wheres.push(format!("issues.created_by_id = {creator}"));
    }

    Ok(builder.finish(format!(
        "SELECT DISTINCT issues.name, issues.id, issues.start_date, issues.sequence_id, \
         projects.name, projects.identifier, issues.project_id, workspaces.slug, \
         states.name, states.group, states.color, issues.created_at \
         FROM issues \
         LEFT OUTER JOIN states ON (issues.state_id = states.id) \
         INNER JOIN projects ON (issues.project_id = projects.id) \
         INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (issues.workspace_id = workspaces.id) \
         WHERE ({}) ORDER BY issues.created_at DESC LIMIT 100",
        wheres.join(" AND ")
    )))
}

/// Row keys of the issue-search projection (`issue.py:151-164`; the
/// `PROJECT_SEARCH_KEYS` contract in `test_search.py`). `state__*` joins
/// are `LEFT` (nullable state → all three null).
pub const PROJECT_SEARCH_KEYS: &[&str] = &[
    "name",
    "id",
    "start_date",
    "sequence_id",
    "project__name",
    "project__identifier",
    "project_id",
    "workspace__slug",
    "state__name",
    "state__group",
    "state__color",
];

#[cfg(test)]
mod tests {
    use super::*;

    const UID: &str = "11111111-1111-1111-1111-111111111111";
    const SLUG: &str = "acme";
    const PID: &str = "33333333-3333-3333-3333-333333333333";

    fn global_scope(narrow: Option<&'static str>) -> GlobalScope<'static> {
        GlobalScope {
            user_id: UID,
            workspace_slug: SLUG,
            narrow_project_id: narrow,
        }
    }

    // -- param kernels --------------------------------------------------------

    #[test]
    fn truthiness_gates() {
        assert_eq!(opt(None), None);
        assert_eq!(opt(Some("")), None);
        assert_eq!(opt(Some("x")), Some("x"));
        assert!(parse_flag("true"));
        assert!(!parse_flag("True"));
        assert!(!parse_flag("1"));
        assert!(!parse_flag("false"));
        assert!(parse_target_date_none(Some("none")));
        assert!(!parse_target_date_none(None));
        assert!(!parse_target_date_none(Some("True")));
        assert_eq!(project_narrow("false", Some(PID)), Some(PID));
        assert_eq!(project_narrow("true", Some(PID)), None);
        assert_eq!(project_narrow("false", None), None);
        assert_eq!(project_narrow("false", Some("")), None);
    }

    // B8: exactly CPython int() inputs parse; the handler 500s the rest
    // (and all negatives) — never 400.
    #[test]
    fn count_parses_py_int() {
        assert_eq!(parse_count(None), Ok(5));
        assert_eq!(parse_count(Some("3")), Ok(3));
        assert_eq!(parse_count(Some(" 12 ")), Ok(12));
        assert_eq!(parse_count(Some("+7")), Ok(7));
        assert_eq!(parse_count(Some("-1")), Ok(-1));
        assert_eq!(parse_count(Some("1_0")), Ok(10));
        assert_eq!(parse_count(Some("0")), Ok(0));
        assert_eq!(parse_count(Some("abc")), Err(CountError::Invalid));
        assert_eq!(parse_count(Some("")), Err(CountError::Invalid));
        assert_eq!(parse_count(Some("5.0")), Err(CountError::Invalid));
        assert_eq!(parse_count(Some("1__2")), Err(CountError::Invalid));
        assert_eq!(parse_count(Some("_12")), Err(CountError::Invalid));
        assert_eq!(parse_count(Some("12_")), Err(CountError::Invalid));
        assert_eq!(parse_count(Some("0x10")), Err(CountError::Invalid));
        assert_eq!(parse_count(Some("--3")), Err(CountError::Invalid));
        assert_eq!(
            parse_count(Some("99999999999999999999")),
            Err(CountError::Invalid)
        );
    }

    #[test]
    fn query_type_and_entity_selection() {
        assert_eq!(split_query_types(None), vec!["user_mention".to_owned()]);
        assert_eq!(
            split_query_types(Some("issue, cycle")),
            vec!["issue".to_owned(), "cycle".to_owned()]
        );
        // Unknown types pass through here; the caller emits no key for them.
        assert_eq!(split_query_types(Some("bogus")), vec!["bogus".to_owned()]);
        assert!(split_query_types(Some("")).is_empty());
        assert_eq!(
            requested_entities(None),
            vec![
                "workspace",
                "project",
                "issue",
                "cycle",
                "module",
                "issue_view",
                "page",
                "intake"
            ]
        );
        // Request order wins (base.py:273-275 iterate the request list);
        // repeats collapse (Python overwrites the same results key).
        assert_eq!(
            requested_entities(Some("issue,project")),
            vec!["issue", "project"]
        );
        assert_eq!(
            requested_entities(Some("intake, issue, bogus, issue")),
            vec!["intake", "issue"]
        );
        assert!(requested_entities(Some("bogus")).is_empty());
        assert_eq!(requested_entities(Some("")).len(), 8);
    }

    // -- bindings ------------------------------------------------------------

    #[test]
    fn placeholders_thread_in_order() {
        let mut builder = Builder::new();
        let a = builder.text("s");
        let b = builder.uuid(UID);
        let c = builder.ints(vec![1, 2]);
        let d = builder.int(7);
        assert_eq!(
            (a, b, c, d),
            (
                "$1".to_owned(),
                "$2".to_owned(),
                "$3".to_owned(),
                "$4".to_owned()
            )
        );
        let built = builder.finish("SELECT 1".to_owned());
        assert_eq!(
            built.params,
            vec![
                SqlParam::Text("s".to_owned()),
                SqlParam::Uuid(UID.to_owned()),
                SqlParam::IntegerArray(vec![1, 2]),
                SqlParam::Integer(7),
            ]
        );
    }

    // -- FTS arms ------------------------------------------------------------

    #[test]
    fn fts_arm_absent_without_query() {
        let mut builder = Builder::new();
        assert!(issue_fts_arm(&mut builder, None, true).is_none());
        assert!(issue_fts_arm(&mut builder, Some(""), true).is_none());
        assert!(builder.finish("".to_owned()).params.is_empty());
    }

    #[test]
    fn fts_arm_binds_three_params_in_order() {
        let mut builder = Builder::new();
        let arm = issue_fts_arm(&mut builder, Some("auth 42"), false).unwrap();
        assert!(arm.contains("@@ websearch_to_tsquery('english'::regconfig, $1)"));
        assert!(arm.contains("UNNEST($2::int[])"));
        assert!(arm.contains("ILIKE '%' || $3 || '%'"));
        assert!(!arm.contains("issue_comments"));
        let built = builder.finish(arm);
        assert_eq!(
            built.params,
            vec![
                SqlParam::Text("auth 42".to_owned()),
                SqlParam::IntegerArray(vec![42]),
                SqlParam::Text("auth 42".to_owned()),
            ]
        );
    }

    #[test]
    fn fts_arm_with_comments_widens() {
        let mut builder = Builder::new();
        let arm = issue_fts_arm(&mut builder, Some("refund"), true).unwrap();
        assert!(arm.contains("issues.id IN (SELECT issue_id FROM issue_comments"));
    }

    // -- global sections -----------------------------------------------------

    #[test]
    fn global_issues_shape() {
        let built = global_issues(&global_scope(Some(PID)), Some("auth"));
        assert!(built
            .sql
            .starts_with("SELECT DISTINCT issues.name, issues.id, issues.sequence_id,"));
        assert!(built.sql.contains("LEFT OUTER JOIN states"));
        assert!(built
            .sql
            .contains("NOT (states.group = 'triage' AND states.group IS NOT NULL)"));
        assert!(built.sql.contains("project_members.member_id = $1"));
        assert!(built.sql.contains("workspaces.slug = $2"));
        assert!(built
            .sql
            .contains("issues.id IN (SELECT issue_id FROM issue_comments"));
        assert!(built
            .sql
            .contains("ORDER BY issues.created_at DESC LIMIT 100"));
        // Narrow resolves after the three FTS binds.
        assert!(built
            .sql
            .contains(&format!("issues.project_id = ${}", built.params.len())));
        assert_eq!(built.params[0], SqlParam::Uuid(UID.to_owned()));
        assert_eq!(built.params[1], SqlParam::Text(SLUG.to_owned()));
        assert_eq!(built.params[2], SqlParam::Text("auth".to_owned()));
    }

    #[test]
    fn global_issues_no_query_no_narrow() {
        let built = global_issues(&global_scope(None), None);
        assert!(!built.sql.contains("@@ websearch_to_tsquery"));
        assert!(!built.sql.contains("project_id = $"));
        assert!(built.sql.contains("LIMIT 100"));
        assert_eq!(built.params.len(), 2);
    }

    // B7: intakes keep the soft-delete arm only — no triage/archived/draft
    // arms, no states join.
    #[test]
    fn intakes_use_unfiltered_manager() {
        let built = global_intakes(&global_scope(None), Some("auth"));
        assert!(built.sql.contains("issues.deleted_at IS NULL"));
        assert!(!built.sql.contains("triage"));
        assert!(!built.sql.contains("is_draft"));
        assert!(!built.sql.contains("LEFT OUTER JOIN states"));
        assert!(!built.sql.contains("issues.archived_at"));
        assert!(built
            .sql
            .contains("(intake_issues.status = 0 OR intake_issues.status = -2)"));
        assert!(built
            .sql
            .contains("INNER JOIN intake_issues ON (issues.id = intake_issues.issue_id)"));
        assert!(built.sql.contains("LIMIT 100"));
    }

    // B9: the ArrayAgg filter carries the coerced UUID-1 literal.
    #[test]
    fn pages_carry_coerced_agg_filter() {
        let built = global_pages(&global_scope(Some(PID)), Some("acme"));
        assert!(built.sql.contains(
            "FILTER (WHERE NOT (project_pages.project_id = '00000000-0000-0000-0000-000000000001'))"
        ));
        assert!(built.sql.contains("'{}'::uuid[]"));
        assert!(built.sql.contains("'{}'::varchar[]"));
        assert!(built.sql.contains("GROUP BY pages.id, workspaces.slug"));
        assert!(built.sql.contains(
            "(SELECT project_id FROM project_pages \
             WHERE deleted_at IS NULL AND page_id = pages.id AND project_id = $"
        ));
        let plain = global_pages(&global_scope(None), None);
        assert!(!plain.sql.contains("SELECT project_id FROM project_pages"));
    }

    #[test]
    fn workspaces_ignore_url_slug() {
        let built = global_workspaces(&global_scope(Some(PID)), Some("acme"));
        // `slug` is a selected key but never a filter on this section.
        assert!(!built.sql.contains("workspaces.slug = $"));
        assert!(built.sql.contains("workspace_members.member_id = $1"));
        assert!(built.sql.contains("workspaces.name ILIKE"));
    }

    // -- entity branches -----------------------------------------------------

    fn entity_scope(project: Option<&'static str>) -> EntityScope<'static> {
        EntityScope {
            user_id: UID,
            workspace_slug: SLUG,
            project_id: project,
            count: 5,
        }
    }

    #[test]
    fn user_mention_branches() {
        let project = entity_user_mention(&entity_scope(Some(PID)), Some("acme"));
        assert!(project.sql.contains("SELECT DISTINCT"));
        assert!(project.sql.contains("LEFT OUTER JOIN users"));
        assert!(project.sql.contains("users.first_name ILIKE"));
        assert!(project.sql.contains("users.last_name ILIKE"));
        assert!(project.sql.contains("users.display_name ILIKE"));
        assert!(project.sql.contains("project_members.project_id = $"));
        assert!(project
            .sql
            .contains("CONCAT(('/api/assets/v2/static/')::text"));
        assert!(project.sql.ends_with("LIMIT 5"));
        let workspace = entity_user_mention(&entity_scope(None), Some("acme"));
        assert!(!workspace.sql.contains("DISTINCT"));
        assert!(workspace.sql.contains("INNER JOIN users"));
        assert!(workspace.sql.contains("workspace_members"));
        assert!(!workspace.sql.contains("project_members.project_id"));
    }

    #[test]
    fn entity_project_has_network_or_without_active() {
        let built = entity_project(&entity_scope(Some(PID)), Some("acme"));
        assert!(built
            .sql
            .contains("(project_members.member_id = $1 OR projects.network = 2)"));
        assert!(!built.sql.contains("is_active"));
        assert!(built.sql.contains("LEFT OUTER JOIN project_members"));
    }

    #[test]
    fn entity_issue_pin_only_on_project_branch() {
        let project = entity_issue(&entity_scope(Some(PID)), Some("acme"));
        assert!(project.sql.contains("issues.project_id = $"));
        assert!(!project.sql.contains("issue_comments"));
        let workspace = entity_issue(&entity_scope(None), None);
        assert!(!workspace.sql.contains("issues.project_id = $"));
        assert!(!workspace.sql.contains("@@ websearch_to_tsquery"));
        assert!(workspace.sql.ends_with("LIMIT 5"));
    }

    #[test]
    fn entity_cycle_status_case_binds_now_four_times() {
        let built = entity_cycle(&entity_scope(Some(PID)), None, "2026-09-29T20:00:00Z");
        assert!(built.sql.contains("THEN 'CURRENT'"));
        assert!(built.sql.contains("THEN 'UPCOMING'"));
        assert!(built.sql.contains("THEN 'COMPLETED'"));
        assert!(built.sql.contains("ELSE 'DRAFT' END AS status"));
        let nows = built
            .params
            .iter()
            .filter(|p| **p == SqlParam::Text("2026-09-29T20:00:00Z".to_owned()))
            .count();
        assert_eq!(nows, 4);
    }

    #[test]
    fn entity_module_selects_plain_status() {
        let built = entity_module(&entity_scope(None), Some("m"));
        assert!(built.sql.contains("modules.status"));
        assert!(!built.sql.contains("CASE"));
    }

    #[test]
    fn entity_page_access_rules() {
        let project = entity_page(&entity_scope(Some(PID)), None);
        assert!(project.sql.contains("pages.access = 0"));
        assert!(project.sql.contains("project_pages.project_id = $"));
        assert!(!project.sql.contains("is_global"));
        let workspace = entity_page(&entity_scope(None), None);
        assert!(workspace.sql.contains("pages.access = 0"));
        assert!(workspace.sql.contains("pages.is_global"));
        assert!(!workspace.sql.contains("project_pages.project_id = $"));
    }

    // -- key contracts (mirror the fixture/contract key sets) -----------------

    #[test]
    fn key_sets_match_contracts() {
        assert_eq!(
            PROJECT_SEARCH_KEYS,
            &[
                "name",
                "id",
                "start_date",
                "sequence_id",
                "project__name",
                "project__identifier",
                "project_id",
                "workspace__slug",
                "state__name",
                "state__group",
                "state__color",
            ]
        );
        assert_eq!(
            GLOBAL_ISSUE_KEYS,
            &[
                "name",
                "id",
                "sequence_id",
                "project__identifier",
                "project_id",
                "workspace__slug",
            ]
        );
        assert_eq!(
            ENTITY_ISSUE_KEYS,
            &[
                "name",
                "id",
                "sequence_id",
                "project__identifier",
                "project_id",
                "priority",
                "state_id",
                "type_id",
            ]
        );
        assert_eq!(
            ENTITY_USER_MENTION_KEYS,
            &["member__avatar_url", "member__display_name", "member__id"]
        );
        assert_eq!(
            ENTITY_CYCLE_KEYS,
            &[
                "name",
                "id",
                "project_id",
                "project__identifier",
                "status",
                "workspace__slug",
            ]
        );
    }

    // -- issue pipeline --------------------------------------------------------

    fn minimal_input() -> IssueSearchInput<'static> {
        IssueSearchInput {
            slug: SLUG,
            url_project_id: PID,
            user_id: UID,
            query: None,
            workspace_search: "true",
            parent: false,
            issue_relation: false,
            sub_issue: false,
            cycle: false,
            module: None,
            target_date_none: false,
            issue_id: None,
            issue_parent_id: None,
            related_ids: Vec::new(),
            guest: false,
        }
    }

    #[test]
    fn pipeline_minimal_shape() {
        let built = build_issue_search(&minimal_input()).unwrap();
        assert!(built
            .sql
            .starts_with("SELECT DISTINCT issues.name, issues.id, issues.start_date,"));
        assert!(built
            .sql
            .contains("states.name, states.group, states.color"));
        assert!(built
            .sql
            .contains("NOT (states.group = 'triage' AND states.group IS NOT NULL)"));
        assert!(built
            .sql
            .contains("ORDER BY issues.created_at DESC LIMIT 100"));
        // workspace_search=true: no project pin; no query: no FTS arm.
        assert!(!built.sql.contains("issues.project_id = $"));
        assert!(!built.sql.contains("@@ websearch_to_tsquery"));
        assert_eq!(built.params.len(), 2);
    }

    #[test]
    fn pipeline_full_shape() {
        let input = IssueSearchInput {
            query: Some("auth"),
            workspace_search: "false",
            parent: true,
            issue_relation: true,
            sub_issue: true,
            cycle: true,
            module: Some("44444444-4444-4444-4444-444444444444"),
            target_date_none: true,
            issue_id: Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            issue_parent_id: Some(Some("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")),
            related_ids: vec![
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned(),
                "cccccccc-cccc-cccc-cccc-cccccccccccc".to_owned(),
            ],
            guest: true,
            ..minimal_input()
        };
        let built = build_issue_search(&input).unwrap();
        let sql = &built.sql;
        assert!(sql.contains("issues.project_id = $"));
        assert!(sql.contains("@@ websearch_to_tsquery"));
        assert!(sql.contains("NOT (issues.parent_id = $"));
        assert!(sql.contains("issues.id NOT IN ($"));
        assert!(sql.contains("issues.parent_id IS NULL"));
        assert!(sql.contains(
            "NOT EXISTS (SELECT 1 FROM cycle_issues \
             WHERE issue_id = issues.id AND deleted_at IS NULL)"
        ));
        assert!(sql.contains("module_issues"));
        assert!(sql.contains("issues.target_date IS NULL"));
        assert!(sql.contains("issues.created_by_id = $"));
        assert!(sql.ends_with("LIMIT 100"));
    }

    // B5: sub_issue with an unknown issue_id errors so the handler 500s.
    #[test]
    fn sub_issue_unknown_issue_errors() {
        let input = IssueSearchInput {
            sub_issue: true,
            issue_id: Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            issue_parent_id: None,
            ..minimal_input()
        };
        assert_eq!(
            build_issue_search(&input),
            Err(IssueSearchError::UnknownIssue)
        );
    }

    #[test]
    fn unknown_issue_leaves_parent_and_related_unchanged() {
        let input = IssueSearchInput {
            parent: true,
            issue_relation: true,
            issue_id: Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            issue_parent_id: None,
            related_ids: vec!["aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned()],
            ..minimal_input()
        };
        let built = build_issue_search(&input).unwrap();
        assert!(!built.sql.contains("NOT (issues.id = $"));
        assert!(!built.sql.contains("NOT IN"));
    }

    #[test]
    fn empty_module_and_issue_id_are_falsy() {
        let input = IssueSearchInput {
            module: Some(""),
            issue_id: Some(""),
            sub_issue: true,
            parent: true,
            ..minimal_input()
        };
        // Empty strings are falsy: no module arm, and the issue_id gates
        // stay shut (in particular no B5 error).
        let built = build_issue_search(&input).unwrap();
        assert!(!built.sql.contains("module_issues"));
        assert!(!built.sql.contains("parent_id"));
    }
}
