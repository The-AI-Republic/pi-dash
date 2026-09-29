//! Intake read queries (D-32, stage 5).
//!
//! Ports the five query units of `apps/api/pi_dash/app/views/intake/base.py`
//! for the db layer:
//!
//! * [`intake_list_sql`] — `IntakeViewSet.get_queryset` + `list`
//!   (`base.py:60-75`).
//! * [`intake_issue_queryset_sql`] — `IntakeIssueViewSet.get_queryset`
//!   (`base.py:100-174`). **Reachability:** this queryset is unreachable
//!   through the viewset — `list` (`:177`), `create` (`:222`),
//!   `partial_update` (`:329`), `retrieve` (`:503`) and `destroy` (`:550`)
//!   every one builds its own queryset and never calls `get_queryset()`.
//!   Ported as written, per the issue instruction.
//! * [`intake_issue_list_sql`] — `IntakeIssueViewSet.list`
//!   (`base.py:176-219`).
//! * [`intake_issue_detail_sql`], [`intake_issue_lookup_sql`],
//!   [`destroy_issue_lookup_sql`] — detail lookups for `retrieve`
//!   (`:506-530`), `partial_update` (`:333-339`, `:379-396`, `:474-498`)
//!   and `destroy` (`:551-557`), plus the guest-role narrowing each
//!   applies.
//! * [`description_versions_sql`] — `IntakeWorkItemDescriptionVersionEndpoint`
//!   list queryset (`base.py:625-627`) with [`REQUIRED_FIELDS`] (`:612-623`)
//!   and the `user_timezone_converter` contract (`:570-576`).
//!
//! Dynamic statements are sea-query builders; fixed lookups are string
//! constants executed with runtime `sqlx::query` (no `query!` macros:
//! there is no build-time database, same as the merged `license/queries`
//! and `loop/queries` precedent). `$N` placeholders are Postgres binds;
//! the Django fixtures spell them `%s` and the tests normalize both.
//! `LIMIT`/`OFFSET` are inlined literals, exactly as Django renders them.
//!
//! SQL semantics are Django's, quirks included (translate, don't
//! redesign):
//!
//! * Every read is soft-delete scoped (`deleted_at IS NULL` from the
//!   default manager, `mixins.py:56-58`). The description-versions
//!   fixture (`description_versions.sql.json`) omits the guard in its
//!   reconstructed template; the builder keeps it because real Django
//!   output has it (`IssueDescriptionVersion` inherits the scope via
//!   `ProjectBaseModel` → `AuditModel` → `SoftDeleteModel`).
//! * Fixture array-aggregate fragments spell joins with Django
//!   lookup-path aliases (`label_issue`, `issue_assignee`,
//!   `issue_module`, `assignees`); the builders use the real table
//!   names (`issue_labels`, `issue_assignees`, `module_issues`,
//!   `users` — the `assignees` M2M target is `AUTH_USER_MODEL`,
//!   `db_table "users"`, `issue.py:161-167`) and the `member_project`
//!   join for the `is_active` guard (`project.py:338`). The tests apply
//!   this documented alias map before comparing.
//! * `link_count` / `attachment_count` / `sub_issues_count` have no
//!   `Coalesce` — they render `NULL`, not `0`, when the subquery finds
//!   no rows. Ported as-is.
//! * Subquery table aliases stay `U0`, Django's own rendering.
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * Raw `order_by` passthrough (`base.py:198`): an unknown field raises
//!   `FieldError`, which `handle_exception` maps to
//!   `500 {"error": "Something went wrong please try again later"}`.
//!   [`order_to_sql`] passes unknown values through untouched; mapping
//!   the failure to the 500 body is handlers-owned.
//! * Falsy status-CSV skip (`base.py:200-202`): `?status=` with only
//!   `null` tokens parses to `[]`, which is falsy, so **no** status
//!   filter applies. [`parse_intake_status`] returns `None` for that
//!   case (and for the absent-param default it returns `Some(vec![-2])`).
//! * Non-numeric status tokens raise at query execution in Django
//!   (`IntegerField` coercion) → 500 via `handle_exception`.
//!   [`parse_intake_status`] surfaces them as [`StatusParseError`];
//!   mapping that error to the 500 body is handlers-owned.
//! * `assignee_ids` guard asymmetry, ported per call site: the
//!   `get_queryset` form (`:149-160`) checks `is_active` **and**
//!   through-table `deleted_at`; the create re-fetch (`:307-315`) form
//!   drops the `deleted_at` check; the `retrieve` (`:518-527`),
//!   partial-update re-fetch (`:486-495`) and partial-update issue
//!   annotate (`:379-396`) forms drop the `is_active` check instead.
//!   Each form has its own fragment function below.
//!
//! Fixture source of truth:
//! `rust-api/fixtures/app_intake/queries/*.sql.json` (recorded by
//! PIDASHCONV-278; trace lines in `../TRACE.md`). The `#[cfg(test)]`
//! suite matches every builder against its fixture statement in
//! normalized semantic form and replays the fixture example rows'
//! shapes.

use sea_query::{Alias, Condition, Expr, JoinType, Order, Query, SelectStatement};

use super::models::{intake, intake_issue};

// ---------------------------------------------------------------------------
// Shared table names (Django `db_table`; see the alias-map note above)
// ---------------------------------------------------------------------------

/// `issues` (`issue.py:253`).
pub const ISSUE_TABLE: &str = "issues";
/// `workspaces` (tenant join target for every `workspace__slug` filter).
pub const WORKSPACE_TABLE: &str = "workspaces";
/// `projects` (`select_related("project")` join target).
pub const PROJECT_TABLE: &str = "projects";
/// `states` (`select_related("state")` join target).
pub const STATE_TABLE: &str = "states";
/// `users` (the `assignees` M2M target, `AUTH_USER_MODEL`).
pub const USER_TABLE: &str = "users";
/// `labels` (the `labels` M2M target, `label.py:43`).
pub const LABEL_TABLE: &str = "labels";
/// `issue_labels` through table (related name `label_issue`, `issue.py:676`).
pub const ISSUE_LABEL_TABLE: &str = "issue_labels";
/// `issue_assignees` through table (related name `issue_assignee`,
/// `issue.py:464`).
pub const ISSUE_ASSIGNEE_TABLE: &str = "issue_assignees";
/// `project_members` (`member_project` related name, `project.py:338`).
pub const PROJECT_MEMBER_TABLE: &str = "project_members";
/// `module_issues` through table (the `issue_module` relation,
/// `module.py:167`).
pub const MODULE_ISSUE_TABLE: &str = "module_issues";
/// `modules` (`module.py:112`).
pub const MODULE_TABLE: &str = "modules";
/// `cycle_issues` (`cycle.py:123`).
pub const CYCLE_ISSUE_TABLE: &str = "cycle_issues";
/// `issue_links` (`issue.py:480`).
pub const ISSUE_LINK_TABLE: &str = "issue_links";
/// `file_assets` (`asset.py:67`).
pub const FILE_ASSET_TABLE: &str = "file_assets";
/// `issue_description_versions` (`issue.py:924`).
pub const DESCRIPTION_VERSION_TABLE: &str = "issue_description_versions";

/// The `ISSUE_ATTACHMENT` entity-type literal
/// (`asset.py:33-34`, `EntityTypeContext`).
pub const ENTITY_TYPE_ISSUE_ATTACHMENT: &str = "ISSUE_ATTACHMENT";

/// Django's subquery table alias, kept verbatim.
const U0: &str = "U0";

fn t(table: &str) -> Alias {
    Alias::new(table.to_owned())
}

fn c(table: &str, col: &str) -> (Alias, Alias) {
    (t(table), Alias::new(col.to_owned()))
}

/// Soft-delete scope every default-manager read carries
/// (`mixins.py:56-58`).
fn active(sel: &mut SelectStatement, table: &str) {
    sel.and_where(Expr::col(c(table, "deleted_at")).is_null());
}

// ---------------------------------------------------------------------------
// Unit 1 — IntakeViewSet.get_queryset + list (base.py:60-70, :72-75)
// ---------------------------------------------------------------------------

/// `IntakeViewSet.get_queryset` (`base.py:60-70`) as SQL text.
///
/// `Intake.objects.filter(workspace__slug=$1, project_id=$2)` with
/// `pending_issue_count = Count("issue_intake", filter=status=-2)`
/// (`:68`), `select_related("workspace", "project")` (`:69` — the join
/// is emitted; the projection stays `intakes.*`, exactly as the
/// `intake_list.sql.json` fixture records), model `Meta.ordering`
/// (`name` ASC, `intake.py:34`), and `.first()` (`:74` → `LIMIT 1`).
/// `list` (`:72-75`) serves the first row through `IntakeSerializer`;
/// a missing row renders serializer-`None` (null body, still 200) —
/// handlers-owned, noted here so the null is reproduced, not 404'd.
pub fn intake_list_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.expr(Expr::cust(format!(r#""{t}".*"#, t = intake::TABLE)))
        .expr_as(
            Expr::cust(format!(
                r#"COUNT("{t}"."id") FILTER (WHERE "{t}"."status" = -2)"#,
                t = intake_issue::TABLE
            )),
            Alias::new("pending_issue_count"),
        )
        .from(t(intake::TABLE))
        .join(
            JoinType::InnerJoin,
            t(WORKSPACE_TABLE),
            Expr::col(c(intake::TABLE, "workspace_id")).equals(c(WORKSPACE_TABLE, "id")),
        )
        .cond_where(
            Condition::all()
                .add(Expr::cust(r#""workspaces"."slug" = $1"#))
                .add(Expr::cust(format!(
                    r#""{t}"."project_id" = $2"#,
                    t = intake::TABLE
                ))),
        );
    active(&mut sel, intake::TABLE);
    sel.group_by_col(c(intake::TABLE, "id"))
        .order_by(c(intake::TABLE, "name"), Order::Asc)
        .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

// ---------------------------------------------------------------------------
// Unit 2 — IntakeIssueViewSet.get_queryset (base.py:100-174)
// ---------------------------------------------------------------------------

/// `cycle_id` subquery annotation (`base.py:114-118`):
/// `CycleIssue` filtered on the outer issue with the explicit
/// `deleted_at__isnull=True`, first row's `cycle_id`.
pub fn cycle_id_fragment() -> String {
    format!(
        r#"(SELECT "{u}"."cycle_id" FROM "{t}" "{u}" WHERE ("{u}"."deleted_at" IS NULL AND "{u}"."issue_id" = ("{i}"."id")) LIMIT 1)"#,
        u = U0,
        t = CYCLE_ISSUE_TABLE,
        i = ISSUE_TABLE,
    )
}

/// `link_count` annotation (`base.py:119-124`).
/// `Func(F("id"), function="Count")` renders as `COUNT("id")`.
/// No `Coalesce`: an issue with no links annotates `NULL`, not `0`.
pub fn link_count_fragment() -> String {
    format!(
        r#"(SELECT COUNT("{u}"."id") FROM "{t}" "{u}" WHERE "{u}"."issue_id" = ("{i}"."id"))"#,
        u = U0,
        t = ISSUE_LINK_TABLE,
        i = ISSUE_TABLE,
    )
}

/// `attachment_count` annotation (`base.py:125-133`): same shape,
/// additionally filtered on the `ISSUE_ATTACHMENT` entity type.
/// No `Coalesce`: renders `NULL` when there are no attachments.
pub fn attachment_count_fragment() -> String {
    format!(
        r#"(SELECT COUNT("{u}"."id") FROM "{t}" "{u}" WHERE ("{u}"."issue_id" = ("{i}"."id") AND "{u}"."entity_type" = '{e}'))"#,
        u = U0,
        t = FILE_ASSET_TABLE,
        i = ISSUE_TABLE,
        e = ENTITY_TYPE_ISSUE_ATTACHMENT,
    )
}

/// `sub_issues_count` annotation (`base.py:134-139`).
/// Goes through `Issue.issue_objects` (`issue.py:229`), the
/// soft-delete-aware manager, hence the `deleted_at IS NULL` guard.
/// No `Coalesce`: renders `NULL` when there are no sub-issues.
pub fn sub_issues_count_fragment() -> String {
    format!(
        r#"(SELECT COUNT("{u}"."id") FROM "{i}" "{u}" WHERE ("{u}"."deleted_at" IS NULL AND "{u}"."parent_id" = ("{i}"."id")))"#,
        u = U0,
        i = ISSUE_TABLE,
    )
}

/// `label_ids` annotation, one shared shape across every call site
/// (`get_queryset` `:140-148`, list `:188-197`, create re-fetch
/// `:296-306`, partial-update issue annotate `:379-387`, re-fetch
/// `:477-485`, retrieve `:509-517`): distinct label ids, dropping nulls
/// and rows whose through-table row is soft-deleted, defaulting to
/// `'{}'` via `Coalesce(..., Value([]))`. The Issue-root
/// (`labels__id`) and IntakeIssue-root (`issue__labels__id`) lookup
/// paths compile to the same tables, so one fragment serves all.
pub fn label_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "labels"."id") FILTER (WHERE ("labels"."id" IS NOT NULL AND "issue_labels"."deleted_at" IS NULL)), '{}')"#.to_owned()
}

/// `assignee_ids` annotation, `get_queryset` form (`base.py:149-160`):
/// the full guard — non-null id, the assignee's `member_project` row
/// active, and the `issue_assignees` through-table row not soft-deleted.
pub fn queryset_assignee_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "users"."id") FILTER (WHERE ("users"."id" IS NOT NULL AND "member_project"."is_active" AND "issue_assignees"."deleted_at" IS NULL)), '{}')"#.to_owned()
}

/// `assignee_ids` annotation, create re-fetch form
/// (`base.py:307-315`): the `issue_assignees.deleted_at` guard is
/// missing — ported as-is (see the ported-bugs note). `retrieve`
/// (`:518-527`) does NOT use this form: it keeps the `deleted_at`
/// guard and drops `is_active` instead, i.e. the
/// [`refetch_assignee_ids_fragment`] shape.
pub fn detail_assignee_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "users"."id") FILTER (WHERE ("users"."id" IS NOT NULL AND "member_project"."is_active")), '{}')"#.to_owned()
}

/// `assignee_ids` annotation, partial-update re-fetch form
/// (`base.py:486-495`): the `member_project.is_active` check is
/// missing — ported as-is.
pub fn refetch_assignee_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "users"."id") FILTER (WHERE ("users"."id" IS NOT NULL AND "issue_assignees"."deleted_at" IS NULL)), '{}')"#.to_owned()
}

/// `module_ids` annotation (`base.py:161-172`): distinct module ids,
/// dropping nulls, archived modules and soft-deleted
/// `module_issues` rows, defaulting to `'{}'`.
pub fn module_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "module_issues"."module_id") FILTER (WHERE ("module_issues"."module_id" IS NOT NULL AND "modules"."archived_at" IS NULL AND "module_issues"."deleted_at" IS NULL)), '{}')"#.to_owned()
}

/// `IntakeIssueViewSet.get_queryset` (`base.py:100-174`) as SQL text.
///
/// `Issue.objects.filter(project_id=$1, workspace__slug=$2)` with
/// `select_related("workspace", "project", "state", "parent")`
/// (the joins are emitted; the projection stays `issues.*`, exactly as
/// the `intake_issue_queryset.sql.json` fixture records),
/// the deferred-field `issue_intake` prefetch (`:108-113`, a separate
/// query at execution — handlers-owned), the seven annotations above,
/// and the trailing `.distinct()` (`:174`).
pub fn intake_issue_queryset_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.distinct()
        .expr(Expr::cust(format!(r#""{i}".*"#, i = ISSUE_TABLE)))
        .expr_as(Expr::cust(cycle_id_fragment()), Alias::new("cycle_id"))
        .expr_as(Expr::cust(link_count_fragment()), Alias::new("link_count"))
        .expr_as(
            Expr::cust(attachment_count_fragment()),
            Alias::new("attachment_count"),
        )
        .expr_as(
            Expr::cust(sub_issues_count_fragment()),
            Alias::new("sub_issues_count"),
        )
        .expr_as(Expr::cust(label_ids_fragment()), Alias::new("label_ids"))
        .expr_as(
            Expr::cust(queryset_assignee_ids_fragment()),
            Alias::new("assignee_ids"),
        )
        .expr_as(Expr::cust(module_ids_fragment()), Alias::new("module_ids"))
        .from(t(ISSUE_TABLE))
        .join(
            JoinType::InnerJoin,
            t(PROJECT_TABLE),
            Expr::col(c(ISSUE_TABLE, "project_id")).equals(c(PROJECT_TABLE, "id")),
        )
        .join(
            JoinType::InnerJoin,
            t(WORKSPACE_TABLE),
            Expr::col(c(ISSUE_TABLE, "workspace_id")).equals(c(WORKSPACE_TABLE, "id")),
        )
        .join(
            JoinType::LeftJoin,
            t(STATE_TABLE),
            Expr::col(c(ISSUE_TABLE, "state_id")).equals(c(STATE_TABLE, "id")),
        )
        .cond_where(
            Condition::all()
                .add(Expr::cust(format!(
                    r#""{i}"."project_id" = $1"#,
                    i = ISSUE_TABLE
                )))
                .add(Expr::cust(r#""workspaces"."slug" = $2"#)),
        );
    active(&mut sel, ISSUE_TABLE);
    sel.to_string(PostgresQueryBuilder)
}

// ---------------------------------------------------------------------------
// Unit 3 — IntakeIssueViewSet.list queryset (base.py:176-219)
// ---------------------------------------------------------------------------

/// The default `order_by` (`base.py:198`).
pub const DEFAULT_LIST_ORDER: &str = "-issue__created_at";

/// The default `status` CSV (`base.py:200`).
pub const DEFAULT_LIST_STATUS: &str = "-2";

/// Error for a non-numeric `status` token. Django coerces `status__in`
/// against the `IntegerField` at execution and the failure surfaces as
/// a 500 through `handle_exception`; the handler layer owns that
/// mapping, this error is its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusParseError(pub String);

impl std::fmt::Display for StatusParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid intake status token: {}", self.0)
    }
}

impl std::error::Error for StatusParseError {}

/// Parse the `status` query param (`base.py:200-202`):
/// `request.GET.get("status", "-2").split(",")` minus every exact
/// `"null"` token, then `filter(status__in=...)` only when the
/// remainder is non-empty.
///
/// Only exact `"null"` tokens are dropped — Python does no trimming
/// and no empty-skip. An empty token (`?status=`, a trailing comma)
/// or a padded `" null"` stays in the list and fails `IntegerField`
/// coercion at execution (500 via `handle_exception`); here it
/// surfaces as [`StatusParseError`]. Surrounding ASCII whitespace
/// around a numeric token parses, matching CPython `int()`.
///
/// Returns `None` when no status filter applies — the falsy-skip bug
/// (`?status=null` lists every status). Returns
/// `Some(vec![-2])` for the absent-param default.
pub fn parse_intake_status(raw: Option<&str>) -> Result<Option<Vec<i32>>, StatusParseError> {
    let csv = raw.unwrap_or(DEFAULT_LIST_STATUS);
    let mut out = Vec::new();
    for token in csv.split(',') {
        if token == "null" {
            continue;
        }
        match token.trim().parse::<i32>() {
            Ok(v) => out.push(v),
            Err(_) => return Err(StatusParseError(token.to_owned())),
        }
    }
    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

/// Map the raw `order_by` param to an `ORDER BY` expression
/// (`base.py:198`). The two known spellings map to the joined issue's
/// `created_at`; **any other value passes through untouched** — Django
/// interpolates it into `ORDER BY`, raises `FieldError`, and
/// `handle_exception` answers
/// `500 {"error": "Something went wrong please try again later"}`.
/// That failure mapping is handlers-owned.
pub fn order_to_sql(raw: &str) -> String {
    match raw {
        "-issue__created_at" => r#""issues"."created_at" DESC"#.to_owned(),
        "issue__created_at" => r#""issues"."created_at" ASC"#.to_owned(),
        other => other.to_owned(),
    }
}

/// Inputs for [`intake_issue_list_sql`].
///
/// Bind contract: `$1` is `intake_id`, `$2` is `project_id`, `$3..`
/// are the caller-supplied `issue_filters` fragment binds (the shared
/// `issue_filters(request.GET, "GET", "issue__")` compiler,
/// `base.py:183`, is F-07-kernel-owned: the caller compiles it and
/// passes the SQL plus its bind count). Status binds follow the
/// filter binds; the guest `created_by` bind comes last. `LIMIT` and
/// `OFFSET` are inlined literals, as Django renders them.
pub struct IntakeIssueListQuery<'a> {
    /// Raw `order_by` param; [`DEFAULT_LIST_ORDER`] when absent.
    pub order_by: &'a str,
    /// Parsed statuses (`None` = no filter — the falsy-skip case).
    pub statuses: Option<Vec<i32>>,
    /// Pre-compiled `issue_filters` SQL fragment (no leading `AND`).
    pub issue_filters_sql: Option<&'a str>,
    /// Number of binds inside `issue_filters_sql`.
    pub issue_filter_binds: usize,
    /// Guest `created_by` narrowing (`base.py:204-214`, handlers-owned
    /// decision): `true` appends `AND created_by_id = $N`.
    pub guest_created_by: bool,
    /// Paginator window (`BasePaginator.paginate`, `base.py:215-219`).
    pub limit: i64,
    /// Paginator window; Django omits `OFFSET` when it is zero.
    pub offset: i64,
}

/// `IntakeIssueViewSet.list` queryset (`base.py:184-202`) as SQL text.
///
/// `IntakeIssue.objects.filter(intake_id=$1, project_id=$2,
/// **issue_filters)` (`:185`) with `select_related("issue")`
/// (`:186`), the `label_ids` annotate (`:188-197`), `.order_by(...)`
/// (`:198`), the status CSV filter (`:200-202`), and the guest
/// narrowing (`:204-214`, via [`IntakeIssueListQuery::guest_created_by`]).
/// The missing-`Intake` 404 (`:179-180`) and the paginator envelope
/// (`:215-219`) are handlers-owned.
pub fn intake_issue_list_sql(q: &IntakeIssueListQuery<'_>) -> String {
    let mut sql = format!(
        r#"SELECT "{t}".*, {labels} AS "label_ids" FROM "{t}" LEFT OUTER JOIN "{i}" ON ("{t}"."issue_id" = "{i}"."id") LEFT OUTER JOIN "{lt}" ON ("{i}"."id" = "{lt}"."issue_id") LEFT OUTER JOIN "{l}" ON ("{lt}"."label_id" = "{l}"."id") WHERE ("{t}"."intake_id" = $1 AND "{t}"."project_id" = $2 AND "{t}"."deleted_at" IS NULL"#,
        t = intake_issue::TABLE,
        i = ISSUE_TABLE,
        lt = ISSUE_LABEL_TABLE,
        l = LABEL_TABLE,
        labels = label_ids_fragment(),
    );
    let mut next_bind = 3;
    if let Some(fragment) = q.issue_filters_sql {
        sql.push_str(&format!(" AND ({fragment})"));
        next_bind += q.issue_filter_binds;
    }
    if let Some(statuses) = &q.statuses {
        let binds = statuses
            .iter()
            .enumerate()
            .map(|(ix, _)| format!("${}", next_bind + ix))
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(&format!(
            r#" AND "{t}"."status" IN ({binds})"#,
            t = intake_issue::TABLE
        ));
        next_bind += statuses.len();
    }
    if q.guest_created_by {
        sql.push_str(&format!(
            r#" AND "{t}"."created_by_id" = ${next_bind}"#,
            t = intake_issue::TABLE
        ));
    }
    sql.push(')');
    // Django groups aggregate-over-join reads by every selected PK:
    // `IntakeIssue` and its `select_related("issue")` row
    // (`base.py:184-188`). Grouping by the intake-issue id alone
    // rejects any ORDER BY on a joined column — including the view
    // default `-issue__created_at` (`:198`) — with `column
    // "issues.created_at" must appear in the GROUP BY clause`.
    sql.push_str(&format!(
        " GROUP BY \"{t}\".\"id\", \"{i}\".\"id\"",
        t = intake_issue::TABLE,
        i = ISSUE_TABLE,
    ));
    sql.push_str(&format!(" ORDER BY {}", order_to_sql(q.order_by)));
    sql.push_str(&format!(" LIMIT {}", q.limit));
    // Django's `qs[start:start+per]` omits OFFSET when start is 0.
    if q.offset > 0 {
        sql.push_str(&format!(" OFFSET {}", q.offset));
    }
    sql
}

// ---------------------------------------------------------------------------
// Unit 4 — detail lookups (base.py:333-339, :379-396, :474-498, :506-530,
// :551-557)
// ---------------------------------------------------------------------------

/// `assignee_ids` annotation, partial-update issue-annotate form
/// (`base.py:379-396`): the `member_project.is_active` check is
/// missing — ported as-is. Same `FILTER` shape as
/// [`refetch_assignee_ids_fragment`]; kept as its own function so the
/// call-site asymmetry stays visible in review.
pub fn issue_annotate_assignee_ids_fragment() -> String {
    refetch_assignee_ids_fragment()
}

/// The shared detail projection: the `IntakeIssue` row, its joined
/// `Issue` row (`select_related("issue")`), and the `label_ids` /
/// caller-chosen `assignee_ids` annotations. `retrieve` (`:506-530`)
/// and the partial-update re-fetch (`:474-498`) share this shape
/// **and** the assignee guard ([`refetch_assignee_ids_fragment`]:
/// through-table `deleted_at`, no `is_active`); the create re-fetch
/// (`:293-322`) shares the shape with the
/// [`detail_assignee_ids_fragment`] guard (`is_active`, no
/// `deleted_at`).
/// `$1` intake_id, `$2` issue_id, `$3` project_id.
///
/// The `GROUP BY` covers both selected PKs — Django's rendering for
/// an aggregate-over-`select_related("issue")` read
/// (`base.py:507-530`). Without it Postgres rejects the statement:
/// a bare column (`"t".*`) may not sit beside an aggregate.
pub fn intake_issue_detail_sql(assignee_ids: &str) -> String {
    format!(
        r#"SELECT "{t}".*, "{i}".*, {labels} AS "label_ids", {assignees} AS "assignee_ids" FROM "{t}" INNER JOIN "{i}" ON ("{t}"."issue_id" = "{i}"."id") WHERE ("{t}"."intake_id" = $1 AND "{t}"."issue_id" = $2 AND "{t}"."project_id" = $3 AND "{t}"."deleted_at" IS NULL) GROUP BY "{t}"."id", "{i}"."id""#,
        t = intake_issue::TABLE,
        i = ISSUE_TABLE,
        labels = label_ids_fragment(),
        assignees = assignee_ids,
    )
}

/// The partial-update issue annotate (`base.py:379-396`): the `Issue`
/// row for the intake issue's `issue_id` with the `label_ids` and
/// reduced-guard `assignee_ids` annotations, tenant-scoped.
/// `$1` issue_id, `$2` project_id, `$3` workspace slug.
pub fn partial_update_issue_sql() -> String {
    format!(
        r#"SELECT "{i}".*, {labels} AS "label_ids", {assignees} AS "assignee_ids" FROM "{i}" INNER JOIN "{w}" ON ("{i}"."workspace_id" = "{w}"."id") WHERE ("{i}"."id" = $1 AND "{i}"."project_id" = $2 AND "{w}"."slug" = $3 AND "{i}"."deleted_at" IS NULL)"#,
        i = ISSUE_TABLE,
        w = WORKSPACE_TABLE,
        labels = label_ids_fragment(),
        assignees = issue_annotate_assignee_ids_fragment(),
    )
}

/// The `.get()` both `partial_update` (`:333-339`) and `destroy`
/// (`:551-557`) open with: `IntakeIssue.objects.get(issue_id=$1,
/// workspace__slug=$2, project_id=$3, intake_id=$4)`. `partial_update`
/// passes the `Intake` instance as `intake_id`; Django compiles it to
/// the same pk comparison, so one lookup serves both.
pub fn intake_issue_lookup_sql() -> String {
    format!(
        r#"SELECT "{t}".* FROM "{t}" INNER JOIN "{w}" ON ("{t}"."workspace_id" = "{w}"."id") WHERE ("{t}"."issue_id" = $1 AND "{w}"."slug" = $2 AND "{t}"."project_id" = $3 AND "{t}"."intake_id" = $4 AND "{t}"."deleted_at" IS NULL)"#,
        t = intake_issue::TABLE,
        w = WORKSPACE_TABLE,
    )
}

/// `destroy`'s conditional issue delete (`:560-563`):
/// `Issue.objects.filter(workspace__slug=$1, project_id=$2,
/// pk=$3).first()` — only executed when the intake-issue status is one
/// of `-2, -1, 0, 2` (`:560`), handlers-owned decision.
pub fn destroy_issue_lookup_sql() -> String {
    format!(
        r#"SELECT "{i}".* FROM "{i}" INNER JOIN "{w}" ON ("{i}"."workspace_id" = "{w}"."id") WHERE ("{w}"."slug" = $1 AND "{i}"."project_id" = $2 AND "{i}"."id" = $3 AND "{i}"."deleted_at" IS NULL) LIMIT 1"#,
        i = ISSUE_TABLE,
        w = WORKSPACE_TABLE,
    )
}

/// Guest-role narrowing on `IntakeIssue` rows: the list queryset
/// (`:204-214`, via [`IntakeIssueListQuery::guest_created_by`]) and
/// the post-fetch check in `retrieve` (`:531-545`,
/// `intake_issue.created_by == request.user`). A `GUEST` member
/// without `guest_view_all_features` may only see rows they created.
/// The membership/flag check is handlers-owned; this is the predicate
/// it appends: `created_by_id = $N`.
///
/// NOT for the versions `get` (unit 5, `:583-597`): that check
/// compares the parent **Issue** row (`issue.created_by`), so it
/// needs the `issues`-table column, not this one.
pub fn guest_creator_predicate(bind: usize) -> String {
    format!(
        r#""{t}"."created_by_id" = ${bind}"#,
        t = intake_issue::TABLE
    )
}

// ---------------------------------------------------------------------------
// Unit 5 — description versions (base.py:569-637)
// ---------------------------------------------------------------------------

/// The 10-key `required_fields` projection (`base.py:612-623`) for the
/// versions list. The `pk` path (`:599-608`) returns the full detail
/// serializer instead — handlers-owned branch.
pub const REQUIRED_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "project",
    "issue",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
];

/// Columns [`REQUIRED_FIELDS`] project to. `workspace` / `project` /
/// `issue` / `owned_by` / `created_by` / `updated_by` are FK attnames,
/// hence the `*_id` columns — exactly the fixture's column list.
pub const REQUIRED_COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "issue_id",
    "last_saved_at",
    "owned_by_id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
];

/// Datetime fields `user_timezone_converter` shifts into the
/// requester's `user_timezone` after the `.values()` projection
/// (`base.py:570-576`). The conversion itself is handlers-owned
/// (pilot-2 `chrono-tz` precedent); the queries layer pins the field
/// set so both layers shift the same keys.
pub const VERSION_DATETIME_FIELDS: &[&str] = &["created_at", "updated_at"];

/// The versions list queryset (`base.py:625-627`) as SQL text:
/// `IssueDescriptionVersion.objects.filter(workspace__slug=$1,
/// project_id=$2, issue_id=$3)` projected to [`REQUIRED_COLUMNS`].
/// Pagination (`paginate`, `:629-636`) and the timezone shift are
/// handlers-owned; `$1` slug, `$2` project_id, `$3` issue_id.
pub fn description_versions_sql() -> String {
    let cols = REQUIRED_COLUMNS
        .iter()
        .map(|c| format!(r#""{t}"."{c}""#, t = DESCRIPTION_VERSION_TABLE))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"SELECT {cols} FROM "{t}" INNER JOIN "{w}" ON ("{t}"."workspace_id" = "{w}"."id") WHERE ("{w}"."slug" = $1 AND "{t}"."project_id" = $2 AND "{t}"."issue_id" = $3 AND "{t}"."deleted_at" IS NULL)"#,
        t = DESCRIPTION_VERSION_TABLE,
        w = WORKSPACE_TABLE,
    )
}

/// The single-version `.get()` (`base.py:600-605`):
/// `workspace__slug=$1, project_id=$2, issue_id=$3, pk=$4`, full row
/// (the detail serializer renders every field — not the
/// [`REQUIRED_FIELDS`] subset).
pub fn description_version_detail_sql() -> String {
    format!(
        r#"SELECT "{t}".* FROM "{t}" INNER JOIN "{w}" ON ("{t}"."workspace_id" = "{w}"."id") WHERE ("{w}"."slug" = $1 AND "{t}"."project_id" = $2 AND "{t}"."issue_id" = $3 AND "{t}"."id" = $4 AND "{t}"."deleted_at" IS NULL)"#,
        t = DESCRIPTION_VERSION_TABLE,
        w = WORKSPACE_TABLE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_intake/queries")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = fixtures_dir().join(name);
        let body =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// Normalize two SQL spellings to one semantic form:
    /// lowercase, drop quotes and parentheses, drop Django's `U0`
    /// subquery alias, spell every bind (`%s`, `$N`) as `?`, and apply
    /// the documented lookup-path → table map (`label_issue` →
    /// `issue_labels`, `issue_assignee` → `issue_assignees`,
    /// `issue_module` → `module_issues`, `assignees` → `users`,
    /// `issue_intake` → `intake_issues`, and the `member_project`
    /// long-path predicate to its joined form).
    fn normalize(sql: &str) -> String {
        let mut s = sql.to_lowercase();
        for (path, table) in [
            ("\"label_issue\".", "\"issue_labels\"."),
            ("\"issue_assignee\".", "\"issue_assignees\"."),
            ("\"issue_module\".", "\"module_issues\"."),
            ("\"issue_intake\".", "\"intake_issues\"."),
            ("\"assignees\".", "\"users\"."),
        ] {
            s = s.replace(path, table);
        }
        s = s.replace(
            "\"users\".\"member_project__is_active\"",
            "\"member_project\".\"is_active\"",
        );
        // Django renders booleans as TRUE; builders spell them bare.
        s = s.replace(" = true", "");
        // Drop Django's U0 subquery qualifier (quoted or bare) before
        // quotes become spaces.
        s = s.replace("\"u0\".", "").replace("u0.", "");
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '"' | '(' | ')' => out.push(' '),
                _ => out.push(ch),
            }
        }
        // Drop the U0 alias declaration (`FROM x U0`), then collapse
        // whitespace. Dots between other identifiers are kept.
        let mut words: Vec<&str> = out.split_whitespace().collect();
        words.retain(|w| *w != "u0");
        let mut joined = words.join(" ");
        joined = joined.replace("u0.", "");
        // Every bind placeholder becomes `?`.
        let mut norm = String::with_capacity(joined.len());
        let bytes = joined.as_bytes();
        let mut ix = 0;
        while ix < bytes.len() {
            if bytes[ix] == b'$' {
                let mut jx = ix + 1;
                while jx < bytes.len() && bytes[jx].is_ascii_digit() {
                    jx += 1;
                }
                if jx > ix + 1 {
                    norm.push('?');
                    ix = jx;
                    continue;
                }
            }
            if ix + 1 < bytes.len() && &joined[ix..ix + 2] == "%s" {
                norm.push('?');
                ix += 2;
                continue;
            }
            norm.push(bytes[ix] as char);
            ix += 1;
        }
        norm.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn assert_semantic_eq(got: &str, fixture_stmt: &str) {
        assert_eq!(
            normalize(got),
            normalize(fixture_stmt),
            "semantic mismatch:\n got: {got}\nfixture: {fixture_stmt}"
        );
    }

    // -- unit 1 ------------------------------------------------------

    #[test]
    fn intake_list_matches_fixture() {
        let v = fixture("intake_list.sql.json");
        let stmt = v["sql"].as_str().expect("intake_list has sql");
        assert_semantic_eq(&intake_list_sql(), stmt);
    }

    #[test]
    fn intake_list_example_row_shape() {
        let v = fixture("intake_list.sql.json");
        let rows = v["example_rows"].as_array().expect("example rows");
        assert!(!rows.is_empty());
        assert!(rows[0].get("pending_issue_count").is_some());
        // The handler serves the first row only (`.first()`).
        assert!(intake_list_sql().contains("LIMIT 1"));
    }

    // -- unit 2 ------------------------------------------------------

    #[test]
    fn queryset_scalar_annotations_match_fixture() {
        let v = fixture("intake_issue_queryset.sql.json");
        let anns = v["annotations"].as_array().expect("annotations");
        let by_name = |name: &str| {
            anns.iter()
                .find(|a| a["name"] == name)
                .unwrap_or_else(|| panic!("fixture has {name}"))
                .get("sql")
                .and_then(|s| s.as_str())
                .unwrap_or_else(|| panic!("{name} has sql"))
                .to_owned()
        };
        assert_semantic_eq(&cycle_id_fragment(), &by_name("cycle_id"));
        assert_semantic_eq(&link_count_fragment(), &by_name("link_count"));
        assert_semantic_eq(&attachment_count_fragment(), &by_name("attachment_count"));
        assert_semantic_eq(&sub_issues_count_fragment(), &by_name("sub_issues_count"));
        assert_semantic_eq(&label_ids_fragment(), &by_name("label_ids"));
        assert_semantic_eq(&module_ids_fragment(), &by_name("module_ids"));
    }

    #[test]
    fn queryset_assignee_guard_has_all_three_predicates() {
        // The fixture spells the joins as lookup paths; assert the
        // semantic content: distinct ids, non-null, active membership,
        // through-table soft-delete scope, `'{}'` default.
        let sql = queryset_assignee_ids_fragment();
        for predicate in [
            "ARRAY_AGG(DISTINCT",
            "\"users\".\"id\" IS NOT NULL",
            "\"member_project\".\"is_active\"",
            "\"issue_assignees\".\"deleted_at\" IS NULL",
            "'{}'",
        ] {
            assert!(sql.contains(predicate), "missing {predicate}: {sql}");
        }
        // The asymmetry, per call site: this form has BOTH guards.
        // The create re-fetch (detail) form keeps is_active and lacks
        // the through-table guard; the retrieve / partial-update
        // (refetch) forms keep the through-table guard and lack
        // is_active.
        assert!(detail_assignee_ids_fragment().contains("is_active"));
        assert!(!detail_assignee_ids_fragment().contains("issue_assignees"));
        assert!(refetch_assignee_ids_fragment().contains("issue_assignees"));
        assert!(!refetch_assignee_ids_fragment().contains("is_active"));
    }

    #[test]
    fn queryset_outer_shape_matches_fixture() {
        let v = fixture("intake_issue_queryset.sql.json");
        let stmt = v["sql"].as_str().expect("queryset has sql");
        let norm_fixture = normalize(stmt);
        // Skeleton: DISTINCT over issues with the workspace/project
        // tenant filter and the soft-delete scope.
        for part in [
            "select distinct",
            "from issues",
            "issues . project_id = ?",
            "workspaces . slug = ?",
            "issues . deleted_at is null",
        ] {
            assert!(
                norm_fixture.contains(part),
                "missing {part}: {norm_fixture}"
            );
        }
        let sql = intake_issue_queryset_sql();
        for alias in [
            "cycle_id",
            "link_count",
            "attachment_count",
            "sub_issues_count",
            "label_ids",
            "assignee_ids",
            "module_ids",
        ] {
            assert!(
                sql.contains(&format!("AS \"{alias}\"")),
                "missing annotation {alias}: {sql}"
            );
        }
        assert!(sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = $2"), "{sql}");
        assert!(sql.contains("\"issues\".\"deleted_at\" IS NULL"), "{sql}");
    }

    #[test]
    fn queryset_example_row_shape() {
        let v = fixture("intake_issue_queryset.sql.json");
        let rows = v["example_rows"].as_array().expect("example rows");
        assert!(!rows.is_empty());
        for key in [
            "cycle_id",
            "link_count",
            "attachment_count",
            "sub_issues_count",
            "label_ids",
            "assignee_ids",
            "module_ids",
        ] {
            assert!(rows[0].get(key).is_some(), "example row lacks {key}");
        }
    }

    // -- unit 3 ------------------------------------------------------

    fn default_list_query() -> IntakeIssueListQuery<'static> {
        IntakeIssueListQuery {
            order_by: DEFAULT_LIST_ORDER,
            statuses: parse_intake_status(None).expect("default parses"),
            issue_filters_sql: None,
            issue_filter_binds: 0,
            guest_created_by: false,
            limit: 50,
            offset: 0,
        }
    }

    #[test]
    fn list_default_shape_matches_fixture() {
        let v = fixture("intake_issue_list.sql.json");
        let stmt = v["sql"].as_str().expect("list has sql");
        let norm_fixture = normalize(stmt);
        // Tenant pair, soft-delete scope, label_ids annotate shape.
        for part in [
            "intake_issues . intake_id = ?",
            "intake_issues . project_id = ?",
            "intake_issues . deleted_at is null",
            "array_agg distinct",
        ] {
            assert!(
                norm_fixture.contains(part),
                "missing {part}: {norm_fixture}"
            );
        }
        let sql = intake_issue_list_sql(&default_list_query());
        // Default `-2` status filter is a single bind, not inlined.
        assert!(
            sql.contains("\"intake_issues\".\"status\" IN ($3)"),
            "{sql}"
        );
        // Default order is the joined issue's creation time, descending.
        assert!(
            sql.contains("ORDER BY \"issues\".\"created_at\" DESC"),
            "{sql}"
        );
        // Django groups by both selected PKs (intake-issue + joined
        // issue): grouping by the intake-issue id alone rejects the
        // default ORDER BY on "issues"."created_at".
        assert!(
            sql.contains("GROUP BY \"intake_issues\".\"id\", \"issues\".\"id\""),
            "{sql}"
        );
        assert!(sql.contains("LIMIT 50"), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    #[test]
    fn parse_intake_status_defaults_and_falsy_skip() {
        // Absent param → the `-2` default.
        assert_eq!(parse_intake_status(None), Ok(Some(vec![-2])));
        // Explicit values, exact `null` tokens dropped.
        assert_eq!(parse_intake_status(Some("-2,-1")), Ok(Some(vec![-2, -1])));
        assert_eq!(parse_intake_status(Some("null,-1")), Ok(Some(vec![-1])));
        // Only `null` tokens → falsy `[]` → NO filter (the ported bug).
        assert_eq!(parse_intake_status(Some("null")), Ok(None));
        assert_eq!(parse_intake_status(Some("null,null")).unwrap(), None);
        // Python does no empty-skip: `?status=` is `[""]`, truthy, so
        // Django filters on `[""]` and 500s on IntegerField coercion.
        // The port surfaces that as an error, never as "no filter".
        assert!(parse_intake_status(Some("")).is_err());
        assert!(parse_intake_status(Some("-2,")).is_err());
        // No trimming before the `null` compare either: `" null"` is
        // kept and fails coercion, while padded numerics parse like
        // CPython `int()`.
        assert!(parse_intake_status(Some(" null")).is_err());
        assert_eq!(parse_intake_status(Some(" -2 ")), Ok(Some(vec![-2])));
        // Non-numeric tokens are a handler-mapped 500, surfaced here.
        assert!(parse_intake_status(Some("bogus")).is_err());
    }

    #[test]
    fn order_mapping_and_passthrough_bug() {
        assert_eq!(DEFAULT_LIST_ORDER, "-issue__created_at");
        assert_eq!(
            order_to_sql("-issue__created_at"),
            "\"issues\".\"created_at\" DESC"
        );
        assert_eq!(
            order_to_sql("issue__created_at"),
            "\"issues\".\"created_at\" ASC"
        );
        // Unknown fields pass through raw (FieldError-as-500 in Django).
        assert_eq!(order_to_sql("name"), "name");
    }

    #[test]
    fn list_guest_and_filter_binds() {
        let q = IntakeIssueListQuery {
            issue_filters_sql: Some("\"issues\".\"priority\" = $3"),
            issue_filter_binds: 1,
            guest_created_by: true,
            offset: 50,
            ..default_list_query()
        };
        let sql = intake_issue_list_sql(&q);
        // Status binds continue after the caller-supplied filter binds.
        assert!(
            sql.contains("\"intake_issues\".\"status\" IN ($4)"),
            "{sql}"
        );
        assert!(
            sql.contains("\"intake_issues\".\"created_by_id\" = $5"),
            "{sql}"
        );
        assert!(sql.contains("OFFSET 50"), "{sql}");
    }

    #[test]
    fn list_fixture_branches_are_all_covered() {
        let v = fixture("intake_issue_list.sql.json");
        let branches = v["branches"].as_array().expect("branches");
        // 404, issue_filters, ordering, status CSV, guest scoping.
        assert!(branches.len() >= 5, "{branches:?}");
        // The falsy-skip branch: explicit all-`null` yields no filter.
        let status = branches
            .iter()
            .find(|b| b.get("when") == Some(&serde_json::json!("status CSV filter")))
            .expect("status branch");
        assert!(status.to_string().contains("null"), "{status}");
        assert!(parse_intake_status(Some("null,null")).unwrap().is_none());
    }

    // -- unit 4 ------------------------------------------------------

    #[test]
    fn detail_variants_carry_their_guards() {
        // retrieve (base.py:518-527) keeps the through-table deleted_at
        // guard and drops is_active: the refetch shape.
        let retrieve = intake_issue_detail_sql(&refetch_assignee_ids_fragment());
        let referetch = intake_issue_detail_sql(&refetch_assignee_ids_fragment());
        // create re-fetch (base.py:307-315) keeps is_active and drops
        // the through-table guard: the detail shape.
        let create_refetch = intake_issue_detail_sql(&detail_assignee_ids_fragment());
        for sql in [&retrieve, &referetch, &create_refetch] {
            assert!(sql.contains(label_ids_fragment().as_str()), "{sql}");
            // Aggregate-over-join: Django groups by both selected PKs.
            // No GROUP BY at all always errors in Postgres.
            assert!(
                sql.contains("GROUP BY \"intake_issues\".\"id\", \"issues\".\"id\""),
                "{sql}"
            );
            assert!(
                sql.contains("\"intake_issues\".\"intake_id\" = $1"),
                "{sql}"
            );
            assert!(sql.contains("\"intake_issues\".\"issue_id\" = $2"), "{sql}");
            assert!(
                sql.contains("\"intake_issues\".\"project_id\" = $3"),
                "{sql}"
            );
        }
        // retrieve: through-table guard, no is_active.
        assert!(retrieve.contains("issue_assignees"), "{retrieve}");
        assert!(!retrieve.contains("is_active"), "{retrieve}");
        // partial-update re-fetch: through-table guard, no is_active.
        assert!(referetch.contains("issue_assignees"), "{referetch}");
        assert!(!referetch.contains("is_active"), "{referetch}");
        // create re-fetch: is_active, no through-table guard.
        assert!(create_refetch.contains("is_active"), "{create_refetch}");
        assert!(
            !create_refetch.contains("issue_assignees"),
            "{create_refetch}"
        );
        // The partial-update issue annotate shares the re-fetch shape
        // under its own name so review sees the call-site asymmetry.
        assert_eq!(
            issue_annotate_assignee_ids_fragment(),
            refetch_assignee_ids_fragment()
        );
        let issue_sql = partial_update_issue_sql();
        assert!(issue_sql.contains("\"issues\".\"id\" = $1"), "{issue_sql}");
        assert!(
            issue_sql.contains("\"workspaces\".\"slug\" = $3"),
            "{issue_sql}"
        );
    }

    #[test]
    fn lookup_sqls_cover_partial_update_and_destroy() {
        let lookup = intake_issue_lookup_sql();
        for predicate in [
            "\"intake_issues\".\"issue_id\" = $1",
            "\"workspaces\".\"slug\" = $2",
            "\"intake_issues\".\"project_id\" = $3",
            "\"intake_issues\".\"intake_id\" = $4",
            "\"intake_issues\".\"deleted_at\" IS NULL",
        ] {
            assert!(lookup.contains(predicate), "missing {predicate}: {lookup}");
        }
        // destroy's conditional issue delete: same tenant triple plus
        // `.first()`; the status gate (`-2, -1, 0, 2`) is
        // handlers-owned.
        let destroy_issue = destroy_issue_lookup_sql();
        assert!(destroy_issue.contains("LIMIT 1"), "{destroy_issue}");
        assert!(!destroy_issue.contains("OFFSET"), "{destroy_issue}");
        assert_eq!(
            guest_creator_predicate(4),
            "\"intake_issues\".\"created_by_id\" = $4"
        );
    }

    // -- unit 5 ------------------------------------------------------

    #[test]
    fn versions_fields_and_sql_match_fixture() {
        let v = fixture("description_versions.sql.json");
        let want: Vec<String> = v["required_fields"]
            .as_array()
            .expect("required_fields")
            .iter()
            .map(|f| f.as_str().expect("str").to_owned())
            .collect();
        let got: Vec<String> = REQUIRED_FIELDS.iter().map(|f| (*f).to_owned()).collect();
        assert_eq!(got, want);
        // The builder projects exactly the fixture's column list.
        let stmt = v["sql"].as_str().expect("versions has sql");
        let select_list = stmt
            .split("FROM")
            .next()
            .expect("has FROM")
            .replace("SELECT", "");
        for col in REQUIRED_COLUMNS {
            assert!(
                select_list.contains(col),
                "missing column {col}: {select_list}"
            );
        }
        let sql = description_versions_sql();
        for predicate in [
            "\"workspaces\".\"slug\" = $1",
            "\"issue_description_versions\".\"project_id\" = $2",
            "\"issue_description_versions\".\"issue_id\" = $3",
            "\"issue_description_versions\".\"deleted_at\" IS NULL",
        ] {
            assert!(sql.contains(predicate), "missing {predicate}: {sql}");
        }
        assert_eq!(VERSION_DATETIME_FIELDS, &["created_at", "updated_at"]);
        // Example rows carry exactly the required keys.
        let rows = v["example_rows"].as_array().expect("example rows");
        let mut keys: Vec<&str> = rows[0]
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut required = got.clone();
        required.sort();
        assert_eq!(keys, required);
        // Single-version path pins the pk bind and the full row.
        let detail = description_version_detail_sql();
        assert!(
            detail.contains("\"issue_description_versions\".\"id\" = $4"),
            "{detail}"
        );
    }
}
