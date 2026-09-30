//! Intake queryset reads for the api-v1 endpoints (D-21, stage 5).
//!
//! Ports the five database query units behind
//! `apps/api/pi_dash/api/views/intake.py` for the db layer:
//!
//! * [`intake_lookup_sql`] — the `Intake.objects.filter(
//!   workspace__slug, project_id).first()` pre-lookup shared by the
//!   list (`:64-67`) and detail (`:234-237`) paths.
//! * [`intake_issue_list_sql`] — `IntakeIssueListCreateAPIEndpoint.
//!   get_queryset` (`:63-83`): the `intake_view` guard (`:71-72`),
//!   the snoozed `Q` (`:76`), tenant + intake scoping (`:77-79`),
//!   `select_related("issue", "workspace", "project")` (`:81`) and
//!   the `order_by` kwarg (`:82`, default `-created_at`).
//! * [`intake_issue_detail_sql`] — `IntakeIssueDetailAPIEndpoint.
//!   get_queryset` (`:233-253`), identical to the list shape, plus
//!   the retrieve `.get(issue_id=...)` (`:277`).
//! * [`triage_lookup_sql`] / [`triage_insert_sql`] — the triage-state
//!   get-or-create (`:173-184`).
//! * [`label_ids_fragment`] / [`patch_assignee_ids_fragment`] /
//!   [`patch_issue_lookup_sql`] — the patch `ArrayAgg` annotations
//!   (`:350-371`).
//!
//! Statements are fixed `format!` templates executed with runtime
//! `sqlx::query` (no `query!` macros: there is no build-time
//! database, same as the merged `v1_projects::queries_stateest` and
//! `app_intake::queries` precedents). `$N` placeholders are Postgres
//! binds; the Django fixture spells them `%s` and the tests compare
//! normalized shapes, not bind spellings.
//!
//! SQL semantics are Django's, quirks included (translate, don't
//! redesign):
//!
//! * Every read is soft-delete scoped on its base table
//!   (`deleted_at IS NULL` from the default managers,
//!   `db/mixins.py:56-58`; states additionally carry the triage-group
//!   scope from `TriageStateManager`, `db/models/state.py:86-90`).
//!   Joined tables carry no deleted predicate: Django scopes only the
//!   base model through its manager (verified against Django's own
//!   query rendering for this exact queryset shape).
//! * `select_related` joins are `INNER JOIN`: every traversed FK
//!   (`IntakeIssue.issue/intake/project/workspace`,
//!   `db/models/intake.py:51-74`, `db/models/project.py:302-304`) is
//!   non-null `CASCADE`. The fx-q-intake schematic writes `LEFT OUTER
//!   JOIN "issues"`; Django renders `INNER JOIN` there (confirmed by
//!   rendering the list queryset through Django 6.0.5: `INNER JOIN
//!   "projects"`, `INNER JOIN "workspaces"`, `INNER JOIN "issues"`).
//!   Row sets coincide under FK integrity either way.
//! * Projection is `"base".*` plus `"issues".*`, `"workspaces".*`,
//!   `"projects".*` for the `select_related` paths, exactly the column
//!   sets Django expands (verified in the same render).
//! * The `intake_view` guard (`:71-72`, `:241-242`) runs in Python on
//!   the pre-lookup rows (`intake is None or not project.intake_view`
//!   → `IntakeIssue.objects.none()`); the builders below cover the
//!   non-empty path. Serving the empty set is handlers-owned.
//! * `.first()` renders `ORDER BY` (model `Meta.ordering`) + `LIMIT 1`;
//!   `.get()` renders its predicates + `LIMIT 1` (same convention as
//!   the merged `v1_projects` detail builders).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * Bare `Project.objects.get` (`:69`, `:239`): a missing project
//!   raises `Project.DoesNotExist` → the 404 mapping, never the
//!   empty-queryset path. The guard order is ported as-is; the
//!   exception mapping is handlers-owned.
//! * Detail `.get(issue_id=issue_id)` (`:277`) runs on the
//!   snoozed-filtered queryset: a snoozed-out row raises
//!   `IntakeIssue.DoesNotExist` → 404, not an empty 200. Ported
//!   as-is (the `$5` predicate sits inside the filtered statement).
//! * Triage creation is a plain `State.objects.create`, not
//!   `get_or_create` (`:176-184`): concurrent posts can 500 on the
//!   partial unique constraint
//!   (`state_unique_name_project_when_deleted_at_null`,
//!   `db/models/state.py:119-125`). Ported as-is.
//!
//! Fixture source of truth:
//! `rust-api/fixtures/v1_assets/fx-q-intake.json` (recorded by
//! PIDASHCONV-375; trace lines in `../TRACE.md`). The `#[cfg(test)]`
//! suite replays every builder against the fixture's SQL shapes and
//! row literals.
//!
//! Wiring note: the crate root declares `pub mod v1_assets;` (seam
//! added by PIDASHCONV-404); sibling issues add their own files under
//! this module (PIDASHCONV-409 queries); on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use super::model::{intake, intake_issue};
use crate::v1_projects::models::state;

/// `issues` (`db/models/issue.py:253`). No v1 module owns it; the
/// api-v1 patch path reads it directly (`views/intake.py:350-371`).
pub const ISSUE_TABLE: &str = "issues";
/// `workspaces` (tenant join target for every `workspace__slug` filter).
pub const WORKSPACE_TABLE: &str = "workspaces";
/// `projects` (`select_related("project")` join target).
pub const PROJECT_TABLE: &str = "projects";
/// `labels` (the `labels` M2M target, `db/models/label.py:43`).
pub const LABEL_TABLE: &str = "labels";
/// `users` (the `assignees` M2M target, `AUTH_USER_MODEL`,
/// `db_table "users"`, `db/models/user.py:136`).
pub const USER_TABLE: &str = "users";
/// `issue_labels` through table (related name `label_issue`,
/// `db/models/issue.py:676`).
pub const ISSUE_LABEL_TABLE: &str = "issue_labels";
/// `issue_assignees` through table (related name `issue_assignee`,
/// `db/models/issue.py:464`).
pub const ISSUE_ASSIGNEE_TABLE: &str = "issue_assignees";
/// `project_members` (the `member_project` related name on
/// `ProjectMember.member`, `db/models/project.py:338`).
pub const PROJECT_MEMBER_TABLE: &str = "project_members";

/// Default `order_by` kwarg (`views/intake.py:82,252`).
pub const DEFAULT_ORDER: &str = "-created_at";

/// Triage-state row literals (`views/intake.py:176-184`,
/// `db/models/state.py:70-78`).
pub const TRIAGE_NAME: &str = "Triage";
/// Stored group string (`StateGroup.TRIAGE.value`, lowercase).
pub const TRIAGE_GROUP: &str = "triage";
/// Stored color literal.
pub const TRIAGE_COLOR: &str = "#4E5355";
/// Stored sequence (`FloatField`; Django renders `65000.0`).
pub const TRIAGE_SEQUENCE: &str = "65000.0";

/// Django field/attname → `intake_issues` column for `ORDER BY`
/// (`db/models/intake.py:51-74` plus the audit/project base,
/// `db/mixins.py:16-89`, `db/models/project.py:302-311`). FK fields
/// carry both spellings (`project` and `project_id`): Django resolves
/// both in `order_by`.
const ORDER_COLUMNS: &[(&str, &str)] = &[
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

/// Map the raw `order_by` kwarg to an `ORDER BY` expression
/// (`views/intake.py:82`). `None` is the kwarg default
/// (`-created_at`). A single leading `-` selects descending.
/// Anything unresolvable passes through untouched — Django raises
/// `FieldError` there and the handlers map it to the 500 body; the
/// raw fragment fails in Postgres and maps to the same 500. That
/// failure mapping is handlers-owned.
pub fn intake_issue_order_sql(raw: Option<&str>) -> String {
    let raw = raw.unwrap_or(DEFAULT_ORDER);
    let (descending, field) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let direction = if descending { "DESC" } else { "ASC" };
    let resolved = if field.is_empty() || field.contains("__") {
        None
    } else {
        ORDER_COLUMNS
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, column)| format!(r#""{}"."{}""#, intake_issue::TABLE, column))
    };
    match resolved {
        Some(expr) => format!("{expr} {direction}"),
        None => raw.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Unit 1 — intake pre-lookup (views/intake.py:64-67, :234-237)
// ---------------------------------------------------------------------------

/// `Intake.objects.filter(workspace__slug=$1,
/// project_id=$2).first()` with model `Meta.ordering` (`name` ASC,
/// `db/models/intake.py:35`) → `LIMIT 1`.
///
/// Bind contract: `$1` workspace slug, `$2` project id.
pub fn intake_lookup_sql() -> String {
    format!(
        r#"SELECT "{t}".* FROM "{t}" INNER JOIN "{w}" ON ("{t}"."workspace_id" = "{w}"."id") WHERE ("{t}"."deleted_at" IS NULL AND "{w}"."slug" = $1 AND "{t}"."project_id" = $2) ORDER BY "{t}"."name" ASC LIMIT 1"#,
        t = intake::TABLE,
        w = WORKSPACE_TABLE,
    )
}

// ---------------------------------------------------------------------------
// Units 1-2 — list/detail get_queryset (views/intake.py:63-83, :233-253)
// ---------------------------------------------------------------------------

/// Shared `FROM`/`WHERE` core of the list and detail querysets:
/// the snoozed `Q(snoozed_till__gte=$1) | Q(snoozed_till__isnull)`
/// with tenant + intake scoping, in Django's own predicate order
/// (`snoozed`, `intake_id`, `project_id`, workspace slug — as Django
/// 6.0.5 renders this exact filter chain).
///
/// Bind contract: `$1` now, `$2` intake id, `$3` project id,
/// `$4` workspace slug.
fn intake_issue_scope_where() -> String {
    format!(
        r#"("{t}"."snoozed_till" >= $1 OR "{t}"."snoozed_till" IS NULL) AND "{t}"."intake_id" = $2 AND "{t}"."project_id" = $3 AND "{w}"."slug" = $4 AND "{t}"."deleted_at" IS NULL"#,
        t = intake_issue::TABLE,
        w = WORKSPACE_TABLE,
    )
}

/// Shared `select_related("issue", "workspace", "project")`
/// projection + joins. All three joins are `INNER JOIN` (every
/// traversed FK is non-null `CASCADE`; verified against Django's own
/// rendering — see the module docs).
fn intake_issue_select_from() -> String {
    format!(
        r#"SELECT "{t}".*, "{i}".*, "{w}".*, "{p}".* FROM "{t}" INNER JOIN "{i}" ON ("{t}"."issue_id" = "{i}"."id") INNER JOIN "{w}" ON ("{t}"."workspace_id" = "{w}"."id") INNER JOIN "{p}" ON ("{t}"."project_id" = "{p}"."id")"#,
        t = intake_issue::TABLE,
        i = ISSUE_TABLE,
        w = WORKSPACE_TABLE,
        p = PROJECT_TABLE,
    )
}

/// `IntakeIssueListCreateAPIEndpoint.get_queryset`
/// (`views/intake.py:63-83`) as SQL text, non-empty path: the
/// intake row was found and `project.intake_view` is set (the
/// `None`/disabled guard returns `.none()` — handlers-owned).
///
/// `order_by` is the raw `order_by` kwarg (`None` = kwarg default).
pub fn intake_issue_list_sql(order_by: Option<&str>) -> String {
    format!(
        "{} WHERE {} ORDER BY {}",
        intake_issue_select_from(),
        intake_issue_scope_where(),
        intake_issue_order_sql(order_by),
    )
}

/// `IntakeIssueDetailAPIEndpoint.get_queryset`
/// (`views/intake.py:233-253`) as SQL text, non-empty path, plus the
/// retrieve `.get(issue_id=$5)` (`:277`).
///
/// Bind contract: `$1..$4` as in the list scope, `$5` issue id.
pub fn intake_issue_detail_sql(order_by: Option<&str>) -> String {
    format!(
        "{} WHERE {} AND \"{t}\".\"issue_id\" = $5 ORDER BY {} LIMIT 1",
        intake_issue_select_from(),
        intake_issue_scope_where(),
        intake_issue_order_sql(order_by),
        t = intake_issue::TABLE,
    )
}

// ---------------------------------------------------------------------------
// Unit 3 — triage state get-or-create (views/intake.py:173-184)
// ---------------------------------------------------------------------------

/// `State.triage_objects.filter(project_id=$1,
/// workspace__slug=$2).first()`: the manager restricts
/// `group = 'triage'` (`db/models/state.py:86-90`) plus the
/// soft-delete scope, ordered by model `Meta.ordering`
/// (`sequence`, `db/models/state.py:126`) → `LIMIT 1`.
///
/// Bind contract: `$1` project id, `$2` workspace slug.
pub fn triage_lookup_sql() -> String {
    format!(
        r#"SELECT "{t}".* FROM "{t}" INNER JOIN "{w}" ON ("{t}"."workspace_id" = "{w}"."id") WHERE ("{t}"."deleted_at" IS NULL AND "{t}"."group" = '{g}' AND "{t}"."project_id" = $1 AND "{w}"."slug" = $2) ORDER BY "{t}"."sequence" ASC LIMIT 1"#,
        t = state::TABLE,
        w = WORKSPACE_TABLE,
        g = state::TRIAGE_GROUP,
    )
}

/// `State.objects.create(name='Triage', group='triage', ...)`
/// (`views/intake.py:176-184`) as SQL text. Columns follow the
/// `states` Django field order; the fixed literals are the exact
/// call-site values. `slug` is `'triage'`: `create` runs `save`,
/// which sets `slugify("Triage")` (`db/models/state.py:130-131`).
/// `description` stores `''` (`TextField(blank=True)`, no
/// `null=True`); `is_triage`/`default` store `FALSE` (field
/// defaults, not passed at the call site).
///
/// Bind contract: `$1` id, `$2` created_at, `$3` updated_at,
/// `$4` project id, `$5` workspace id.
pub fn triage_insert_sql() -> String {
    format!(
        r#"INSERT INTO "{t}" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id", "deleted_at", "project_id", "workspace_id", "name", "description", "color", "slug", "sequence", "group", "is_triage", "default", "external_source", "external_id") VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, '{n}', '', '{c}', 'triage', {s}, '{g}', FALSE, FALSE, NULL, NULL)"#,
        t = state::TABLE,
        n = TRIAGE_NAME,
        c = TRIAGE_COLOR,
        s = TRIAGE_SEQUENCE,
        g = TRIAGE_GROUP,
    )
}

// ---------------------------------------------------------------------------
// Units 4-5 — patch ArrayAgg annotations (views/intake.py:350-371)
// ---------------------------------------------------------------------------

/// `label_ids` annotation (`:351-358`):
/// `Coalesce(ArrayAgg("labels__id", distinct=True,
/// filter=~Q(labels__id__isnull=True) &
/// Q(label_issue__deleted_at__isnull=True)), [])`.
/// Labels drop deleted links only — no active check.
pub fn label_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "labels"."id") FILTER (WHERE (NOT ("labels"."id" IS NULL) AND "issue_labels"."deleted_at" IS NULL)), '{}')"#.to_owned()
}

/// `assignee_ids` annotation (`:359-370`):
/// `Coalesce(ArrayAgg("assignees__id", distinct=True,
/// filter=~Q(assignees__id__isnull=True) &
/// Q(assignees__member_project__is_active=True) &
/// Q(issue_assignee__deleted_at__isnull=True)), [])`.
/// Assignees drop inactive members AND deleted links.
pub fn patch_assignee_ids_fragment() -> String {
    r#"COALESCE(ARRAY_AGG(DISTINCT "users"."id") FILTER (WHERE (NOT ("users"."id" IS NULL) AND "project_members"."is_active" AND "issue_assignees"."deleted_at" IS NULL)), '{}')"#.to_owned()
}

/// `Issue.objects.annotate(label_ids=..., assignees...).get(
/// pk=$1, workspace__slug=$2, project_id=$3)` (`:350-371`) as SQL
/// text. Django groups an aggregate-over-join read by the selected
/// PK (`GROUP BY "issues"."id"`).
///
/// Bind contract: `$1` issue id, `$2` workspace slug,
/// `$3` project id.
pub fn patch_issue_lookup_sql() -> String {
    format!(
        r#"SELECT "{i}".*, {labels} AS "label_ids", {assignees} AS "assignee_ids" FROM "{i}" INNER JOIN "{w}" ON ("{i}"."workspace_id" = "{w}"."id") WHERE ("{i}"."deleted_at" IS NULL AND "{i}"."id" = $1 AND "{w}"."slug" = $2 AND "{i}"."project_id" = $3) GROUP BY "{i}"."id""#,
        i = ISSUE_TABLE,
        w = WORKSPACE_TABLE,
        labels = label_ids_fragment(),
        assignees = patch_assignee_ids_fragment(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intake_lookup_is_first_by_name() {
        // fx-q-intake list_detail_queryset (`views/intake.py:64-67`).
        let sql = intake_lookup_sql();
        assert!(sql.contains(r#"FROM "intakes""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "workspaces""#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = $1"#), "{sql}");
        assert!(sql.contains(r#""intakes"."project_id" = $2"#), "{sql}");
        assert!(sql.contains(r#""intakes"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "intakes"."name" ASC"#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn list_carries_snoozed_guard_and_default_order() {
        // fx-q-intake list_detail_queryset (`views/intake.py:74-83`).
        let sql = intake_issue_list_sql(None);
        assert!(
            sql.contains(
                r#"("intake_issues"."snoozed_till" >= $1 OR "intake_issues"."snoozed_till" IS NULL)"#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""intake_issues"."intake_id" = $2"#), "{sql}");
        assert!(
            sql.contains(r#""intake_issues"."project_id" = $3"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = $4"#), "{sql}");
        assert!(
            sql.contains(r#""intake_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#"INNER JOIN "issues""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "workspaces""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "projects""#), "{sql}");
        // Joined tables carry no deleted predicate (manager scopes the
        // base model only).
        assert_eq!(sql.matches(r#""deleted_at" IS NULL"#).count(), 1, "{sql}");
        // Default order_by kwarg `-created_at`.
        assert!(
            sql.contains(r#"ORDER BY "intake_issues"."created_at" DESC"#),
            "{sql}"
        );
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn order_kwarg_maps_fields_and_passes_through() {
        // `order_by` kwarg (`views/intake.py:82`).
        assert_eq!(
            intake_issue_order_sql(None),
            r#""intake_issues"."created_at" DESC"#
        );
        assert_eq!(
            intake_issue_order_sql(Some("snoozed_till")),
            r#""intake_issues"."snoozed_till" ASC"#
        );
        assert_eq!(
            intake_issue_order_sql(Some("-status")),
            r#""intake_issues"."status" DESC"#
        );
        // FK attname spelling resolves like the field spelling.
        assert_eq!(
            intake_issue_order_sql(Some("project_id")),
            r#""intake_issues"."project_id" ASC"#
        );
        // Unknown fields pass through for the handlers-owned 500.
        assert_eq!(intake_issue_order_sql(Some("nope")), "nope");
        assert_eq!(intake_issue_order_sql(Some("")), "");
        let sql = intake_issue_list_sql(Some("status"));
        assert!(
            sql.contains(r#"ORDER BY "intake_issues"."status" ASC"#),
            "{sql}"
        );
    }

    #[test]
    fn detail_adds_issue_get_to_list_scope() {
        // fx-q-intake list_detail_queryset, retrieve path
        // (`views/intake.py:233-253,277`).
        let sql = intake_issue_detail_sql(None);
        assert!(sql.contains(r#""intake_issues"."issue_id" = $5"#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "intake_issues"."created_at" DESC"#),
            "{sql}"
        );
        assert!(sql.contains("LIMIT 1"), "{sql}");
        // Same snoozed filter as the list path (snoozed-out rows 404).
        assert!(
            sql.contains(
                r#"("intake_issues"."snoozed_till" >= $1 OR "intake_issues"."snoozed_till" IS NULL)"#
            ),
            "{sql}"
        );
    }

    #[test]
    fn triage_lookup_scopes_triage_group() {
        // fx-q-intake triage_get_or_create (`views/intake.py:173`).
        let sql = triage_lookup_sql();
        assert!(sql.contains(r#"FROM "states""#), "{sql}");
        assert!(sql.contains(r#""states"."group" = 'triage'"#), "{sql}");
        assert!(sql.contains(r#""states"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""states"."project_id" = $1"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = $2"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "states"."sequence" ASC"#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn triage_insert_carries_call_site_literals() {
        // fx-q-intake triage_get_or_create (`views/intake.py:176-184`):
        // name Triage, group triage, color #4E5355, sequence 65000,
        // default false.
        let sql = triage_insert_sql();
        assert!(sql.starts_with(r#"INSERT INTO "states""#), "{sql}");
        assert!(sql.contains("'Triage'"), "{sql}");
        assert!(sql.contains("'triage'"), "{sql}");
        assert!(sql.contains("'#4E5355'"), "{sql}");
        assert!(sql.contains("65000"), "{sql}");
        assert!(sql.contains("$4, $5"), "{sql}");
        // Plain create (not get_or_create): no ON CONFLICT clause.
        assert!(!sql.contains("ON CONFLICT"), "{sql}");
    }

    #[test]
    fn patch_fragments_match_fixture_guards() {
        // fx-q-intake patch_issue_annotations
        // (`views/intake.py:350-371`). Exact fixture strings.
        assert_eq!(
            label_ids_fragment(),
            r#"COALESCE(ARRAY_AGG(DISTINCT "labels"."id") FILTER (WHERE (NOT ("labels"."id" IS NULL) AND "issue_labels"."deleted_at" IS NULL)), '{}')"#
        );
        assert_eq!(
            patch_assignee_ids_fragment(),
            r#"COALESCE(ARRAY_AGG(DISTINCT "users"."id") FILTER (WHERE (NOT ("users"."id" IS NULL) AND "project_members"."is_active" AND "issue_assignees"."deleted_at" IS NULL)), '{}')"#
        );
        // Labels drop deleted links only (no active check);
        // assignees check both.
        assert!(
            !label_ids_fragment().contains("is_active"),
            "{}",
            label_ids_fragment()
        );
        assert!(patch_assignee_ids_fragment().contains("is_active"));
    }

    #[test]
    fn patch_lookup_scopes_issue_get() {
        // fx-q-intake patch_issue_annotations `.get(...)`
        // (`views/intake.py:371`).
        let sql = patch_issue_lookup_sql();
        assert!(sql.contains(r#"FROM "issues""#), "{sql}");
        assert!(sql.contains(r#""issues"."id" = $1"#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = $2"#), "{sql}");
        assert!(sql.contains(r#""issues"."project_id" = $3"#), "{sql}");
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#"AS "label_ids""#), "{sql}");
        assert!(sql.contains(r#"AS "assignee_ids""#), "{sql}");
        assert!(sql.contains(r#"GROUP BY "issues"."id""#), "{sql}");
    }
}
