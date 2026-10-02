#![forbid(unsafe_code)]

//! App-project model columns + field semantics (D-25, stage 5).
//!
//! Ports the column lists, defaults, constraints, manager scopes, and
//! save-rule helpers for every table the D-25 app views touch, adopting the
//! Django-owned schema column-for-column. Migrations are not ported; Django
//! stays schema owner until switchover.
//!
//! Sources (`apps/api/`): `pi_dash/db/models/project.py:25-495` (`ROLE`
//! L28-31, `ProjectNetwork` L34-40, `Project` L72-301, `ProjectBaseModel`
//! L302-311, `ProjectMemberInvite` L314-331, `ProjectMember` L332-385,
//! `ProjectIdentifier` L386-403, `ProjectDeployBoard` L422-439,
//! `ProjectPublicMember` L442-461, `ProjectUserProperty` L464-495);
//! `pi_dash/db/models/state.py:14-140` (`StateGroup` L14-22,
//! `DEFAULT_STATES` L26-76, managers L79-91, `State` L93-140);
//! `pi_dash/db/models/estimate.py:13-57` (`EstimateType` L13-15, `Estimate`
//! L18-40, `EstimatePoint` L43-57); `pi_dash/db/models/deploy_board.py:15-57`
//! (`get_anchor` L15-16, `DeployBoard` L19-57). Audit columns
//! `pi_dash/db/mixins.py:21-67`, UUID pk `pi_dash/db/models/base.py:17-18`.
//!
//! Fixture: `rust-api/fixtures/app_project/FX-APROJ-05.models.json`.
//! Column order in each `COLUMNS` const follows the fixture's introspected
//! Django `_meta` field order (audit columns first, then `id`, then the
//! model fields in declaration order — except `ProjectIdentifier`, whose
//! `BigAutoField` pk sorts first); FK entries use the Django attnames
//! (`workspace_id`, `default_assignee_id`, ...).
//!
//! Reads serve from the per-table soft-delete views (`<table>_active`,
//! [`crate::soft_delete::active_view_ddl`]) wherever Django uses its default
//! managers; writes hit the tables so the partial unique indexes keep
//! working. Every application-level default below must be supplied
//! explicitly on insert — the live tables carry no `column_default`.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `ProjectMember.__str__` dereferences `self.member.email` while `member`
//!   is nullable (`project.py:333-339,380-382`): a null-member row crashes
//!   the Python repr. Kept as a docs note; there is no Rust `Display` that
//!   panics.
//! * `State.save` overwrites any explicitly set `sequence` on add whenever
//!   non-triage siblings exist, and the max excludes triage states (it reads
//!   through the triage-excluding default manager, `state.py:131-139`).
//!   [`state::sequence_on_add`] mirrors both behaviors.
//! * `EstimatePoint.Meta.ordering = ("value",)` sorts by the *string*
//!   column, so `"10"` orders before `"2"` (`estimate.py:53-57`).
//! * `Project.resolve` catches `TypeError`, but `str(value)` cannot raise
//!   it, so that branch is dead (`project.py:197-200`). The classifier
//!   below treats every parse failure as an identifier lookup.
//! * `DeployBoard.TYPE_CHOICES` is defined (`deploy_board.py:20-28`) but
//!   never wired to any field — `entity_name` has no `choices=` — so the
//!   database accepts any value up to 30 chars. Ported as an unenforced
//!   const with this note.
//! * `Project.save` sets `is_default = True` on the first project of a
//!   workspace even when the caller explicitly passed `is_default = False`
//!   (`project.py:265-272`); [`project::auto_default_on_create`] keeps that.
//! * `WorkspaceBaseModel.save` backfills `workspace` from `project.workspace`
//!   only `if self.project` (`workspace.py:192-195`), while
//!   `ProjectBaseModel.save` does it unconditionally
//!   (`project.py:309-311`). Both are documented on the inheriting modules.
//!
//! # Shared-consumer tables
//!
//! `DeployBoard` is consumed by D-25, D-19, and the space views; its column
//! list and read shape are defined here ([`deploy_board`]) and must not be
//! redefined elsewhere. `ProjectDeployBoard` is defined in `project.py`
//! (`:422-439`, marked DEPRECATED there) but consumed by the space views.
//!
//! # Referenced, never ported here
//!
//! `Workspace`, `WorkspaceMember`, `User`, `UserFavorite`, `Intake`,
//! `IntakeIssueStatus`, `Issue`, `IssueSequence` are owned by D-24 / D-26 /
//! D-32; only the FK target names, `on_delete` behaviors, and nullability
//! Django records on *this* side appear below. `ProjectUserProperty`'s
//! `filters` / `display_filters` / `display_properties` defaults live in
//! `db/models/issue.py` (D-26) and are referenced, not ported.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

/// Django-level FK delete behavior (ORM-emulated; same shape as the D-27
/// `app_cycles::models::OnDelete` and the sibling domain ports).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// `ROLE_CHOICES` (`db/models/project.py:25`): `(20, "Admin")`,
/// `(15, "Member")`, `(5, "Guest")`. Application-level only; the column is
/// a plain smallint with no `CHECK` constraint.
pub const ROLE_CHOICES: &[(i32, &str)] = &[(20, "Admin"), (15, "Member"), (5, "Guest")];

/// Default `role` on project members and invites (`project.py:320,341`).
pub const DEFAULT_ROLE: i32 = 5;

/// Member role values (`db/models/project.py:28-31`, ported as-is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Admin,
    Member,
    Guest,
}

impl Role {
    /// The stored integer (`ROLE.ADMIN.value` etc.).
    pub fn as_int(self) -> i32 {
        match self {
            Role::Admin => 20,
            Role::Member => 15,
            Role::Guest => 5,
        }
    }
}

/// `ProjectBaseModel` (`db/models/project.py:302-311`, abstract): the shared
/// 8-column prefix of every project-scoped table plus the save rule.
/// `DeployBoard` instead extends `WorkspaceBaseModel` (nullable `project`
/// FK); `Project` and `ProjectIdentifier` extend `BaseModel` / `AuditModel`
/// directly.
pub mod project_base {
    /// Inherited columns in fixture order: the 5 audit columns, the UUID pk,
    /// then `project_id` + `workspace_id`. Every `ProjectBaseModel` child's
    /// `COLUMNS` starts with exactly this prefix.
    pub const INHERITED_COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
    ];

    /// `ProjectBaseModel.save` (`project.py:309-311`) overwrites `workspace`
    /// with `project.workspace` on *every* save (insert and update), even
    /// when the caller supplied a different workspace: Rust writes must
    /// resolve `workspace_id` from the `project_id` row explicitly and must
    /// not trust a caller-supplied value (fixture
    /// `save_behaviors.projectbasemodel_workspace_backfill` proves a sent
    /// foreign workspace is discarded).
    pub const SAVE_BACKFILLS_WORKSPACE_FROM_PROJECT: &str = "project.workspace";
}

/// `projects` table (`db/models/project.py:72-301`, `db_table = "projects"`).
pub mod project {
    use super::OnDelete;
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "projects";

    /// Soft-delete read view (`projects_active`).
    pub const VIEW: &str = "projects_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.Project.columns`). FK columns use the Django attnames.
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "name",
        "description",
        "description_text",
        "description_html",
        "network",
        "workspace_id",
        "identifier",
        "default_assignee_id",
        "project_lead_id",
        "emoji",
        "icon_prop",
        "module_view",
        "cycle_view",
        "issue_views_view",
        "page_view",
        "intake_view",
        "is_time_tracking_enabled",
        "is_issue_type_enabled",
        "is_default",
        "guest_view_all_features",
        "members_can_edit_states",
        "cover_image",
        "cover_image_asset_id",
        "estimate_id",
        "archive_in",
        "close_in",
        "logo_props",
        "default_state_id",
        "archived_at",
        "timezone",
        "external_source",
        "external_id",
        "repo_url",
        "base_branch",
        "agent_default_interval_seconds",
        "agent_default_max_ticks",
        "agent_review_default_interval_seconds",
        "agent_test_default_interval_seconds",
        "agent_ticking_enabled",
        "default_agent_executor",
    ];

    /// Partial unique index names (`Meta.constraints`, `:233-249`).
    pub const CONSTRAINTS: &[&str] = &[
        "project_unique_identifier_workspace_when_deleted_at_null",
        "project_unique_name_workspace_when_deleted_at_null",
        "project_unique_default_per_workspace",
    ];

    /// `unique_together` (`:229-232`); the partial constraints above are the
    /// enforced form.
    pub const UNIQUE_TOGETHER: &[&[&str]] = &[
        &["identifier", "workspace_id", "deleted_at"],
        &["name", "workspace_id", "deleted_at"],
    ];

    /// `ProjectNetwork` values (`db/models/project.py:34-40`, ported as-is).
    /// Note the gap: there is no `1`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum ProjectNetwork {
        Secret,
        Public,
    }

    impl ProjectNetwork {
        /// The stored integer (`ProjectNetwork.SECRET.value` etc.).
        pub fn as_int(self) -> i32 {
            match self {
                ProjectNetwork::Secret => 0,
                ProjectNetwork::Public => 2,
            }
        }

        /// `ProjectNetwork.choices()` (`:38-40`).
        pub fn choices() -> &'static [(i32, &'static str)] {
            NETWORK_CHOICES
        }
    }

    /// `NETWORK_CHOICES` (`:73`) / `ProjectNetwork.choices()` (`:40`):
    /// `((0, "Secret"), (2, "Public"))`. Application-level only.
    pub const NETWORK_CHOICES: &[(i32, &str)] = &[(0, "Secret"), (2, "Public")];

    /// Default `network` (`:78`): public.
    pub const DEFAULT_NETWORK: i32 = 2;

    /// Forbidden-identifier pattern (`Project.FORBIDDEN_IDENTIFIER_CHARS_PATTERN`,
    /// `project.py:226`), matched with `re.match` semantics by the project
    /// name/identifier validators: any identifier containing one of these
    /// characters is rejected.
    pub const FORBIDDEN_IDENTIFIER_CHARS_PATTERN: &str = r"^.*[&+,:;$^}{*=?@#|'<>.()%!-].*$";

    /// View-flag defaults (`:97-101`).
    pub const DEFAULT_MODULE_VIEW: bool = false;
    pub const DEFAULT_CYCLE_VIEW: bool = false;
    pub const DEFAULT_ISSUE_VIEWS_VIEW: bool = false;
    pub const DEFAULT_PAGE_VIEW: bool = true;
    pub const DEFAULT_INTAKE_VIEW: bool = false;

    /// Feature-flag defaults (`:102-106`).
    pub const DEFAULT_IS_TIME_TRACKING_ENABLED: bool = false;
    pub const DEFAULT_IS_ISSUE_TYPE_ENABLED: bool = false;
    pub const DEFAULT_IS_DEFAULT: bool = false;
    pub const DEFAULT_GUEST_VIEW_ALL_FEATURES: bool = false;
    pub const DEFAULT_MEMBERS_CAN_EDIT_STATES: bool = true;

    /// Default `archive_in` / `close_in` (`:116-117`).
    pub const DEFAULT_ARCHIVE_IN: i64 = 0;
    pub const DEFAULT_CLOSE_IN: i64 = 0;

    /// `MinValueValidator(0)` / `MaxValueValidator(12)` bounds on
    /// `archive_in` / `close_in` (`:116-117`). Application-level only; there
    /// is no `CHECK` constraint.
    pub const ARCHIVE_IN_MIN: i64 = 0;
    pub const ARCHIVE_IN_MAX: i64 = 12;
    pub const CLOSE_IN_MIN: i64 = 0;
    pub const CLOSE_IN_MAX: i64 = 12;

    /// Default `timezone` (`:123`, `"UTC"`). `choices=TIMEZONE_CHOICES` is
    /// form-validation only (no DB constraint); on create without an
    /// explicit timezone the workspace timezone wins (see
    /// [`timezone_on_create`]).
    pub const DEFAULT_TIMEZONE: &str = "UTC";

    /// Default `repo_url` (`:129`, `""`, never `NULL`).
    pub const DEFAULT_REPO_URL: &str = "";

    /// Default `base_branch` (`:130-140`, `"main"`, never `NULL`).
    pub const DEFAULT_BASE_BRANCH: &str = "main";

    /// `base_branch` validator (`:135-138`, `RegexValidator`, full-string
    /// match on `^[A-Za-z0-9._/-]*$`). Application-level only.
    pub const BASE_BRANCH_PATTERN: &str = r"^[A-Za-z0-9._/-]*$";
    pub const BASE_BRANCH_MESSAGE: &str =
        "Branch name may contain only letters, numbers, and . _ / -";

    /// Agent tick-cadence defaults (`:157-161`): 3 h per stage, pool of 10
    /// ticks per issue, ticking enabled project-wide.
    pub const DEFAULT_AGENT_INTERVAL_SECONDS: i64 = 10800;
    pub const DEFAULT_AGENT_MAX_TICKS: i64 = 10;
    pub const DEFAULT_AGENT_REVIEW_INTERVAL_SECONDS: i64 = 10800;
    pub const DEFAULT_AGENT_TEST_INTERVAL_SECONDS: i64 = 10800;
    pub const DEFAULT_AGENT_TICKING_ENABLED: bool = true;

    /// `AgentExecutorKind` values
    /// (`core/agent_execution.py:7-16`, ported as-is).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum AgentExecutorKind {
        LocalRunner,
        CloudAgent,
        ManagedRunner,
    }

    impl AgentExecutorKind {
        /// The stored string (`AgentExecutorKind.LOCAL_RUNNER.value` etc.).
        pub fn as_str(self) -> &'static str {
            match self {
                AgentExecutorKind::LocalRunner => "local_runner",
                AgentExecutorKind::CloudAgent => "cloud_agent",
                AgentExecutorKind::ManagedRunner => "managed_runner",
            }
        }

        /// `AgentExecutorKind.choices`.
        pub fn choices() -> &'static [(&'static str, &'static str)] {
            &[
                ("local_runner", "Local Runner"),
                ("cloud_agent", "Pi Dash Cloud Agent"),
                ("managed_runner", "Pi Dash Agent"),
            ]
        }
    }

    /// Default `default_agent_executor` (`project.py:164-168`):
    /// `get_default_agent_executor()` returns the Django setting
    /// `DEFAULT_AGENT_EXECUTOR` validated against the choices above,
    /// falling back to `"local_runner"` (`core/agent_execution.py:26-31`).
    pub const DEFAULT_AGENT_EXECUTOR: &str = "local_runner";

    /// `workspace` FK: `CASCADE`, required (`:79`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `default_assignee` / `project_lead` FKs: `CASCADE`, nullable
    /// (`:81-94`). Deleting the referenced user row deletes the project
    /// row; ported as recorded.
    pub const DEFAULT_ASSIGNEE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const PROJECT_LEAD_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `cover_image_asset` / `estimate` / `default_state` FKs: `SET_NULL`,
    /// nullable (`:108-115,119`).
    pub const COVER_IMAGE_ASSET_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const ESTIMATE_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const DEFAULT_STATE_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default `logo_props` (`:118`, `default=dict` → `{}`).
    pub fn default_logo_props() -> serde_json::Value {
        serde_json::Value::Object(Default::default())
    }

    /// `Project.save` identifier rule (`project.py:258`):
    /// `self.identifier.strip().upper()`. `name` is untouched.
    pub fn normalize_identifier(raw: &str) -> String {
        raw.trim().to_uppercase()
    }

    /// `Project.cover_image_url` (`project.py:175-185`): the cover-asset URL
    /// wins when an asset row is attached, else the legacy `cover_image`
    /// text, else `None`. Callers resolve `asset_url` from the `FileAsset`
    /// row first.
    pub fn cover_image_url<'a>(
        asset_url: Option<&'a str>,
        cover_image: Option<&'a str>,
    ) -> Option<&'a str> {
        asset_url.or(cover_image)
    }

    /// `Project.__str__` (`project.py:187-189`): `"{name} <{workspace_name}>"`.
    /// Callers supply the already-resolved workspace name.
    pub fn display_name(name: &str, workspace_name: &str) -> String {
        format!("{name} <{workspace_name}>")
    }

    /// `Project.save` timezone rule on create (`project.py:261-263`):
    /// without an explicit `timezone` kwarg (`__init__` `:170-173` records
    /// `kwargs.get("timezone") is not None`) the project inherits the
    /// workspace timezone; an explicit value is kept verbatim.
    pub fn timezone_on_create(
        is_timezone_provided: bool,
        requested: &str,
        workspace_timezone: &str,
    ) -> String {
        if is_timezone_provided {
            requested.to_owned()
        } else {
            workspace_timezone.to_owned()
        }
    }

    /// `Project.save` auto-default rule on create (`project.py:265-272`): a
    /// new project becomes the workspace default when no live default
    /// exists — even when the caller explicitly passed `is_default = False`
    /// (ported as-is, see module quirks).
    pub fn auto_default_on_create(is_default: bool, workspace_has_default: bool) -> bool {
        is_default || !workspace_has_default
    }

    /// `ValidationError` message raised when unsetting the last live default
    /// without a replacement (`project.py:290`).
    pub const UNSET_DEFAULT_ERROR: &str =
        "Default project cannot be unset without assigning another default project.";

    /// `Project.save` unset-default guard on update (`project.py:274-290`):
    /// saving with `is_default = False` is refused when this row is
    /// currently the live default and no live replacement exists in the
    /// workspace.
    pub fn unset_default_check(
        is_default: bool,
        was_default: bool,
        has_replacement: bool,
    ) -> Result<(), &'static str> {
        if !is_default && was_default && !has_replacement {
            return Err(UNSET_DEFAULT_ERROR);
        }
        Ok(())
    }

    /// `WHERE` for the default-flip `UPDATE` inside `Project.save`
    /// (`project.py:292-298`): when saving with `is_default = True`, every
    /// other live default of the workspace is cleared in the same
    /// transaction.
    pub fn clear_old_default_condition(workspace_id: uuid::Uuid, self_id: uuid::Uuid) -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("workspace_id")).eq(workspace_id))
            .add(Expr::col(Alias::new("is_default")).eq(true))
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("id")).ne(self_id))
    }

    /// One `Project` row. `description` is `blank=True` without `null`
    /// (`""`, never `NULL`); `description_text` / `description_html` /
    /// `icon_prop` are nullable JSON; `timezone`, `repo_url`,
    /// `base_branch`, and `default_agent_executor` are non-null strings.
    /// Python `IntegerField` maps to `i64` and `PositiveSmallIntegerField`
    /// (`network`) to `i32` (see Semantic traps).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Project {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub name: String,
        pub description: String,
        pub description_text: Option<serde_json::Value>,
        pub description_html: Option<serde_json::Value>,
        pub network: i32,
        pub workspace_id: uuid::Uuid,
        pub identifier: String,
        pub default_assignee_id: Option<uuid::Uuid>,
        pub project_lead_id: Option<uuid::Uuid>,
        pub emoji: Option<String>,
        pub icon_prop: Option<serde_json::Value>,
        pub module_view: bool,
        pub cycle_view: bool,
        pub issue_views_view: bool,
        pub page_view: bool,
        pub intake_view: bool,
        pub is_time_tracking_enabled: bool,
        pub is_issue_type_enabled: bool,
        pub is_default: bool,
        pub guest_view_all_features: bool,
        pub members_can_edit_states: bool,
        pub cover_image: Option<String>,
        pub cover_image_asset_id: Option<uuid::Uuid>,
        pub estimate_id: Option<uuid::Uuid>,
        pub archive_in: i64,
        pub close_in: i64,
        pub logo_props: serde_json::Value,
        pub default_state_id: Option<uuid::Uuid>,
        pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
        pub timezone: String,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
        pub repo_url: String,
        pub base_branch: String,
        pub agent_default_interval_seconds: i64,
        pub agent_default_max_ticks: i64,
        pub agent_review_default_interval_seconds: i64,
        pub agent_test_default_interval_seconds: i64,
        pub agent_ticking_enabled: bool,
        pub default_agent_executor: String,
    }

    /// Outcome of `Project.resolve` input classification
    /// (`db/models/project.py:191-219`).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ProjectLookup {
        /// The input parsed as a UUID: filter `pk = value` (never tried as
        /// an identifier).
        Pk(uuid::Uuid),
        /// The input did not parse as a UUID: filter
        /// `identifier = <stripped, upper-cased input>` with exact equality
        /// (never `iexact`, so the composite btree is used; never tried as
        /// a pk).
        Identifier(String),
    }

    /// Ports the `try: uuid.UUID(str(value)) / except (ValueError,
    /// AttributeError, TypeError)` branch of `Project.resolve`
    /// (`project.py:197-212`). `Uuid::parse_str` accepts the same
    /// spellings Python does (hyphenated, simple 32-hex, braced, urn), so a
    /// UUID-looking input is always a pk lookup and anything else is an
    /// identifier lookup — never both.
    pub fn classify_lookup(raw: &str) -> ProjectLookup {
        match uuid::Uuid::parse_str(raw) {
            Ok(id) => ProjectLookup::Pk(id),
            Err(_) => ProjectLookup::Identifier(raw.trim().to_uppercase()),
        }
    }

    /// The generic 404 detail `Project.resolve` raises (`project.py:214-218`).
    /// DRF propagates `Http404` args into a `{"detail": ...}` body; the
    /// input value is never echoed.
    pub const NOT_FOUND_DETAIL: &str = "Project not found";

    /// Table-local `WHERE` for `Project.resolve` once the workspace scoping
    /// join (`workspace__slug`) is applied by the caller: pk branch filters
    /// `id = value AND deleted_at IS NULL`; identifier branch filters
    /// `identifier = value AND deleted_at IS NULL` (`project.py:199-212`).
    /// `resolve_id` (`:221-224`) is `resolve(...).pk` — the same code path,
    /// callers just project the `id` column.
    pub fn resolve_condition(lookup: &ProjectLookup) -> Condition {
        let mut cond = Condition::all().add(Expr::col(Alias::new("deleted_at")).is_null());
        match lookup {
            ProjectLookup::Pk(id) => {
                cond = cond.add(Expr::col(Alias::new("id")).eq(*id));
            }
            ProjectLookup::Identifier(name) => {
                cond = cond.add(Expr::col(Alias::new("identifier")).eq(name.clone()));
            }
        }
        cond
    }
}

/// `project_member_invites` table (`db/models/project.py:314-331`,
/// `db_table = "project_member_invites"`).
pub mod project_member_invite {
    use super::DEFAULT_ROLE;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_member_invites";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_member_invites_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.ProjectMemberInvite.columns`). No `unique_together`, no
    /// partial constraints.
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "email",
        "accepted",
        "token",
        "message",
        "responded_at",
        "role",
    ];

    /// Default `accepted` (`:316`).
    pub const DEFAULT_ACCEPTED: bool = false;

    /// Default `role` (`:320`, guest).
    pub const DEFAULT_ROLE_VALUE: i32 = DEFAULT_ROLE;

    /// One `ProjectMemberInvite` row. `message` and `responded_at` are
    /// nullable; `email` and `token` are required.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectMemberInvite {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub email: String,
        pub accepted: bool,
        pub token: String,
        pub message: Option<String>,
        pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
        pub role: i32,
    }
}

/// `project_members` table (`db/models/project.py:332-385`,
/// `db_table = "project_members"`).
pub mod project_member {
    use super::{OnDelete, DEFAULT_ROLE};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_members";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_members_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.ProjectMember.columns`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "member_id",
        "comment",
        "role",
        "view_props",
        "default_props",
        "preferences",
        "sort_order",
        "is_active",
    ];

    /// Partial unique index name (`Meta.constraints`, `:368-374`).
    pub const CONSTRAINT: &str = "project_member_unique_project_member_when_deleted_at_null";

    /// `unique_together = ["project", "member", "deleted_at"]` (`:367`).
    pub const UNIQUE_TOGETHER: &[&str] = &["project_id", "member_id", "deleted_at"];

    /// `member` FK: `CASCADE`, nullable (`:333-339`).
    pub const MEMBER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const MEMBER_NULLABLE: bool = true;

    /// Default `role` (`:341`, guest).
    pub const DEFAULT_ROLE_VALUE: i32 = DEFAULT_ROLE;

    /// Default `sort_order` (`:345`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;

    /// Default `is_active` (`:346`).
    pub const DEFAULT_IS_ACTIVE: bool = true;

    /// `ProjectMember.save` step (`project.py:361`): the companion
    /// `ProjectUserProperty` sorts `10000` below the member's current
    /// workspace minimum.
    pub const SORT_ORDER_STEP: f64 = 10000.0;

    /// `ProjectMember.save` (`project.py:348-364`): on add with a member set,
    /// a `ProjectUserProperty` row is created for the member in the
    /// project's workspace with `sort_order = min - 10000`, or `65535` when
    /// the member has no property rows yet.
    pub fn sort_order_on_create(min_workspace_sort: Option<f64>) -> f64 {
        min_workspace_sort.map_or(DEFAULT_SORT_ORDER, |min| min - SORT_ORDER_STEP)
    }

    /// Default `view_props` / `default_props` (`project.py:43-65`,
    /// `get_default_props`). The nested `filters` / `display_filters` dicts
    /// are value-identical to `issue.get_default_filters()` /
    /// `issue.get_default_display_filters()`.
    pub fn default_props() -> serde_json::Value {
        serde_json::json!({
            "filters": {
                "priority": null,
                "state": null,
                "state_group": null,
                "assignees": null,
                "created_by": null,
                "labels": null,
                "start_date": null,
                "target_date": null,
                "subscriber": null,
            },
            "display_filters": {
                "group_by": null,
                "order_by": "-created_at",
                "type": null,
                "sub_issue": true,
                "show_empty_groups": true,
                "layout": "list",
                "calendar_date_range": "",
            },
        })
    }

    /// Default `preferences` (`project.py:68-69`,
    /// `get_default_preferences`).
    pub fn default_preferences() -> serde_json::Value {
        serde_json::json!({
            "pages": {"block_display": true},
            "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
        })
    }

    /// One `ProjectMember` row. `view_props` / `default_props` /
    /// `preferences` are non-null JSON; `comment` is nullable text.
    ///
    /// `__str__` (`:380-382`) renders `member.email`, which raises on a
    /// null-member row — kept as this note (see module quirks).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectMember {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub member_id: Option<uuid::Uuid>,
        pub comment: Option<String>,
        pub role: i32,
        pub view_props: serde_json::Value,
        pub default_props: serde_json::Value,
        pub preferences: serde_json::Value,
        pub sort_order: f64,
        pub is_active: bool,
    }
}

/// `project_identifiers` table (`db/models/project.py:386-403`,
/// `db_table = "project_identifiers"`).
///
/// `ProjectIdentifier` extends `AuditModel` directly (not `BaseModel`), so
/// Django adds the default `BigAutoField` integer pk — the only
/// integer-pk table in this module.
pub mod project_identifier {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_identifiers";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_identifiers_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.ProjectIdentifier.columns`): the auto pk sorts first here.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
        "name",
    ];

    /// Partial unique index name (`Meta.constraints`, `:393-398`).
    pub const CONSTRAINT: &str = "unique_name_workspace_when_deleted_at_null";

    /// `unique_together = ["name", "workspace", "deleted_at"]` (`:392`).
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "workspace_id", "deleted_at"];

    /// `workspace` FK: `CASCADE`, nullable (`:387`; the `# TODO: Remove
    /// workspace relation later` above the class is still open).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = true;

    /// `project` one-to-one: `CASCADE`, required, unique (`:388`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One `ProjectIdentifier` row. `id` is a `BigAutoField` (Postgres
    /// `bigint`, hence `i64`); `name` is max 12 chars, indexed.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectIdentifier {
        pub id: i64,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: Option<uuid::Uuid>,
        pub project_id: uuid::Uuid,
        pub name: String,
    }
}

/// `project_user_properties` table (`db/models/project.py:464-495`,
/// `db_table = "project_user_properties"`).
pub mod project_user_property {
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_user_properties";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_user_properties_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.ProjectUserProperty.columns`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
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

    /// Partial unique index name (`Meta.constraints`, `:485-490`).
    pub const CONSTRAINT: &str = "project_user_property_unique_user_project_when_deleted_at_null";

    /// `unique_together = ["user", "project", "deleted_at"]` (`:484`).
    pub const UNIQUE_TOGETHER: &[&str] = &["user_id", "project_id", "deleted_at"];

    /// Default `sort_order` (`:477`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;

    /// Default `rich_filters` (`:475`, `default=dict` → `{}`).
    pub fn default_rich_filters() -> serde_json::Value {
        serde_json::Value::Object(Default::default())
    }

    /// Default `preferences` (`:476`): `project.get_default_preferences`,
    /// shared with [`super::project_member::default_preferences`].
    pub use super::project_member::default_preferences;

    /// Default `filters` / `display_filters` / `display_properties`
    /// (`:472-474`): `issue.get_default_filters`,
    /// `issue.get_default_display_filters`,
    /// `issue.get_default_display_properties`
    /// (`db/models/issue.py:50-92`, owned by D-26) — referenced here, ported
    /// there. Rust inserts must supply these JSON values explicitly.
    pub const FILTER_DEFAULTS_OWNED_BY: &str = "D-26 (db/models/issue.py:50-92)";

    /// One `ProjectUserProperty` row. All five JSON columns are non-null;
    /// `user_id` is required.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectUserProperty {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
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
}

/// `project_deploy_boards` table (`db/models/project.py:422-439`,
/// `db_table = "project_deploy_boards"`).
///
/// Marked `# DEPRECATED TODO` at the definition site (`:420-421`: "used to
/// get the old anchors for the project deploy boards") and consumed by the
/// space views — defined here, read there.
pub mod project_deploy_board {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_deploy_boards";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_deploy_boards_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.ProjectDeployBoard.columns`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "anchor",
        "comments",
        "reactions",
        "intake_id",
        "votes",
        "views",
    ];

    /// `unique_together = ["project", "anchor"]` (`:431`). There is no
    /// partial unique constraint on this table; `anchor` is additionally
    /// `unique=True` on its own (`:423`).
    pub const UNIQUE_TOGETHER: &[&str] = &["project_id", "anchor"];

    /// `anchor` is `unique=True, db_index=True` with `default=get_anchor`
    /// (`:423`).
    pub const ANCHOR_UNIQUE: bool = true;

    /// Flag defaults (`:424-425,427`).
    pub const DEFAULT_COMMENTS: bool = false;
    pub const DEFAULT_REACTIONS: bool = false;
    pub const DEFAULT_VOTES: bool = false;

    /// `intake` FK: `SET_NULL`, nullable (`:426`).
    pub const INTAKE_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default `anchor` (`project.py:406-407`, `get_anchor`):
    /// `uuid4().hex` — 32 lowercase hex chars, no dashes, with the v4
    /// version and variant bits set.
    pub fn new_anchor() -> String {
        use rand::RngCore as _;
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Default `views` (`project.py:410-417`, `get_default_views`).
    pub fn default_views() -> serde_json::Value {
        serde_json::json!({
            "list": true,
            "kanban": true,
            "calendar": true,
            "gantt": true,
            "spreadsheet": true,
        })
    }

    /// One `ProjectDeployBoard` row. `views` is non-null JSON.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectDeployBoard {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub anchor: String,
        pub comments: bool,
        pub reactions: bool,
        pub intake_id: Option<uuid::Uuid>,
        pub votes: bool,
        pub views: serde_json::Value,
    }
}

/// `project_public_members` table (`db/models/project.py:442-461`,
/// `db_table = "project_public_members"`).
pub mod project_public_member {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_public_members";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_public_members_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.ProjectPublicMember.columns`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "member_id",
    ];

    /// Partial unique index name (`Meta.constraints`, `:451-456`).
    pub const CONSTRAINT: &str = "project_public_member_unique_project_member_when_deleted_at_null";

    /// `unique_together = ["project", "member", "deleted_at"]` (`:450`).
    pub const UNIQUE_TOGETHER: &[&str] = &["project_id", "member_id", "deleted_at"];

    /// `member` FK: `CASCADE`, required (`:443-448`).
    pub const MEMBER_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One `ProjectPublicMember` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectPublicMember {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub member_id: uuid::Uuid,
    }
}

/// `deploy_boards` table (`db/models/deploy_board.py:19-57`,
/// `db_table = "deploy_boards"`).
///
/// Shared-consumer table: D-25, D-19, and the space views all read it; the
/// column list and read shape are defined here and must not be redefined
/// elsewhere.
///
/// Unlike the project-scoped tables above, `DeployBoard` extends
/// `WorkspaceBaseModel` (`workspace.py:185-195`): the `project` FK is
/// nullable and `save()` backfills `workspace` from `project.workspace`
/// only `if self.project` (contrast the unconditional
/// `ProjectBaseModel.save`).
pub mod deploy_board {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "deploy_boards";

    /// Soft-delete read view.
    pub const VIEW: &str = "deploy_boards_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.DeployBoard.columns`). Note `workspace_id` sorts before the
    /// nullable `project_id` (`WorkspaceBaseModel` field order).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "workspace_id",
        "project_id",
        "entity_identifier",
        "entity_name",
        "anchor",
        "is_comments_enabled",
        "is_reactions_enabled",
        "intake_id",
        "is_votes_enabled",
        "view_props",
        "is_activity_enabled",
        "is_disabled",
    ];

    /// Partial unique index name (`Meta.constraints`, `:47-52`).
    pub const CONSTRAINT: &str =
        "deploy_board_unique_entity_name_entity_identifier_when_deleted_at_null";

    /// `unique_together = ["entity_name", "entity_identifier", "deleted_at"]`
    /// (`:46`).
    pub const UNIQUE_TOGETHER: &[&str] = &["entity_name", "entity_identifier", "deleted_at"];

    /// `TYPE_CHOICES` (`deploy_board.py:20-28`), ported verbatim. Defined
    /// but never wired to any field — `entity_name` has no `choices=` — so
    /// these values are unenforced documentation (see module quirks).
    pub const TYPE_CHOICES: &[(&str, &str)] = &[
        ("project", "Project"),
        ("issue", "Issue"),
        ("module", "Module"),
        ("cycle", "Task"),
        ("page", "Page"),
        ("view", "View"),
        ("intake", "Intake"),
    ];

    /// `project` FK (`WorkspaceBaseModel`, `workspace.py:187`): `CASCADE`,
    /// nullable.
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const PROJECT_NULLABLE: bool = true;

    /// `anchor` is `unique=True, db_index=True` with `default=get_anchor`
    /// (`:32`).
    pub const ANCHOR_UNIQUE: bool = true;

    /// Default `anchor` (`deploy_board.py:15-16`, `get_anchor`): identical
    /// to `project.get_anchor` (`uuid4().hex`), shared here rather than
    /// duplicated.
    pub use super::project_deploy_board::new_anchor;

    /// Flag defaults (`:33-34,36,38-39`).
    pub const DEFAULT_IS_COMMENTS_ENABLED: bool = false;
    pub const DEFAULT_IS_REACTIONS_ENABLED: bool = false;
    pub const DEFAULT_IS_VOTES_ENABLED: bool = false;
    pub const DEFAULT_IS_ACTIVITY_ENABLED: bool = true;
    pub const DEFAULT_IS_DISABLED: bool = false;

    /// `intake` FK: `SET_NULL`, nullable (`:35`).
    pub const INTAKE_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Default `view_props` (`:37`, `default=dict` → `{}`).
    pub fn default_view_props() -> serde_json::Value {
        serde_json::Value::Object(Default::default())
    }

    /// One `DeployBoard` row. `entity_identifier` is a nullable UUID;
    /// `entity_name` is nullable, max 30 chars; `view_props` is non-null
    /// JSON.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct DeployBoard {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub entity_identifier: Option<uuid::Uuid>,
        pub entity_name: Option<String>,
        pub anchor: String,
        pub is_comments_enabled: bool,
        pub is_reactions_enabled: bool,
        pub intake_id: Option<uuid::Uuid>,
        pub is_votes_enabled: bool,
        pub view_props: serde_json::Value,
        pub is_activity_enabled: bool,
        pub is_disabled: bool,
    }
}

/// `states` table (`db/models/state.py:93-140`, `db_table = "states"`).
pub mod state {
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "states";

    /// Soft-delete read view. Note the default manager reads
    /// `states_active` *plus* the triage exclusion
    /// ([`objects_condition`]); the plain view alone is the
    /// `all_state_objects` scope.
    pub const VIEW: &str = "states_active";

    /// Default `ORDER BY` (`Meta.ordering = ("sequence",)`).
    pub const ORDERING: &str = "sequence";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.State.columns`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "name",
        "description",
        "color",
        "slug",
        "sequence",
        "group",
        "is_triage",
        "default",
        "external_source",
        "external_id",
    ];

    /// Partial unique index name (`Meta.constraints`, `:119-124`).
    pub const CONSTRAINT: &str = "state_unique_name_project_when_deleted_at_null";

    /// `unique_together = ["name", "project", "deleted_at"]` (`:118`).
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project_id", "deleted_at"];

    /// Default `sequence` (`:98`).
    pub const DEFAULT_SEQUENCE: f64 = 65535.0;

    /// `State.save` step (`state.py:138`): a new state sorts `15000` above
    /// the current sibling maximum.
    pub const SEQUENCE_STEP: f64 = 15000.0;

    /// Default `group` (`:99-103`, `StateGroup.BACKLOG`).
    pub const DEFAULT_GROUP: &str = "backlog";

    /// The triage group value the default manager excludes (`:83,90`).
    pub const TRIAGE_GROUP: &str = "triage";

    /// Default `is_triage` / `default` (`:104-105`).
    pub const DEFAULT_IS_TRIAGE: bool = false;
    pub const DEFAULT_IS_DEFAULT: bool = false;

    /// `StateGroup` values (`db/models/state.py:14-22`, ported as-is).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum StateGroup {
        Backlog,
        Unstarted,
        Started,
        Review,
        Test,
        Completed,
        Cancelled,
        Triage,
    }

    impl StateGroup {
        /// The stored string (`StateGroup.BACKLOG.value` etc.).
        pub fn as_str(self) -> &'static str {
            match self {
                StateGroup::Backlog => "backlog",
                StateGroup::Unstarted => "unstarted",
                StateGroup::Started => "started",
                StateGroup::Review => "review",
                StateGroup::Test => "test",
                StateGroup::Completed => "completed",
                StateGroup::Cancelled => "cancelled",
                StateGroup::Triage => "triage",
            }
        }
    }

    /// All group values in declaration order.
    pub const ALL_GROUPS: &[&str] = &[
        "backlog",
        "unstarted",
        "started",
        "review",
        "test",
        "completed",
        "cancelled",
        "triage",
    ];

    /// `StateGroup.choices`: `(value, label)` pairs.
    pub const GROUP_CHOICES: &[(&str, &str)] = &[
        ("backlog", "Backlog"),
        ("unstarted", "Unstarted"),
        ("started", "Started"),
        ("review", "Review"),
        ("test", "Test"),
        ("completed", "Completed"),
        ("cancelled", "Cancelled"),
        ("triage", "Triage"),
    ];

    /// Seed rows for `DEFAULT_STATES` (`state.py:26-76`), bulk-created on
    /// project POST (bypassing `save()`):
    /// `(name, color, sequence, group, default)`. Only the Backlog row sets
    /// `default`; every other row omits the key (Django applies `False`).
    pub const DEFAULT_STATES: &[(&str, &str, f64, &str, bool)] = &[
        ("Backlog", "#60646C", 15000.0, "backlog", true),
        ("Todo", "#60646C", 25000.0, "unstarted", false),
        ("In Progress", "#F59E0B", 35000.0, "started", false),
        ("In Review", "#5B5BD6", 40000.0, "review", false),
        ("In Test", "#14B8A6", 42500.0, "test", false),
        ("Done", "#46A758", 45000.0, "completed", false),
        ("Cancelled", "#9AA4BC", 55000.0, "cancelled", false),
        ("Triage", "#4E5355", 65000.0, "triage", false),
    ];

    /// Default-manager scope (`StateManager.get_queryset`, `:82-83`):
    /// soft-delete filter plus `group != 'triage'`.
    pub fn objects_condition() -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("group")).ne(TRIAGE_GROUP))
    }

    /// `TriageStateManager` scope (`:89-90`): triage group only (still
    /// soft-delete filtered, via `SoftDeletionManager`).
    pub fn triage_condition() -> Condition {
        Condition::all()
            .add(Expr::col(Alias::new("deleted_at")).is_null())
            .add(Expr::col(Alias::new("group")).eq(TRIAGE_GROUP))
    }

    /// Ports the ASCII behavior of Django's `slugify(name)`
    /// (`state.py:132`, `django.utils.text.slugify` with
    /// `allow_unicode=False`): lowercase, drop every character that is not
    /// a word character (`[^\w\s-]` — underscore is kept), collapse each
    /// run of spaces/hyphens to one hyphen, and strip leading/trailing
    /// hyphens and underscores. Non-ASCII input differs: Django strips
    /// diacritics via NFKD (`"Café"` -> `"cafe"`) while this keeps the
    /// characters as-is; no D-25 fixture covers that case.
    pub fn slugify_name(name: &str) -> String {
        let lowered = name.to_lowercase();
        let mut out = String::with_capacity(lowered.len());
        let mut prev_dash = true;
        for ch in lowered.chars() {
            if ch.is_alphanumeric() || ch == '_' {
                out.push(ch);
                prev_dash = false;
            } else if (ch.is_whitespace() || ch == '-') && !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        out.trim_matches(|c| c == '-' || c == '_').to_owned()
    }

    /// Ports the `State.save` sequence rule (`state.py:133-138`): on add,
    /// `sequence = max(sibling sequence) + 15000`, where the max runs over
    /// the triage-excluding default manager. Returns `None` when there are
    /// no siblings, in which case the field default (`65535`) applies.
    /// An explicitly set sequence on a non-first row is overwritten —
    /// ported as-is (see module quirks).
    pub fn sequence_on_add(max_sibling_sequence: Option<f64>) -> Option<f64> {
        max_sibling_sequence.map(|max| max + SEQUENCE_STEP)
    }

    /// One `State` row. `description` is `blank=True` without `null`
    /// (`""`, never `NULL`); `external_*` are `null=True, blank=True`;
    /// `slug` is max 100 chars, indexed.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct State {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub description: String,
        pub color: String,
        pub slug: String,
        pub sequence: f64,
        pub group: String,
        pub is_triage: bool,
        pub default: bool,
        pub external_source: Option<String>,
        pub external_id: Option<String>,
    }
}

/// `estimates` table (`db/models/estimate.py:18-40`,
/// `db_table = "estimates"`).
pub mod estimate {
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "estimates";

    /// Soft-delete read view.
    pub const VIEW: &str = "estimates_active";

    /// Default `ORDER BY` (`Meta.ordering = ("name",)`).
    pub const ORDERING: &str = "name";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.Estimate.columns`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "name",
        "description",
        "type",
        "last_used",
    ];

    /// Partial unique index name (`Meta.constraints`, `:30-35`).
    pub const CONSTRAINT: &str = "estimate_unique_name_project_when_deleted_at_null";

    /// `unique_together = ["name", "project", "deleted_at"]` (`:29`).
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project_id", "deleted_at"];

    /// Default `last_used` (`:22`).
    pub const DEFAULT_LAST_USED: bool = false;

    /// `EstimateType` values (`db/models/estimate.py:13-15`, ported as-is).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum EstimateType {
        Categories,
        Points,
    }

    impl EstimateType {
        /// The stored string (`EstimateType.CATEGORIES.value` etc.).
        pub fn as_str(self) -> &'static str {
            match self {
                EstimateType::Categories => "categories",
                EstimateType::Points => "points",
            }
        }
    }

    /// Default `type` (`:21`, `EstimateType.CATEGORIES`).
    pub const DEFAULT_TYPE: EstimateType = EstimateType::Categories;

    /// One `Estimate` row. `description` is `blank=True` without `null`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Estimate {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub name: String,
        pub description: String,
        /// Maps the `type` column (`type` is a Rust keyword).
        pub estimate_type: String,
        pub last_used: bool,
    }
}

/// `estimate_points` table (`db/models/estimate.py:43-57`,
/// `db_table = "estimate_points"`).
pub mod estimate_point {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "estimate_points";

    /// Soft-delete read view.
    pub const VIEW: &str = "estimate_points_active";

    /// Default `ORDER BY` (`Meta.ordering = ("value",)`): the *string*
    /// column, so `"10"` orders before `"2"` (ported as-is).
    pub const ORDERING: &str = "value";

    /// Columns in Django `_meta` field order (matches FX-APROJ-05
    /// `models.EstimatePoint.columns`). No `unique_together`, no partial
    /// constraints.
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "estimate_id",
        "key",
        "description",
        "value",
    ];

    /// `estimate` FK: `CASCADE`, required (`:44`).
    pub const ESTIMATE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// Default `key` (`:45`).
    pub const DEFAULT_KEY: i64 = 0;

    /// `MinValueValidator(0)` bound on `key` (`:45`).
    /// Application-level only; there is no `CHECK` constraint.
    pub const KEY_MIN: i64 = 0;

    /// One `EstimatePoint` row. `key` is a Python `IntegerField`
    /// (unbounded) with a `>= 0` application-level validator.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct EstimatePoint {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub id: uuid::Uuid,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub estimate_id: uuid::Uuid,
        pub key: i64,
        pub description: String,
        pub value: String,
    }
}

#[cfg(test)]
mod tests {
    use super::project::{
        auto_default_on_create, classify_lookup, clear_old_default_condition, cover_image_url,
        display_name, normalize_identifier, resolve_condition, timezone_on_create,
        unset_default_check, ProjectLookup,
    };
    use super::project::{
        AgentExecutorKind, ProjectNetwork, NOT_FOUND_DETAIL, UNSET_DEFAULT_ERROR,
    };
    use super::state::{objects_condition, sequence_on_add, slugify_name, triage_condition};
    use super::state::{StateGroup, ALL_GROUPS, DEFAULT_STATES, GROUP_CHOICES};
    use super::{
        deploy_board, estimate, estimate_point, project, project_base, project_deploy_board,
        project_identifier, project_member, project_member_invite, project_public_member,
        project_user_property, state,
    };
    use super::{estimate::EstimateType, OnDelete, Role, DEFAULT_ROLE, ROLE_CHOICES};
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn select_where(table: &str, cond: sea_query::Condition) -> String {
        let mut q = Query::select();
        q.column(Alias::new("id"))
            .from(Alias::new(table.to_owned()))
            .cond_where(cond);
        q.to_string(PostgresQueryBuilder)
    }

    fn update_where(table: &str, cond: sea_query::Condition) -> String {
        let mut q = Query::update();
        q.table(Alias::new(table.to_owned()))
            .value(Alias::new("is_default"), false)
            .cond_where(cond);
        q.to_string(PostgresQueryBuilder)
    }

    fn fixture() -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_project/FX-APROJ-05.models.json");
        let body =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read FX-APROJ-05: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// One fixture column entry (panics when absent, so drift fails loudly).
    fn col_entry<'a>(v: &'a serde_json::Value, model: &str, column: &str) -> &'a serde_json::Value {
        v["models"][model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} columns"))
            .iter()
            .find(|c| c["column"] == column)
            .unwrap_or_else(|| panic!("{model}.{column} in fixture"))
    }

    /// The recorded `default` of one fixture column (panics when the column
    /// or its default is absent, so drift fails loudly).
    fn col_default(v: &serde_json::Value, model: &str, column: &str) -> serde_json::Value {
        v["models"][model]["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("{model} columns"))
            .iter()
            .find(|c| c["column"] == column)
            .unwrap_or_else(|| panic!("{model}.{column} in fixture"))["default"]
            .clone()
    }

    #[test]
    fn project_columns_match_fixture() {
        assert_eq!(
            project::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "name",
                "description",
                "description_text",
                "description_html",
                "network",
                "workspace_id",
                "identifier",
                "default_assignee_id",
                "project_lead_id",
                "emoji",
                "icon_prop",
                "module_view",
                "cycle_view",
                "issue_views_view",
                "page_view",
                "intake_view",
                "is_time_tracking_enabled",
                "is_issue_type_enabled",
                "is_default",
                "guest_view_all_features",
                "members_can_edit_states",
                "cover_image",
                "cover_image_asset_id",
                "estimate_id",
                "archive_in",
                "close_in",
                "logo_props",
                "default_state_id",
                "archived_at",
                "timezone",
                "external_source",
                "external_id",
                "repo_url",
                "base_branch",
                "agent_default_interval_seconds",
                "agent_default_max_ticks",
                "agent_review_default_interval_seconds",
                "agent_test_default_interval_seconds",
                "agent_ticking_enabled",
                "default_agent_executor",
            ]
        );
        assert_eq!(project::TABLE, "projects");
        assert_eq!(project::VIEW, "projects_active");
        assert_eq!(project::ORDERING, "-created_at");
        assert_eq!(
            project::CONSTRAINTS,
            &[
                "project_unique_identifier_workspace_when_deleted_at_null",
                "project_unique_name_workspace_when_deleted_at_null",
                "project_unique_default_per_workspace",
            ]
        );
        assert_eq!(
            project::UNIQUE_TOGETHER,
            &[
                &["identifier", "workspace_id", "deleted_at"][..],
                &["name", "workspace_id", "deleted_at"][..],
            ]
        );
    }

    #[test]
    fn membership_columns_match_fixtures() {
        assert_eq!(
            project_member::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "member_id",
                "comment",
                "role",
                "view_props",
                "default_props",
                "preferences",
                "sort_order",
                "is_active",
            ]
        );
        assert_eq!(project_member::TABLE, "project_members");
        assert_eq!(
            project_member::CONSTRAINT,
            "project_member_unique_project_member_when_deleted_at_null"
        );
        assert_eq!(
            project_member_invite::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "email",
                "accepted",
                "token",
                "message",
                "responded_at",
                "role",
            ]
        );
        assert_eq!(project_member_invite::TABLE, "project_member_invites");
        assert_eq!(
            project_identifier::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "workspace_id",
                "project_id",
                "name",
            ]
        );
        assert_eq!(
            project_identifier::CONSTRAINT,
            "unique_name_workspace_when_deleted_at_null"
        );
        assert_eq!(
            project_user_property::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "user_id",
                "filters",
                "display_filters",
                "display_properties",
                "rich_filters",
                "preferences",
                "sort_order",
            ]
        );
        assert_eq!(
            project_user_property::CONSTRAINT,
            "project_user_property_unique_user_project_when_deleted_at_null"
        );
    }

    #[test]
    fn board_columns_match_fixtures() {
        assert_eq!(
            project_deploy_board::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "anchor",
                "comments",
                "reactions",
                "intake_id",
                "votes",
                "views",
            ]
        );
        assert_eq!(
            project_deploy_board::UNIQUE_TOGETHER,
            &["project_id", "anchor"]
        );
        assert_eq!(
            project_public_member::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "member_id",
            ]
        );
        assert_eq!(
            project_public_member::CONSTRAINT,
            "project_public_member_unique_project_member_when_deleted_at_null"
        );
        assert_eq!(
            deploy_board::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "workspace_id",
                "project_id",
                "entity_identifier",
                "entity_name",
                "anchor",
                "is_comments_enabled",
                "is_reactions_enabled",
                "intake_id",
                "is_votes_enabled",
                "view_props",
                "is_activity_enabled",
                "is_disabled",
            ]
        );
        assert_eq!(
            deploy_board::CONSTRAINT,
            "deploy_board_unique_entity_name_entity_identifier_when_deleted_at_null"
        );
        assert_eq!(deploy_board::ORDERING, "-created_at");
    }

    #[test]
    fn state_estimate_columns_match_fixtures() {
        assert_eq!(
            state::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "name",
                "description",
                "color",
                "slug",
                "sequence",
                "group",
                "is_triage",
                "default",
                "external_source",
                "external_id",
            ]
        );
        assert_eq!(state::ORDERING, "sequence");
        assert_eq!(
            state::CONSTRAINT,
            "state_unique_name_project_when_deleted_at_null"
        );
        assert_eq!(
            estimate::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "name",
                "description",
                "type",
                "last_used",
            ]
        );
        assert_eq!(estimate::ORDERING, "name");
        assert_eq!(
            estimate::CONSTRAINT,
            "estimate_unique_name_project_when_deleted_at_null"
        );
        assert_eq!(
            estimate_point::COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
                "estimate_id",
                "key",
                "description",
                "value",
            ]
        );
        // String-column ordering quirk, ported as-is.
        assert_eq!(estimate_point::ORDERING, "value");
    }

    #[test]
    fn base_prefix_shared_by_children() {
        assert_eq!(
            project_base::INHERITED_COLUMNS,
            &[
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "id",
                "project_id",
                "workspace_id",
            ]
        );
        for columns in [
            project_member::COLUMNS,
            project_member_invite::COLUMNS,
            project_user_property::COLUMNS,
            project_deploy_board::COLUMNS,
            project_public_member::COLUMNS,
            state::COLUMNS,
            estimate::COLUMNS,
            estimate_point::COLUMNS,
        ] {
            assert_eq!(&columns[..8], project_base::INHERITED_COLUMNS);
        }
        assert_eq!(
            project_base::SAVE_BACKFILLS_WORKSPACE_FROM_PROJECT,
            "project.workspace"
        );
    }

    #[test]
    fn enums_match_fixture() {
        assert_eq!(ROLE_CHOICES, &[(20, "Admin"), (15, "Member"), (5, "Guest")]);
        assert_eq!(DEFAULT_ROLE, 5);
        assert_eq!(Role::Admin.as_int(), 20);
        assert_eq!(Role::Member.as_int(), 15);
        assert_eq!(Role::Guest.as_int(), 5);
        assert_eq!(ProjectNetwork::Secret.as_int(), 0);
        assert_eq!(ProjectNetwork::Public.as_int(), 2);
        assert_eq!(ProjectNetwork::choices(), &[(0, "Secret"), (2, "Public")]);
        assert_eq!(project::DEFAULT_NETWORK, 2);
        assert_eq!(
            project::FORBIDDEN_IDENTIFIER_CHARS_PATTERN,
            r"^.*[&+,:;$^}{*=?@#|'<>.()%!-].*$"
        );
        assert_eq!(
            ALL_GROUPS,
            &[
                "backlog",
                "unstarted",
                "started",
                "review",
                "test",
                "completed",
                "cancelled",
                "triage"
            ]
        );
        assert_eq!(StateGroup::Backlog.as_str(), "backlog");
        assert_eq!(StateGroup::Triage.as_str(), "triage");
        assert_eq!(GROUP_CHOICES.len(), 8);
        assert_eq!(EstimateType::Categories.as_str(), "categories");
        assert_eq!(EstimateType::Points.as_str(), "points");
        assert_eq!(estimate::DEFAULT_TYPE, EstimateType::Categories);
        assert_eq!(AgentExecutorKind::LocalRunner.as_str(), "local_runner");
        assert_eq!(
            AgentExecutorKind::choices(),
            &[
                ("local_runner", "Local Runner"),
                ("cloud_agent", "Pi Dash Cloud Agent"),
                ("managed_runner", "Pi Dash Agent"),
            ]
        );
        assert_eq!(deploy_board::TYPE_CHOICES.len(), 7);
        assert_eq!(deploy_board::TYPE_CHOICES[3], ("cycle", "Task"));
    }

    #[test]
    fn default_states_verbatim() {
        assert_eq!(
            DEFAULT_STATES,
            &[
                ("Backlog", "#60646C", 15000.0, "backlog", true),
                ("Todo", "#60646C", 25000.0, "unstarted", false),
                ("In Progress", "#F59E0B", 35000.0, "started", false),
                ("In Review", "#5B5BD6", 40000.0, "review", false),
                ("In Test", "#14B8A6", 42500.0, "test", false),
                ("Done", "#46A758", 45000.0, "completed", false),
                ("Cancelled", "#9AA4BC", 55000.0, "cancelled", false),
                ("Triage", "#4E5355", 65000.0, "triage", false),
            ]
        );
    }

    #[test]
    fn scalar_defaults_match_fixture() {
        // Every default is compared against the value FX-APROJ-05 recorded
        // from the live Django `_meta` (comparing against runtime fixture
        // values also keeps clippy's constant-assertion lints quiet).
        let fx = fixture();
        let b = |model: &str, column: &str| col_default(&fx, model, column).as_bool().unwrap();
        let i = |model: &str, column: &str| col_default(&fx, model, column).as_i64().unwrap();
        let f = |model: &str, column: &str| col_default(&fx, model, column).as_f64().unwrap();
        let s = |model: &str, column: &str| {
            col_default(&fx, model, column).as_str().unwrap().to_owned()
        };
        assert_eq!(project::DEFAULT_MODULE_VIEW, b("Project", "module_view"));
        assert_eq!(project::DEFAULT_CYCLE_VIEW, b("Project", "cycle_view"));
        assert_eq!(
            project::DEFAULT_ISSUE_VIEWS_VIEW,
            b("Project", "issue_views_view")
        );
        assert_eq!(project::DEFAULT_PAGE_VIEW, b("Project", "page_view"));
        assert_eq!(project::DEFAULT_INTAKE_VIEW, b("Project", "intake_view"));
        assert_eq!(
            project::DEFAULT_IS_TIME_TRACKING_ENABLED,
            b("Project", "is_time_tracking_enabled")
        );
        assert_eq!(
            project::DEFAULT_IS_ISSUE_TYPE_ENABLED,
            b("Project", "is_issue_type_enabled")
        );
        assert_eq!(project::DEFAULT_IS_DEFAULT, b("Project", "is_default"));
        assert_eq!(
            project::DEFAULT_GUEST_VIEW_ALL_FEATURES,
            b("Project", "guest_view_all_features")
        );
        assert_eq!(
            project::DEFAULT_MEMBERS_CAN_EDIT_STATES,
            b("Project", "members_can_edit_states")
        );
        assert_eq!(project::DEFAULT_ARCHIVE_IN, i("Project", "archive_in"));
        assert_eq!(project::DEFAULT_CLOSE_IN, i("Project", "close_in"));
        assert_eq!((project::ARCHIVE_IN_MIN, project::ARCHIVE_IN_MAX), (0, 12));
        assert_eq!((project::CLOSE_IN_MIN, project::CLOSE_IN_MAX), (0, 12));
        assert_eq!(project::DEFAULT_TIMEZONE, s("Project", "timezone"));
        assert_eq!(project::DEFAULT_REPO_URL, s("Project", "repo_url"));
        assert_eq!(project::DEFAULT_BASE_BRANCH, s("Project", "base_branch"));
        assert_eq!(project::BASE_BRANCH_PATTERN, r"^[A-Za-z0-9._/-]*$");
        assert_eq!(
            project::BASE_BRANCH_MESSAGE,
            "Branch name may contain only letters, numbers, and . _ / -"
        );
        assert_eq!(
            project::DEFAULT_AGENT_INTERVAL_SECONDS,
            i("Project", "agent_default_interval_seconds")
        );
        assert_eq!(
            project::DEFAULT_AGENT_MAX_TICKS,
            i("Project", "agent_default_max_ticks")
        );
        assert_eq!(
            project::DEFAULT_AGENT_REVIEW_INTERVAL_SECONDS,
            i("Project", "agent_review_default_interval_seconds")
        );
        assert_eq!(
            project::DEFAULT_AGENT_TEST_INTERVAL_SECONDS,
            i("Project", "agent_test_default_interval_seconds")
        );
        assert_eq!(
            project::DEFAULT_AGENT_TICKING_ENABLED,
            b("Project", "agent_ticking_enabled")
        );
        // The fixture records the callable path; the const is its effective
        // value (Django setting absent → `LOCAL_RUNNER`).
        assert_eq!(
            s("Project", "default_agent_executor"),
            "pi_dash.core.agent_execution.get_default_agent_executor"
        );
        assert_eq!(project::DEFAULT_AGENT_EXECUTOR, "local_runner");
        assert_eq!(s("Project", "logo_props"), "builtins.dict");
        assert_eq!(project::default_logo_props(), serde_json::json!({}));
        assert_eq!(project::DEFAULT_NETWORK, i("Project", "network") as i32);
        assert_eq!(
            project_member_invite::DEFAULT_ACCEPTED,
            b("ProjectMemberInvite", "accepted")
        );
        assert_eq!(
            project_member_invite::DEFAULT_ROLE_VALUE,
            i("ProjectMemberInvite", "role") as i32
        );
        assert_eq!(
            project_member::DEFAULT_ROLE_VALUE,
            i("ProjectMember", "role") as i32
        );
        assert_eq!(
            project_member::DEFAULT_SORT_ORDER,
            f("ProjectMember", "sort_order")
        );
        assert_eq!(
            project_member::DEFAULT_IS_ACTIVE,
            b("ProjectMember", "is_active")
        );
        assert_eq!(project_member::SORT_ORDER_STEP, 10000.0);
        assert_eq!(
            project_user_property::DEFAULT_SORT_ORDER,
            f("ProjectUserProperty", "sort_order")
        );
        assert_eq!(s("ProjectUserProperty", "rich_filters"), "builtins.dict");
        assert_eq!(
            project_user_property::default_rich_filters(),
            serde_json::json!({})
        );
        assert_eq!(
            project_user_property::FILTER_DEFAULTS_OWNED_BY,
            "D-26 (db/models/issue.py:50-92)"
        );
        assert_eq!(
            project_deploy_board::DEFAULT_COMMENTS,
            b("ProjectDeployBoard", "comments")
        );
        assert_eq!(
            project_deploy_board::DEFAULT_REACTIONS,
            b("ProjectDeployBoard", "reactions")
        );
        assert_eq!(
            project_deploy_board::DEFAULT_VOTES,
            b("ProjectDeployBoard", "votes")
        );
        assert_eq!(
            s("ProjectDeployBoard", "anchor"),
            "pi_dash.db.models.project.get_anchor"
        );
        assert_eq!(
            s("ProjectDeployBoard", "views"),
            "pi_dash.db.models.project.get_default_views"
        );
        assert_eq!(
            project_deploy_board::ANCHOR_UNIQUE,
            col_entry(&fx, "ProjectDeployBoard", "anchor")["unique"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(
            deploy_board::DEFAULT_IS_COMMENTS_ENABLED,
            b("DeployBoard", "is_comments_enabled")
        );
        assert_eq!(
            deploy_board::DEFAULT_IS_REACTIONS_ENABLED,
            b("DeployBoard", "is_reactions_enabled")
        );
        assert_eq!(
            deploy_board::DEFAULT_IS_VOTES_ENABLED,
            b("DeployBoard", "is_votes_enabled")
        );
        assert_eq!(
            deploy_board::DEFAULT_IS_ACTIVITY_ENABLED,
            b("DeployBoard", "is_activity_enabled")
        );
        assert_eq!(
            deploy_board::DEFAULT_IS_DISABLED,
            b("DeployBoard", "is_disabled")
        );
        assert_eq!(s("DeployBoard", "view_props"), "builtins.dict");
        assert_eq!(deploy_board::default_view_props(), serde_json::json!({}));
        assert_eq!(
            s("DeployBoard", "anchor"),
            "pi_dash.db.models.deploy_board.get_anchor"
        );
        assert_eq!(
            deploy_board::ANCHOR_UNIQUE,
            col_entry(&fx, "DeployBoard", "anchor")["unique"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(state::DEFAULT_SEQUENCE, f("State", "sequence"));
        assert_eq!(state::SEQUENCE_STEP, 15000.0);
        assert_eq!(state::DEFAULT_GROUP, s("State", "group"));
        assert_eq!(state::TRIAGE_GROUP, "triage");
        assert_eq!(state::DEFAULT_IS_TRIAGE, b("State", "is_triage"));
        assert_eq!(state::DEFAULT_IS_DEFAULT, b("State", "default"));
        assert_eq!(estimate::DEFAULT_LAST_USED, b("Estimate", "last_used"));
        assert_eq!(estimate::DEFAULT_TYPE.as_str(), s("Estimate", "type"));
        assert_eq!(estimate_point::DEFAULT_KEY, i("EstimatePoint", "key"));
        assert_eq!(estimate_point::KEY_MIN, 0);
    }

    #[test]
    fn on_delete_matches_fixture() {
        // Nullability flags are compared against the fixture's recorded
        // `null` marker at runtime (clippy denies asserting on bool consts).
        let fx = fixture();
        let nullable = |model: &str, column: &str| {
            col_entry(&fx, model, column)["null"]
                .as_bool()
                .unwrap_or(false)
        };
        assert_eq!(project::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(project::DEFAULT_ASSIGNEE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(project::PROJECT_LEAD_ON_DELETE, OnDelete::Cascade);
        assert_eq!(project::COVER_IMAGE_ASSET_ON_DELETE, OnDelete::SetNull);
        assert_eq!(project::ESTIMATE_ON_DELETE, OnDelete::SetNull);
        assert_eq!(project::DEFAULT_STATE_ON_DELETE, OnDelete::SetNull);
        assert_eq!(project_member::MEMBER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            project_member::MEMBER_NULLABLE,
            nullable("ProjectMember", "member_id")
        );
        assert_eq!(project_identifier::WORKSPACE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            project_identifier::WORKSPACE_NULLABLE,
            nullable("ProjectIdentifier", "workspace_id")
        );
        assert_eq!(project_identifier::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(project_deploy_board::INTAKE_ON_DELETE, OnDelete::SetNull);
        assert_eq!(project_public_member::MEMBER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(deploy_board::PROJECT_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            deploy_board::PROJECT_NULLABLE,
            nullable("DeployBoard", "project_id")
        );
        assert_eq!(deploy_board::INTAKE_ON_DELETE, OnDelete::SetNull);
        assert_eq!(estimate_point::ESTIMATE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn json_defaults_match_python() {
        assert_eq!(
            project_member::default_props(),
            serde_json::json!({
                "filters": {
                    "priority": null,
                    "state": null,
                    "state_group": null,
                    "assignees": null,
                    "created_by": null,
                    "labels": null,
                    "start_date": null,
                    "target_date": null,
                    "subscriber": null,
                },
                "display_filters": {
                    "group_by": null,
                    "order_by": "-created_at",
                    "type": null,
                    "sub_issue": true,
                    "show_empty_groups": true,
                    "layout": "list",
                    "calendar_date_range": "",
                },
            })
        );
        assert_eq!(
            project_member::default_preferences(),
            serde_json::json!({
                "pages": {"block_display": true},
                "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
            })
        );
        assert_eq!(
            project_user_property::default_preferences(),
            project_member::default_preferences()
        );
        assert_eq!(
            project_deploy_board::default_views(),
            serde_json::json!({
                "list": true,
                "kanban": true,
                "calendar": true,
                "gantt": true,
                "spreadsheet": true,
            })
        );
    }

    #[test]
    fn anchor_is_uuid4_hex() {
        for _ in 0..8 {
            let anchor = project_deploy_board::new_anchor();
            assert_eq!(anchor.len(), 32);
            assert!(anchor.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(!anchor.chars().any(|c| c.is_ascii_uppercase()));
            assert_eq!(&anchor[12..13], "4");
            assert!(matches!(&anchor[16..17], "8" | "9" | "a" | "b"));
        }
        let via_deploy = deploy_board::new_anchor();
        assert_eq!(via_deploy.len(), 32);
    }

    #[test]
    fn member_sort_order_on_create() {
        assert_eq!(project_member::sort_order_on_create(None), 65535.0);
        assert_eq!(project_member::sort_order_on_create(Some(65535.0)), 55535.0);
    }

    #[test]
    fn project_save_rules() {
        // identifier upper-normalization; name untouched.
        assert_eq!(normalize_identifier("  x709770 "), "X709770");
        assert_eq!(normalize_identifier("ENG"), "ENG");
        // cover_image_url precedence: asset > legacy text > None.
        assert_eq!(
            cover_image_url(Some("https://cdn/a.png"), Some("legacy")),
            Some("https://cdn/a.png")
        );
        assert_eq!(cover_image_url(None, Some("legacy")), Some("legacy"));
        assert_eq!(cover_image_url(None, None), None);
        assert_eq!(display_name("Eng", "Acme"), "Eng <Acme>");
        // timezone backfill on create.
        assert_eq!(
            timezone_on_create(false, "UTC", "America/New_York"),
            "America/New_York"
        );
        assert_eq!(
            timezone_on_create(true, "Asia/Tokyo", "America/New_York"),
            "Asia/Tokyo"
        );
        // first project becomes default even when created non-default.
        assert!(auto_default_on_create(false, false));
        assert!(auto_default_on_create(true, true));
        assert!(!auto_default_on_create(false, true));
        // unset-last-default guard.
        assert_eq!(
            unset_default_check(false, true, false),
            Err(UNSET_DEFAULT_ERROR)
        );
        assert_eq!(
            UNSET_DEFAULT_ERROR,
            "Default project cannot be unset without assigning another default project."
        );
        assert!(unset_default_check(false, true, true).is_ok());
        assert!(unset_default_check(false, false, false).is_ok());
        assert!(unset_default_check(true, true, false).is_ok());
    }

    #[test]
    fn clear_old_default_sql() {
        let ws = uuid::Uuid::parse_str("d04b3616-d0d4-4e5f-8bd2-b33f19b9ae3f").unwrap();
        let me = uuid::Uuid::parse_str("b3b89233-332f-47cc-8d29-af99b0d90707").unwrap();
        let sql = update_where("projects", clear_old_default_condition(ws, me));
        assert!(sql.contains(r#""workspace_id" = '"#), "{sql}");
        assert!(sql.contains(r#""is_default" = TRUE"#), "{sql}");
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""id" <> '"#), "{sql}");
    }

    #[test]
    fn resolve_matrix() {
        let id = uuid::Uuid::parse_str("b3b89233-332f-47cc-8d29-af99b0d90707").unwrap();
        assert_eq!(
            classify_lookup("b3b89233-332f-47cc-8d29-af99b0d90707"),
            ProjectLookup::Pk(id)
        );
        assert_eq!(
            classify_lookup("X709770"),
            ProjectLookup::Identifier("X709770".to_owned())
        );
        assert_eq!(
            classify_lookup("x709770"),
            ProjectLookup::Identifier("X709770".to_owned())
        );
        assert_eq!(
            classify_lookup("  x709770 "),
            ProjectLookup::Identifier("X709770".to_owned())
        );
        // Non-UUID, non-matching input still classifies as an identifier
        // lookup (the 404 comes from the empty result, never the classifier).
        assert_eq!(
            classify_lookup("NOPE"),
            ProjectLookup::Identifier("NOPE".to_owned())
        );
        // Python call-site artifacts (`str(123)`, `str(None)`) take the same
        // identifier path and miss the same way.
        assert_eq!(
            classify_lookup("123"),
            ProjectLookup::Identifier("123".to_owned())
        );
        assert_eq!(
            classify_lookup("None"),
            ProjectLookup::Identifier("NONE".to_owned())
        );
        assert_eq!(NOT_FOUND_DETAIL, "Project not found");

        let pk_sql = select_where("projects", resolve_condition(&ProjectLookup::Pk(id)));
        assert!(pk_sql.contains(r#""id" = '"#), "{pk_sql}");
        assert!(pk_sql.contains(r#""deleted_at" IS NULL"#), "{pk_sql}");
        assert!(!pk_sql.contains("identifier"), "{pk_sql}");
        let ident_sql = select_where(
            "projects",
            resolve_condition(&ProjectLookup::Identifier("X709770".to_owned())),
        );
        assert!(
            ident_sql.contains(r#""identifier" = 'X709770'"#),
            "{ident_sql}"
        );
        assert!(ident_sql.contains(r#""deleted_at" IS NULL"#), "{ident_sql}");
        assert!(!ident_sql.contains("UPPER"), "{ident_sql}");
    }

    #[test]
    fn state_managers_and_save() {
        let objects = select_where("states", objects_condition());
        assert!(objects.contains(r#""deleted_at" IS NULL"#), "{objects}");
        assert!(objects.contains(r#""group" <> 'triage'"#), "{objects}");
        let triage = select_where("states", triage_condition());
        assert!(triage.contains(r#""deleted_at" IS NULL"#), "{triage}");
        assert!(triage.contains(r#""group" = 'triage'"#), "{triage}");
        // slugify goldens from the fixture (state_save_slug_sequence).
        assert_eq!(slugify_name("My New State"), "my-new-state");
        assert_eq!(slugify_name("Second"), "second");
        assert_eq!(slugify_name("  Padded -- Name__ "), "padded-name");
        // sequence goldens: first keeps 65535, second is max + 15000.
        assert_eq!(sequence_on_add(None), None);
        assert_eq!(sequence_on_add(Some(65535.0)), Some(80535.0));
    }
}
