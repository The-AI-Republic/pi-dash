//! Project / member / invite / user querysets (D-19, stage 5).
//!
//! Ports the read querysets behind the api-v1 project, member, invite and
//! user routes to sea-query builders executed through SeaORM, recorded in
//! `rust-api/fixtures/v1_projects/queries/project_member.sql` (FX-Q-PROJMEM)
//! with result shapes in `project_member.rows.json`:
//!
//! * Q1 project list/detail base (`api/views/project.py:82-140` list,
//!   `:297-355` detail): workspace scoping + member scoping
//!   (`project_projectmember__member = user AND is_active OR network = 2`)
//!   with the six annotations `is_member`, `total_members`, `total_cycles`,
//!   `total_modules`, `member_role`, `is_deployed`. The detail base adds the
//!   `select_related("workspace", "workspace__owner", "default_assignee",
//!   "project_lead")` joins; the list base selects `project_lead` only.
//!   Both order by `kwargs order_by` default `-created_at` (always the
//!   default: `BaseAPIView` never sets the kwarg).
//! * Q2 list-GET variant (`:169-187`): the `sort_order` subquery annotation,
//!   the `project_projectmember` prefetch
//!   (`workspace__slug = slug, is_active`, `select_related("member")`),
//!   and re-ordering by `request.GET order_by` default `'sort_order'`.
//! * Q3 workspace members (`api/views/member.py:76-82`; 400 when the slug is
//!   unknown).
//! * Q4 project member ids + users (`:135-141`).
//! * Q5 project member detail + user (`:189-192,204,219`; `.get()` misses
//!   become 404 through the base exception handler).
//! * Q6 invite queryset + object (`api/views/invite.py:36-40`; `get_object`
//!   adds the pk predicate).
//! * Q7 current user (`api/views/user.py:40`): no query — the view
//!   serializes `request.user` from authentication. There is deliberately no
//!   builder for it here.
//!
//! Identifier-vs-uuid resolution (`Project.resolve`,
//! `db/models/project.py:191-224`, plus the `_rewrite_project_kwarg`
//! middleware, `api/views/base.py:52-104`) reuses the classification in
//! [`super::models::project`]: [`resolve_project`] picks the pk or the
//! identifier statement, [`fetch_project_id`] runs it. A UUID-looking input
//! is never tried as an identifier and non-UUID input is never tried as a
//! pk; no row means the caller 404s with the generic
//! [`super::models::project::NOT_FOUND_DETAIL`] (the input is never echoed).
//!
//! Every builder and fetch takes a [`TenantScope`]: the workspace slug from
//! the URL kwargs plus the acting user id (`self.kwargs.get("slug")` and
//! `request.user`). There is no unscoped entry point — the write-path
//! [`crate::context::RequestContext`] counterpart carries the same tenant for
//! writes; reads scope by slug + user exactly as the Django views do.
//!
//! Each builder returns a complete sea-query [`SelectStatement`] with its
//! values bound (the workspace slug, the acting user, the project id, the
//! membership pk, the member id, the invite pk). Execution runs through
//! SeaORM ([`ConnectionTrait`], [`DbBackend::Postgres`]) and rows decode by
//! name into [`FromQueryResult`] structs, so duplicated output names are
//! impossible by construction. The Q1 correlated subqueries are nested
//! selects; `EXISTS` renders via [`Expr::exists`]; counts use
//! [`Func::count`].
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `member_role` / `sort_order` are bare scalar subqueries with no
//!   `LIMIT 1`: a second active membership row fails the query instead of
//!   picking a row, mirroring Django's `MultipleObjectsReturned` 500. The
//!   partial unique index
//!   (`project_member_unique_project_member_when_deleted_at_null`) is what
//!   keeps this to one row.
//! * `total_members` counts non-bot active memberships with no
//!   `users.deleted_at` predicate — exactly as the fixture records it.
//!   Django's stock `UserManager` adds no soft-delete scope anywhere, so the
//!   `u.deleted_at IS NULL` guards in Q3/Q4 below are fixture-pinned shapes
//!   (kept verbatim; flagged for the domain gate to confirm against live
//!   Django).
//! * Q4 with zero member ids issues no query and returns `[]`, mirroring
//!   Django's `id__in=[]` empty-result short-circuit.
//! * Q4 keeps the `JOIN workspaces` + slug predicate from the fixture;
//!   `member_id` NULLs (the FK is nullable) are dropped before the users
//!   query, mirroring `id__in` semantics.
//! * The Q2 prefetch scopes by `workspace__slug`, not by project: Django
//!   matches prefetched rows to projects in Python. Handler-layer ports must
//!   do the same grouping.
//! * The invite list carries no `ORDER BY` (fixture Q6); row order is the
//!   plan order, as in Django.
//! * The workspace-exists check carries no `deleted_at` predicate (fixture
//!   Q3 `exists()`), while the workspace-id-for-slug lookup filters
//!   `deleted_at IS NULL` (a soft-deleted workspace reads as missing, the
//!   400 branch). Both shapes are kept, never unified.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use sea_orm::{ConnectionTrait, DbBackend, DbErr, FromQueryResult, QueryResult};
use sea_query::{
    Alias, Asterisk, Condition, Expr, Func, JoinType, Order, Query, SelectStatement, SimpleExpr,
};

use super::models::project::{classify_lookup, ProjectLookup};

// ---------------------------------------------------------------------------
// Tenant scope
// ---------------------------------------------------------------------------

/// Explicit per-request tenant scope for every query in this module.
///
/// `workspace_slug` is the URL `slug` kwarg (`self.kwargs.get("slug")`);
/// `actor_id` is `request.user.id`. Binding both on every statement is what
/// keeps the member scoping (`vis` join), the `is_member` / `member_role` /
/// `sort_order` subqueries and the workspace predicates tenant-correct.
#[derive(Debug, Clone, Copy)]
pub struct TenantScope<'a> {
    pub workspace_slug: &'a str,
    pub actor_id: uuid::Uuid,
}

impl<'a> TenantScope<'a> {
    pub fn new(workspace_slug: &'a str, actor_id: uuid::Uuid) -> Self {
        Self {
            workspace_slug,
            actor_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Builder helpers
// ---------------------------------------------------------------------------

fn col(table: &str, column: &str) -> Expr {
    Expr::col((Alias::new(table), Alias::new(column)))
}

fn tbl(name: &str) -> Alias {
    Alias::new(name)
}

fn scalar(sub: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(sub.into_sub_query_statement()))
}

// ---------------------------------------------------------------------------
// Q1 annotation subqueries (correlated to the outer `p` / `w` aliases)
// ---------------------------------------------------------------------------

/// `is_member`: the caller's active membership in this project + workspace.
fn is_member_expr(scope: &TenantScope<'_>) -> SimpleExpr {
    let mut sub = Query::select();
    sub.expr(Expr::value(1));
    sub.from_as(tbl("project_members"), Alias::new("pm"));
    sub.cond_where(
        Condition::all()
            .add(col("pm", "member_id").eq(scope.actor_id))
            .add(col("pm", "project_id").eq(col("p", "id")))
            .add(col("pm", "workspace_id").eq(col("w", "id")))
            .add(col("pm", "is_active").eq(true))
            .add(col("pm", "deleted_at").is_null()),
    );
    Expr::exists(sub)
}

/// `total_members`: non-bot active memberships of this project.
fn total_members_expr() -> SimpleExpr {
    let mut sub = Query::select();
    sub.expr(Func::count(col("pm", "id")));
    sub.from_as(tbl("project_members"), Alias::new("pm"));
    sub.join_as(
        JoinType::InnerJoin,
        tbl("users"),
        Alias::new("u"),
        col("pm", "member_id").equals((Alias::new("u"), Alias::new("id"))),
    );
    sub.cond_where(
        Condition::all()
            .add(col("pm", "project_id").eq(col("p", "id")))
            .add(col("u", "is_bot").eq(false))
            .add(col("pm", "is_active").eq(true))
            .add(col("pm", "deleted_at").is_null()),
    );
    scalar(sub)
}

/// `total_cycles`: live cycles of this project.
fn total_cycles_expr() -> SimpleExpr {
    let mut sub = Query::select();
    sub.expr(Func::count(col("c", "id")));
    sub.from_as(tbl("cycles"), Alias::new("c"));
    sub.cond_where(
        Condition::all()
            .add(col("c", "project_id").eq(col("p", "id")))
            .add(col("c", "deleted_at").is_null()),
    );
    scalar(sub)
}

/// `total_modules`: live modules of this project.
fn total_modules_expr() -> SimpleExpr {
    let mut sub = Query::select();
    sub.expr(Func::count(col("m", "id")));
    sub.from_as(tbl("modules"), Alias::new("m"));
    sub.cond_where(
        Condition::all()
            .add(col("m", "project_id").eq(col("p", "id")))
            .add(col("m", "deleted_at").is_null()),
    );
    scalar(sub)
}

/// `member_role`: the caller's own active role in this project, or NULL.
fn member_role_expr(scope: &TenantScope<'_>) -> SimpleExpr {
    let mut sub = Query::select();
    sub.column((Alias::new("pm"), Alias::new("role")));
    sub.from_as(tbl("project_members"), Alias::new("pm"));
    sub.cond_where(
        Condition::all()
            .add(col("pm", "project_id").eq(col("p", "id")))
            .add(col("pm", "member_id").eq(scope.actor_id))
            .add(col("pm", "is_active").eq(true))
            .add(col("pm", "deleted_at").is_null()),
    );
    scalar(sub)
}

/// `is_deployed`: a live deploy board for this project + workspace exists.
fn is_deployed_expr() -> SimpleExpr {
    let mut sub = Query::select();
    sub.expr(Expr::value(1));
    sub.from_as(tbl("deploy_boards"), Alias::new("d"));
    sub.cond_where(
        Condition::all()
            .add(col("d", "project_id").eq(col("p", "id")))
            .add(col("d", "workspace_id").eq(col("w", "id")))
            .add(col("d", "deleted_at").is_null()),
    );
    Expr::exists(sub)
}

/// `sort_order`: the caller's own active sort order in this workspace.
fn sort_order_expr(scope: &TenantScope<'_>) -> SimpleExpr {
    let mut sub = Query::select();
    sub.column((Alias::new("pm"), Alias::new("sort_order")));
    sub.from_as(tbl("project_members"), Alias::new("pm"));
    sub.cond_where(
        Condition::all()
            .add(col("pm", "member_id").eq(scope.actor_id))
            .add(col("pm", "project_id").eq(col("p", "id")))
            .add(col("pm", "workspace_id").eq(col("w", "id")))
            .add(col("pm", "is_active").eq(true))
            .add(col("pm", "deleted_at").is_null()),
    );
    scalar(sub)
}

// ---------------------------------------------------------------------------
// Q1 project list / detail builders
// ---------------------------------------------------------------------------

/// Shared Q1 select list and `FROM`/`JOIN` block (`views/project.py:82-140`
/// list, `:297-355` detail).
///
/// `SELECT "p".*` carries every project column (the fixture `SELECT p.*`);
/// the six annotations follow. The detail base adds the workspace-owner
/// (`INNER`, required FK), default-assignee and project-lead (`LEFT`,
/// nullable FKs) joins; the list base has only the project-lead join. Both
/// join `workspaces w` (slug predicate) and `project_members vis` (member
/// scoping) and group by `p.id, w.id`.
fn project_head(scope: &TenantScope<'_>, detail: bool, with_sort: bool) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("p"), Asterisk));
    sel.expr_as(is_member_expr(scope), Alias::new("is_member"));
    sel.expr_as(total_members_expr(), Alias::new("total_members"));
    sel.expr_as(total_cycles_expr(), Alias::new("total_cycles"));
    sel.expr_as(total_modules_expr(), Alias::new("total_modules"));
    sel.expr_as(member_role_expr(scope), Alias::new("member_role"));
    sel.expr_as(is_deployed_expr(), Alias::new("is_deployed"));
    if with_sort {
        sel.expr_as(sort_order_expr(scope), Alias::new("sort_order"));
    }
    sel.from_as(tbl("projects"), Alias::new("p"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("workspaces"),
        Alias::new("w"),
        col("p", "workspace_id").equals((Alias::new("w"), Alias::new("id"))),
    );
    if detail {
        sel.join_as(
            JoinType::InnerJoin,
            tbl("users"),
            Alias::new("workspace_owner"),
            col("w", "owner_id").equals((Alias::new("workspace_owner"), Alias::new("id"))),
        );
        sel.join_as(
            JoinType::LeftJoin,
            tbl("users"),
            Alias::new("default_assignee"),
            col("p", "default_assignee_id")
                .equals((Alias::new("default_assignee"), Alias::new("id"))),
        );
    }
    sel.join_as(
        JoinType::LeftJoin,
        tbl("users"),
        Alias::new("project_lead"),
        col("p", "project_lead_id").equals((Alias::new("project_lead"), Alias::new("id"))),
    );
    sel.join_as(
        JoinType::LeftJoin,
        tbl("project_members"),
        Alias::new("vis"),
        Condition::all()
            .add(col("vis", "project_id").equals((Alias::new("p"), Alias::new("id"))))
            .add(col("vis", "member_id").eq(scope.actor_id))
            .add(col("vis", "is_active").eq(true))
            .add(col("vis", "deleted_at").is_null()),
    );
    sel.cond_where(
        Condition::all()
            .add(col("w", "slug").eq(scope.workspace_slug))
            .add(col("p", "deleted_at").is_null())
            .add(
                Condition::any()
                    .add(col("vis", "id").is_not_null())
                    .add(col("p", "network").eq(2)),
            ),
    );
    sel.group_by_col((Alias::new("p"), Alias::new("id")));
    sel.group_by_col((Alias::new("w"), Alias::new("id")));
    sel
}

/// Q1 list base: `SELECT p.*` + six annotations, kwargs `-created_at` order.
pub fn project_list(scope: &TenantScope<'_>) -> SelectStatement {
    let mut sel = project_head(scope, false, false);
    sel.order_by((Alias::new("p"), Alias::new("created_at")), Order::Desc);
    sel
}

/// Q1 detail base: list base plus `select_related` joins, `-created_at`.
pub fn project_detail(scope: &TenantScope<'_>) -> SelectStatement {
    let mut sel = project_head(scope, true, false);
    sel.order_by((Alias::new("p"), Alias::new("created_at")), Order::Desc);
    sel
}

// ---------------------------------------------------------------------------
// Q2 list-GET ordering + builders
// ---------------------------------------------------------------------------

/// Default GET ordering (`request.GET order_by`, `project.py:186`).
pub const ORDER_GET_DEFAULT: &str = "sort_order";

/// Map a GET `order_by` key to its `(renders-on-alias, column, descending)`
/// triple. Unknown keys fall back to the GET default rather than emitting
/// SQL for an unmapped identifier (the view validates first).
fn get_order(key: &str) -> (bool, &'static str, Order) {
    match key {
        "sort_order" => (true, "sort_order", Order::Asc),
        "-sort_order" => (true, "sort_order", Order::Desc),
        "created_at" => (false, "created_at", Order::Asc),
        "-created_at" => (false, "created_at", Order::Desc),
        "updated_at" => (false, "updated_at", Order::Asc),
        "-updated_at" => (false, "updated_at", Order::Desc),
        "name" => (false, "name", Order::Asc),
        "-name" => (false, "name", Order::Desc),
        _ => (true, "sort_order", Order::Asc),
    }
}

/// Map a GET `order_by` key to its `ORDER BY` fragment.
///
/// Django resolves field names against the queryset (annotations included);
/// unknown keys fail in the view, so the handlers validate there. Unknown
/// keys here fall back to the GET default rather than emitting SQL for an
/// unmapped identifier.
pub fn order_by_sql(key: &str) -> &'static str {
    match key {
        "sort_order" => "ORDER BY sort_order",
        "-sort_order" => "ORDER BY sort_order DESC",
        "created_at" => "ORDER BY p.created_at ASC",
        "-created_at" => "ORDER BY p.created_at DESC",
        "updated_at" => "ORDER BY p.updated_at ASC",
        "-updated_at" => "ORDER BY p.updated_at DESC",
        "name" => "ORDER BY p.name ASC",
        "-name" => "ORDER BY p.name DESC",
        _ => "ORDER BY sort_order",
    }
}

/// Full list-GET statement: list base + `sort_order` annotation + GET
/// ordering (`views/project.py:169-187`).
pub fn project_list_get(scope: &TenantScope<'_>, order_by: &str) -> SelectStatement {
    let mut sel = project_head(scope, false, true);
    let (on_alias, column, direction) = get_order(order_by);
    if on_alias {
        sel.order_by(Alias::new(column), direction);
    } else {
        sel.order_by((Alias::new("p"), Alias::new(column)), direction);
    }
    sel
}

/// Member prefetch for the projects on the page (`:178-185`).
///
/// `WHERE pm.workspace_id = <id> AND pm.is_active AND pm.deleted_at IS NULL`
/// with the member user left-joined (`select_related("member")`). The user
/// columns are aliased `u_<col>` so the two `id` columns never collide.
/// Django scopes by `workspace__slug`; callers resolve the id once
/// ([`workspace_id_for_slug`]) and reuse it.
pub fn project_member_prefetch(workspace_id: uuid::Uuid) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("pm"), Asterisk));
    for column in ["id", "first_name", "last_name", "email", "avatar"] {
        sel.expr_as(col("u", column), Alias::new(format!("u_{column}")));
    }
    sel.from_as(tbl("project_members"), Alias::new("pm"));
    sel.join_as(
        JoinType::LeftJoin,
        tbl("users"),
        Alias::new("u"),
        col("pm", "member_id").equals((Alias::new("u"), Alias::new("id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("pm", "workspace_id").eq(workspace_id))
            .add(col("pm", "is_active").eq(true))
            .add(col("pm", "deleted_at").is_null()),
    );
    sel
}

// ---------------------------------------------------------------------------
// Q3 workspace members
// ---------------------------------------------------------------------------

/// Slug existence check (`views/member.py:76`): Django's `exists()` renders
/// `SELECT 1 ... LIMIT 1`; this carries no `deleted_at` predicate.
pub fn workspace_exists(scope: &TenantScope<'_>) -> SelectStatement {
    let mut sel = Query::select();
    sel.expr(Expr::value(1));
    sel.from(tbl("workspaces"));
    sel.cond_where(col("workspaces", "slug").eq(scope.workspace_slug));
    sel.limit(1);
    sel
}

/// Workspace id lookup for a slug: filters `deleted_at IS NULL` (a
/// soft-deleted workspace reads as missing, the 400 branch).
pub fn workspace_id_for_slug(workspace_slug: &str) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("workspaces"), Alias::new("id")));
    sel.from(tbl("workspaces"));
    sel.cond_where(
        Condition::all()
            .add(col("workspaces", "slug").eq(workspace_slug))
            .add(col("workspaces", "deleted_at").is_null()),
    );
    sel.limit(1);
    sel
}

/// Workspace members with their users (`views/member.py:82`).
///
/// Columns are named explicitly (`wm.member_id/role/workspace_id`) so the
/// two join tables' `id` columns never collide; the membership row is
/// filtered on both `wm` and `u` liveness, and users are left via the
/// inner join keep-on-user-exists semantics of the fixture `JOIN users`.
pub fn workspace_members_stmt(workspace_id: uuid::Uuid) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("wm"), Alias::new("member_id")));
    sel.column((Alias::new("wm"), Alias::new("role")));
    sel.column((Alias::new("wm"), Alias::new("workspace_id")));
    sel.from_as(tbl("workspace_members"), Alias::new("wm"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("users"),
        Alias::new("u"),
        col("u", "id").equals((Alias::new("wm"), Alias::new("member_id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("wm", "workspace_id").eq(workspace_id))
            .add(col("wm", "deleted_at").is_null())
            .add(col("u", "deleted_at").is_null()),
    );
    sel
}

// ---------------------------------------------------------------------------
// Q4 project member ids + users
// ---------------------------------------------------------------------------

/// Project member ids (`views/member.py:135-137`): bare `member_id` column,
/// joined to `workspaces` for the slug predicate. No `is_active` filter.
pub fn project_member_ids_stmt(workspace_slug: &str, project_id: uuid::Uuid) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("pm"), Alias::new("member_id")));
    sel.from_as(tbl("project_members"), Alias::new("pm"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("workspaces"),
        Alias::new("w"),
        col("pm", "workspace_id").equals((Alias::new("w"), Alias::new("id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("pm", "project_id").eq(project_id))
            .add(col("w", "slug").eq(workspace_slug))
            .add(col("pm", "deleted_at").is_null()),
    );
    sel
}

/// Users for an id list (`views/member.py:140`): the `UserLiteSerializer`
/// columns the views embed (id, names, email, avatar). Callers short-circuit
/// an empty id list before calling (Django `id__in=[]`).
pub fn project_users_stmt(ids: &[uuid::Uuid]) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("u"), Asterisk));
    sel.from_as(tbl("users"), Alias::new("u"));
    sel.cond_where(
        Condition::all()
            .add(col("u", "id").is_in(ids.to_vec()))
            .add(col("u", "deleted_at").is_null()),
    );
    sel
}

// ---------------------------------------------------------------------------
// Q5 project member detail
// ---------------------------------------------------------------------------

/// Project member detail (`.get(project_id, workspace__slug, pk)`,
/// `views/member.py:189,204,219`). `SELECT "pm".*` — a bare star would also
/// return `w.*` and duplicate `id`, mapping the workspace id over the
/// membership id. No `is_active` filter (fixture-pinned).
pub fn project_member_get_stmt(
    workspace_slug: &str,
    project_id: uuid::Uuid,
    pk: uuid::Uuid,
) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("pm"), Asterisk));
    sel.from_as(tbl("project_members"), Alias::new("pm"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("workspaces"),
        Alias::new("w"),
        col("pm", "workspace_id").equals((Alias::new("w"), Alias::new("id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("pm", "project_id").eq(project_id))
            .add(col("w", "slug").eq(workspace_slug))
            .add(col("pm", "id").eq(pk))
            .add(col("pm", "deleted_at").is_null()),
    );
    sel
}

/// Member user (`User.objects.get(id=...)`, `member.py:190`).
pub fn user_get_stmt(member_id: uuid::Uuid) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("users"), Asterisk));
    sel.from(tbl("users"));
    sel.cond_where(
        Condition::all()
            .add(col("users", "id").eq(member_id))
            .add(col("users", "deleted_at").is_null()),
    );
    sel
}

// ---------------------------------------------------------------------------
// Q6 invites
// ---------------------------------------------------------------------------

/// Invite list (`views/invite.py:36-37`): `SELECT "i".*`, no `ORDER BY`.
pub fn invites_list_stmt(workspace_slug: &str) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("i"), Asterisk));
    sel.from_as(tbl("workspace_member_invites"), Alias::new("i"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("workspaces"),
        Alias::new("w"),
        col("i", "workspace_id").equals((Alias::new("w"), Alias::new("id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("w", "slug").eq(workspace_slug))
            .add(col("i", "deleted_at").is_null()),
    );
    sel
}

/// Invite object (`.get(pk)`, `invite.py:40`): list scope + pk.
pub fn invite_get_stmt(workspace_slug: &str, pk: uuid::Uuid) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("i"), Asterisk));
    sel.from_as(tbl("workspace_member_invites"), Alias::new("i"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("workspaces"),
        Alias::new("w"),
        col("i", "workspace_id").equals((Alias::new("w"), Alias::new("id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("w", "slug").eq(workspace_slug))
            .add(col("i", "deleted_at").is_null())
            .add(col("i", "id").eq(pk)),
    );
    sel
}

// ---------------------------------------------------------------------------
// Identifier-vs-uuid resolution builders
// ---------------------------------------------------------------------------

/// `Project.resolve` pk branch (`db/models/project.py:199`).
pub fn resolve_by_pk_stmt(workspace_slug: &str, id: uuid::Uuid) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("p"), Alias::new("id")));
    sel.from_as(tbl("projects"), Alias::new("p"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("workspaces"),
        Alias::new("w"),
        col("p", "workspace_id").equals((Alias::new("w"), Alias::new("id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("w", "slug").eq(workspace_slug))
            .add(col("p", "id").eq(id))
            .add(col("p", "deleted_at").is_null()),
    );
    sel.limit(1);
    sel
}

/// `Project.resolve` identifier branch (`:204-212`): exact equality on the
/// stripped upper-cased identifier (never `iexact`).
pub fn resolve_by_identifier_stmt(workspace_slug: &str, name: &str) -> SelectStatement {
    let mut sel = Query::select();
    sel.column((Alias::new("p"), Alias::new("id")));
    sel.from_as(tbl("projects"), Alias::new("p"));
    sel.join_as(
        JoinType::InnerJoin,
        tbl("workspaces"),
        Alias::new("w"),
        col("p", "workspace_id").equals((Alias::new("w"), Alias::new("id"))),
    );
    sel.cond_where(
        Condition::all()
            .add(col("w", "slug").eq(workspace_slug))
            .add(col("p", "identifier").eq(name))
            .add(col("p", "deleted_at").is_null()),
    );
    sel.limit(1);
    sel
}

/// Pick the resolve statement for raw input, classifying exactly like
/// `Project.resolve` (`uuid.UUID(str(value))` → pk, errors → identifier).
pub fn resolve_project(scope: &TenantScope<'_>, raw: &str) -> SelectStatement {
    match classify_lookup(raw) {
        ProjectLookup::Pk(id) => resolve_by_pk_stmt(scope.workspace_slug, id),
        ProjectLookup::Identifier(name) => resolve_by_identifier_stmt(scope.workspace_slug, &name),
    }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// The six Q1 annotations plus the project identity columns the fixture
/// rows pin (`project_member.rows.json`).
///
/// `sort_order` is `Some` only on the list-GET statement (Q2); on the base
/// statements the column is absent and reads as `None` — mirroring the
/// fixture note that detail responses carry no `sort_order` key from the
/// queryset. `member_role` is NULL (not absent) without a membership row.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectAnnotations {
    pub id: uuid::Uuid,
    pub name: String,
    pub identifier: String,
    pub is_member: bool,
    pub total_members: i64,
    pub total_cycles: i64,
    pub total_modules: i64,
    pub member_role: Option<i32>,
    pub is_deployed: bool,
    pub sort_order: Option<f64>,
}

/// Q1 base row (no `sort_order` column).
#[derive(Debug, FromQueryResult)]
struct ProjectBaseRow {
    id: uuid::Uuid,
    name: String,
    identifier: String,
    is_member: bool,
    total_members: i64,
    total_cycles: i64,
    total_modules: i64,
    member_role: Option<i16>,
    is_deployed: bool,
}

/// Q2 list-GET row (with `sort_order`).
#[derive(Debug, FromQueryResult)]
struct ProjectGetRow {
    id: uuid::Uuid,
    name: String,
    identifier: String,
    is_member: bool,
    total_members: i64,
    total_cycles: i64,
    total_modules: i64,
    member_role: Option<i16>,
    is_deployed: bool,
    sort_order: Option<f64>,
}

impl From<ProjectBaseRow> for ProjectAnnotations {
    fn from(row: ProjectBaseRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            identifier: row.identifier,
            is_member: row.is_member,
            total_members: row.total_members,
            total_cycles: row.total_cycles,
            total_modules: row.total_modules,
            member_role: row.member_role.map(i32::from),
            is_deployed: row.is_deployed,
            sort_order: None,
        }
    }
}

impl From<ProjectGetRow> for ProjectAnnotations {
    fn from(row: ProjectGetRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            identifier: row.identifier,
            is_member: row.is_member,
            total_members: row.total_members,
            total_cycles: row.total_cycles,
            total_modules: row.total_modules,
            member_role: row.member_role.map(i32::from),
            is_deployed: row.is_deployed,
            sort_order: row.sort_order,
        }
    }
}

/// Map the annotation columns of a Q1/Q2 row (column names,
/// order-independent). A missing `sort_order` column reads as `None` (base
/// statements); a NULL value also reads as `None`.
pub fn map_project_annotations(row: &QueryResult) -> Result<ProjectAnnotations, DbErr> {
    let sort_order = row
        .try_get::<Option<f64>>("", "sort_order")
        .unwrap_or_default();
    let member_role: Option<i16> = row.try_get("", "member_role")?;
    Ok(ProjectAnnotations {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        identifier: row.try_get("", "identifier")?,
        is_member: row.try_get("", "is_member")?,
        total_members: row.try_get("", "total_members")?,
        total_cycles: row.try_get("", "total_cycles")?,
        total_modules: row.try_get("", "total_modules")?,
        member_role: member_role.map(i32::from),
        is_deployed: row.try_get("", "is_deployed")?,
        sort_order,
    })
}

/// One workspace membership with its workspace (`member.py:86-89` loop shape:
/// user data plus `role`).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceMemberRow {
    pub member_id: uuid::Uuid,
    pub role: i32,
    pub workspace_id: uuid::Uuid,
}

/// Raw Q3 row (`role` is `smallint`, widened to `i32` on mapping).
#[derive(Debug, FromQueryResult)]
struct WorkspaceMemberRaw {
    member_id: uuid::Uuid,
    role: i16,
    workspace_id: uuid::Uuid,
}

impl From<WorkspaceMemberRaw> for WorkspaceMemberRow {
    fn from(row: WorkspaceMemberRaw) -> Self {
        Self {
            member_id: row.member_id,
            role: i32::from(row.role),
            workspace_id: row.workspace_id,
        }
    }
}

/// Map one Q3 row.
pub fn map_workspace_member(row: &QueryResult) -> Result<WorkspaceMemberRow, DbErr> {
    let role: i16 = row.try_get("", "role")?;
    Ok(WorkspaceMemberRow {
        member_id: row.try_get("", "member_id")?,
        role: i32::from(role),
        workspace_id: row.try_get("", "workspace_id")?,
    })
}

/// The `UserLiteSerializer` columns the member views embed
/// (`api/serializers/user.py:13-38`: id, names, email, avatar; `avatar_url`
/// and `display_name` are derived properties, not columns).
#[derive(Debug, Clone, PartialEq)]
pub struct MemberUserLite {
    pub id: uuid::Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub avatar: Option<String>,
}

/// Raw user row for the lite projection.
#[derive(Debug, FromQueryResult)]
struct MemberUserRaw {
    id: uuid::Uuid,
    first_name: String,
    last_name: String,
    email: String,
    avatar: Option<String>,
}

impl From<MemberUserRaw> for MemberUserLite {
    fn from(row: MemberUserRaw) -> Self {
        Self {
            id: row.id,
            first_name: row.first_name,
            last_name: row.last_name,
            email: row.email,
            avatar: row.avatar,
        }
    }
}

/// Map one user row.
pub fn map_member_user(row: &QueryResult) -> Result<MemberUserLite, DbErr> {
    Ok(MemberUserLite {
        id: row.try_get("", "id")?,
        first_name: row.try_get("", "first_name")?,
        last_name: row.try_get("", "last_name")?,
        email: row.try_get("", "email")?,
        avatar: row.try_get("", "avatar")?,
    })
}

/// Membership reference for Q5 detail lookups.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberRef {
    pub id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub member_id: Option<uuid::Uuid>,
}

/// Raw Q5 row.
#[derive(Debug, FromQueryResult)]
struct ProjectMemberRefRaw {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    member_id: Option<uuid::Uuid>,
}

impl From<ProjectMemberRefRaw> for ProjectMemberRef {
    fn from(row: ProjectMemberRefRaw) -> Self {
        Self {
            id: row.id,
            project_id: row.project_id,
            member_id: row.member_id,
        }
    }
}

/// Map one Q5 membership row.
pub fn map_project_member_ref(row: &QueryResult) -> Result<ProjectMemberRef, DbErr> {
    Ok(ProjectMemberRef {
        id: row.try_get("", "id")?,
        project_id: row.try_get("", "project_id")?,
        member_id: row.try_get("", "member_id")?,
    })
}

/// The invite columns the fixture row pins (`invite.py` serialize shape).
#[derive(Debug, Clone, PartialEq)]
pub struct InviteRow {
    pub id: uuid::Uuid,
    pub email: String,
    pub accepted: bool,
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub role: i32,
}

/// Raw Q6 row (`role` is `smallint`; `responded_at` is `timestamptz`).
#[derive(Debug, FromQueryResult)]
struct InviteRaw {
    id: uuid::Uuid,
    email: String,
    accepted: bool,
    responded_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    role: i16,
}

impl From<InviteRaw> for InviteRow {
    fn from(row: InviteRaw) -> Self {
        Self {
            id: row.id,
            email: row.email,
            accepted: row.accepted,
            responded_at: row.responded_at.map(|t| t.with_timezone(&chrono::Utc)),
            role: i32::from(row.role),
        }
    }
}

/// Map one Q6 invite row.
pub fn map_invite_row(row: &QueryResult) -> Result<InviteRow, DbErr> {
    let role: i16 = row.try_get("", "role")?;
    let responded: Option<chrono::DateTime<chrono::FixedOffset>> =
        row.try_get("", "responded_at")?;
    Ok(InviteRow {
        id: row.try_get("", "id")?,
        email: row.try_get("", "email")?,
        accepted: row.try_get("", "accepted")?,
        responded_at: responded.map(|t| t.with_timezone(&chrono::Utc)),
        role: i32::from(role),
    })
}

// ---------------------------------------------------------------------------
// Fetches (SeaORM over `ConnectionTrait`, errors `DbErr`)
// ---------------------------------------------------------------------------

/// Whether the workspace slug exists (the `exists()` branch).
pub async fn fetch_workspace_exists<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
) -> Result<bool, DbErr> {
    let stmt = workspace_exists(scope);
    let row = db.query_one(DbBackend::Postgres.build(&stmt)).await?;
    Ok(row.is_some())
}

/// Workspace id for the scope slug, or `None` (the 400 branch).
pub async fn fetch_workspace_id<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
) -> Result<Option<uuid::Uuid>, DbErr> {
    let stmt = workspace_id_for_slug(scope.workspace_slug);
    let row = db.query_one(DbBackend::Postgres.build(&stmt)).await?;
    row.map(|r| r.try_get::<uuid::Uuid>("", "id")).transpose()
}

/// Member ids for a project, NULLs dropped.
///
/// `member` is nullable; `id__in` never matches NULL, so NULLs are filtered
/// before the users query.
pub async fn fetch_project_member_ids<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
    project_id: uuid::Uuid,
) -> Result<Vec<uuid::Uuid>, DbErr> {
    let stmt = project_member_ids_stmt(scope.workspace_slug, project_id);
    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
    rows.iter()
        .map(|r| r.try_get::<Option<uuid::Uuid>>("", "member_id"))
        .collect::<Result<Vec<Option<uuid::Uuid>>, DbErr>>()
        .map(|ids| ids.into_iter().flatten().collect())
}

/// Users for a member-id list; empty in, empty out with no query.
///
/// Mirrors Django's `id__in=[]` empty-result short-circuit: no statement is
/// issued for an empty list.
pub async fn fetch_project_users<C: ConnectionTrait>(
    db: &C,
    ids: &[uuid::Uuid],
) -> Result<Vec<MemberUserLite>, DbErr> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let stmt = project_users_stmt(ids);
    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
    rows.iter()
        .map(|r| MemberUserRaw::from_query_result(r, "").map(MemberUserLite::from))
        .collect()
}

/// Membership reference for a detail lookup, or `None` (the 404 branch).
pub async fn fetch_project_member<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
    project_id: uuid::Uuid,
    pk: uuid::Uuid,
) -> Result<Option<ProjectMemberRef>, DbErr> {
    let stmt = project_member_get_stmt(scope.workspace_slug, project_id, pk);
    let row = db.query_one(DbBackend::Postgres.build(&stmt)).await?;
    row.map(|r| ProjectMemberRefRaw::from_query_result(&r, "").map(ProjectMemberRef::from))
        .transpose()
}

/// User by id, or `None` (the 404 branch).
pub async fn fetch_user_by_id<C: ConnectionTrait>(
    db: &C,
    member_id: uuid::Uuid,
) -> Result<Option<MemberUserLite>, DbErr> {
    let stmt = user_get_stmt(member_id);
    let row = db.query_one(DbBackend::Postgres.build(&stmt)).await?;
    row.map(|r| MemberUserRaw::from_query_result(&r, "").map(MemberUserLite::from))
        .transpose()
}

/// All invites for the scope workspace.
pub async fn fetch_invites<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
) -> Result<Vec<InviteRow>, DbErr> {
    let stmt = invites_list_stmt(scope.workspace_slug);
    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
    rows.iter()
        .map(|r| InviteRaw::from_query_result(r, "").map(InviteRow::from))
        .collect()
}

/// One invite by pk, or `None` (the 404 branch).
pub async fn fetch_invite<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
    pk: uuid::Uuid,
) -> Result<Option<InviteRow>, DbErr> {
    let stmt = invite_get_stmt(scope.workspace_slug, pk);
    let row = db.query_one(DbBackend::Postgres.build(&stmt)).await?;
    row.map(|r| InviteRaw::from_query_result(&r, "").map(InviteRow::from))
        .transpose()
}

/// Resolve raw project input (uuid or identifier) to the project id.
///
/// Returns `None` when nothing matches; the caller raises the generic 404.
pub async fn fetch_project_id<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
    raw: &str,
) -> Result<Option<uuid::Uuid>, DbErr> {
    let stmt = resolve_project(scope, raw);
    let row = db.query_one(DbBackend::Postgres.build(&stmt)).await?;
    row.map(|r| r.try_get::<uuid::Uuid>("", "id")).transpose()
}

/// Annotated project rows for the list base statement.
pub async fn fetch_project_list<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
) -> Result<Vec<ProjectAnnotations>, DbErr> {
    let stmt = project_list(scope);
    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
    rows.iter()
        .map(|r| ProjectBaseRow::from_query_result(r, "").map(ProjectAnnotations::from))
        .collect()
}

/// Annotated project rows for the list-GET statement (with `sort_order`).
pub async fn fetch_project_list_get<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
    order_by: &str,
) -> Result<Vec<ProjectAnnotations>, DbErr> {
    let stmt = project_list_get(scope, order_by);
    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
    rows.iter()
        .map(|r| ProjectGetRow::from_query_result(r, "").map(ProjectAnnotations::from))
        .collect()
}

/// Annotated project rows for the detail base statement (no `sort_order`).
pub async fn fetch_project_detail<C: ConnectionTrait>(
    db: &C,
    scope: &TenantScope<'_>,
) -> Result<Vec<ProjectAnnotations>, DbErr> {
    let stmt = project_detail(scope);
    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
    rows.iter()
        .map(|r| ProjectBaseRow::from_query_result(r, "").map(ProjectAnnotations::from))
        .collect()
}

/// Workspace memberships for a resolved workspace id.
pub async fn fetch_workspace_members<C: ConnectionTrait>(
    db: &C,
    workspace_id: uuid::Uuid,
) -> Result<Vec<WorkspaceMemberRow>, DbErr> {
    let stmt = workspace_members_stmt(workspace_id);
    let rows = db.query_all(DbBackend::Postgres.build(&stmt)).await?;
    rows.iter()
        .map(|r| WorkspaceMemberRaw::from_query_result(r, "").map(WorkspaceMemberRow::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;
    use std::path::{Path, PathBuf};

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/v1_projects/queries")
    }

    fn read_fixture() -> String {
        let path = fixtures_dir().join("project_member.sql");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
    }

    fn fixture_lines() -> Vec<String> {
        read_fixture().lines().map(str::to_owned).collect()
    }

    fn clean(line: &str) -> String {
        let t = line.trim_start();
        let (is_comment, body) = match t.strip_prefix("--") {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let body = match body.find("--") {
            Some(pos) => &body[..pos],
            None => body,
        };
        let body = body.trim_end().trim_end_matches(';').trim_end();
        if is_comment {
            body.trim_start().to_owned()
        } else {
            body.to_owned()
        }
    }

    fn block(lines: &[&str], from: usize, to: usize) -> String {
        lines[from..=to]
            .iter()
            .map(|l| clean(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render(sel: &SelectStatement) -> String {
        sel.to_string(PostgresQueryBuilder)
    }

    fn actor() -> uuid::Uuid {
        uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()
    }

    fn scope() -> TenantScope<'static> {
        TenantScope::new("acme", actor())
    }

    #[test]
    fn list_base_carries_q1_annotations_scoping_and_grouping() {
        let sql = render(&project_list(&scope()));
        assert!(sql.contains(r#"FROM "projects" AS "p""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "workspaces" AS "w" ON "p"."workspace_id" = "w"."id""#),
            "{sql}"
        );
        for alias in [
            r#"AS "is_member""#,
            r#"AS "total_members""#,
            r#"AS "total_cycles""#,
            r#"AS "total_modules""#,
            r#"AS "member_role""#,
            r#"AS "is_deployed""#,
        ] {
            assert!(sql.contains(alias), "{sql}");
        }
        assert!(
            sql.contains(r#"LEFT JOIN "users" AS "project_lead""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "project_members" AS "vis""#),
            "{sql}"
        );
        assert!(sql.contains(r#""vis"."id" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""p"."network" = 2"#), "{sql}");
        assert!(sql.contains(r#"GROUP BY "p"."id", "w"."id""#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "p"."created_at" DESC"#), "{sql}");
        assert!(sql.contains(r#""w"."slug" = 'acme'"#), "{sql}");
        assert!(!sql.contains("sort_order"), "{sql}");
    }

    #[test]
    fn detail_base_adds_select_related_joins_without_sort_order() {
        let sql = render(&project_detail(&scope()));
        for join in [
            r#"INNER JOIN "users" AS "workspace_owner" ON "w"."owner_id" = "workspace_owner"."id""#,
            r#"LEFT JOIN "users" AS "default_assignee" ON "p"."default_assignee_id" = "default_assignee"."id""#,
            r#"LEFT JOIN "users" AS "project_lead""#,
        ] {
            assert!(sql.contains(join), "{sql}");
        }
        assert!(!sql.contains("sort_order"), "{sql}");
        assert!(sql.contains(r#"GROUP BY "p"."id", "w"."id""#), "{sql}");
    }

    #[test]
    fn list_get_adds_sort_order_annotation_and_get_ordering() {
        let sql = render(&project_list_get(&scope(), "sort_order"));
        assert!(sql.contains(r#"AS "sort_order""#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "sort_order" ASC"#), "{sql}");
        let bogus = render(&project_list_get(&scope(), "bogus"));
        assert!(bogus.contains(r#"ORDER BY "sort_order" ASC"#), "{bogus}");
        let created = render(&project_list_get(&scope(), "-created_at"));
        assert!(
            created.contains(r#"ORDER BY "p"."created_at" DESC"#),
            "{created}"
        );
        let base = render(&project_list(&scope()));
        assert!(base.contains(r#"ORDER BY "p"."created_at" DESC"#), "{base}");
        let detail = render(&project_detail(&scope()));
        assert!(
            detail.contains(r#"ORDER BY "p"."created_at" DESC"#),
            "{detail}"
        );
    }

    #[test]
    fn order_by_keys_map() {
        assert_eq!(order_by_sql("sort_order"), "ORDER BY sort_order");
        assert_eq!(order_by_sql("-sort_order"), "ORDER BY sort_order DESC");
        assert_eq!(order_by_sql("created_at"), "ORDER BY p.created_at ASC");
        assert_eq!(order_by_sql("-created_at"), "ORDER BY p.created_at DESC");
        assert_eq!(order_by_sql("updated_at"), "ORDER BY p.updated_at ASC");
        assert_eq!(order_by_sql("-updated_at"), "ORDER BY p.updated_at DESC");
        assert_eq!(order_by_sql("name"), "ORDER BY p.name ASC");
        assert_eq!(order_by_sql("-name"), "ORDER BY p.name DESC");
        assert_eq!(order_by_sql(""), "ORDER BY sort_order");
    }

    #[test]
    fn q2_sort_order_annotation_matches_fixture_shape() {
        let raw = read_fixture();
        for fragment in [
            "SELECT pm.sort_order FROM project_members pm",
            "pm.member_id = %(user)s AND pm.project_id = p.id",
            "pm.workspace_id = w.id AND pm.is_active",
            "pm.deleted_at IS NULL) AS sort_order",
        ] {
            assert!(raw.contains(fragment), "fixture Q2 misses {fragment}");
        }
        let sql = render(&project_list_get(&scope(), "sort_order"));
        assert!(sql.contains(r#""pm"."sort_order""#), "{sql}");
        assert!(sql.contains(r#""pm"."member_id""#), "{sql}");
        assert!(sql.contains(r#"AS "sort_order""#), "{sql}");
    }

    #[test]
    fn workspace_queries_match_fixture_q3() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let exists_at = refs
            .iter()
            .position(|l| clean(l).starts_with("SELECT 1 FROM workspaces"))
            .expect("Q3 exists");
        assert!(clean(refs[exists_at]).contains("slug = %(slug)s"));
        let members_at = refs
            .iter()
            .position(|l| l.trim_start().starts_with("SELECT wm.*, u.*"))
            .expect("Q3 members");
        let members_stmt = block(&refs, members_at, members_at + 2);
        assert!(members_stmt.contains("wm.workspace_id = %(ws)s"));
        let exists = render(&workspace_exists(&scope()));
        assert!(exists.contains(r#"FROM "workspaces""#), "{exists}");
        assert!(exists.contains(r#"LIMIT 1"#), "{exists}");
        assert!(!exists.contains("deleted_at"), "{exists}");
        let members = render(&workspace_members_stmt(
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
        ));
        assert!(
            members.contains(r#"FROM "workspace_members" AS "wm""#),
            "{members}"
        );
        assert!(
            members.contains(r#"INNER JOIN "users" AS "u" ON "u"."id" = "wm"."member_id""#),
            "{members}"
        );
        assert!(
            members.contains(r#""wm"."deleted_at" IS NULL"#),
            "{members}"
        );
        assert!(members.contains(r#""u"."deleted_at" IS NULL"#), "{members}");
    }

    #[test]
    fn project_member_queries_match_fixture_q4() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let ids_at = refs
            .iter()
            .position(|l| l.trim_start().starts_with("SELECT pm.member_id"))
            .expect("Q4 ids");
        assert!(block(&refs, ids_at, ids_at + 3).contains("pm.project_id = %(project)s"));
        let project_id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let ids = render(&project_member_ids_stmt("acme", project_id));
        assert!(ids.contains(r#""pm"."member_id""#), "{ids}");
        assert!(ids.contains(r#"JOIN "workspaces" AS "w""#), "{ids}");
        assert!(ids.contains(r#""w"."slug" = 'acme'"#), "{ids}");
        assert!(!ids.contains("is_active"), "{ids}");
        let users = render(&project_users_stmt(&[
            uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            uuid::Uuid::parse_str("77777777-7777-7777-7777-777777777777").unwrap(),
        ]));
        assert!(users.contains(r#"FROM "users" AS "u""#), "{users}");
        assert!(users.contains(" IN ("), "{users}");
        assert!(users.contains(r#""u"."deleted_at" IS NULL"#), "{users}");
    }

    #[test]
    fn member_detail_queries_match_fixture_q5() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let get_at = refs
            .iter()
            .position(|l| {
                l.trim_start()
                    .starts_with("SELECT * FROM project_members pm JOIN workspaces")
            })
            .expect("Q5 get");
        assert!(block(&refs, get_at, get_at + 2).contains("pm.id = %(pk)s"));
        let project_id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let pk = uuid::Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap();
        let got = render(&project_member_get_stmt("acme", project_id, pk));
        assert!(got.contains(r#"FROM "project_members" AS "pm""#), "{got}");
        assert!(got.contains(r#""pm"."id""#), "{got}");
        assert!(got.contains(r#""pm"."deleted_at" IS NULL"#), "{got}");
        assert!(!got.contains("is_active"), "{got}");
        let member_id = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let user = render(&user_get_stmt(member_id));
        assert!(user.contains(r#"FROM "users""#), "{user}");
        assert!(user.contains(r#""users"."deleted_at" IS NULL"#), "{user}");
    }

    #[test]
    fn invite_queries_match_fixture_q6() {
        let lines = fixture_lines();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let list_at = refs
            .iter()
            .position(|l| {
                l.trim_start()
                    .starts_with("SELECT * FROM workspace_member_invites i")
            })
            .expect("Q6 list");
        assert!(block(&refs, list_at, list_at + 2).contains("w.slug = %(slug)s"));
        let list = render(&invites_list_stmt("acme"));
        assert!(
            list.contains(r#"FROM "workspace_member_invites" AS "i""#),
            "{list}"
        );
        assert!(!list.contains("ORDER BY"), "{list}");
        let pk = uuid::Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();
        let one = render(&invite_get_stmt("acme", pk));
        assert!(one.contains(r#""i"."id""#), "{one}");
        assert!(one.contains(r#"JOIN "workspaces" AS "w""#), "{one}");
    }

    #[test]
    fn resolve_sql_branches_match_lookup() {
        let scope = scope();
        let pk_sql = render(&resolve_project(
            &scope,
            "11111111-1111-1111-1111-111111111111",
        ));
        assert!(pk_sql.contains(r#""p"."id""#), "{pk_sql}");
        assert!(pk_sql.contains(r#"LIMIT 1"#), "{pk_sql}");
        let ident_sql = render(&resolve_project(&scope, "eng"));
        assert!(ident_sql.contains(r#""p"."identifier""#), "{ident_sql}");
        assert!(ident_sql.contains("'ENG'"), "{ident_sql}");
        let padded = render(&resolve_project(&scope, " ENG "));
        assert!(padded.contains("'ENG'"), "{padded}");
    }

    // -- live scratch-DB tests (env-gated, SeaORM over `TxConn`) ----------------

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    const LIVE_DDL: &[&str] = &[
        "CREATE TEMPORARY TABLE workspaces (id UUID PRIMARY KEY, slug TEXT NOT NULL UNIQUE, owner_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE users (id UUID PRIMARY KEY, first_name TEXT NOT NULL DEFAULT '', last_name TEXT NOT NULL DEFAULT '', email TEXT NOT NULL DEFAULT '', avatar TEXT, is_bot BOOLEAN NOT NULL DEFAULT FALSE, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE projects (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, name TEXT NOT NULL DEFAULT '', identifier TEXT NOT NULL DEFAULT '', network INTEGER NOT NULL DEFAULT 2, project_lead_id UUID, default_assignee_id UUID, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE project_members (id UUID PRIMARY KEY, project_id UUID NOT NULL, workspace_id UUID NOT NULL, member_id UUID, role SMALLINT NOT NULL DEFAULT 5, sort_order DOUBLE PRECISION NOT NULL DEFAULT 65535, is_active BOOLEAN NOT NULL DEFAULT TRUE, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE cycles (id UUID PRIMARY KEY, project_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE modules (id UUID PRIMARY KEY, project_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE deploy_boards (id UUID PRIMARY KEY, project_id UUID NOT NULL, workspace_id UUID NOT NULL, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE workspace_members (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, member_id UUID NOT NULL, role SMALLINT NOT NULL DEFAULT 5, deleted_at TIMESTAMPTZ)",
        "CREATE TEMPORARY TABLE workspace_member_invites (id UUID PRIMARY KEY, workspace_id UUID NOT NULL, email TEXT NOT NULL, accepted BOOLEAN NOT NULL DEFAULT FALSE, responded_at TIMESTAMPTZ, role SMALLINT NOT NULL DEFAULT 5, deleted_at TIMESTAMPTZ)",
    ];

    fn live_uuid(s: &str) -> uuid::Uuid {
        uuid::Uuid::parse_str(s).expect("fixed test uuid")
    }

    const WS: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const MEMBER: &str = "22222222-2222-2222-2222-222222222222";
    const MEMBER2: &str = "77777777-7777-7777-7777-777777777777";
    const BOT: &str = "33333333-3333-3333-3333-333333333333";
    const PROJECT: &str = "11111111-1111-1111-1111-111111111111";
    const PRIVATE_PROJECT: &str = "55555555-5555-5555-5555-555555555555";
    const MEMBERSHIP: &str = "66666666-6666-6666-6666-666666666666";
    const INVITE: &str = "44444444-4444-4444-4444-444444444444";

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        for ddl in LIVE_DDL {
            sqlx::query(ddl)
                .execute(&mut *tx)
                .await
                .expect("create temp table");
        }
        tx
    }

    async fn seed_full(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) {
        sqlx::query("INSERT INTO workspaces (id, slug, owner_id) VALUES ($1, 'acme', $2)")
            .bind(live_uuid(WS))
            .bind(live_uuid(MEMBER))
            .execute(&mut **tx)
            .await
            .expect("seed workspace");
        for (id, first, last, email, bot) in [
            (MEMBER, "Ct", "Member", "m@example.com", false),
            (MEMBER2, "Second", "Human", "s@example.com", false),
            (BOT, "Bot", "B", "b@example.com", true),
        ] {
            sqlx::query("INSERT INTO users (id, first_name, last_name, email, is_bot) VALUES ($1, $2, $3, $4, $5)")
                .bind(live_uuid(id))
                .bind(first)
                .bind(last)
                .bind(email)
                .bind(bot)
                .execute(&mut **tx)
                .await
                .expect("seed user");
        }
        sqlx::query("INSERT INTO projects (id, workspace_id, name, identifier, network) VALUES ($1, $2, 'Engine', 'ENG', 2)")
            .bind(live_uuid(PROJECT))
            .bind(live_uuid(WS))
            .execute(&mut **tx)
            .await
            .expect("seed project");
        sqlx::query("INSERT INTO projects (id, workspace_id, name, identifier, network) VALUES ($1, $2, 'Secret', 'SEC', 0)")
            .bind(live_uuid(PRIVATE_PROJECT))
            .bind(live_uuid(WS))
            .execute(&mut **tx)
            .await
            .expect("seed private project");
        for (id, project, member, role, sort) in [
            (MEMBERSHIP, PROJECT, MEMBER, 20i16, 55535.0),
            (
                "88888888-8888-8888-8888-888888888888",
                PROJECT,
                MEMBER2,
                15i16,
                60000.0,
            ),
            (
                "99999999-9999-9999-9999-999999999999",
                PROJECT,
                BOT,
                15i16,
                61000.0,
            ),
            (
                "aaaaaaaa-1111-2222-3333-444444444444",
                PRIVATE_PROJECT,
                MEMBER,
                15i16,
                50000.0,
            ),
        ] {
            sqlx::query("INSERT INTO project_members (id, project_id, workspace_id, member_id, role, sort_order) VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(live_uuid(id))
                .bind(live_uuid(project))
                .bind(live_uuid(WS))
                .bind(live_uuid(member))
                .bind(role)
                .bind(sort)
                .execute(&mut **tx)
                .await
                .expect("seed membership");
        }
        sqlx::query("INSERT INTO workspace_members (id, workspace_id, member_id, role) VALUES ($1, $2, $3, 20)")
            .bind(live_uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"))
            .bind(live_uuid(WS))
            .bind(live_uuid(MEMBER))
            .execute(&mut **tx)
            .await
            .expect("seed workspace membership");
        sqlx::query("INSERT INTO workspace_member_invites (id, workspace_id, email, role) VALUES ($1, $2, 'new@example.com', 15)")
            .bind(live_uuid(INVITE))
            .bind(live_uuid(WS))
            .execute(&mut **tx)
            .await
            .expect("seed invite");
    }

    #[tokio::test]
    async fn live_list_annotations_match_fixture_rows() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        seed_full(&mut tx).await;
        let rows = {
            let conn = crate::orm_tx::TxConn::new(&mut tx);
            fetch_project_list_get(&conn, &scope(), "sort_order")
                .await
                .expect("list get")
        };
        assert_eq!(rows.len(), 2, "public + member-visible private project");
        let eng = rows
            .iter()
            .find(|r| r.id == live_uuid(PROJECT))
            .expect("Engine row");
        assert_eq!(eng.name, "Engine");
        assert_eq!(eng.identifier, "ENG");
        assert!(eng.is_member);
        assert_eq!(eng.member_role, Some(20));
        assert_eq!(eng.total_members, 2);
        assert_eq!(eng.total_cycles, 0);
        assert_eq!(eng.total_modules, 0);
        assert!(!eng.is_deployed);
        assert_eq!(eng.sort_order, Some(55535.0));
        let sec = rows
            .iter()
            .find(|r| r.id == live_uuid(PRIVATE_PROJECT))
            .expect("Secret row");
        assert!(sec.is_member);
        assert_eq!(sec.member_role, Some(15));
        let base = {
            let conn = crate::orm_tx::TxConn::new(&mut tx);
            fetch_project_list(&conn, &scope())
                .await
                .expect("list base")
        };
        let eng_base = base
            .iter()
            .find(|r| r.id == live_uuid(PROJECT))
            .expect("Engine base row");
        assert_eq!(eng_base.member_role, Some(20));
        assert_eq!(eng_base.sort_order, None);
    }

    #[tokio::test]
    async fn live_member_scoping() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        seed_full(&mut tx).await;
        let stranger = TenantScope::new("acme", live_uuid("00000000-0000-0000-0000-000000000000"));
        let rows = {
            let conn = crate::orm_tx::TxConn::new(&mut tx);
            fetch_project_list(&conn, &stranger)
                .await
                .expect("stranger list")
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, live_uuid(PROJECT));
        assert!(!rows[0].is_member);
        assert_eq!(rows[0].member_role, None);
        let detail = {
            let conn = crate::orm_tx::TxConn::new(&mut tx);
            fetch_project_detail(&conn, &stranger)
                .await
                .expect("stranger detail")
        };
        assert_eq!(detail.len(), 1);
        let elsewhere = TenantScope::new("nope", live_uuid(MEMBER));
        let empty = {
            let conn = crate::orm_tx::TxConn::new(&mut tx);
            fetch_project_list(&conn, &elsewhere).await.expect("empty")
        };
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn live_resolve_uuid_identifier_missing() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        seed_full(&mut tx).await;
        assert_eq!(
            {
                let conn = crate::orm_tx::TxConn::new(&mut tx);
                fetch_project_id(&conn, &scope(), PROJECT)
                    .await
                    .expect("pk")
            },
            Some(live_uuid(PROJECT))
        );
        assert_eq!(
            {
                let conn = crate::orm_tx::TxConn::new(&mut tx);
                fetch_project_id(&conn, &scope(), "eng")
                    .await
                    .expect("ident")
            },
            Some(live_uuid(PROJECT))
        );
        assert_eq!(
            {
                let conn = crate::orm_tx::TxConn::new(&mut tx);
                fetch_project_id(&conn, &scope(), " ENG ")
                    .await
                    .expect("padded")
            },
            Some(live_uuid(PROJECT))
        );
        assert_eq!(
            {
                let conn = crate::orm_tx::TxConn::new(&mut tx);
                fetch_project_id(&conn, &scope(), "00000000-0000-0000-0000-000000000000")
                    .await
                    .expect("bogus uuid")
            },
            None
        );
        assert_eq!(
            {
                let conn = crate::orm_tx::TxConn::new(&mut tx);
                fetch_project_id(&conn, &scope(), "NOPE")
                    .await
                    .expect("unknown")
            },
            None
        );
        sqlx::query("UPDATE projects SET deleted_at = now() WHERE id = $1")
            .bind(live_uuid(PROJECT))
            .execute(&mut *tx)
            .await
            .expect("soft delete");
        assert_eq!(
            {
                let conn = crate::orm_tx::TxConn::new(&mut tx);
                fetch_project_id(&conn, &scope(), "ENG")
                    .await
                    .expect("deleted")
            },
            None
        );
    }
}
