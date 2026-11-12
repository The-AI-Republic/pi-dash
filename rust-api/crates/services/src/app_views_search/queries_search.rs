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
//! The section querysets are sea-query [`SelectStatement`]s executed
//! through SeaORM by the `api` layer; the handler binds nothing (binds
//! live in the statement). Every selected column is aliased to its
//! `values()` key so rows decode by name. Table/column names are the
//! Django physical names verified against Django 4.2.30's own compiler
//! output for every section below.
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

use super::fts::{escape_like, search_applies, sequence_tokens};
use sea_query::extension::postgres::{PgBinOper, PgExpr, PgFunc};
use sea_query::{Alias, Condition, Expr, Func, Order, Query, SelectStatement, SimpleExpr, Value};

// ---------------------------------------------------------------------------
// Statement helpers
// ---------------------------------------------------------------------------

/// `table`.`column` as a column expression.
fn col(table: &str, column: &str) -> Expr {
    Expr::col((Alias::new(table), Alias::new(column)))
}

/// Select `table`.`column` aliased to `key` (its `values()` key), so rows
/// decode by name with no duplicate output names.
fn select_keyed(stmt: &mut SelectStatement, table: &str, column: &str, key: &str) {
    stmt.expr_as(col(table, column), Alias::new(key));
}

/// `table`.`column` = bound value.
fn col_eq(table: &str, column: &str, value: impl Into<Value>) -> SimpleExpr {
    col(table, column).eq(Expr::val(value))
}

/// A UUID id arriving as text (every scope carries ids as strings — the
/// handler validates them upstream), bound as text and cast to `uuid`:
/// the cast resolves the id at execution, so malformed input errors at
/// the same place the old typed bind did (a 500, never a match).
fn uuid_val(raw: &str) -> SimpleExpr {
    SimpleExpr::from(Expr::val(raw)).cast_as(Alias::new("uuid"))
}

/// `table`.`column` = bound UUID (see [`uuid_val`]).
fn col_eq_uuid(table: &str, column: &str, raw: &str) -> SimpleExpr {
    col(table, column).eq(uuid_val(raw))
}

/// `table`.`column` IS NULL.
fn col_is_null(table: &str, column: &str) -> SimpleExpr {
    col(table, column).is_null()
}

/// The `triage` literal for the manager scope (compiled from the models
/// port via [`issue_ref::MANAGER_EXCLUDE_STATE_GROUP`] in the old builder;
/// kept as the pinned literal so the scope cannot drift from `issue.py`).
const MANAGER_EXCLUDE_STATE_GROUP: &str = "triage";

/// `'english'::regconfig`: the FTS config literal, bound as text and cast
/// (a bound `$N::regconfig` resolves the name at runtime, like the
/// literal Django compiles).
fn regconfig() -> SimpleExpr {
    SimpleExpr::from(Expr::val("english")).cast_as(Alias::new("regconfig"))
}

/// `to_tsvector('english'::regconfig, <text>)` as a function expression.
/// `Func::cust` (the merged `row_to_json` precedent) because `PgFunc`
/// only omits the config or takes an OID, and the config name is
/// normative here.
fn tsvector(text: SimpleExpr) -> sea_query::FunctionCall {
    Func::cust("to_tsvector").arg(regconfig()).arg(text)
}

/// `websearch_to_tsquery('english'::regconfig, <query>)`.
fn tsquery(query: &str) -> sea_query::FunctionCall {
    Func::cust("websearch_to_tsquery")
        .arg(regconfig())
        .arg(Expr::val(query))
}

/// `to_tsvector('english'::regconfig, COALESCE(<col>, ''))` over one column.
fn column_vector(table: &str, column: &str) -> sea_query::FunctionCall {
    tsvector(Func::coalesce([col(table, column).into(), SimpleExpr::from(Expr::val(""))]).into())
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

/// The issue-text search arm (`_build_search_filter`, `search/issue.py:93-125`),
/// or `None` when the query is absent/empty (the queryset passes through
/// unchanged — `search/issue.py:161-162`, `search_issues:201-207`). Django
/// arm order: FTS, name fallback, optional comment arm, sequence ints,
/// project identifier.
pub fn issue_fts_arm(query: Option<&str>, include_comments: bool) -> Option<SimpleExpr> {
    let query = opt(query)?;
    if !search_applies(query) {
        return None;
    }
    let like = format!("%{}%", escape_like(query));
    // `to_tsvector('english'::regconfig, COALESCE(name, '') || ' ' || COALESCE(description_stripped, ''))`
    let issue_vector = tsvector(
        Func::coalesce([
            col("issues", "name").into(),
            SimpleExpr::from(Expr::val("")),
        ])
        .concatenate(Expr::val(" "))
        .concatenate(Func::coalesce([
            col("issues", "description_stripped").into(),
            SimpleExpr::from(Expr::val("")),
        ])),
    );
    let mut arms = Condition::any()
        // FTS match arm.
        .add(issue_vector.matches(tsquery(query)))
        // Name icontains fallback.
        .add(col("issues", "name").binary(PgBinOper::ILike, Expr::val(like.clone())));
    if include_comments {
        // Comment-text arm: any non-deleted comment of the issue matches.
        // Django's subquery selects `issue_id` (uuid) — the IN comparison
        // is uuid-to-uuid.
        let mut comment = Query::select();
        comment
            .expr(Expr::col(Alias::new("issue_id")))
            .from(Alias::new("issue_comments"))
            .cond_where(
                Condition::all()
                    .add(col("issue_comments", "deleted_at").is_null())
                    .add(
                        column_vector("issue_comments", "comment_stripped").matches(tsquery(query)),
                    ),
            );
        arms = arms.add(col("issues", "id").in_subquery(comment));
    }
    // Legacy sequence-id ints: IN-list over the token binds. An empty
    // token list renders the UNNEST-empty outcome (no match) — as an
    // OR-arm false, it is simply omitted (an OR with a false arm is the
    // same result set), which also avoids sea-query's untyped
    // `$N = $M` false-condition.
    let tokens = sequence_tokens(query);
    if !tokens.is_empty() {
        arms = arms.add(col("issues", "sequence_id").is_in(tokens));
    }
    // Project identifier icontains, as an EXISTS subquery like Django.
    let mut identifier = Query::select();
    identifier
        .expr(Expr::val(1))
        .from(Alias::new("projects"))
        .cond_where(
            Condition::all()
                .add(col("projects", "id").equals(("issues", "project_id")))
                .add(col("projects", "identifier").binary(PgBinOper::ILike, Expr::val(like))),
        );
    arms = arms.add(Expr::exists(identifier));
    Some(arms.into())
}

/// `field__icontains` OR-chain over `fields` (`base.py:57-59` pattern:
/// empty `Q()` when the query is falsy matches all rows, so no arm).
/// The pattern is pre-concatenated (`%<escaped>%`); Postgres' default LIKE
/// escape is the backslash the escaper emits, so the match set is the
/// same as the old `'%' || <p> || '%'` + explicit-ESCAPE form.
pub fn icontains_arm(table: &str, fields: &[&str], query: &str) -> Option<SimpleExpr> {
    if fields.is_empty() {
        return None;
    }
    let like = format!("%{}%", escape_like(query));
    let mut arms = Condition::any();
    for field in fields {
        arms = arms.add(col(table, field).binary(PgBinOper::ILike, Expr::val(like.clone())));
    }
    Some(arms.into())
}

// ---------------------------------------------------------------------------
// Shared scopes
// ---------------------------------------------------------------------------

/// `Issue.issue_objects` exclusions (`db/models/issue.py:95-104`): soft
/// delete, triage-state group (in the odd `NOT (group = triage AND group IS
/// NOT NULL)` Django form — kept verbatim), archived issue, archived
/// project, drafts. Requires the `LEFT JOIN states` + `INNER JOIN projects`
/// in the `FROM` clause.
fn issue_manager_condition() -> Condition {
    Condition::all()
        .add(col_is_null("issues", "deleted_at"))
        .add(
            col("states", "group")
                .eq(Expr::val(MANAGER_EXCLUDE_STATE_GROUP))
                .and(col("states", "group").is_not_null())
                .not(),
        )
        .add(col("issues", "archived_at").is_null())
        .add(col("projects", "archived_at").is_null())
        .add(col("issues", "is_draft").eq(Expr::val(false)))
}

/// Membership + tenancy scope shared by the traversal sections
/// (`project__project_projectmember__member/is_active`,
/// `project__archived_at__isnull`, `workspace__slug`).
fn member_scope_condition(user_id: &str, workspace_slug: &str) -> Condition {
    Condition::all()
        .add(col_is_null("projects", "archived_at"))
        .add(col("project_members", "is_active").eq(Expr::val(true)))
        .add(col_eq_uuid("project_members", "member_id", user_id))
        .add(col_eq("workspaces", "slug", workspace_slug))
}

/// Membership + tenancy scope for the entity-search `cycle`/`module`
/// branches (`base.py:405-411,453-459,597-602,644-648`): member, `is_active`
/// and slug only. Unlike the global sections, Django applies no
/// `project__archived_at__isnull` here (PIDASHCONV-1450).
fn entity_member_scope_condition(user_id: &str, workspace_slug: &str) -> Condition {
    Condition::all()
        .add(col("project_members", "is_active").eq(Expr::val(true)))
        .add(col_eq_uuid("project_members", "member_id", user_id))
        .add(col_eq("workspaces", "slug", workspace_slug))
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
pub fn global_workspaces(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "workspaces", "name", "name");
    select_keyed(&mut stmt, "workspaces", "id", "id");
    select_keyed(&mut stmt, "workspaces", "slug", "slug");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, "workspaces", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("workspaces"))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspace_members",
            Alias::new("workspace_members"),
            col("workspaces", "id").equals(("workspace_members", "workspace_id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("workspaces", "deleted_at"))
                .add(col_eq_uuid("workspace_members", "member_id", scope.user_id)),
        );
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm("workspaces", &["name"], q) {
            stmt.and_where(arm);
        }
    }
    stmt.order_by(("workspaces", "created_at"), Order::Desc);
    stmt
}

/// Row keys of the global workspace section (`values("name", "id", "slug")`,
/// `base.py:64`).
pub const GLOBAL_WORKSPACE_KEYS: &[&str] = &["name", "id", "slug"];

/// `filter_projects` (`base.py:67-84`): `name` OR `identifier` icontains;
/// membership-active, archived-null, URL-slug scope. No project narrow —
/// the source never filters this section by `project_id`.
pub fn global_projects(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "projects", "name", "name");
    select_keyed(&mut stmt, "projects", "id", "id");
    select_keyed(&mut stmt, "projects", "identifier", "identifier");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    select_keyed(&mut stmt, "projects", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("projects"))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("projects", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("projects", "deleted_at"))
                .add(member_scope_condition(scope.user_id, scope.workspace_slug)),
        );
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm("projects", &["name", "identifier"], q) {
            stmt.and_where(arm);
        }
    }
    stmt.order_by(("projects", "created_at"), Order::Desc);
    stmt
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
pub fn global_issues(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "issues", "name", "name");
    select_keyed(&mut stmt, "issues", "id", "id");
    select_keyed(&mut stmt, "issues", "sequence_id", "sequence_id");
    select_keyed(&mut stmt, "projects", "identifier", "project__identifier");
    select_keyed(&mut stmt, "issues", "project_id", "project_id");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    select_keyed(&mut stmt, "issues", "created_at", "created_at");
    let mut where_all = issue_manager_condition();
    where_all = where_all.add(member_scope_condition(scope.user_id, scope.workspace_slug));
    stmt.distinct()
        .from(Alias::new("issues"))
        .join_as(
            sea_query::JoinType::LeftJoin,
            "states",
            Alias::new("states"),
            col("issues", "state_id").equals(("states", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("issues", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("issues", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(where_all);
    if let Some(arm) = issue_fts_arm(query, true) {
        stmt.and_where(arm);
    }
    if let Some(pid) = scope.narrow_project_id {
        stmt.and_where(col_eq_uuid("issues", "project_id", pid));
    }
    stmt.order_by(("issues", "created_at"), Order::Desc)
        .limit(100);
    stmt
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
    table: &'static str,
    scope: &GlobalScope<'_>,
    query: Option<&str>,
) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, table, "name", "name");
    select_keyed(&mut stmt, table, "id", "id");
    select_keyed(&mut stmt, table, "project_id", "project_id");
    select_keyed(&mut stmt, "projects", "identifier", "project__identifier");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, table, "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new(table))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col(table, "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col(table, "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null(table, "deleted_at"))
                .add(member_scope_condition(scope.user_id, scope.workspace_slug)),
        );
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm(table, &["name"], q) {
            stmt.and_where(arm);
        }
    }
    if let Some(pid) = scope.narrow_project_id {
        stmt.and_where(col_eq_uuid(table, "project_id", pid));
    }
    stmt.order_by((table, "created_at"), Order::Desc);
    stmt
}

/// `filter_cycles` (`base.py:111-133`).
pub fn global_cycles(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    global_named_in_project("cycles", scope, query)
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
pub fn global_modules(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    global_named_in_project("modules", scope, query)
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
pub fn global_views(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    global_named_in_project("issue_views", scope, query)
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
pub fn global_pages(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    // B9: `~Q(projects__id=True)` coerces True to uuid(int=1), so the
    // ArrayAgg keeps every project except that impossible id (no-op).
    let kept = || {
        col("project_pages", "project_id")
            .eq(uuid_val(PAGE_AGG_EXCLUDED_PROJECT_ID))
            .not()
    };
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "pages", "name", "name");
    select_keyed(&mut stmt, "pages", "id", "id");
    // `ARRAY_AGG(DISTINCT col) FILTER (WHERE NOT <excluded>)` is exactly
    // `ARRAY_AGG(DISTINCT CASE WHEN NOT <excluded> THEN col END)`: the
    // CASE yields NULL on excluded rows and ARRAY_AGG skips NULLs, so the
    // merged set (and the empty→COALESCE outcome) is identical.
    stmt.expr_as(
        Func::coalesce([
            PgFunc::array_agg_distinct({
                let case: SimpleExpr = sea_query::CaseStatement::new()
                    .case(kept(), col("project_pages", "project_id"))
                    .into();
                case
            })
            .into(),
            SimpleExpr::from(Expr::val(Value::Array(
                sea_query::ArrayType::Uuid,
                Some(Box::new(Vec::new())),
            ))),
        ]),
        Alias::new("project_ids"),
    );
    stmt.expr_as(
        Func::coalesce([
            PgFunc::array_agg_distinct({
                let case: SimpleExpr = sea_query::CaseStatement::new()
                    .case(kept(), col("projects", "identifier"))
                    .into();
                case
            })
            .into(),
            SimpleExpr::from(Expr::val(Value::Array(
                sea_query::ArrayType::String,
                Some(Box::new(Vec::new())),
            ))),
        ]),
        Alias::new("project_identifiers"),
    );
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    select_keyed(&mut stmt, "pages", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("pages"))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_pages",
            Alias::new("project_pages"),
            col("pages", "id").equals(("project_pages", "page_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("project_pages", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("pages", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("pages", "deleted_at"))
                .add(col_is_null("projects", "archived_at"))
                .add(col("project_members", "is_active").eq(Expr::val(true)))
                .add(col_eq_uuid("project_members", "member_id", scope.user_id))
                .add(col_eq("workspaces", "slug", scope.workspace_slug)),
        )
        .group_by_columns([
            (Alias::new("pages"), Alias::new("id")),
            (Alias::new("workspaces"), Alias::new("slug")),
        ]);
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm("pages", &["name"], q) {
            stmt.and_where(arm);
        }
    }
    if let Some(pid) = scope.narrow_project_id {
        // `pages.annotate(project_id=Subquery(...)).filter(project_id=…)`
        // (`:192-197`): the subquery carries the m2m model's default
        // `-created_at` ordering and `LIMIT 1` verbatim.
        let mut subquery = Query::select();
        subquery
            .expr(Expr::col(Alias::new("project_id")))
            .from(Alias::new("project_pages"))
            .cond_where(
                Condition::all()
                    .add(col_is_null("project_pages", "deleted_at"))
                    .add(col("project_pages", "page_id").equals(("pages", "id")))
                    .add(col_eq_uuid("project_pages", "project_id", pid)),
            )
            .order_by(("project_pages", "created_at"), Order::Desc)
            .limit(1);
        stmt.and_where(
            SimpleExpr::SubQuery(
                None,
                Box::new(sea_query::SubQueryStatement::SelectStatement(subquery)),
            )
            .eq(Expr::val(pid)),
        );
    }
    stmt.order_by(("pages", "created_at"), Order::Desc);
    stmt
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
pub fn global_intakes(scope: &GlobalScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "issues", "name", "name");
    select_keyed(&mut stmt, "issues", "id", "id");
    select_keyed(&mut stmt, "issues", "sequence_id", "sequence_id");
    select_keyed(&mut stmt, "projects", "identifier", "project__identifier");
    select_keyed(&mut stmt, "issues", "project_id", "project_id");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    select_keyed(&mut stmt, "issues", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("issues"))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("issues", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("issues", "workspace_id").equals(("workspaces", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "intake_issues",
            Alias::new("intake_issues"),
            col("issues", "id").equals(("intake_issues", "issue_id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("issues", "deleted_at"))
                .add(col_is_null("projects", "archived_at"))
                .add(col("project_members", "is_active").eq(Expr::val(true)))
                .add(col_eq_uuid("project_members", "member_id", scope.user_id))
                .add(col_eq("workspaces", "slug", scope.workspace_slug))
                .add(
                    Condition::any()
                        .add(col_eq("intake_issues", "status", 0i16))
                        .add(col_eq("intake_issues", "status", -2i16)),
                ),
        );
    if let Some(arm) = issue_fts_arm(query, false) {
        stmt.and_where(arm);
    }
    if let Some(pid) = scope.narrow_project_id {
        stmt.and_where(col_eq_uuid("issues", "project_id", pid));
    }
    stmt.order_by(("issues", "created_at"), Order::Desc)
        .limit(100);
    stmt
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
pub fn avatar_url_case() -> SimpleExpr {
    sea_query::CaseStatement::new()
        .case(
            col("users", "avatar_asset_id").is_not_null(),
            // `CONCAT(('/api/assets/v2/static/')::text, (CONCAT((avatar_asset_id)::text, ('/')::text))::text)`:
            // CONCAT ignores NULLs but both args are non-NULL on this arm.
            Func::cust("concat")
                .arg(
                    SimpleExpr::from(Expr::val("/api/assets/v2/static/"))
                        .cast_as(Alias::new("text")),
                )
                .arg(
                    Func::cust("concat")
                        .arg(col("users", "avatar_asset_id").cast_as(Alias::new("text")))
                        .arg(SimpleExpr::from(Expr::val("/")).cast_as(Alias::new("text"))),
                ),
        )
        .case(
            col("users", "avatar_asset_id").is_null(),
            col("users", "avatar"),
        )
        .finally(SimpleExpr::from(Expr::val::<Option<String>>(None)))
        .into()
}

/// Cycle `status` annotation (`base.py:412-430,604-621`). Django evaluates
/// `timezone.now()` once per `When`, so the handler binds `now` four times
/// (same value four times is semantically identical). The binds render with
/// an explicit `::timestamptz`: Django sends a timestamptz param while a
/// text bind would arrive typed `text`, and Postgres has no
/// `timestamptz <= text` operator — the cast keeps the identical instant
/// comparison (a parameter cast, so no plan change).
pub fn cycle_status_case(now: &str) -> SimpleExpr {
    let at = || SimpleExpr::from(Expr::val(now)).cast_as(Alias::new("timestamptz"));
    sea_query::CaseStatement::new()
        .case(
            col("cycles", "start_date")
                .lte(at())
                .and(col("cycles", "end_date").gte(at())),
            Expr::val("CURRENT"),
        )
        .case(col("cycles", "start_date").gt(at()), Expr::val("UPCOMING"))
        .case(col("cycles", "end_date").lt(at()), Expr::val("COMPLETED"))
        .case(
            col("cycles", "start_date")
                .is_null()
                .and(col("cycles", "end_date").is_null()),
            Expr::val("DRAFT"),
        )
        .finally(Expr::val("DRAFT"))
        .into()
}

/// `user_mention` branch (`base.py:304-351,505-544`): name-field icontains
/// over active non-bot memberships with the avatar annotation.
///
/// Asymmetry ported as-is: the project branch queries `ProjectMember`
/// (`DISTINCT`, `LEFT JOIN users`, `LIMIT` applied in Python after
/// `values()` — same emitted `LIMIT`), the workspace branch queries
/// `WorkspaceMember` (no `DISTINCT`, `INNER JOIN users`, in-DB `[:count]`).
pub fn entity_user_mention(scope: &EntityScope<'_>, query: Option<&str>) -> SelectStatement {
    // `member__first_name` etc. traverse the member join; the SQL columns
    // live on `users`.
    const FIELDS: &[&str] = &["first_name", "last_name", "display_name"];
    let limit = scope.count as u64;
    if let Some(pid) = scope.project_id {
        let mut stmt = Query::select();
        // Aliased to the `values()` keys (`member__*`), matching the decode.
        select_keyed(&mut stmt, "users", "display_name", "member__display_name");
        select_keyed(&mut stmt, "project_members", "member_id", "member__id");
        stmt.expr_as(avatar_url_case(), Alias::new("member__avatar_url"));
        // DISTINCT+ORDER BY compliance extra, never decoded.
        select_keyed(&mut stmt, "project_members", "created_at", "created_at");
        stmt.distinct()
            .from(Alias::new("project_members"))
            .join_as(
                sea_query::JoinType::LeftJoin,
                "users",
                Alias::new("users"),
                col("project_members", "member_id").equals(("users", "id")),
            )
            .join_as(
                sea_query::JoinType::InnerJoin,
                "workspaces",
                Alias::new("workspaces"),
                col("project_members", "workspace_id").equals(("workspaces", "id")),
            )
            .cond_where(
                Condition::all()
                    .add(col_is_null("project_members", "deleted_at"))
                    .add(col("project_members", "is_active").eq(Expr::val(true)))
                    .add(col("users", "is_bot").eq(Expr::val(false)))
                    .add(col_eq_uuid("project_members", "project_id", pid))
                    .add(col_eq("workspaces", "slug", scope.workspace_slug)),
            );
        if let Some(q) = opt(query) {
            if let Some(arm) = icontains_arm("users", FIELDS, q) {
                stmt.and_where(arm);
            }
        }
        stmt.order_by(("project_members", "created_at"), Order::Desc)
            .limit(limit);
        stmt
    } else {
        let mut stmt = Query::select();
        select_keyed(&mut stmt, "users", "display_name", "member__display_name");
        select_keyed(&mut stmt, "workspace_members", "member_id", "member__id");
        stmt.expr_as(avatar_url_case(), Alias::new("member__avatar_url"));
        // ORDER BY compliance extra, never decoded (no DISTINCT here).
        select_keyed(&mut stmt, "workspace_members", "created_at", "created_at");
        stmt.from(Alias::new("workspace_members"))
            .join_as(
                sea_query::JoinType::InnerJoin,
                "users",
                Alias::new("users"),
                col("workspace_members", "member_id").equals(("users", "id")),
            )
            .join_as(
                sea_query::JoinType::InnerJoin,
                "workspaces",
                Alias::new("workspaces"),
                col("workspace_members", "workspace_id").equals(("workspaces", "id")),
            )
            .cond_where(
                Condition::all()
                    .add(col_is_null("workspace_members", "deleted_at"))
                    .add(col("workspace_members", "is_active").eq(Expr::val(true)))
                    .add(col("users", "is_bot").eq(Expr::val(false)))
                    .add(col_eq("workspaces", "slug", scope.workspace_slug)),
            );
        if let Some(q) = opt(query) {
            if let Some(arm) = icontains_arm("users", FIELDS, q) {
                stmt.and_where(arm);
            }
        }
        stmt.order_by(("workspace_members", "created_at"), Order::Desc)
            .limit(limit);
        stmt
    }
}

/// Row keys of the `user_mention` branch (`base.py:345-349,542`).
pub const ENTITY_USER_MENTION_KEYS: &[&str] =
    &["member__avatar_url", "member__display_name", "member__id"];

/// `project` branch (`base.py:353-370,546-563`): `name` OR `identifier`
/// icontains; member-or-public (`network = 2`) scope; URL-slug scope. Both
/// source branches are identical, so this one builder serves both.
pub fn entity_project(scope: &EntityScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "projects", "name", "name");
    select_keyed(&mut stmt, "projects", "id", "id");
    select_keyed(&mut stmt, "projects", "identifier", "identifier");
    select_keyed(&mut stmt, "projects", "logo_props", "logo_props");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, "projects", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("projects"))
        .join_as(
            sea_query::JoinType::LeftJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("projects", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("projects", "deleted_at"))
                .add(
                    Condition::any()
                        .add(col_eq_uuid("project_members", "member_id", scope.user_id))
                        .add(col_eq("projects", "network", 2i16)),
                )
                .add(col_eq("workspaces", "slug", scope.workspace_slug)),
        );
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm("projects", &["name", "identifier"], q) {
            stmt.and_where(arm);
        }
    }
    stmt.order_by(("projects", "created_at"), Order::Desc)
        .limit(scope.count as u64);
    stmt
}

/// Row keys of the `project` branch (`base.py:368,561`).
pub const ENTITY_PROJECT_KEYS: &[&str] =
    &["name", "id", "identifier", "logo_props", "workspace__slug"];

/// `issue` branch (`base.py:372-394,565-586`): `issue_objects` scope plus
/// the project pin on the project branch, FTS with `include_comments=False`,
/// `-created_at`, `DISTINCT`, `values` + `[:count]`.
pub fn entity_issue(scope: &EntityScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "issues", "name", "name");
    select_keyed(&mut stmt, "issues", "id", "id");
    select_keyed(&mut stmt, "issues", "sequence_id", "sequence_id");
    select_keyed(&mut stmt, "projects", "identifier", "project__identifier");
    select_keyed(&mut stmt, "issues", "project_id", "project_id");
    select_keyed(&mut stmt, "issues", "priority", "priority");
    select_keyed(&mut stmt, "issues", "state_id", "state_id");
    select_keyed(&mut stmt, "issues", "type_id", "type_id");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, "issues", "created_at", "created_at");
    let mut where_all = issue_manager_condition();
    where_all = where_all.add(member_scope_condition(scope.user_id, scope.workspace_slug));
    stmt.distinct()
        .from(Alias::new("issues"))
        .join_as(
            sea_query::JoinType::LeftJoin,
            "states",
            Alias::new("states"),
            col("issues", "state_id").equals(("states", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("issues", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("issues", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(where_all);
    if let Some(pid) = scope.project_id {
        stmt.and_where(col_eq_uuid("issues", "project_id", pid));
    }
    if let Some(arm) = issue_fts_arm(query, false) {
        stmt.and_where(arm);
    }
    stmt.order_by(("issues", "created_at"), Order::Desc)
        .limit(scope.count as u64);
    stmt
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
/// scope (no archived-project arm — Django filters archived only on the
/// global sections), project pin on the project branch, the `status`
/// annotation, `-created_at`, `DISTINCT`, `values` + `[:count]`. `now` is the
/// `timezone.now()` value, bound four times (see [`cycle_status_case`]).
pub fn entity_cycle(scope: &EntityScope<'_>, query: Option<&str>, now: &str) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "cycles", "name", "name");
    select_keyed(&mut stmt, "cycles", "id", "id");
    select_keyed(&mut stmt, "cycles", "project_id", "project_id");
    select_keyed(&mut stmt, "projects", "identifier", "project__identifier");
    stmt.expr_as(cycle_status_case(now), Alias::new("status"));
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, "cycles", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("cycles"))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("cycles", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("cycles", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("cycles", "deleted_at"))
                .add(entity_member_scope_condition(
                    scope.user_id,
                    scope.workspace_slug,
                )),
        );
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm("cycles", &["name"], q) {
            stmt.and_where(arm);
        }
    }
    if let Some(pid) = scope.project_id {
        stmt.and_where(col_eq_uuid("cycles", "project_id", pid));
    }
    stmt.order_by(("cycles", "created_at"), Order::Desc)
        .limit(scope.count as u64);
    stmt
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
pub fn entity_module(scope: &EntityScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "modules", "name", "name");
    select_keyed(&mut stmt, "modules", "id", "id");
    select_keyed(&mut stmt, "modules", "project_id", "project_id");
    select_keyed(&mut stmt, "projects", "identifier", "project__identifier");
    select_keyed(&mut stmt, "modules", "status", "status");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, "modules", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("modules"))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("modules", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("modules", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("modules", "deleted_at"))
                .add(entity_member_scope_condition(
                    scope.user_id,
                    scope.workspace_slug,
                )),
        );
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm("modules", &["name"], q) {
            stmt.and_where(arm);
        }
    }
    if let Some(pid) = scope.project_id {
        stmt.and_where(col_eq_uuid("modules", "project_id", pid));
    }
    stmt.order_by(("modules", "created_at"), Order::Desc)
        .limit(scope.count as u64);
    stmt
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
pub fn entity_page(scope: &EntityScope<'_>, query: Option<&str>) -> SelectStatement {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "pages", "name", "name");
    select_keyed(&mut stmt, "pages", "id", "id");
    select_keyed(&mut stmt, "pages", "logo_props", "logo_props");
    select_keyed(&mut stmt, "project_pages", "project_id", "project_id");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, "pages", "created_at", "created_at");
    stmt.distinct()
        .from(Alias::new("pages"))
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_pages",
            Alias::new("project_pages"),
            col("pages", "id").equals(("project_pages", "page_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("project_pages", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("pages", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(
            Condition::all()
                .add(col_is_null("pages", "deleted_at"))
                .add(col("project_members", "is_active").eq(Expr::val(true)))
                .add(col_eq_uuid("project_members", "member_id", scope.user_id))
                .add(col_eq("workspaces", "slug", scope.workspace_slug))
                .add(col_eq("pages", "access", 0i16)),
        );
    if let Some(q) = opt(query) {
        if let Some(arm) = icontains_arm("pages", &["name"], q) {
            stmt.and_where(arm);
        }
    }
    if let Some(pid) = scope.project_id {
        stmt.and_where(col_eq_uuid("project_pages", "project_id", pid));
    } else {
        stmt.and_where(col_eq("pages", "is_global", true));
    }
    stmt.order_by(("pages", "created_at"), Order::Desc)
        .limit(scope.count as u64);
    stmt
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
pub fn build_issue_search(
    input: &IssueSearchInput<'_>,
) -> Result<SelectStatement, IssueSearchError> {
    let mut stmt = Query::select();
    select_keyed(&mut stmt, "issues", "name", "name");
    select_keyed(&mut stmt, "issues", "id", "id");
    select_keyed(&mut stmt, "issues", "start_date", "start_date");
    select_keyed(&mut stmt, "issues", "sequence_id", "sequence_id");
    select_keyed(&mut stmt, "projects", "name", "project__name");
    select_keyed(&mut stmt, "projects", "identifier", "project__identifier");
    select_keyed(&mut stmt, "issues", "project_id", "project_id");
    select_keyed(&mut stmt, "workspaces", "slug", "workspace__slug");
    select_keyed(&mut stmt, "states", "name", "state__name");
    select_keyed(&mut stmt, "states", "group", "state__group");
    select_keyed(&mut stmt, "states", "color", "state__color");
    // DISTINCT+ORDER BY compliance extra, never decoded.
    select_keyed(&mut stmt, "issues", "created_at", "created_at");
    let mut where_all = issue_manager_condition();
    where_all = where_all.add(member_scope_condition(input.user_id, input.slug));
    stmt.distinct()
        .from(Alias::new("issues"))
        .join_as(
            sea_query::JoinType::LeftJoin,
            "states",
            Alias::new("states"),
            col("issues", "state_id").equals(("states", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "projects",
            Alias::new("projects"),
            col("issues", "project_id").equals(("projects", "id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "project_members",
            Alias::new("project_members"),
            col("projects", "id").equals(("project_members", "project_id")),
        )
        .join_as(
            sea_query::JoinType::InnerJoin,
            "workspaces",
            Alias::new("workspaces"),
            col("issues", "workspace_id").equals(("workspaces", "id")),
        )
        .cond_where(where_all);

    // `filter_issues_by_project` (:24-31), gated by `project_narrow`.
    if project_narrow(input.workspace_search, Some(input.url_project_id)).is_some() {
        stmt.and_where(col_eq_uuid("issues", "project_id", input.url_project_id));
    }

    // `search_issues_by_query` (:33-40).
    if let Some(arm) = issue_fts_arm(input.query, false) {
        stmt.and_where(arm);
    }

    let known = input.issue_parent_id.is_some();
    let issue_id = opt(input.issue_id);

    // `search_issues_and_excluding_parent` (:42-50): self, parent, children.
    // A `None` parent renders Django's `NOT (id IS NULL)` tautology for that
    // arm, which is a no-op — skipped here with identical effect.
    // The children arm keeps Django's nullable-negation form: `~Q` over a
    // nullable column compiles to `NOT (col = X AND col IS NOT NULL)`
    // (verified against Django 4.2.30's compiler output) — a plain
    // `NOT (col = X)` would drop every NULL-parent (i.e. root) row.
    if input.parent {
        if let Some(xid) = issue_id {
            if let Some(parent) = input.issue_parent_id {
                stmt.and_where(col_eq_uuid("issues", "id", xid).not());
                if let Some(pid) = parent {
                    stmt.and_where(col_eq_uuid("issues", "id", pid).not());
                }
                stmt.and_where(
                    col_eq_uuid("issues", "parent_id", xid)
                        .and(col("issues", "parent_id").is_not_null())
                        .not(),
                );
            }
        }
    }

    // `filter_issues_excluding_related_issues` (:52-70): unknown `issue_id`
    // leaves the queryset unchanged (`if issue:` guard, `:67`).
    if input.issue_relation && issue_id.is_some() && known && !input.related_ids.is_empty() {
        // `NOT (id IN ...)` is exactly `NOT IN` (same NULL semantics).
        stmt.and_where(
            col("issues", "id")
                .is_in(input.related_ids.iter().map(|id| uuid_val(id)))
                .not(),
        );
    }

    // `filter_root_issues_only` (:72-81), B5 port: unknown `issue_id`
    // errors (the handler 500s) instead of filtering.
    if input.sub_issue && issue_id.is_some() {
        match input.issue_parent_id {
            None => return Err(IssueSearchError::UnknownIssue),
            Some(parent) => {
                stmt.and_where(col("issues", "id").ne(uuid_val(issue_id.unwrap_or_default())));
                stmt.and_where(col_is_null("issues", "parent_id"));
                if let Some(pid) = parent {
                    stmt.and_where(col("issues", "id").ne(uuid_val(pid)));
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
        let mut cycle_link = Query::select();
        cycle_link
            .expr(Expr::val(1))
            .from(Alias::new("cycle_issues"))
            .cond_where(
                Condition::all()
                    .add(col("cycle_issues", "issue_id").equals(("issues", "id")))
                    .add(col_is_null("cycle_issues", "deleted_at")),
            );
        stmt.and_where(Expr::exists(cycle_link).not());
    }

    // `exclude_issues_in_module` (:90-95): Django splits the multi-valued
    // `exclude()` into two subqueries — any link row to M (live or
    // soft-deleted) AND any live link to *any* module — so unlike the
    // cycle arm above, pinning M does NOT collapse to one `NOT EXISTS`.
    // Emitted verbatim in Django's shape (verified against the 4.2.30
    // compiler). `if module:` — empty was normalised to `None` by the
    // caller.
    if let Some(mid) = opt(input.module) {
        let mut to_module = Query::select();
        to_module
            .expr(Expr::val(1))
            .from(Alias::new("module_issues"))
            .cond_where(
                Condition::all()
                    .add(col("module_issues", "issue_id").equals(("issues", "id")))
                    .add(col_eq_uuid("module_issues", "module_id", mid)),
            );
        let mut any_module = Query::select();
        // Self-joined shape (`issues U0` / `module_issues U1`) kept verbatim:
        // correlated live-link existence. The U0/U1 aliases are literal table
        // aliases, matching Django's compiler output.
        any_module
            .expr(Expr::val(1))
            .from_as("issues", Alias::new("U0"))
            .join_as(
                sea_query::JoinType::LeftJoin,
                "module_issues",
                Alias::new("U1"),
                col("U0", "id").equals(("U1", "issue_id")),
            )
            .cond_where(
                Condition::all()
                    .add(col_is_null("U1", "deleted_at"))
                    .add(col("U0", "id").equals(("issues", "id"))),
            );
        stmt.and_where(Expr::exists(to_module).and(Expr::exists(any_module)).not());
    }

    // `filter_issues_without_target_date` (:97-102).
    if input.target_date_none {
        stmt.and_where(col_is_null("issues", "target_date"));
    }

    // Guest scoping (:146-149): `role=5` channel — `created_by` only.
    if input.guest {
        stmt.and_where(col_eq_uuid("issues", "created_by_id", input.user_id));
    }

    stmt.order_by(("issues", "created_at"), Order::Desc)
        .limit(100);
    Ok(stmt)
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

    // -- statement shapes --------------------------------------------------
    //
    // Builders render sea-query statements; shape tests pin the structure
    // (joins, scopes, arms, ordering, bind order). Lowercase match.

    fn sql_of(stmt: SelectStatement) -> String {
        sea_orm::DbBackend::Postgres.build(&stmt).sql.to_lowercase()
    }

    fn arm_of(expr: SimpleExpr) -> String {
        let mut stmt = Query::select();
        stmt.expr(Expr::val(1))
            .from(Alias::new("issues"))
            .and_where(expr);
        sql_of(stmt)
    }

    #[test]
    fn fts_arm_absent_without_query() {
        assert!(issue_fts_arm(None, true).is_none());
        assert!(issue_fts_arm(Some(""), true).is_none());
    }

    #[test]
    fn fts_arm_binds_three_params_in_order() {
        let arm = arm_of(issue_fts_arm(Some("auth 42"), false).unwrap());
        assert!(arm.contains("@@"), "{arm}");
        assert!(arm.contains("websearch_to_tsquery"), "{arm}");
        assert!(arm.contains("ilike"), "{arm}");
        assert!(arm.contains("exists"), "{arm}");
        assert!(arm.contains(" in ("), "{arm}");
        assert!(!arm.contains("issue_comments"), "{arm}");
    }

    #[test]
    fn fts_arm_with_comments_widens() {
        let arm = arm_of(issue_fts_arm(Some("refund"), true).unwrap());
        assert!(arm.contains("issue_comments"), "{arm}");
        assert!(arm.contains("deleted_at"), "{arm}");
    }

    // -- global sections -----------------------------------------------------

    #[test]
    fn global_issues_shape() {
        let sql = sql_of(global_issues(&global_scope(Some(PID)), Some("auth")));
        assert!(sql.starts_with("select distinct"), "{sql}");
        assert!(sql.contains("left join"), "{sql}");
        assert!(sql.contains("\"states\".\"group\""), "{sql}");
        assert!(sql.contains("in (select"), "{sql}");
        assert!(sql.contains("order by"), "{sql}");
        assert!(sql.contains("limit"), "{sql}");
        // Binds bind user and slug (values ride every statement param).
        assert!(sql.contains("\"project_members\".\"member_id\""), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\""), "{sql}");
    }

    #[test]
    fn global_issues_no_query_no_narrow() {
        let sql = sql_of(global_issues(&global_scope(None), None));
        assert!(!sql.contains("websearch_to_tsquery"), "{sql}");
        assert!(sql.contains("limit"), "{sql}");
    }

    // B7: intakes keep the soft-delete arm only — no triage/archived/draft
    // arms, no states join.
    #[test]
    fn intakes_use_unfiltered_manager() {
        let sql = sql_of(global_intakes(&global_scope(None), Some("auth")));
        assert!(sql.contains("deleted_at"), "{sql}");
        assert!(!sql.contains("triage"), "{sql}");
        assert!(!sql.contains("is_draft"), "{sql}");
        assert!(!sql.contains("states"), "{sql}");
        assert!(sql.contains("intake_issues"), "{sql}");
        assert!(sql.contains("limit"), "{sql}");
    }

    // B9: the ArrayAgg filter carries the coerced UUID-1 literal (bound).
    #[test]
    fn pages_carry_coerced_agg_filter() {
        let stmt = global_pages(&global_scope(Some(PID)), Some("acme"));
        let values = format!("{:?}", sea_orm::DbBackend::Postgres.build(&stmt).values);
        assert!(
            values.contains("00000000-0000-0000-0000-000000000001"),
            "{values}"
        );
        let sql = sql_of(stmt);
        assert!(sql.contains("array_agg(distinct"), "{sql}");
        assert!(sql.contains("coalesce"), "{sql}");
        assert!(sql.contains("group by"), "{sql}");
        // Narrow rides the ProjectPage subquery (which carries its own limit).
        assert!(sql.contains("project_pages"), "{sql}");
        assert!(sql.contains("limit"), "{sql}");
        let plain = sql_of(global_pages(&global_scope(None), None));
        assert!(!plain.contains("limit"), "{plain}");
    }

    #[test]
    fn workspaces_ignore_url_slug() {
        let sql = sql_of(global_workspaces(&global_scope(Some(PID)), Some("acme")));
        // `slug` is a selected key but never a filter on this section.
        assert!(!sql.contains("\"workspaces\".\"slug\" = $"), "{sql}");
        assert!(sql.contains("member_id"), "{sql}");
        assert!(sql.contains("ilike"), "{sql}");
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
        let project = sql_of(entity_user_mention(&entity_scope(Some(PID)), Some("acme")));
        assert!(project.contains("select distinct"), "{project}");
        assert!(project.contains("left join"), "{project}");
        assert!(project.contains("first_name"), "{project}");
        assert!(project.contains("ilike"), "{project}");
        assert!(project.contains("concat"), "{project}");
        assert!(project.contains("limit"), "{project}");
        let workspace = sql_of(entity_user_mention(&entity_scope(None), Some("acme")));
        assert!(!workspace.contains("distinct"), "{workspace}");
        assert!(workspace.contains("inner join"), "{workspace}");
        assert!(workspace.contains("workspace_members"), "{workspace}");
        assert!(
            !workspace.contains("\"project_members\".\"project_id\""),
            "{workspace}"
        );
    }

    #[test]
    fn entity_project_has_network_or_without_active() {
        let sql = sql_of(entity_project(&entity_scope(Some(PID)), Some("acme")));
        assert!(sql.contains("network"), "{sql}");
        assert!(!sql.contains("is_active"), "{sql}");
        assert!(sql.contains("left join"), "{sql}");
    }

    #[test]
    fn entity_issue_pin_only_on_project_branch() {
        let project = sql_of(entity_issue(&entity_scope(Some(PID)), Some("acme")));
        assert!(
            project.contains("\"issues\".\"project_id\" = cast("),
            "{project}"
        );
        assert!(!project.contains("issue_comments"), "{project}");
        let workspace = sql_of(entity_issue(&entity_scope(None), None));
        assert!(!workspace.contains("websearch_to_tsquery"), "{workspace}");
        assert!(workspace.contains("limit"), "{workspace}");
    }

    #[test]
    fn entity_cycle_status_case_binds_now_four_times() {
        let sql = sql_of(entity_cycle(
            &entity_scope(Some(PID)),
            None,
            "2026-09-29T20:00:00Z",
        ));
        assert!(sql.contains("case when"), "{sql}");
        // The four `now::timestamptz` comparisons (see cycle_status_case).
        assert_eq!(sql.matches("timestamptz").count(), 4, "{sql}");
        assert!(sql.contains("as \"status\""), "{sql}");
    }

    #[test]
    fn entity_module_selects_plain_status() {
        let sql = sql_of(entity_module(&entity_scope(None), Some("m")));
        assert!(sql.contains("status"), "{sql}");
        assert!(!sql.contains("case when"), "{sql}");
    }

    // PIDASHCONV-1450: the entity-search cycle/module branches carry no
    // archived-project arm — Django filters member/is_active/slug only.
    #[test]
    fn entity_cycle_module_include_archived_projects() {
        let now = "2026-09-29T20:00:00Z";
        for sql in [
            sql_of(entity_cycle(&entity_scope(Some(PID)), Some("acme"), now)),
            sql_of(entity_cycle(&entity_scope(None), Some("acme"), now)),
            sql_of(entity_module(&entity_scope(Some(PID)), Some("acme"))),
            sql_of(entity_module(&entity_scope(None), Some("acme"))),
        ] {
            assert!(!sql.contains("archived_at"), "{sql}");
            assert!(sql.contains("is_active"), "{sql}");
            assert!(sql.contains("member_id"), "{sql}");
        }
    }

    #[test]
    fn entity_page_access_rules() {
        let project = sql_of(entity_page(&entity_scope(Some(PID)), None));
        assert!(project.contains("access"), "{project}");
        assert!(project.contains("project_pages"), "{project}");
        assert!(!project.contains("is_global"), "{project}");
        let workspace = sql_of(entity_page(&entity_scope(None), None));
        assert!(workspace.contains("access"), "{workspace}");
        assert!(workspace.contains("is_global"), "{workspace}");
        assert!(
            !workspace.contains("\"project_pages\".\"project_id\" = $"),
            "{workspace}"
        );
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
        let sql = sql_of(build_issue_search(&minimal_input()).unwrap());
        assert!(sql.starts_with("select distinct"), "{sql}");
        assert!(sql.contains("\"states\".\"group\""), "{sql}");
        assert!(sql.contains("limit"), "{sql}");
        // workspace_search=true: no project pin; no query: no FTS arm.
        assert!(!sql.contains("websearch_to_tsquery"), "{sql}");
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
        let sql = sql_of(build_issue_search(&input).unwrap());
        assert!(sql.contains("websearch_to_tsquery"), "{sql}");
        // Nullable-negation form: NULL-parent rows survive the children
        // exclusion (Django 4.2.30 compiler output for `~Q(parent_id=…)`).
        assert!(sql.contains("parent_id"), "{sql}");
        assert!(
            sql.contains("not in") || sql.contains("\"issues\".\"id\" in"),
            "{sql}"
        );
        assert!(sql.contains("exists"), "{sql}");
        // Split exclude, not the cycle-style collapse: any link row to M
        // AND any live link to any module (Django 4.2.30 compiler output).
        assert!(sql.contains("module_issues"), "{sql}");
        assert!(sql.contains("target_date"), "{sql}");
        assert!(sql.contains("created_by_id"), "{sql}");
        assert!(sql.contains("limit"), "{sql}");
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
        let sql = sql_of(build_issue_search(&input).unwrap());
        assert!(!sql.contains("not in"), "{sql}");
        assert!(!sql.contains("parent_id"), "{sql}");
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
        let sql = sql_of(build_issue_search(&input).unwrap());
        assert!(!sql.contains("module_issues"), "{sql}");
        assert!(!sql.contains("parent_id"), "{sql}");
    }
}
