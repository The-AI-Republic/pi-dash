//! Intake read queries (D-32, stage 5; audited by NEWPIDASH-75).
//!
//! Ports the live list query of `apps/api/pi_dash/app/views/intake/base.py`
//! for the db layer:
//!
//! * [`intake_issue_list_sql`] — `IntakeIssueViewSet.list`
//!   (`base.py:176-219`) with [`parse_intake_status`] (`:200-202`) and
//!   [`order_to_sql`] (`:198`).
//!
//! # Reachability audit (NEWPIDASH-75, HEAD `d93b417f4`)
//!
//! The five-unit port also carried `intake_list_sql` (`base.py:60-75`),
//! `intake_issue_queryset_sql` (`base.py:100-174`), the detail lookups
//! (`:333-558`) and the description-versions builders (`:569-637`).
//! A per-symbol grep across `rust-api/` (excluding this file's own
//! tests) shows **no production caller** for any of them: the only
//! import of this module is `api/src/app_intake/issues.rs:65`, which
//! calls exactly `parse_intake_status` (`:3390`),
//! `IntakeIssueListQuery` (`:3398`) and `intake_issue_list_sql`
//! (`:3410`) — the live trio above, plus `order_to_sql` and
//! `label_ids_fragment` transitively. The 14 guard-counted
//! `Expr::cust` sites all lived in the dead `intake_list_sql` (4) and
//! `intake_issue_queryset_sql` (10) builders. Those builders and their
//! fixture-pin tests are deleted here rather than translated.
//! Same-named functions in `v1_assets::intake_queries` and
//! `services::space::queries::intake_assets` are separate modules,
//! not callers of this file.
//!
//! The live query builds SQL with `format!` and `$N` binds (the
//! `v1_assets::intake_queries` string-builder precedent: binds are
//! caller-supplied `sqlx` parameters, so `$N` inside `format!` text
//! keeps the `api/src/app_intake/issues.rs:3410` call contract
//! `sqlx::query(&sql).bind(...)` unchanged). A sea-query `Condition`
//! rewrite would need `Expr::cust("$N")` for every bind (new counted
//! sites) and cannot splice the caller-compiled F-07 `issue_filters_sql`
//! fragment or its opaque `joins` FROM-clause string (shared helper
//! owned by open INTAKE-2 #3005, outside this batch's exclusive
//! scope), so the live builder is retained verbatim.
//!
//! SQL semantics are Django's, quirks included (translate, don't
//! redesign):
//!
//! * Every read is soft-delete scoped (`deleted_at IS NULL` from the
//!   default manager, `mixins.py:56-58`).
//! * The fixture spells joins with Django lookup-path aliases
//!   (`label_issue`); the builder uses the real table name
//!   (`issue_labels`, related name `label_issue`, `issue.py:676`).
//!   The tests apply this documented alias map before comparing.
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * `order_by` field resolution (`base.py:198`): Django resolves the
//!   param against the queryset's fields, so [`order_to_sql`] maps a
//!   single-level `issue__<field>` to the `issues` columns and a bare
//!   `<field>` to the `intake_issues` columns (direction from one
//!   leading `-`). Anything else is rejected (`None`) — Django raises
//!   `FieldError` there, which `handle_exception` maps to `500
//!   {"error": "Something went wrong please try again later"}`;
//!   mapping the rejection to the 500 body is handlers-owned. User
//!   text never reaches the statement (PIDASHCONV-1426).
//! * Falsy status-CSV skip (`base.py:200-202`): `?status=` with only
//!   `null` tokens parses to `[]`, which is falsy, so **no** status
//!   filter applies. [`parse_intake_status`] returns `None` for that
//!   case (and for the absent-param default it returns `Some(vec![-2])`).
//! * Non-numeric status tokens raise at query execution in Django
//!   (`IntegerField` coercion) → 500 via `handle_exception`.
//!   [`parse_intake_status`] surfaces them as [`StatusParseError`];
//!   mapping that error to the 500 body is handlers-owned.
//!
//! Fixture source of truth:
//! `rust-api/fixtures/app_intake/queries/intake_issue_list.sql.json`
//! (recorded by PIDASHCONV-278). The `#[cfg(test)]` suite matches the
//! retained builder against its fixture statement in normalized
//! semantic form.

use super::models::intake_issue;

// ---------------------------------------------------------------------------
// Table names for the live list query (Django `db_table`; see the alias-map
// note above). Only the three joined tables the list query touches are kept;
// the dead units' tables (`workspaces`, `projects`, `states`, `users`,
// `issue_assignees`, `project_members`, `module_issues`, `modules`,
// `cycle_issues`, `issue_links`, `file_assets`, `issue_description_versions`)
// were deleted with their builders.
// ---------------------------------------------------------------------------

/// `issues` (`issue.py:253`).
pub const ISSUE_TABLE: &str = "issues";
/// `labels` (the `labels` M2M target, `label.py:43`).
pub const LABEL_TABLE: &str = "labels";
/// `issue_labels` through table (related name `label_issue`, `issue.py:676`).
pub const ISSUE_LABEL_TABLE: &str = "issue_labels";

// ---------------------------------------------------------------------------
// Live list query — IntakeIssueViewSet.list queryset (base.py:176-219)
// ---------------------------------------------------------------------------

/// `label_ids` annotation (`base.py:188-197`): distinct label ids,
/// dropping nulls and rows whose through-table row is soft-deleted,
/// defaulting to `'{}'` via `Coalesce(..., Value([]))`.
pub fn label_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "labels"."id") FILTER (WHERE ("labels"."id" IS NOT NULL AND "issue_labels"."deleted_at" IS NULL)), '{}')"#.to_owned()
}

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

/// Django field name → `intake_issues` column for `ORDER BY`
/// (`db/models/intake.py:51-74` plus the audit/project base from
/// `db/mixins.py:15-83` and `db/models/project.py:302-311`). FK fields
/// carry both the field spelling (`project`) and the attname spelling
/// (`project_id`): Django resolves both spellings in `order_by`.
const BRIDGE_ORDER_COLUMNS: &[(&str, &str)] = &[
    ("id", "id"),
    ("created_at", "created_at"),
    ("updated_at", "updated_at"),
    ("created_by", "created_by_id"),
    ("created_by_id", "created_by_id"),
    ("updated_by", "updated_by_id"),
    ("updated_by_id", "updated_by_id"),
    ("deleted_at", "deleted_at"),
    ("project", "project_id"),
    ("project_id", "project_id"),
    ("workspace", "workspace_id"),
    ("workspace_id", "workspace_id"),
    ("intake", "intake_id"),
    ("intake_id", "intake_id"),
    ("issue", "issue_id"),
    ("issue_id", "issue_id"),
    ("status", "status"),
    ("snoozed_till", "snoozed_till"),
    ("duplicate_to", "duplicate_to_id"),
    ("duplicate_to_id", "duplicate_to_id"),
    ("source", "source"),
    ("source_email", "source_email"),
    ("external_source", "external_source"),
    ("external_id", "external_id"),
    ("extra", "extra"),
];

/// Django field name → `issues` column for `ORDER BY` through the
/// `issue__` prefix (`db/models/issue.py:107-~250` plus the same
/// audit/project base). The M2M fields (`assignees`, `labels`) have no
/// column and are rejected — Django joins them, which this mapping
/// does not reproduce (residual divergence, documented below).
const ISSUE_ORDER_COLUMNS: &[(&str, &str)] = &[
    ("id", "id"),
    ("created_at", "created_at"),
    ("updated_at", "updated_at"),
    ("created_by", "created_by_id"),
    ("created_by_id", "created_by_id"),
    ("updated_by", "updated_by_id"),
    ("updated_by_id", "updated_by_id"),
    ("deleted_at", "deleted_at"),
    ("project", "project_id"),
    ("project_id", "project_id"),
    ("workspace", "workspace_id"),
    ("workspace_id", "workspace_id"),
    ("parent", "parent_id"),
    ("parent_id", "parent_id"),
    ("state", "state_id"),
    ("state_id", "state_id"),
    ("point", "point"),
    ("estimate_point", "estimate_point_id"),
    ("estimate_point_id", "estimate_point_id"),
    ("name", "name"),
    ("description_json", "description_json"),
    ("description_html", "description_html"),
    ("description_stripped", "description_stripped"),
    ("description_binary", "description_binary"),
    ("priority", "priority"),
    ("complexity_score", "complexity_score"),
    ("start_date", "start_date"),
    ("target_date", "target_date"),
    ("sequence_id", "sequence_id"),
    ("sort_order", "sort_order"),
    ("completed_at", "completed_at"),
    ("archived_at", "archived_at"),
    ("is_draft", "is_draft"),
    ("external_source", "external_source"),
    ("external_id", "external_id"),
    ("type", "type_id"),
    ("type_id", "type_id"),
    ("git_work_branch", "git_work_branch"),
    ("workpad", "workpad"),
    ("created_via", "created_via"),
    ("assigned_pod", "assigned_pod_id"),
    ("assigned_pod_id", "assigned_pod_id"),
    ("agent_executor", "agent_executor"),
];

/// Look a Django field (or attname) spelling up in an order map.
fn lookup_order_column<'a>(map: &[(&'a str, &'a str)], field: &str) -> Option<&'a str> {
    map.iter()
        .find(|(name, _)| *name == field)
        .map(|(_, column)| *column)
}

/// Map the raw `order_by` param to an `ORDER BY` expression
/// (`base.py:198`, which resolves the param against the queryset's
/// fields). A single leading `-` selects descending; `issue__<field>`
/// (one level) resolves against the joined issue's columns and a bare
/// `<field>` against the `intake_issues` bridge columns. Anything else
/// — unknown fields, M2M names, deeper `__` join paths, empty
/// spellings, SQL syntax — is rejected (`None`): Django raises
/// `FieldError` there and `handle_exception` answers
/// `500 {"error": "Something went wrong please try again later"}`.
/// That rejection mapping is handlers-owned. User text never reaches
/// the statement (PIDASHCONV-1426).
///
/// Residual divergence (kept): multi-level join paths
/// (`issue__project__name`) and M2M names (`issue__labels`, and bare
/// `assignees` has no bridge column either) are valid Django orderings
/// via join, but are rejected here and 500. The default
/// (`-issue__created_at`) rendering is unchanged.
pub fn order_to_sql(raw: &str) -> Option<String> {
    let (descending, field) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let direction = if descending { "DESC" } else { "ASC" };
    let resolved = if let Some(issue_field) = field.strip_prefix("issue__") {
        if issue_field.is_empty() || issue_field.contains("__") {
            None
        } else {
            lookup_order_column(ISSUE_ORDER_COLUMNS, issue_field)
                .map(|column| format!(r#""{ISSUE_TABLE}"."{column}""#))
        }
    } else if field.is_empty() || field.contains("__") {
        None
    } else {
        lookup_order_column(BRIDGE_ORDER_COLUMNS, field)
            .map(|column| format!(r#""{t}"."{column}""#, t = intake_issue::TABLE))
    };
    resolved.map(|expr| format!("{expr} {direction}"))
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
    /// Extra JOINs the fragment's predicates need (states, assignees,
    /// mentions, cycles, modules, subscribers); empty when the fragment
    /// touches only the base tables. Spliced after the label JOINs, as
    /// the count query does — without them Postgres rejects the
    /// statement (`missing FROM-clause entry`).
    pub joins: &'a str,
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
///
/// Returns `None` when `order_by` names no allowlisted column
/// (Django's `FieldError` path); the handler answers the generic 500.
pub fn intake_issue_list_sql(q: &IntakeIssueListQuery<'_>) -> Option<String> {
    let mut sql = format!(
        r#"SELECT "{t}".*, {labels} AS "label_ids" FROM "{t}" LEFT OUTER JOIN "{i}" ON ("{t}"."issue_id" = "{i}"."id") LEFT OUTER JOIN "{lt}" ON ("{i}"."id" = "{lt}"."issue_id") LEFT OUTER JOIN "{l}" ON ("{lt}"."label_id" = "{l}"."id")"#,
        t = intake_issue::TABLE,
        i = ISSUE_TABLE,
        lt = ISSUE_LABEL_TABLE,
        l = LABEL_TABLE,
        labels = label_ids_fragment(),
    );
    sql.push_str(q.joins);
    sql.push_str(&format!(
        r#" WHERE ("{t}"."intake_id" = $1 AND "{t}"."project_id" = $2 AND "{t}"."deleted_at" IS NULL"#,
        t = intake_issue::TABLE
    ));
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
    sql.push_str(&format!(" ORDER BY {}", order_to_sql(q.order_by)?));
    sql.push_str(&format!(" LIMIT {}", q.limit));
    // Django's `qs[start:start+per]` omits OFFSET when start is 0.
    if q.offset > 0 {
        sql.push_str(&format!(" OFFSET {}", q.offset));
    }
    Some(sql)
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
    /// lowercase, drop quotes and parentheses, spell every bind
    /// (`%s`, `$N`) as `?`, and apply the documented label-lookup
    /// map (`label_issue` → `issue_labels`).
    fn normalize(sql: &str) -> String {
        let mut s = sql.to_lowercase();
        s = s.replace("\"label_issue\".", "\"issue_labels\".");
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '"' | '(' | ')' => out.push(' '),
                _ => out.push(ch),
            }
        }
        let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
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

    // -- live list query ---------------------------------------------

    fn default_list_query() -> IntakeIssueListQuery<'static> {
        IntakeIssueListQuery {
            order_by: DEFAULT_LIST_ORDER,
            statuses: parse_intake_status(None).expect("default parses"),
            issue_filters_sql: None,
            issue_filter_binds: 0,
            joins: "",
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
        let sql = intake_issue_list_sql(&default_list_query()).expect("default order renders");
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
    fn order_mapping_and_rejection() {
        assert_eq!(DEFAULT_LIST_ORDER, "-issue__created_at");
        // The default rendering is unchanged.
        assert_eq!(
            order_to_sql("-issue__created_at").as_deref(),
            Some("\"issues\".\"created_at\" DESC")
        );
        assert_eq!(
            order_to_sql("issue__created_at").as_deref(),
            Some("\"issues\".\"created_at\" ASC")
        );
        // Bare bridge fields resolve against intake_issues; one leading
        // `-` selects descending (base.py:198 resolves `-created_at` /
        // `-id` to the bridge columns).
        assert_eq!(
            order_to_sql("status").as_deref(),
            Some("\"intake_issues\".\"status\" ASC")
        );
        assert_eq!(
            order_to_sql("-status").as_deref(),
            Some("\"intake_issues\".\"status\" DESC")
        );
        assert_eq!(
            order_to_sql("created_at").as_deref(),
            Some("\"intake_issues\".\"created_at\" ASC")
        );
        assert_eq!(
            order_to_sql("-created_at").as_deref(),
            Some("\"intake_issues\".\"created_at\" DESC")
        );
        assert_eq!(
            order_to_sql("id").as_deref(),
            Some("\"intake_issues\".\"id\" ASC")
        );
        assert_eq!(
            order_to_sql("-id").as_deref(),
            Some("\"intake_issues\".\"id\" DESC")
        );
        // Single-level issue__ fields resolve against issues.
        assert_eq!(
            order_to_sql("issue__name").as_deref(),
            Some("\"issues\".\"name\" ASC")
        );
        assert_eq!(
            order_to_sql("-issue__name").as_deref(),
            Some("\"issues\".\"name\" DESC")
        );
        // FK names resolve to their attname columns, both spellings.
        assert_eq!(
            order_to_sql("created_by").as_deref(),
            Some("\"intake_issues\".\"created_by_id\" ASC")
        );
        assert_eq!(
            order_to_sql("project_id").as_deref(),
            Some("\"intake_issues\".\"project_id\" ASC")
        );
        assert_eq!(
            order_to_sql("issue__state").as_deref(),
            Some("\"issues\".\"state_id\" ASC")
        );
        assert_eq!(
            order_to_sql("-issue__priority").as_deref(),
            Some("\"issues\".\"priority\" DESC")
        );
        // Rejection keeps the 500 quirk without running SQL: unknown
        // fields, M2M names, deeper join paths, empty spellings and SQL
        // syntax all answer Django's FieldError 500 on both backends
        // (PIDASHCONV-1426). The bridge has no `name` column, so bare
        // `name` is rejected too.
        for raw in [
            "name",
            "bogus_field",
            "-bogus_field",
            "issue__bogus",
            "-issue__bogus",
            "issue__labels",
            "issue__project__name",
            "project__name",
            "issue__",
            "",
            "-",
            // A second `-` is part of the field name, not direction.
            "--created_at",
            // SQL fragments never reach the statement.
            "(SELECT 1)",
            "id; SELECT 1",
            "(SELECT 1 FROM pg_sleep(2))",
            "\"intake_issues\".\"id\" DESC",
            "?created_at",
        ] {
            assert_eq!(order_to_sql(raw), None, "order_by={raw:?}");
        }
    }

    #[test]
    fn list_renders_mapped_order_by() {
        let q = IntakeIssueListQuery {
            order_by: "-created_at",
            ..default_list_query()
        };
        let sql = intake_issue_list_sql(&q).expect("mapped order renders");
        assert!(
            sql.contains("ORDER BY \"intake_issues\".\"created_at\" DESC"),
            "{sql}"
        );
        let q = IntakeIssueListQuery {
            order_by: "issue__name",
            ..default_list_query()
        };
        let sql = intake_issue_list_sql(&q).expect("mapped order renders");
        assert!(sql.contains("ORDER BY \"issues\".\"name\" ASC"), "{sql}");
        // The default still renders the joined issue's creation time.
        let sql = intake_issue_list_sql(&default_list_query()).expect("default order renders");
        assert!(
            sql.contains("ORDER BY \"issues\".\"created_at\" DESC"),
            "{sql}"
        );
    }

    #[test]
    fn list_rejects_unknown_order_by() {
        // Django's `FieldError` path: no statement is rendered and the
        // handler answers the generic 500 (PIDASHCONV-1426).
        for order_by in ["bogus_field", "(SELECT 1)", "id; SELECT 1", ""] {
            let q = IntakeIssueListQuery {
                order_by,
                ..default_list_query()
            };
            assert_eq!(intake_issue_list_sql(&q), None, "order_by={order_by:?}");
        }
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
        let sql = intake_issue_list_sql(&q).expect("default order renders");
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
    fn list_splices_filter_joins_after_label_joins() {
        let q = IntakeIssueListQuery {
            joins: " INNER JOIN \"issue_assignees\" ON (\"issue_assignees\".\"issue_id\" = \"issues\".\"id\")",
            ..default_list_query()
        };
        let sql = intake_issue_list_sql(&q).expect("default order renders");
        let labels = sql
            .find("LEFT OUTER JOIN \"labels\"")
            .expect("label join present");
        let extra = sql
            .find("INNER JOIN \"issue_assignees\"")
            .expect("joins spliced");
        let where_at = sql.find(" WHERE (").expect("where present");
        assert!(labels < extra && extra < where_at, "{sql}");
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

    #[test]
    fn list_pins_base_joins_and_projection() {
        // The retained statement shape (NEWPIDASH-75 durable collateral):
        // `intake_issues.*` + the label array, LEFT-joined through
        // `issues` → `issue_labels` → `labels`, tenant-scoped.
        let sql = intake_issue_list_sql(&default_list_query()).expect("default order renders");
        assert!(sql.starts_with("SELECT \"intake_issues\".*"), "{sql}");
        for join in [
            "LEFT OUTER JOIN \"issues\" ON (\"intake_issues\".\"issue_id\" = \"issues\".\"id\")",
            "LEFT OUTER JOIN \"issue_labels\" ON (\"issues\".\"id\" = \"issue_labels\".\"issue_id\")",
            "LEFT OUTER JOIN \"labels\" ON (\"issue_labels\".\"label_id\" = \"labels\".\"id\")",
        ] {
            assert!(sql.contains(join), "missing {join}: {sql}");
        }
        assert!(
            sql.contains(" AS \"label_ids\" FROM \"intake_issues\""),
            "{sql}"
        );
        // No fragment: binds stay $1 intake, $2 project, $3 status.
        assert!(
            sql.contains("\"intake_issues\".\"intake_id\" = $1"),
            "{sql}"
        );
        assert!(
            sql.contains("\"intake_issues\".\"project_id\" = $2"),
            "{sql}"
        );
    }
}
