//! Issue core table models (D-26, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/issue.py:95-357` (`IssueManager`,
//! `Issue`), `:445-469` (`IssueAssignee`), `:669-681` (`IssueLabel`),
//! `db/models/label.py:11-57` (`Label`) and
//! `db/models/project.py:464-495` (`ProjectUserProperty`), adopting the
//! Django-owned schema column-for-column; migrations are not ported —
//! Django stays schema owner until switchover. Fixture source of truth:
//! `rust-api/fixtures/app_issues/models/FX-ISS-07.core.json` (recorded by
//! PIDASHCONV-637); the `#[cfg(test)]` suite replays it section by
//! section.
//!
//! Column order in each `*_COLUMNS` const follows the fixture: the 8
//! inherited audit/project columns first (`id`, `created_at`,
//! `updated_at`, `created_by_id`, `updated_by_id`, `deleted_at`, then
//! `project_id`, `workspace_id` from `ProjectBaseModel` at
//! `db/models/project.py:302-311` — or `workspace_id`, `project_id`
//! from `WorkspaceBaseModel` at `db/models/workspace.py:185-195` for
//! `Label`, which declares them in that order), then the model fields in
//! declaration order. FK entries use the Django attnames (`project_id`,
//! `parent_id`, `state_id`, `type_id`, `assigned_pod_id`, …). Every
//! application-level default below is Django-side (the live tables carry
//! no `column_default` in `information_schema`, as established for D-01);
//! Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All five tables inherit the soft-delete marker (`deleted_at`, from
//! `SoftDeleteModel` in `pi_dash/db/mixins.py:61-67`) and the default
//! manager filters `deleted_at IS NULL` (`objects = SoftDeletionManager`,
//! `mixins.py:66`; `all_objects` is the plain unscoped manager,
//! `mixins.py:67`). Every read built from these tables must apply
//! [`crate::soft_delete::active_condition`]; the tests pin this by
//! rendering a scoped `SELECT` per table.
//!
//! `Issue` declares no `objects =`, so `Issue.objects` is that plain
//! inherited soft-delete scope; the narrower `issue_objects =
//! IssueManager` (`issue.py:95-104,229`) additionally excludes
//! `state__group = 'triage'`, `archived_at IS NOT NULL`,
//! `project__archived_at IS NOT NULL` and `is_draft = true`. That scope
//! is already merged as [`crate::space::columns::issue_objects_scope`]
//! (D-02) and is reused, not re-ported; likewise the `State` default /
//! triage scopes live in `space::columns` (`state_default_scope`,
//! `state_triage_scope`). The tests pin that every single-table column
//! those scopes read exists in [`issue::COLUMNS`].
//!
//! # Writes backfill the workspace
//!
//! `ProjectBaseModel.save()` (`db/models/project.py:309-311`) sets
//! `workspace` from `project.workspace` on every save: Rust inserts and
//! updates of `Issue`, `IssueAssignee`, `IssueLabel` and
//! `ProjectUserProperty` must resolve `workspace_id` from the
//! `project_id` row explicitly. `Label` extends `WorkspaceBaseModel`
//! instead (`workspace.py:192-195`), whose `save()` backfills only when
//! `project` is set — workspace-level labels keep their explicit
//! `workspace_id`.
//!
//! # No `delete()` override (TRACE bug 24)
//!
//! `Issue` defines `save()` only; `delete()` is
//! `SoftDeleteModel.delete` (`mixins.py:72-78`), which stamps
//! `deleted_at` and calls `save()` — so destroy DOES emit
//! `pre_save`/`post_save`, and both orchestration receiver pairs no-op
//! on the unchanged state. The same holds for the other four models.
//! There is no code to port here (shared kernel behavior); the write
//! layers must not assume "no signal on destroy".
//!
//! # Save orchestration owned elsewhere
//!
//! `Issue.save` (`issue.py:267-351`) creation path — the
//! project advisory lock, `sequence_id = max + 1` (1 when none), the
//! `IssueSequence` row, `Pod.default_for_project_id` resolution and the
//! default-state lookup — plus `has_active_run` (`:232-248`, an
//! `AgentRun` existence check) are write-path orchestration over tables
//! this layer does not own; the create handler (PIDASHCONV-651) and the
//! queries layer (PIDASHCONV-648/649) port them. This module ports the
//! pure column-value halves: [`issue::new_sort_order`] /
//! [`issue::largest_sort_order_sql`],
//! [`label::new_sort_order`] / [`label::largest_sort_order_sql`], the
//! `completed_at` group marker and the `description_stripped` none rule.
//!
//! # Foreign reads: reuse, else pinned literals
//!
//! The columns D-26 reads on foreign tables (fixture `foreign_reads`)
//! reuse the owning domain's merged defs where present, else the SQL
//! literals pinned in [`foreign`] (query builders copy those literals;
//! the tests assert they equal the fixture):
//!
//! | Table | Columns D-26 reads | Merged def or pin |
//! | --- | --- | --- |
//! | `projects` | `id`, `workspace_id`, `identifier`, `default_assignee_id`, `guest_view_all_features`, `archived_at`, ticker columns, `default_agent_executor` | `v1_projects::models::project` (D-19, covers all incl. the agent/ticker tail) |
//! | `project_members` | `id`, `project_id`, `workspace_id`, `member_id`, `role`, `is_active` | `v1_projects::models::project_member` (D-19) |
//! | `states` | `id`, `project_id`, `name`, `group`, `default`, `is_triage` | `v1_projects::models::state` (D-19) |
//! | `estimate_points` | `id`, `project_id` | `v1_projects::models::estimate_point` (D-19) |
//! | `users` | `id`, `display_name`, `email` | [`foreign`] literals (D-24 owns the port) |
//! | `workspaces` | `id`, `slug` | [`foreign`] literals (D-24 owns the port) |
//! | `issue_types` | `id`, `is_epic` | [`foreign`] literals |
//!
//! # Ported quirks (translate as-is)
//!
//! `issue.py:95-357`, `label.py:11-57` and `project.py:464-495` were read
//! line by line for this layer. The following behaviors mistranslate
//! easily and are ported exactly:
//!
//! * `IssueLabel` carries no partial-unique constraint (dupes are
//!   prevented only by the write serializer's `ignore_conflicts`),
//!   while `IssueAssignee` has one. Ported as-is
//!   ([`issue_label::HAS_PARTIAL_UNIQUE`]).
//! * `Label.save` aggregates `MAX(sort_order)` over
//!   `project=self.project`; for workspace-level labels (`project`
//!   None) that is the global null-project max, not a per-workspace
//!   one. Ported as-is ([`label::largest_sort_order_sql`] renders
//!   `project_id IS NULL` for `None`).
//! * `Issue.state` and `Issue.parent` are `CASCADE`: deleting a state
//!   or a parent deletes its issues. Ported as-is.
//! * `archived_at` is a `DateField` while `completed_at` is a
//!   `DateTimeField`. Ported as `NaiveDate` vs `DateTime<Utc>`.
//! * A new issue's `description_stripped` is `""`
//!   (`strip_tags("<p></p>")`), not `None` — `None` only when the html
//!   input is `""`/`None`. Ported as-is
//!   ([`issue::description_stripped_is_none`]).

use serde::{Deserialize, Serialize};

/// Django-level FK delete behavior (ORM-emulated; same shape as the
/// D-05 `integrations::OnDelete`, D-19 `v1_projects::models::OnDelete`
/// and D-22 `v1_cli_auth::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
    /// `models.PROTECT` — deleting the parent raises instead.
    Protect,
}

/// `issues` table (`issue.py:107-357`).
pub mod issue {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:253`).
    pub const TABLE: &str = "issues";
    /// Default ordering (`Meta.ordering`, `issue.py:254`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` / `verbose_name_plural` (`issue.py:251-252`).
    pub const VERBOSE_NAME: &str = "Issue";
    /// `verbose_name_plural` (`issue.py:252`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issues";

    /// Columns in fixture FX-ISS-07 order: 8 inherited audit/project
    /// columns, then `issue.py:115-227` in declaration order (the
    /// `assignees` `:161-167` and `labels` `:169` M2Ms contribute no
    /// columns — they materialize through `issue_assignees` /
    /// `issue_labels`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "parent_id",
        "state_id",
        "point",
        "estimate_point_id",
        "name",
        "description_json",
        "description_html",
        "description_stripped",
        "description_binary",
        "priority",
        "complexity_score",
        "start_date",
        "target_date",
        "sequence_id",
        "sort_order",
        "completed_at",
        "archived_at",
        "is_draft",
        "external_source",
        "external_id",
        "type_id",
        "git_work_branch",
        "workpad",
        "created_via",
        "assigned_pod_id",
        "agent_executor",
    ];

    /// `name` bound (`issue.py:137`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;
    /// `priority` bound (`issue.py:142-147`, `max_length=30`).
    pub const PRIORITY_MAX_LENGTH: usize = 30;
    /// `external_source` bound (`issue.py:174`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`issue.py:175`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;
    /// `git_work_branch` bound (`issue.py:183-193`, `max_length=128`).
    pub const GIT_WORK_BRANCH_MAX_LENGTH: usize = 128;
    /// `created_via` bound (`issue.py:202`, `max_length=32`).
    pub const CREATED_VIA_MAX_LENGTH: usize = 32;
    /// `agent_executor` bound (`issue.py:222-227`, `max_length=24`).
    pub const AGENT_EXECUTOR_MAX_LENGTH: usize = 24;

    /// `point` validators (`issue.py:129`, 0..12 inclusive).
    pub const POINT_MIN: i32 = 0;
    /// `point` validators (`issue.py:129`, 0..12 inclusive).
    pub const POINT_MAX: i32 = 12;
    /// `complexity_score` validators (`issue.py:154-158`, 0..10
    /// inclusive; 0 is the "unrated" default).
    pub const COMPLEXITY_SCORE_MIN: i32 = 0;
    /// `complexity_score` validators (`issue.py:154-158`, 0..10
    /// inclusive; 0 is the "unrated" default).
    pub const COMPLEXITY_SCORE_MAX: i32 = 10;
    /// `git_work_branch` pattern (`issue.py:187-192`,
    /// `RegexValidator`). No `regex` dependency in this crate, so the
    /// pattern ships as a string; matching lives with the validating
    /// caller.
    pub const GIT_WORK_BRANCH_PATTERN: &str = r"^[A-Za-z0-9._/-]*$";
    /// `git_work_branch` violation message (`issue.py:190`).
    pub const GIT_WORK_BRANCH_MESSAGE: &str =
        "Branch name may contain only letters, numbers, and . _ / -";

    /// `priority` choices (`PRIORITY_CHOICES`, `issue.py:108-114`).
    pub const PRIORITY_CHOICES: &[(&str, &str)] = &[
        ("urgent", "Urgent"),
        ("high", "High"),
        ("medium", "Medium"),
        ("low", "Low"),
        ("none", "None"),
    ];
    /// `agent_executor` choices (`AgentExecutorKind.choices`,
    /// `core/agent_execution.py:7-16`).
    pub const AGENT_EXECUTOR_CHOICES: &[(&str, &str)] = &[
        ("local_runner", "Local Runner"),
        ("cloud_agent", "Pi Dash Cloud Agent"),
        ("managed_runner", "Pi Dash Agent"),
    ];

    /// `description_json` Django-side default (`issue.py:138`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const DESCRIPTION_JSON_DEFAULT: &str = "{}";
    /// `description_html` Django-side default (`issue.py:139`,
    /// `default="<p></p>"`).
    pub const DESCRIPTION_HTML_DEFAULT: &str = "<p></p>";
    /// `priority` Django-side default (`issue.py:146`,
    /// `default="none"`).
    pub const PRIORITY_DEFAULT: &str = "none";
    /// `complexity_score` Django-side default (`issue.py:155`,
    /// `default=0`, i.e. unrated).
    pub const COMPLEXITY_SCORE_DEFAULT: i32 = 0;
    /// `sequence_id` Django-side default (`issue.py:168`, `default=1`);
    /// `save()` overwrites it with `max + 1` on creation (`:324-328`).
    pub const SEQUENCE_ID_DEFAULT: i32 = 1;
    /// `sort_order` Django-side default (`issue.py:170`,
    /// `default=65535`). Kept when the state has no issues yet
    /// (`issue.py:338-339`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// Step applied above the state maximum on create (`issue.py:339`);
    /// note the direction is opposite to `Cycle.save`, which subtracts
    /// below the project minimum.
    pub const SORT_ORDER_STEP: f64 = 10000.0;
    /// `is_draft` Django-side default (`issue.py:173`,
    /// `default=False`).
    pub const IS_DRAFT_DEFAULT: bool = false;
    /// `git_work_branch` Django-side default (`issue.py:186`,
    /// `default=""`).
    pub const GIT_WORK_BRANCH_DEFAULT: &str = "";
    /// `workpad` Django-side default (`issue.py:198`, `default=""`).
    pub const WORKPAD_DEFAULT: &str = "";

    /// State group that stamps `completed_at`: `save()` sets
    /// `completed_at = now` when `state.group == "completed"`, else
    /// `None` (`issue.py:306-309`). The value is
    /// `StateGroup.COMPLETED` (`db/models/state.py:20`); the write path
    /// owns the timestamp.
    pub const COMPLETED_GROUP: &str = "completed";

    /// Full-text index name (`Meta.indexes`, `issue.py:261-264`). The
    /// index expression must stay byte-for-byte identical to the runtime
    /// `SearchVector` for the planner to use it (`:256-260`).
    pub const FTS_INDEX_NAME: &str = "issues_fts_idx";
    /// Full-text indexed columns (`issue.py:262`).
    pub const FTS_COLUMNS: &[&str] = &["name", "description_stripped"];
    /// Full-text search config (`issue.py:262`).
    pub const FTS_CONFIG: &str = "english";

    /// `parent` FK: `CASCADE` (`issue.py:115-121`).
    pub const PARENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `state` FK: `CASCADE` (`issue.py:122-128`).
    pub const STATE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `estimate_point` FK: `SET_NULL`, nullable (`issue.py:130-136`).
    pub const ESTIMATE_POINT_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `type` FK: `SET_NULL`, nullable (`issue.py:176-182`).
    pub const TYPE_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `assigned_pod` FK: `PROTECT`, nullable (`issue.py:206-212`;
    /// safe because pods are soft-deleted, never physically removed,
    /// `:205`).
    pub const ASSIGNED_POD_ON_DELETE: OnDelete = OnDelete::Protect;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue row. `start_date`/`target_date`/`archived_at` are
    /// calendar dates (`DateField`, `issue.py:159-160,172`) while
    /// `completed_at` is a timestamp (`DateTimeField`, `:171`);
    /// `sort_order` is a float (`FloatField`, `:170`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Issue {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub parent_id: Option<uuid::Uuid>,
        pub state_id: Option<uuid::Uuid>,
        pub point: Option<i32>,
        pub estimate_point_id: Option<uuid::Uuid>,
        pub name: String,
        pub description_json: serde_json::Value,
        pub description_html: String,
        pub description_stripped: Option<String>,
        pub description_binary: Option<Vec<u8>>,
        pub priority: String,
        pub complexity_score: i32,
        pub start_date: Option<chrono::NaiveDate>,
        pub target_date: Option<chrono::NaiveDate>,
        pub sequence_id: i32,
        pub sort_order: f64,
        pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
        pub archived_at: Option<chrono::NaiveDate>,
        pub is_draft: bool,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub type_id: Option<uuid::Uuid>,
        pub git_work_branch: String,
        pub workpad: String,
        pub created_via: Option<String>,
        pub assigned_pod_id: Option<uuid::Uuid>,
        pub agent_executor: Option<String>,
    }

    impl std::fmt::Display for Issue {
        /// `__str__` (`issue.py:353-355`): `"{name} <{project.name}>"`.
        /// Rust holds only the project FK, so the display renders the
        /// project id; the name join is owned by the queries layer.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} <{}>", self.name, self.project_id)
        }
    }

    /// Pure half of the creation `sort_order` rule (`issue.py:335-339`):
    /// `sort_order = largest + 10000` when the state has issues;
    /// returns `None` to mean "keep `DEFAULT_SORT_ORDER`".
    pub fn new_sort_order(largest: Option<f64>) -> Option<f64> {
        largest.map(|max| max + SORT_ORDER_STEP)
    }

    /// Render the creation maximum lookup (`issue.py:335-337`) as SQL:
    /// `SELECT MAX("sort_order") FROM "issues" WHERE "project_id" = ...`
    /// `AND "state_id" = ... AND "deleted_at" IS NULL`.
    ///
    /// The `deleted_at IS NULL` conjunct is the `Issue.objects` scope:
    /// `Issue` declares no `objects =`, so `:335` uses the plain
    /// inherited `SoftDeletionManager` (`mixins.py:66`), *not* the
    /// narrower `issue_objects`. `state_id` renders `IS NULL` when the
    /// issue has no state yet (the default-state lookup may leave it
    /// `None`, `:288-301`).
    pub fn largest_sort_order_sql(project_id: uuid::Uuid, state_id: Option<uuid::Uuid>) -> String {
        use sea_query::{Alias, Expr, Func, PostgresQueryBuilder, Query};
        let state_cond = match state_id {
            Some(id) => Expr::col(Alias::new("state_id")).eq(id),
            None => Expr::col(Alias::new("state_id")).is_null(),
        };
        let mut select = Query::select();
        select
            .expr(Func::max(Expr::col(Alias::new("sort_order"))))
            .from(Alias::new(TABLE))
            .cond_where(Expr::col(Alias::new("project_id")).eq(project_id))
            .cond_where(state_cond)
            .cond_where(crate::soft_delete::active_condition());
        select.to_string(PostgresQueryBuilder)
    }

    /// Pure none-condition of the `description_stripped` rule
    /// (`issue.py:330-334,346-350`): `None` when the html input is `""`
    /// or `None`, otherwise `strip_tags(html)`. The strip itself needs
    /// the HTML engine and lives with the write path; this pins the
    /// branch, including the quirk that a default-constructed issue
    /// strips to `""` (`strip_tags("<p></p>")`), not `None`.
    pub fn description_stripped_is_none(html: Option<&str>) -> bool {
        matches!(html, None | Some(""))
    }
}

/// `issue_assignees` link table (`issue.py:445-469`).
pub mod issue_assignee {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:464`).
    pub const TABLE: &str = "issue_assignees";
    /// Default ordering (`Meta.ordering`, `issue.py:465`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:462`).
    pub const VERBOSE_NAME: &str = "Issue Assignee";
    /// `verbose_name_plural` (`issue.py:463`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Assignees";

    /// Columns in fixture FX-ISS-07 order: 8 inherited audit/project
    /// columns, then `issue_id` (`issue.py:446`) and `assignee_id`
    /// (`issue.py:447-451`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "issue_id",
        "assignee_id",
    ];

    /// `Meta.unique_together` (`issue.py:454`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "assignee", "deleted_at"];
    /// Partial unique constraint name (`issue.py:459`).
    pub const UNIQUE_ISSUE_ASSIGNEE_NAME: &str =
        "issue_assignee_unique_issue_assignee_when_deleted_at_null";
    /// Columns of [`UNIQUE_ISSUE_ASSIGNEE_NAME`] (`issue.py:457`),
    /// physical.
    pub const UNIQUE_ISSUE_ASSIGNEE_COLUMNS: &[&str] = &["issue_id", "assignee_id"];
    /// `WHERE` of [`UNIQUE_ISSUE_ASSIGNEE_NAME`] (`issue.py:458`,
    /// `deleted_at__isnull=True`): the link is unique among live rows
    /// only, so a soft-deleted link can be re-created.
    pub const UNIQUE_ISSUE_ASSIGNEE_WHERE: &str = "deleted_at IS NULL";

    /// `issue` FK: `CASCADE` (`issue.py:446`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `assignee` FK: `CASCADE` (`issue.py:447-451`).
    pub const ASSIGNEE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-assignee link row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueAssignee {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub assignee_id: uuid::Uuid,
    }

    impl IssueAssignee {
        /// `__str__` (`issue.py:467-468`):
        /// `"{issue.name} {assignee.email}"`. Rust holds only the FKs,
        /// so the label takes both joined values; the join itself is
        /// owned by the queries layer.
        pub fn label(issue_name: &str, assignee_email: &str) -> String {
            format!("{issue_name} {assignee_email}")
        }
    }
}

/// `issue_labels` link table (`issue.py:669-681`).
pub mod issue_label {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `issue.py:676`).
    pub const TABLE: &str = "issue_labels";
    /// Default ordering (`Meta.ordering`, `issue.py:677`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`issue.py:674`).
    pub const VERBOSE_NAME: &str = "Issue Label";
    /// `verbose_name_plural` (`issue.py:675`).
    pub const VERBOSE_NAME_PLURAL: &str = "Issue Labels";

    /// Columns in fixture FX-ISS-07 order: 8 inherited audit/project
    /// columns, then `issue_id` (`issue.py:670`) and `label_id`
    /// (`issue.py:671`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "issue_id",
        "label_id",
    ];

    /// No `unique_together`, no partial-unique constraint
    /// (`issue.py:673-677` declares `Meta` without either): duplicate
    /// (issue, label) rows are prevented only by the write serializer's
    /// `ignore_conflicts`, unlike [`super::issue_assignee`]. Ported
    /// as-is — a future "fix" adding a constraint would change write
    /// semantics.
    pub const HAS_PARTIAL_UNIQUE: bool = false;

    /// `issue` FK: `CASCADE` (`issue.py:670`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `label` FK: `CASCADE` (`issue.py:671`).
    pub const LABEL_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One issue-label link row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueLabel {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub issue_id: uuid::Uuid,
        pub label_id: uuid::Uuid,
    }

    impl IssueLabel {
        /// `__str__` (`issue.py:679-680`):
        /// `"{issue.name} {label.name}"`. Rust holds only the FKs, so
        /// the label takes both joined values; the join itself is owned
        /// by the queries layer.
        pub fn label(issue_name: &str, label_name: &str) -> String {
            format!("{issue_name} {label_name}")
        }
    }
}

/// `labels` table (`db/models/label.py:11-57`).
pub mod label {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `label.py:43`).
    pub const TABLE: &str = "labels";
    /// Default ordering (`Meta.ordering`, `label.py:44`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`label.py:41`).
    pub const VERBOSE_NAME: &str = "Label";
    /// `verbose_name_plural` (`label.py:42`).
    pub const VERBOSE_NAME_PLURAL: &str = "Labels";

    /// Columns in fixture FX-ISS-07 order: 6 inherited audit columns,
    /// then `workspace_id`, `project_id` (`WorkspaceBaseModel`,
    /// `workspace.py:186-187`, in that declaration order — note
    /// `project` is nullable: workspace-level labels are allowed),
    /// then `label.py:12-24` in declaration order. FK columns use the
    /// Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
        "parent_id",
        "name",
        "description",
        "color",
        "sort_order",
        "external_source",
        "external_id",
    ];

    /// `name` bound (`label.py:19`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;
    /// `color` bound (`label.py:21`, `max_length=255`).
    pub const COLOR_MAX_LENGTH: usize = 255;
    /// `external_source` bound (`label.py:23`, `max_length=255`).
    pub const EXTERNAL_SOURCE_MAX_LENGTH: usize = 255;
    /// `external_id` bound (`label.py:24`, `max_length=255`).
    pub const EXTERNAL_ID_MAX_LENGTH: usize = 255;

    /// `description` Django-side default (`label.py:20`, `blank=True`
    /// with no explicit default: the empty string).
    pub const DESCRIPTION_DEFAULT: &str = "";
    /// `color` Django-side default (`label.py:21`, `blank=True` with no
    /// explicit default: the empty string).
    pub const COLOR_DEFAULT: &str = "";
    /// `sort_order` Django-side default (`label.py:22`,
    /// `default=65535`). Kept when the project has no labels yet
    /// (`label.py:51-52`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// Step applied above the project maximum on create (`label.py:52`).
    pub const SORT_ORDER_STEP: f64 = 10000.0;

    /// Name-uniqueness constraint for workspace-level labels
    /// (`label.py:29-33`).
    pub const UNIQUE_NAME_WHEN_PROJECT_NULL_NAME: &str =
        "unique_name_when_project_null_and_not_deleted";
    /// Columns of [`UNIQUE_NAME_WHEN_PROJECT_NULL_NAME`]
    /// (`label.py:30`), physical.
    pub const UNIQUE_NAME_WHEN_PROJECT_NULL_COLUMNS: &[&str] = &["name"];
    /// `WHERE` of [`UNIQUE_NAME_WHEN_PROJECT_NULL_NAME`]
    /// (`label.py:31`, `project__isnull=True, deleted_at__isnull=True`).
    pub const UNIQUE_NAME_WHEN_PROJECT_NULL_WHERE: &str =
        "project_id IS NULL AND deleted_at IS NULL";
    /// Name-uniqueness constraint for project labels (`label.py:35-39`).
    pub const UNIQUE_PROJECT_NAME_NAME: &str = "unique_project_name_when_not_deleted";
    /// Columns of [`UNIQUE_PROJECT_NAME_NAME`] (`label.py:36`), physical.
    pub const UNIQUE_PROJECT_NAME_COLUMNS: &[&str] = &["project_id", "name"];
    /// `WHERE` of [`UNIQUE_PROJECT_NAME_NAME`] (`label.py:37`,
    /// `project__isnull=False, deleted_at__isnull=True`).
    pub const UNIQUE_PROJECT_NAME_WHERE: &str = "project_id IS NOT NULL AND deleted_at IS NULL";

    /// `parent` FK: `CASCADE`, nullable (`label.py:12-18`).
    pub const PARENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE`, nullable (`workspace.py:187`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`workspace.py:186`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One label row. `project_id` is nullable (workspace-level labels,
    /// `workspace.py:187`); `workspace_id` is not.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Label {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub parent_id: Option<uuid::Uuid>,
        pub name: String,
        pub description: String,
        pub color: String,
        pub sort_order: f64,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
    }

    impl std::fmt::Display for Label {
        /// `__str__` (`label.py:56-57`): `str(self.name)`.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.name)
        }
    }

    /// Pure half of the creation `sort_order` rule (`label.py:47-52`):
    /// `sort_order = largest + 10000` when the project has labels;
    /// returns `None` to mean "keep `DEFAULT_SORT_ORDER`".
    pub fn new_sort_order(largest: Option<f64>) -> Option<f64> {
        largest.map(|max| max + SORT_ORDER_STEP)
    }

    /// Render the creation maximum lookup (`label.py:49`) as SQL:
    /// `SELECT MAX("sort_order") FROM "labels" WHERE "project_id" = ...`
    /// `AND "deleted_at" IS NULL`.
    ///
    /// The `deleted_at IS NULL` conjunct is the default-manager scope
    /// (`SoftDeletionManager`, `mixins.py:56-58`): `Label` declares no
    /// custom manager, so `Label.objects.filter(project=...)` in `:49`
    /// already excludes tombstones. `project_id` renders `IS NULL` for
    /// workspace-level labels — ported as-is, including the quirk that
    /// the max then spans every null-project label rather than one
    /// workspace's.
    pub fn largest_sort_order_sql(project_id: Option<uuid::Uuid>) -> String {
        use sea_query::{Alias, Expr, Func, PostgresQueryBuilder, Query};
        let project_cond = match project_id {
            Some(id) => Expr::col(Alias::new("project_id")).eq(id),
            None => Expr::col(Alias::new("project_id")).is_null(),
        };
        let mut select = Query::select();
        select
            .expr(Func::max(Expr::col(Alias::new("sort_order"))))
            .from(Alias::new(TABLE))
            .cond_where(project_cond)
            .cond_where(crate::soft_delete::active_condition());
        select.to_string(PostgresQueryBuilder)
    }
}

/// `project_user_properties` table (`db/models/project.py:464-495`).
pub mod project_user_property {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Physical table (`Meta.db_table`, `project.py:482`).
    pub const TABLE: &str = "project_user_properties";
    /// Default ordering (`Meta.ordering`, `project.py:483`).
    pub const ORDERING: &str = "-created_at";
    /// `verbose_name` (`project.py:480`).
    pub const VERBOSE_NAME: &str = "Project User Property";
    /// `verbose_name_plural` (`project.py:481`).
    pub const VERBOSE_NAME_PLURAL: &str = "Project User Properties";

    /// Columns in fixture FX-ISS-07 order: 8 inherited audit/project
    /// columns, then `project.py:467-477` in declaration order. FK
    /// columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "user_id",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "preferences",
        "sort_order",
    ];

    /// `Meta.unique_together` (`project.py:484`), Django field names.
    pub const UNIQUE_TOGETHER: &[&str] = &["user", "project", "deleted_at"];
    /// Partial unique constraint name (`project.py:489`).
    pub const UNIQUE_USER_PROJECT_NAME: &str =
        "project_user_property_unique_user_project_when_deleted_at_null";
    /// Columns of [`UNIQUE_USER_PROJECT_NAME`] (`project.py:487`),
    /// physical.
    pub const UNIQUE_USER_PROJECT_COLUMNS: &[&str] = &["user_id", "project_id"];
    /// `WHERE` of [`UNIQUE_USER_PROJECT_NAME`] (`project.py:488`,
    /// `deleted_at__isnull=True`): one live properties row per
    /// (user, project); a soft-deleted row can be replaced.
    pub const UNIQUE_USER_PROJECT_WHERE: &str = "deleted_at IS NULL";

    /// `user` FK: `CASCADE` (`project.py:467-471`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `project` FK: `CASCADE` (`project.py:303`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `workspace` FK: `CASCADE` (`project.py:304`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `created_by` FK: `SET_NULL`, nullable (`mixins.py:29-35`).
    pub const CREATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `updated_by` FK: `SET_NULL`, nullable (`mixins.py:36-42`).
    pub const UPDATED_BY_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// `sort_order` Django-side default (`project.py:477`,
    /// `default=65535`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;
    /// `rich_filters` Django-side default (`project.py:475`,
    /// `default=dict`): empty JSON object, supplied explicitly on every
    /// Rust insert.
    pub const RICH_FILTERS_EMPTY: &str = "{}";

    /// `filters` Django-side default (`project.py:472`,
    /// `default=get_default_filters` from `db/models/issue.py:50-61`).
    /// Fresh value per call, matching the Python function returning a
    /// new dict each time.
    pub fn default_filters() -> serde_json::Value {
        serde_json::json!({
            "priority": null,
            "state": null,
            "state_group": null,
            "assignees": null,
            "created_by": null,
            "labels": null,
            "start_date": null,
            "target_date": null,
            "subscriber": null,
        })
    }

    /// `display_filters` Django-side default (`project.py:473`,
    /// `default=get_default_display_filters` from
    /// `db/models/issue.py:64-73`). Fresh value per call.
    pub fn default_display_filters() -> serde_json::Value {
        serde_json::json!({
            "group_by": null,
            "order_by": "-created_at",
            "type": null,
            "sub_issue": true,
            "show_empty_groups": true,
            "layout": "list",
            "calendar_date_range": "",
        })
    }

    /// `display_properties` Django-side default (`project.py:474`,
    /// `default=get_default_display_properties` from
    /// `db/models/issue.py:76-91`). Fresh value per call.
    pub fn default_display_properties() -> serde_json::Value {
        serde_json::json!({
            "assignee": true,
            "attachment_count": true,
            "created_on": true,
            "due_date": true,
            "estimate": true,
            "key": true,
            "labels": true,
            "link": true,
            "priority": true,
            "start_date": true,
            "state": true,
            "sub_issue_count": true,
            "updated_on": true,
        })
    }

    /// `preferences` Django-side default (`project.py:476`,
    /// `default=get_default_preferences` from
    /// `db/models/project.py:68-69`). Fresh value per call.
    pub fn default_preferences() -> serde_json::Value {
        serde_json::json!({
            "pages": {"block_display": true},
            "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
        })
    }

    /// One project-user-properties row. The JSON columns carry the
    /// Django-side defaults above on insert (same as the Python
    /// callables); `rich_filters` defaults to `{}` and `sort_order` to
    /// 65535.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectUserProperty {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub filters: serde_json::Value,
        pub display_filters: serde_json::Value,
        pub display_properties: serde_json::Value,
        pub rich_filters: serde_json::Value,
        pub preferences: serde_json::Value,
        pub sort_order: f64,
    }

    impl ProjectUserProperty {
        /// `__str__` (`project.py:493-495`): `str(self.user)`, i.e. the
        /// user's own `__str__` (`"{username} <{email}>"`,
        /// `db/models/user.py:139-140`). Rust holds only the user FK, so
        /// the label takes both joined values; the join itself is owned
        /// by the queries layer.
        pub fn label(username: &str, email: &str) -> String {
            format!("{username} <{email}>")
        }
    }
}

/// Foreign-table columns D-26 reads that have no merged db def yet
/// (fixture FX-ISS-07 `foreign_reads`).
///
/// `projects`, `project_members`, `states` and `estimate_points` reuse
/// the merged D-19 defs (`crate::v1_projects::models::{project,
/// project_member, state, estimate_point}`) — the tests assert the
/// fixture's `columns_read` for each is covered there. The three tables
/// below are unmerged (`users`/`workspaces` belong to D-24), so their
/// table names and read columns are pinned here as SQL literals; query
/// builders copy these literals verbatim.
pub mod foreign {
    /// Physical `users` table; D-26 reads `id` (joins),
    /// `display_name` (reaction-lite) and `email` (`__str__` labels).
    pub const USER_TABLE: &str = "users";
    /// Columns D-26 reads on [`USER_TABLE`], fixture order.
    pub const USER_COLUMNS_READ: &[&str] = &["id", "display_name", "email"];

    /// Physical `workspaces` table; D-26 reads `id` (joins) and `slug`
    /// (tenant scoping on every queryset).
    pub const WORKSPACE_TABLE: &str = "workspaces";
    /// Columns D-26 reads on [`WORKSPACE_TABLE`], fixture order.
    pub const WORKSPACE_COLUMNS_READ: &[&str] = &["id", "slug"];

    /// Physical `issue_types` table (`db/models/issue_type.py:14-29`);
    /// D-26 reads `type__is_epic` (archive queryset, identifier lite).
    pub const ISSUE_TYPE_TABLE: &str = "issue_types";
    /// Columns D-26 reads on [`ISSUE_TYPE_TABLE`], fixture order.
    pub const ISSUE_TYPE_COLUMNS_READ: &[&str] = &["id", "is_epic"];
}

#[cfg(test)]
mod tests {
    use super::foreign;
    use super::issue::{self, description_stripped_is_none};
    use super::issue_assignee;
    use super::issue_label;
    use super::label;
    use super::project_user_property::{
        self, default_display_filters, default_display_properties, default_filters,
        default_preferences,
    };
    use super::OnDelete;
    use crate::soft_delete::active_condition;
    use crate::v1_projects::models::{estimate_point, project, project_member, state};
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/app_issues/models")
    }

    fn fixture() -> serde_json::Value {
        let path = fixtures_dir().join("FX-ISS-07.core.json");
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read FX-ISS-07: {e}"));
        serde_json::from_str(&body).expect("FX-ISS-07.core.json is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// `issue.columns[].name` in order (the object-shaped entries).
    fn issue_column_names(value: &serde_json::Value) -> Vec<String> {
        value["issue"]["columns"]
            .as_array()
            .expect("issue has columns array")
            .iter()
            .map(|c| {
                c["name"]
                    .as_str()
                    .expect("column entry has name")
                    .to_string()
            })
            .collect()
    }

    /// Find an `issue.columns` entry by column name.
    fn issue_entry<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["issue"]["columns"]
            .as_array()
            .expect("issue has columns array")
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("issue fixture has column {name}"))
    }

    /// Expand the compact string-list entries (`"<a>/<b> <type> …"`)
    /// used by the four non-issue models into column names in order.
    fn string_column_names(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c
                    .as_str()
                    .unwrap_or_else(|| panic!("{model} column entry is str"));
                let head = s.split_whitespace().next().expect("entry has head");
                head.split('/').map(str::to_string).collect::<Vec<_>>()
            })
            .collect()
    }

    /// Nullable column names from the compact string-list entries: an
    /// entry marks `NULL` (vs `NOT NULL` or the bare `PK`).
    fn string_nullable(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has columns array"))
            .iter()
            .flat_map(|c| {
                let s = c.as_str().expect("column entry is str");
                let nullable = s.contains("NULL") && !s.contains("NOT NULL");
                let head = s.split_whitespace().next().expect("entry has head");
                if nullable {
                    head.split('/').map(str::to_string).collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    fn constraint_strings(value: &serde_json::Value, model: &str) -> Vec<String> {
        value[model]["constraints"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} has constraints"))
            .iter()
            .map(|c| c.as_str().expect("constraint is str").to_string())
            .collect()
    }

    fn foreign_columns_read(value: &serde_json::Value, table: &str) -> Vec<String> {
        value["foreign_reads"][table]["columns_read"]
            .as_array()
            .unwrap_or_else(|| panic!("foreign_reads has {table}"))
            .iter()
            .map(|c| c.as_str().expect("columns_read entry is str").to_string())
            .collect()
    }

    #[test]
    fn fixture_loads() {
        let v = fixture();
        assert_eq!(v["_fixture"].as_str().unwrap(), "FX-ISS-07");
        let consumers: Vec<&str> = v["_source"]["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        assert!(
            consumers.iter().any(|c| c.contains("PIDASHCONV-644")),
            "{consumers:?}"
        );
    }

    #[test]
    fn issue_columns_match_fixture() {
        let v = fixture();
        assert_eq!(owned(issue::COLUMNS), issue_column_names(&v));
        assert_eq!(issue::COLUMNS.len(), 34);
        let table: &str = issue::TABLE;
        assert_eq!(table, v["issue"]["table"].as_str().unwrap());
        assert_eq!(table, "issues");
        let ordering: &str = issue::ORDERING;
        assert_eq!(ordering, "-created_at");
        let indexes: Vec<&str> = v["issue"]["constraints_indexes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        assert!(indexes.iter().any(|c| c.contains("ordering -created_at")));
        assert!(indexes.iter().any(|c| c.contains("PK issues_pkey (id)")));
        assert!(v["issue"]["m2m_note"]
            .as_str()
            .unwrap()
            .contains("no columns"));
    }

    #[test]
    fn issue_field_details_match_fixture() {
        let v = fixture();
        // Nullability: exact set in fixture order (20 nullable of 34).
        let nullable: Vec<String> = v["issue"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["null"].as_bool().unwrap())
            .map(|c| c["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            nullable,
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "parent_id",
                "state_id",
                "point",
                "estimate_point_id",
                "description_stripped",
                "description_binary",
                "start_date",
                "target_date",
                "completed_at",
                "archived_at",
                "external_source",
                "external_id",
                "type_id",
                "created_via",
                "assigned_pod_id",
                "agent_executor",
            ]
        );
        // FK delete behavior, fixture `on_delete` per column.
        let mut on_delete: Vec<(String, String)> = v["issue"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| {
                c.get("on_delete").map(|d| {
                    (
                        c["name"].as_str().unwrap().to_string(),
                        d.as_str().unwrap().to_string(),
                    )
                })
            })
            .collect();
        on_delete.sort();
        let mut expected = vec![
            ("assigned_pod_id", "PROTECT"),
            ("created_by_id", "SET_NULL"),
            ("estimate_point_id", "SET_NULL"),
            ("parent_id", "CASCADE"),
            ("project_id", "CASCADE"),
            ("state_id", "CASCADE"),
            ("type_id", "SET_NULL"),
            ("updated_by_id", "SET_NULL"),
            ("workspace_id", "CASCADE"),
        ];
        expected.sort();
        let expected: Vec<(String, String)> = expected
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        assert_eq!(on_delete, expected);
        assert_eq!(issue::PARENT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue::STATE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue::ESTIMATE_POINT_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue::TYPE_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue::ASSIGNED_POD_ON_DELETE, OnDelete::Protect);
        assert_eq!(issue::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(issue::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
        // Django-side defaults (fixture quotes string defaults SQL-style).
        assert_eq!(issue_entry(&v, "id")["default"].as_str().unwrap(), "uuid4");
        assert_eq!(
            issue_entry(&v, "description_json")["default"]
                .as_str()
                .unwrap(),
            issue::DESCRIPTION_JSON_DEFAULT
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(issue::DESCRIPTION_JSON_DEFAULT).unwrap(),
            serde_json::json!({})
        );
        assert!(issue_entry(&v, "description_html")["default"]
            .as_str()
            .unwrap()
            .contains(issue::DESCRIPTION_HTML_DEFAULT));
        assert_eq!(issue::DESCRIPTION_HTML_DEFAULT, "<p></p>");
        assert!(issue_entry(&v, "priority")["default"]
            .as_str()
            .unwrap()
            .contains(issue::PRIORITY_DEFAULT));
        assert_eq!(issue::PRIORITY_DEFAULT, "none");
        assert_eq!(
            issue_entry(&v, "complexity_score")["default"]
                .as_i64()
                .unwrap(),
            i64::from(issue::COMPLEXITY_SCORE_DEFAULT)
        );
        assert_eq!(
            issue_entry(&v, "sequence_id")["default"].as_i64().unwrap(),
            i64::from(issue::SEQUENCE_ID_DEFAULT)
        );
        assert_eq!(
            issue_entry(&v, "sort_order")["default"].as_f64().unwrap(),
            issue::DEFAULT_SORT_ORDER
        );
        assert_eq!(issue::DEFAULT_SORT_ORDER, 65535.0);
        assert_eq!(issue::SORT_ORDER_STEP, 10000.0);
        assert_eq!(
            issue_entry(&v, "is_draft")["default"].as_bool().unwrap(),
            issue::IS_DRAFT_DEFAULT
        );
        let is_draft_default: bool = issue::IS_DRAFT_DEFAULT;
        assert!(!is_draft_default);
        assert_eq!(
            issue_entry(&v, "git_work_branch")["default"]
                .as_str()
                .unwrap(),
            "''"
        );
        assert_eq!(issue::GIT_WORK_BRANCH_DEFAULT, "");
        assert_eq!(
            issue_entry(&v, "workpad")["default"].as_str().unwrap(),
            "''"
        );
        assert_eq!(issue::WORKPAD_DEFAULT, "");
        // Length bounds, validators, choices (fixture type strings).
        assert!(issue_entry(&v, "name")["type"]
            .as_str()
            .unwrap()
            .contains("varchar(255)"));
        assert_eq!(issue::NAME_MAX_LENGTH, 255);
        assert!(issue_entry(&v, "priority")["type"]
            .as_str()
            .unwrap()
            .contains("varchar(30)"));
        assert_eq!(issue::PRIORITY_MAX_LENGTH, 30);
        assert_eq!(issue::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(issue::EXTERNAL_ID_MAX_LENGTH, 255);
        assert!(issue_entry(&v, "git_work_branch")["type"]
            .as_str()
            .unwrap()
            .contains("varchar(128)"));
        assert_eq!(issue::GIT_WORK_BRANCH_MAX_LENGTH, 128);
        assert!(issue_entry(&v, "git_work_branch")["type"]
            .as_str()
            .unwrap()
            .contains(issue::GIT_WORK_BRANCH_PATTERN));
        assert_eq!(issue::GIT_WORK_BRANCH_PATTERN, r"^[A-Za-z0-9._/-]*$");
        assert_eq!(
            issue::GIT_WORK_BRANCH_MESSAGE,
            "Branch name may contain only letters, numbers, and . _ / -"
        );
        assert!(issue_entry(&v, "created_via")["type"]
            .as_str()
            .unwrap()
            .contains("varchar(32)"));
        assert_eq!(issue::CREATED_VIA_MAX_LENGTH, 32);
        assert!(issue_entry(&v, "agent_executor")["type"]
            .as_str()
            .unwrap()
            .contains("varchar(24)"));
        assert_eq!(issue::AGENT_EXECUTOR_MAX_LENGTH, 24);
        assert!(issue_entry(&v, "point")["type"]
            .as_str()
            .unwrap()
            .contains("0..12"));
        assert_eq!((issue::POINT_MIN, issue::POINT_MAX), (0, 12));
        assert!(issue_entry(&v, "complexity_score")["type"]
            .as_str()
            .unwrap()
            .contains("0..10"));
        assert_eq!(
            (issue::COMPLEXITY_SCORE_MIN, issue::COMPLEXITY_SCORE_MAX),
            (0, 10)
        );
        assert_eq!(
            issue::PRIORITY_CHOICES,
            &[
                ("urgent", "Urgent"),
                ("high", "High"),
                ("medium", "Medium"),
                ("low", "Low"),
                ("none", "None"),
            ]
        );
        let executor_kinds: Vec<&str> = issue::AGENT_EXECUTOR_CHOICES
            .iter()
            .map(|(kind, _)| *kind)
            .collect();
        assert_eq!(
            executor_kinds,
            vec!["local_runner", "cloud_agent", "managed_runner"]
        );
        assert!(issue_entry(&v, "agent_executor")["type"]
            .as_str()
            .unwrap()
            .contains("local_runner/cloud_agent/managed_runner"));
        // Full-text index: name, columns and config all appear in the
        // recorded constraint line.
        let fts = v["issue"]["constraints_indexes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .find(|c| c.contains("issues_fts_idx"))
            .expect("fixture records the FTS index");
        assert!(fts.contains(issue::FTS_INDEX_NAME));
        for col in issue::FTS_COLUMNS {
            assert!(fts.contains(col), "{fts}");
        }
        assert!(fts.contains(issue::FTS_CONFIG));
        assert_eq!(issue::FTS_INDEX_NAME, "issues_fts_idx");
        assert_eq!(issue::FTS_COLUMNS, &["name", "description_stripped"]);
        assert_eq!(issue::FTS_CONFIG, "english");
    }

    #[test]
    fn issue_manager_scope_columns_exist() {
        // The merged `space::columns::issue_objects_scope` (D-02, reused
        // not re-ported) reads these single-table columns plus the
        // joined `states.group` / `projects.archived_at`; the joins are
        // owned by the queries layer.
        for col in [
            "deleted_at",
            "archived_at",
            "is_draft",
            "state_id",
            "project_id",
        ] {
            assert!(issue::COLUMNS.contains(&col), "{col}");
        }
        let completed: &str = issue::COMPLETED_GROUP;
        assert_eq!(completed, "completed");
    }

    #[test]
    fn link_tables_columns_match_fixture() {
        let v = fixture();
        for (model, cols) in [
            ("issue_assignee", issue_assignee::COLUMNS),
            ("issue_label", issue_label::COLUMNS),
        ] {
            let inherited: &[&str] = &cols[..8];
            assert_eq!(owned(inherited), owned(&issue::COLUMNS[..8]));
            assert_eq!(cols.len(), 10);
            assert_eq!(owned(cols), string_column_names(&v, model));
        }
        assert_eq!(
            owned(&issue_assignee::COLUMNS[8..]),
            vec!["issue_id", "assignee_id"]
        );
        assert_eq!(
            owned(&issue_label::COLUMNS[8..]),
            vec!["issue_id", "label_id"]
        );
        let assignee_table: &str = issue_assignee::TABLE;
        assert_eq!(
            assignee_table,
            v["issue_assignee"]["table"].as_str().unwrap()
        );
        assert_eq!(assignee_table, "issue_assignees");
        let label_table: &str = issue_label::TABLE;
        assert_eq!(label_table, v["issue_label"]["table"].as_str().unwrap());
        assert_eq!(label_table, "issue_labels");
        assert_eq!(issue_assignee::ORDERING, "-created_at");
        assert_eq!(issue_label::ORDERING, "-created_at");
        assert_eq!(
            string_nullable(&v, "issue_assignee"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        assert_eq!(
            string_nullable(&v, "issue_label"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        // Assignee: partial unique (name recorded verbatim).
        let constraints = constraint_strings(&v, "issue_assignee");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains(issue_assignee::UNIQUE_ISSUE_ASSIGNEE_NAME));
        assert_eq!(
            issue_assignee::UNIQUE_ISSUE_ASSIGNEE_NAME,
            "issue_assignee_unique_issue_assignee_when_deleted_at_null"
        );
        assert!(constraints[0].contains("(issue,assignee)"));
        assert_eq!(
            issue_assignee::UNIQUE_ISSUE_ASSIGNEE_COLUMNS,
            &["issue_id", "assignee_id"]
        );
        assert!(constraints[0].contains(issue_assignee::UNIQUE_ISSUE_ASSIGNEE_WHERE));
        assert_eq!(
            issue_assignee::UNIQUE_ISSUE_ASSIGNEE_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(
            issue_assignee::UNIQUE_TOGETHER,
            &["issue", "assignee", "deleted_at"]
        );
        assert_eq!(issue_assignee::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_assignee::ASSIGNEE_ON_DELETE, OnDelete::Cascade);
        // Label link: NO partial unique — ported as-is.
        let constraints = constraint_strings(&v, "issue_label");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("no partial-unique"));
        let has_partial_unique: bool = issue_label::HAS_PARTIAL_UNIQUE;
        assert!(!has_partial_unique);
        assert_eq!(issue_label::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(issue_label::LABEL_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn label_columns_match_fixture() {
        let v = fixture();
        assert_eq!(owned(label::COLUMNS), string_column_names(&v, "label"));
        assert_eq!(label::COLUMNS.len(), 15);
        // WorkspaceBaseModel order: workspace BEFORE project (contrast
        // ProjectBaseModel's project-first order on the other tables).
        assert_eq!(&label::COLUMNS[6..8], &["workspace_id", "project_id"]);
        let table: &str = label::TABLE;
        assert_eq!(table, v["label"]["table"].as_str().unwrap());
        assert_eq!(table, "labels");
        assert_eq!(label::ORDERING, "-created_at");
        assert!(v["label"]["note"]
            .as_str()
            .unwrap()
            .contains("WorkspaceBaseModel"));
        assert_eq!(
            string_nullable(&v, "label"),
            vec![
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "project_id",
                "parent_id",
                "external_source",
                "external_id",
            ]
        );
        assert_eq!(label::NAME_MAX_LENGTH, 255);
        assert_eq!(label::COLOR_MAX_LENGTH, 255);
        assert_eq!(label::EXTERNAL_SOURCE_MAX_LENGTH, 255);
        assert_eq!(label::EXTERNAL_ID_MAX_LENGTH, 255);
        assert_eq!(label::DESCRIPTION_DEFAULT, "");
        assert_eq!(label::COLOR_DEFAULT, "");
        assert_eq!(label::DEFAULT_SORT_ORDER, 65535.0);
        assert_eq!(label::SORT_ORDER_STEP, 10000.0);
        let constraints = constraint_strings(&v, "label");
        assert_eq!(constraints.len(), 2);
        assert!(constraints[0].contains("(name)"));
        assert!(constraints[0].contains("project IS NULL"));
        assert_eq!(
            label::UNIQUE_NAME_WHEN_PROJECT_NULL_NAME,
            "unique_name_when_project_null_and_not_deleted"
        );
        assert_eq!(label::UNIQUE_NAME_WHEN_PROJECT_NULL_COLUMNS, &["name"]);
        assert_eq!(
            label::UNIQUE_NAME_WHEN_PROJECT_NULL_WHERE,
            "project_id IS NULL AND deleted_at IS NULL"
        );
        assert!(constraints[1].contains("(project,name)"));
        assert!(constraints[1].contains("project IS NOT NULL"));
        assert_eq!(
            label::UNIQUE_PROJECT_NAME_NAME,
            "unique_project_name_when_not_deleted"
        );
        assert_eq!(label::UNIQUE_PROJECT_NAME_COLUMNS, &["project_id", "name"]);
        assert_eq!(
            label::UNIQUE_PROJECT_NAME_WHERE,
            "project_id IS NOT NULL AND deleted_at IS NULL"
        );
        assert!(v["label"]["save"]
            .as_str()
            .unwrap()
            .contains("max(project)+10000"));
        assert_eq!(label::PARENT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(label::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(label::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(label::CREATED_BY_ON_DELETE, OnDelete::SetNull);
        assert_eq!(label::UPDATED_BY_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn project_user_property_columns_match_fixture() {
        let v = fixture();
        assert_eq!(
            owned(project_user_property::COLUMNS),
            string_column_names(&v, "project_user_property")
        );
        assert_eq!(project_user_property::COLUMNS.len(), 15);
        let table: &str = project_user_property::TABLE;
        assert_eq!(table, v["project_user_property"]["table"].as_str().unwrap());
        assert_eq!(table, "project_user_properties");
        assert_eq!(project_user_property::ORDERING, "-created_at");
        assert_eq!(
            string_nullable(&v, "project_user_property"),
            vec!["created_by_id", "updated_by_id", "deleted_at"]
        );
        let constraints = constraint_strings(&v, "project_user_property");
        assert_eq!(constraints.len(), 1);
        assert!(constraints[0].contains("(user,project)"));
        assert!(constraints[0].contains(project_user_property::UNIQUE_USER_PROJECT_WHERE));
        assert_eq!(
            project_user_property::UNIQUE_USER_PROJECT_NAME,
            "project_user_property_unique_user_project_when_deleted_at_null"
        );
        assert_eq!(
            project_user_property::UNIQUE_USER_PROJECT_COLUMNS,
            &["user_id", "project_id"]
        );
        assert_eq!(
            project_user_property::UNIQUE_USER_PROJECT_WHERE,
            "deleted_at IS NULL"
        );
        assert_eq!(
            project_user_property::UNIQUE_TOGETHER,
            &["user", "project", "deleted_at"]
        );
        assert_eq!(project_user_property::USER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(project_user_property::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            project_user_property::WORKSPACE_ON_DELETE,
            OnDelete::Cascade
        );
        assert_eq!(project_user_property::DEFAULT_SORT_ORDER, 65535.0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(project_user_property::RICH_FILTERS_EMPTY)
                .unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn user_property_defaults_match_python() {
        // Exact dicts from issue.py:50-91 and project.py:68-69.
        assert_eq!(
            default_filters(),
            serde_json::json!({
                "priority": null,
                "state": null,
                "state_group": null,
                "assignees": null,
                "created_by": null,
                "labels": null,
                "start_date": null,
                "target_date": null,
                "subscriber": null,
            })
        );
        assert_eq!(
            default_display_filters(),
            serde_json::json!({
                "group_by": null,
                "order_by": "-created_at",
                "type": null,
                "sub_issue": true,
                "show_empty_groups": true,
                "layout": "list",
                "calendar_date_range": "",
            })
        );
        assert_eq!(
            default_display_properties(),
            serde_json::json!({
                "assignee": true,
                "attachment_count": true,
                "created_on": true,
                "due_date": true,
                "estimate": true,
                "key": true,
                "labels": true,
                "link": true,
                "priority": true,
                "start_date": true,
                "state": true,
                "sub_issue_count": true,
                "updated_on": true,
            })
        );
        assert_eq!(
            default_preferences(),
            serde_json::json!({
                "pages": {"block_display": true},
                "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
            })
        );
        // Key counts pin silent additions/drops (9 / 7 / 13 / 2).
        assert_eq!(default_filters().as_object().unwrap().len(), 9);
        assert_eq!(default_display_filters().as_object().unwrap().len(), 7);
        assert_eq!(default_display_properties().as_object().unwrap().len(), 13);
        assert_eq!(default_preferences().as_object().unwrap().len(), 2);
    }

    #[test]
    fn user_property_defaults_are_fresh_per_call() {
        // Mirrors the Python callables returning a new dict each time:
        // mutating one caller's value must not leak into the next.
        let mut first = default_filters();
        first["priority"] = serde_json::json!(["high"]);
        assert_eq!(default_filters()["priority"], serde_json::Value::Null);
        let mut prefs = default_preferences();
        prefs["pages"] = serde_json::json!({});
        assert_eq!(
            default_preferences()["pages"],
            serde_json::json!({"block_display": true})
        );
    }

    #[test]
    fn new_sort_order_matches_python() {
        // None (no rows) keeps the Django-side default ...
        assert_eq!(issue::new_sort_order(None), None);
        assert_eq!(label::new_sort_order(None), None);
        // ... otherwise largest plus the step, as f64 arithmetic.
        assert_eq!(issue::new_sort_order(Some(65535.0)), Some(75535.0));
        assert_eq!(issue::new_sort_order(Some(-500.25)), Some(9499.75));
        assert_eq!(label::new_sort_order(Some(65535.0)), Some(75535.0));
    }

    #[test]
    fn largest_sort_order_sql_matches_django_semantics() {
        let project = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let state = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        // Issue: scoped to project + state + live rows (Issue.objects,
        // the plain soft-delete scope — not the narrower issue_objects).
        assert_eq!(
            issue::largest_sort_order_sql(project, Some(state)),
            r#"SELECT MAX("sort_order") FROM "issues" WHERE "project_id" = '11111111-1111-1111-1111-111111111111' AND "state_id" = '22222222-2222-2222-2222-222222222222' AND "deleted_at" IS NULL"#
        );
        // Stateless issue: `filter(state=None)` renders IS NULL.
        assert_eq!(
            issue::largest_sort_order_sql(project, None),
            r#"SELECT MAX("sort_order") FROM "issues" WHERE "project_id" = '11111111-1111-1111-1111-111111111111' AND "state_id" IS NULL AND "deleted_at" IS NULL"#
        );
        // Label: scoped to project + live rows.
        assert_eq!(
            label::largest_sort_order_sql(Some(project)),
            r#"SELECT MAX("sort_order") FROM "labels" WHERE "project_id" = '11111111-1111-1111-1111-111111111111' AND "deleted_at" IS NULL"#
        );
        // Workspace-level label: global null-project max, ported as-is.
        assert_eq!(
            label::largest_sort_order_sql(None),
            r#"SELECT MAX("sort_order") FROM "labels" WHERE "project_id" IS NULL AND "deleted_at" IS NULL"#
        );
    }

    #[test]
    fn description_stripped_none_rule_matches_python() {
        assert!(description_stripped_is_none(None));
        assert!(description_stripped_is_none(Some("")));
        // The create/update default input strips to "" — not None.
        assert!(!description_stripped_is_none(Some("<p></p>")));
        assert!(!description_stripped_is_none(Some("<p>hi</p>")));
    }

    #[test]
    fn foreign_reads_replay_fixture() {
        let v = fixture();
        // Unmerged tables: literals pinned here equal the fixture.
        assert_eq!(
            owned(foreign::USER_COLUMNS_READ),
            foreign_columns_read(&v, "user")
        );
        assert_eq!(foreign::USER_COLUMNS_READ, &["id", "display_name", "email"]);
        let user_table: &str = foreign::USER_TABLE;
        assert_eq!(user_table, "users");
        assert_eq!(
            owned(foreign::WORKSPACE_COLUMNS_READ),
            foreign_columns_read(&v, "workspace")
        );
        assert_eq!(foreign::WORKSPACE_COLUMNS_READ, &["id", "slug"]);
        let workspace_table: &str = foreign::WORKSPACE_TABLE;
        assert_eq!(workspace_table, "workspaces");
        assert_eq!(
            owned(foreign::ISSUE_TYPE_COLUMNS_READ),
            foreign_columns_read(&v, "issue_type_extra")
        );
        assert_eq!(foreign::ISSUE_TYPE_COLUMNS_READ, &["id", "is_epic"]);
        let issue_type_table: &str = foreign::ISSUE_TYPE_TABLE;
        assert_eq!(issue_type_table, "issue_types");
        assert!(v["foreign_reads"]["issue_type_extra"]["note"]
            .as_str()
            .unwrap()
            .contains("type__is_epic"));
        // Merged tables: every literal column the fixture names is
        // covered by the owning D-19 def (reuse, not re-port).
        let merged_table: &str = project::TABLE;
        assert_eq!(merged_table, "projects");
        let merged_table: &str = project_member::TABLE;
        assert_eq!(merged_table, "project_members");
        let merged_table: &str = state::TABLE;
        assert_eq!(merged_table, "states");
        let merged_table: &str = estimate_point::TABLE;
        assert_eq!(merged_table, "estimate_points");
        for (table, cols) in [
            ("project_member", project_member::COLUMNS),
            ("state", state::COLUMNS),
            ("estimate_point", estimate_point::COLUMNS),
        ] {
            for col in foreign_columns_read(&v, table) {
                assert!(cols.contains(&col.as_str()), "{table}.{col}");
            }
        }
        // Project: literal heads are covered; the ticker entry is
        // descriptive, so its concrete columns are pinned explicitly.
        for col in foreign_columns_read(&v, "project") {
            let head = col.split(" (").next().unwrap();
            if head.contains(' ') || head.contains('+') {
                continue;
            }
            assert!(project::COLUMNS.contains(&head), "project.{head}");
        }
        for col in [
            "agent_default_interval_seconds",
            "agent_default_max_ticks",
            "agent_review_default_interval_seconds",
            "agent_test_default_interval_seconds",
            "agent_ticking_enabled",
            "default_agent_executor",
        ] {
            assert!(project::COLUMNS.contains(&col), "project.{col}");
        }
        assert!(foreign_columns_read(&v, "project")
            .iter()
            .any(|c| c.contains("default_agent_executor")));
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            issue::TABLE,
            issue_assignee::TABLE,
            issue_label::TABLE,
            label::TABLE,
            project_user_property::TABLE,
        ] {
            let mut select = Query::select();
            select
                .column(Alias::new("id"))
                .from(Alias::new(table))
                .cond_where(active_condition());
            let sql = select.to_string(PostgresQueryBuilder);
            assert_eq!(
                sql,
                format!(r#"SELECT "id" FROM "{table}" WHERE "deleted_at" IS NULL"#)
            );
        }
    }

    #[test]
    fn display_matches_python_str() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let row = issue::Issue {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            parent_id: None,
            state_id: None,
            point: None,
            estimate_point_id: None,
            name: "Ship it".to_string(),
            description_json: serde_json::json!({}),
            description_html: issue::DESCRIPTION_HTML_DEFAULT.to_string(),
            description_stripped: None,
            description_binary: None,
            priority: issue::PRIORITY_DEFAULT.to_string(),
            complexity_score: issue::COMPLEXITY_SCORE_DEFAULT,
            start_date: None,
            target_date: None,
            sequence_id: issue::SEQUENCE_ID_DEFAULT,
            sort_order: issue::DEFAULT_SORT_ORDER,
            completed_at: None,
            archived_at: None,
            is_draft: issue::IS_DRAFT_DEFAULT,
            external_source: None,
            external_id: None,
            type_id: None,
            git_work_branch: issue::GIT_WORK_BRANCH_DEFAULT.to_string(),
            workpad: issue::WORKPAD_DEFAULT.to_string(),
            created_via: None,
            assigned_pod_id: None,
            agent_executor: None,
        };
        assert_eq!(row.to_string(), format!("Ship it <{}>", uuid::Uuid::nil()));
        let tag = label::Label {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::nil(),
            project_id: None,
            parent_id: None,
            name: "backend".to_string(),
            description: label::DESCRIPTION_DEFAULT.to_string(),
            color: label::COLOR_DEFAULT.to_string(),
            sort_order: label::DEFAULT_SORT_ORDER,
            external_source: None,
            external_id: None,
        };
        assert_eq!(tag.to_string(), "backend");
        assert_eq!(
            issue_assignee::IssueAssignee::label("Ship it", "a@x.com"),
            "Ship it a@x.com"
        );
        assert_eq!(
            issue_label::IssueLabel::label("Ship it", "backend"),
            "Ship it backend"
        );
        assert_eq!(
            project_user_property::ProjectUserProperty::label("ann", "a@x.com"),
            "ann <a@x.com>"
        );
    }
}
