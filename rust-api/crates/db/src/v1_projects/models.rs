#![forbid(unsafe_code)]

//! D-19 model columns + field semantics (PIDASHCONV-352).
//!
//! Ports the column lists, defaults, constraints, manager scopes, and
//! save-rule helpers for every table the D-19 views touch, adopting the
//! Django-owned schema column-for-column. Migrations are not ported; Django
//! stays schema owner until switchover.
//!
//! Sources (`apps/api/`): `pi_dash/db/models/project.py:72-495`
//! (`Project` L72-301, `ProjectMemberInvite` L314-331, `ProjectMember`
//! L332-385, `ProjectIdentifier` L386-403); `pi_dash/db/models/state.py:93-140`
//! (`State`, managers L79-91, `DEFAULT_STATES` L26-76);
//! `pi_dash/db/models/estimate.py:18-57` (`EstimateType` L13-17, `Estimate`
//! L18-40, `EstimatePoint` L43-57); `pi_dash/db/models/workspace.py:234-258`
//! (`WorkspaceMemberInvite`); `pi_dash/db/models/favorite.py:14-63`
//! (`UserFavorite`, project touch points `api/views/project.py:493,537`).
//! Audit columns `pi_dash/db/mixins.py:16-89`, UUID pk
//! `pi_dash/db/models/base.py:17-21`.
//!
//! Fixtures (FX-MODELS): `rust-api/fixtures/v1_projects/models/*.columns.json`
//! plus `identifier_routing.golden.json`. Column order in each `COLUMNS`
//! const follows Django `_meta` field order as recorded in those fixtures.
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

use crate::license::models::OnDelete;

/// `ROLE_CHOICES` (`db/models/project.py:25`): `(20, "Admin")`,
/// `(15, "Member")`, `(5, "Guest")`. Application-level only; the column is
/// a plain smallint with no `CHECK` constraint.
pub const ROLE_CHOICES: &[(i32, &str)] = &[(20, "Admin"), (15, "Member"), (5, "Guest")];

/// Default `role` everywhere a member/invite role appears
/// (`project.py:320,341`, `workspace.py:241`).
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

/// `projects` table (`db/models/project.py:72-301`, `db_table = "projects"`).
pub mod project {
    use super::{OnDelete, Role};
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "projects";

    /// Soft-delete read view (`projects_active`).
    pub const VIEW: &str = "projects_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/project.columns.json`). FK columns use
    /// the Django attnames (`workspace_id`, `default_assignee_id`, ...).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
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

    /// `unique_together` (`:229-232`).
    pub const UNIQUE_TOGETHER: &[&[&str]] = &[
        &["identifier", "workspace_id", "deleted_at"],
        &["name", "workspace_id", "deleted_at"],
    ];

    /// `network` choices (`NETWORK_CHOICES`, `:73`).
    pub const NETWORK_CHOICES: &[(i32, &str)] = &[(0, "Secret"), (2, "Public")];

    /// Default `network` (`:78`, `default=2`, public).
    pub const DEFAULT_NETWORK: i32 = 2;

    /// Default `page_view` (`:100`, the only view flag defaulting to true).
    pub const DEFAULT_PAGE_VIEW: bool = true;

    /// Default `members_can_edit_states` (`:106`).
    pub const DEFAULT_MEMBERS_CAN_EDIT_STATES: bool = true;

    /// Default `agent_ticking_enabled` (`:161`).
    pub const DEFAULT_AGENT_TICKING_ENABLED: bool = true;

    /// Agent cadence defaults (`:157-160`, unified at 3 h per PDASHOSS01-167;
    /// pool `agent_default_max_ticks` is also the Re-tick grant size).
    pub const DEFAULT_AGENT_INTERVAL_SECONDS: i64 = 10800;
    pub const DEFAULT_AGENT_MAX_TICKS: i64 = 10;

    /// Default `timezone` (`:123`).
    pub const DEFAULT_TIMEZONE: &str = "UTC";

    /// Default `repo_url` (`:129`, blank string, never `NULL`).
    pub const DEFAULT_REPO_URL: &str = "";

    /// Default `base_branch` (`:130-140`, regex `^[A-Za-z0-9._/-]*$`).
    pub const DEFAULT_BASE_BRANCH: &str = "main";

    /// `workspace` FK: `CASCADE`, required (`:79`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `default_assignee` / `project_lead` FKs: `CASCADE`, nullable
    /// (`:81-94`).
    pub const DEFAULT_ASSIGNEE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const PROJECT_LEAD_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `cover_image_asset` / `estimate` / `default_state` FKs: `SET_NULL`,
    /// nullable (`:108-119`).
    pub const COVER_IMAGE_ASSET_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const ESTIMATE_ON_DELETE: OnDelete = OnDelete::SetNull;
    pub const DEFAULT_STATE_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Fresh default for `logo_props` (`JSONField(default=dict)`, `:118`).
    pub fn default_logo_props() -> serde_json::Value {
        serde_json::Value::Object(Default::default())
    }

    /// Ports `Project.save` identifier normalisation
    /// (`db/models/project.py:258`): strip then upper-case. `str::trim` and
    /// `to_uppercase` are Unicode-aware like Python's `strip`/`upper`.
    pub fn normalize_identifier(raw: &str) -> String {
        raw.trim().to_uppercase()
    }

    /// One `Project` row. `name`/`description`/`repo_url`/`base_branch` are
    /// `NOT NULL` (blank string when unset); every `*_id` user/asset FK and
    /// the `cover_image`, `emoji`, `external_*` columns are nullable.
    /// Python `IntegerField` (unbounded) maps to `i64`;
    /// `PositiveSmallIntegerField` (`network`) to `i32` (see Semantic traps).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Project {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
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

    /// Default member role for project membership contexts.
    pub const MEMBER_DEFAULT_ROLE: Role = Role::Guest;

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

    /// Ports the `_rewrite_project_kwarg` target-key rule
    /// (`api/views/base.py:72-82`): the `project_id` kwarg (member/state /
    /// estimate routes) is always a rewrite target; the `pk` kwarg only when
    /// the resolved URL name is `"project"` (project-detail route).
    /// Member-detail `pk`, `state_id`, `estimate_id`, `estimate_point_id`
    /// are UUID converters and accept UUIDs only; the workspace slug is
    /// never rewritten.
    pub fn should_rewrite_pk(url_name: &str) -> bool {
        url_name == "project"
    }

    /// The URL name of the project-detail route
    /// (`api/views/base.py:75-80`).
    pub const DETAIL_URL_NAME: &str = "project";
}

/// `project_member_invites` table (`db/models/project.py:314-331`).
///
/// No D-19 view or serializer touches this table (member/invite views use
/// `ProjectMember` and `WorkspaceMemberInvite` instead); the columns are
/// recorded for the models layer, and no handler/queryset fixture
/// references it.
pub mod project_member_invite {
    use super::DEFAULT_ROLE;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_member_invites";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_member_invites_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/project_member_invite.columns.json`).
    /// `ProjectBaseModel` contributes `project_id` + `workspace_id`
    /// (workspace backfilled from the project on save, `:302-311`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
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

    /// Default `role` (`:320`).
    pub const DEFAULT_ROLE_VALUE: i32 = DEFAULT_ROLE;

    /// One `ProjectMemberInvite` row. `message` is `null=True` without
    /// `blank=True`: `NULL`, never `""`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectMemberInvite {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
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

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/project_member.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
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

    /// Partial unique index name (`Meta.constraints`, `:368-373`).
    pub const CONSTRAINT: &str = "project_member_unique_project_member_when_deleted_at_null";

    /// `unique_together = ["project", "member", "deleted_at"]` (`:367`).
    pub const UNIQUE_TOGETHER: &[&str] = &["project_id", "member_id", "deleted_at"];

    /// `member` FK: `CASCADE`, nullable (`:333-339`).
    pub const MEMBER_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const MEMBER_NULLABLE: bool = true;

    /// Default `role` (`:341`).
    pub const DEFAULT_ROLE_VALUE: i32 = DEFAULT_ROLE;

    /// Default `sort_order` (`:345`).
    pub const DEFAULT_SORT_ORDER: f64 = 65535.0;

    /// Default `is_active` (`:346`).
    pub const DEFAULT_IS_ACTIVE: bool = true;

    /// Sort-order step for the `ProjectUserProperty` side-effect row
    /// (`:361`).
    pub const SORT_ORDER_STEP: f64 = 10000.0;

    /// Ports the `ProjectMember.save` side effect (`project.py:348-364`):
    /// on create with a member, a `ProjectUserProperty` row is inserted
    /// with `sort_order = member-workspace-min - 10000`, or `65535` when
    /// the member has no property rows in the workspace yet. `None` keeps
    /// the field default.
    pub fn sort_order_on_create(min_workspace_sort: Option<f64>) -> f64 {
        match min_workspace_sort {
            Some(min) => min - SORT_ORDER_STEP,
            None => DEFAULT_SORT_ORDER,
        }
    }

    /// One `ProjectMember` row. `comment` is `blank=True, null=True`;
    /// `view_props` / `default_props` / `preferences` are non-null JSON
    /// with callable defaults; `sort_order` is a float.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectMember {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
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
/// Django adds the default `AutoField` integer pk — the only integer-pk
/// table in this module. Written only by `ProjectSerializer.create`
/// (`api/serializers/project.py:346-350`); `ProjectCreateSerializer.create`
/// only pre-checks this table (`:174-175`).
pub mod project_identifier {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "project_identifiers";

    /// Soft-delete read view.
    pub const VIEW: &str = "project_identifiers_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/project_identifier.columns.json`).
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

    /// `workspace` FK: `CASCADE`, nullable (`:387`). The `TODO: Remove
    /// workspace relation later` comment (`:385`) is carried over as-is.
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const WORKSPACE_NULLABLE: bool = true;

    /// `project` OneToOne: `CASCADE`, required (`:388`).
    pub const PROJECT_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One `ProjectIdentifier` row. `id` is the Django-default `AutoField`
    /// (32-bit integer), not a UUID.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ProjectIdentifier {
        pub id: i32,
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

/// `states` table (`db/models/state.py:93-140`, `db_table = "states"`).
pub mod state {
    use sea_query::{Alias, Condition, Expr};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "states";

    /// Soft-delete read view.
    pub const VIEW: &str = "states_active";

    /// Default `ORDER BY` (`Meta.ordering = ("sequence",)`).
    pub const ORDERING: &str = "sequence";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/state.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
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

    /// Partial unique index name (`Meta.constraints`, `:119-125`).
    pub const CONSTRAINT: &str = "state_unique_name_project_when_deleted_at_null";

    /// `unique_together = ["name", "project", "deleted_at"]` (`:118`).
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project_id", "deleted_at"];

    /// Default `sequence` (`:98`).
    pub const DEFAULT_SEQUENCE: f64 = 65535.0;

    /// Sequence step for `save` on add (`:138`).
    pub const SEQUENCE_STEP: f64 = 15000.0;

    /// Default `group` (`:99-103`, backlog).
    pub const DEFAULT_GROUP: &str = "backlog";

    /// Group value the default manager excludes (`StateManager`, `:79-84`).
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

        /// Every group value, in `StateGroup` declaration order.
        pub const ALL: &[&str] = &[
            "backlog",
            "unstarted",
            "started",
            "review",
            "test",
            "completed",
            "cancelled",
            "triage",
        ];
    }

    /// Seed rows for `DEFAULT_STATES` (`state.py:26-76`), bulk-created on
    /// project POST (`api/views/project.py:240-254`, bypassing `save()`):
    /// `(name, color, sequence, group, default)`.
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
    /// characters as-is; no D-19 fixture covers that case.
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
    /// (`""`, never `NULL`); `external_*` are `null=True, blank=True`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct State {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
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

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/estimate.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
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

    /// `EstimateType` values (`db/models/estimate.py:13-17`, ported as-is).
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
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
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

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/estimate_point.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "estimate_id",
        "key",
        "description",
        "value",
    ];

    /// `estimate` FK: `CASCADE`, required (`:44`).
    pub const ESTIMATE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// Default `key` (`:45`, `MinValueValidator(0)`).
    pub const DEFAULT_KEY: i64 = 0;

    /// One `EstimatePoint` row. `key` is a Python `IntegerField`
    /// (unbounded) with a `>= 0` application-level validator.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct EstimatePoint {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub project_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub estimate_id: uuid::Uuid,
        pub key: i64,
        pub description: String,
        pub value: String,
    }
}

/// `workspace_member_invites` table (`db/models/workspace.py:234-258`,
/// `db_table = "workspace_member_invites"`).
pub mod workspace_member_invite {
    use super::{OnDelete, DEFAULT_ROLE};
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "workspace_member_invites";

    /// Soft-delete read view.
    pub const VIEW: &str = "workspace_member_invites_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/workspace_member_invite.columns.json`).
    /// Direct `workspace_id` FK; no project FK (`:235`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "email",
        "accepted",
        "token",
        "message",
        "responded_at",
        "role",
    ];

    /// Partial unique index name (`Meta.constraints`, `:245-250`).
    pub const CONSTRAINT: &str =
        "workspace_member_invite_unique_email_workspace_when_deleted_at_null";

    /// `unique_together = ["email", "workspace", "deleted_at"]` (`:244`).
    pub const UNIQUE_TOGETHER: &[&str] = &["email", "workspace_id", "deleted_at"];

    /// `workspace` FK: `CASCADE`, required (`:235`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// Default `accepted` (`:237`).
    pub const DEFAULT_ACCEPTED: bool = false;

    /// Default `role` (`:241`). Invite create passes
    /// `created_by=request.user` into `serializer.save()`
    /// (`api/views/invite.py:93`); `created_by` is a real column.
    pub const DEFAULT_ROLE_VALUE: i32 = DEFAULT_ROLE;

    /// One `WorkspaceMemberInvite` row.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct WorkspaceMemberInvite {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub email: String,
        pub accepted: bool,
        pub token: String,
        pub message: Option<String>,
        pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
        pub role: i32,
    }
}

/// `user_favorites` table, project-relevant columns
/// (`db/models/favorite.py:14-63`, `db_table = "user_favorites"`).
///
/// `UserFavorite` extends `WorkspaceBaseModel` (`workspace.py:185-195`):
/// required `workspace_id` plus nullable `project_id` (backfilled from the
/// project on save when set). Project touch points: project delete
/// soft-deletes favorites filtered on `entity_type='project'` +
/// `entity_identifier=<pk-str>` + `project_id=<pk>` *before*
/// `project.delete()` (`api/views/project.py:493`); project archive
/// soft-deletes favorites filtered on `workspace__slug` + `project`
/// (`:537`). A queryset `delete()` stamps `deleted_at`.
pub mod user_favorite {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "user_favorites";

    /// Soft-delete read view.
    pub const VIEW: &str = "user_favorites_active";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Project-relevant columns in Django `_meta` field order (matches
    /// `fixtures/v1_projects/models/user_favorite.columns.json`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "project_id",
        "user_id",
        "entity_type",
        "entity_identifier",
        "name",
        "is_folder",
        "sequence",
        "parent_id",
    ];

    /// Partial unique index name (`Meta.constraints`, `:35-41`).
    pub const CONSTRAINT: &str =
        "user_favorite_unique_entity_type_entity_identifier_user_when_deleted_at_null";

    /// `unique_together` (`:34`).
    pub const UNIQUE_TOGETHER: &[&str] =
        &["entity_type", "user_id", "entity_identifier", "deleted_at"];

    /// Secondary indexes (`Meta.indexes`, `:46-50`).
    pub const INDEXES: &[&str] = &[
        "fav_entity_type_idx",
        "fav_entity_identifier_idx",
        "fav_entity_idx",
    ];

    /// `entity_type` value used by the project delete/archive paths.
    pub const ENTITY_TYPE_PROJECT: &str = "project";

    /// `user` FK: `CASCADE`, required (`:19`).
    pub const USER_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// `parent` self-FK: `CASCADE`, nullable (`:25-31`).
    pub const PARENT_ON_DELETE: OnDelete = OnDelete::Cascade;
    pub const PARENT_NULLABLE: bool = true;

    /// Default `is_folder` (`:23`).
    pub const DEFAULT_IS_FOLDER: bool = false;

    /// Default `sequence` (`:24`).
    pub const DEFAULT_SEQUENCE: f64 = 65535.0;

    /// Sequence step for `save` on add (`favorite.py:62-63`).
    pub const SEQUENCE_STEP: f64 = 10000.0;

    /// Ports the `UserFavorite.save` sequence rule (`favorite.py:52-64`):
    /// on add, `sequence = max(workspace sequence) + 10000`. Returns `None`
    /// when no rows exist yet, in which case the field default (`65535`)
    /// applies. The max runs over the workspace of the project when
    /// `project` is set, else over `workspace` directly.
    pub fn sequence_on_add(largest_workspace_sequence: Option<f64>) -> Option<f64> {
        largest_workspace_sequence.map(|largest| largest + SEQUENCE_STEP)
    }

    /// One `UserFavorite` row (project-relevant columns). `name` is
    /// `blank=True, null=True`; `entity_identifier` is a nullable UUID.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct UserFavorite {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub user_id: uuid::Uuid,
        pub entity_type: String,
        pub entity_identifier: Option<uuid::Uuid>,
        pub name: Option<String>,
        pub is_folder: bool,
        pub sequence: f64,
        pub parent_id: Option<uuid::Uuid>,
    }
}

#[cfg(test)]
mod tests {
    use super::project::{
        classify_lookup, resolve_condition, should_rewrite_pk, ProjectLookup, DETAIL_URL_NAME,
        NOT_FOUND_DETAIL,
    };
    use super::state::{objects_condition, sequence_on_add, slugify_name, triage_condition};
    use super::{estimate, estimate_point, user_favorite, workspace_member_invite};
    use super::{project, project_identifier, project_member, project_member_invite, state};
    use super::{Role, DEFAULT_ROLE, ROLE_CHOICES};
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn select_where(table: &str, cond: sea_query::Condition) -> String {
        let mut q = Query::select();
        q.column(Alias::new("id"))
            .from(Alias::new(table.to_owned()))
            .cond_where(cond);
        q.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn project_columns_match_fixture() {
        assert_eq!(
            project::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
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
    }

    #[test]
    fn member_invite_member_identifier_columns_match_fixtures() {
        assert_eq!(
            project_member_invite::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
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
        assert_eq!(
            project_member::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
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
        assert_eq!(project_member_invite::TABLE, "project_member_invites");
        assert_eq!(project_member::TABLE, "project_members");
        assert_eq!(project_identifier::TABLE, "project_identifiers");
    }

    #[test]
    fn state_estimate_columns_match_fixtures() {
        assert_eq!(
            state::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
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
        assert_eq!(
            estimate::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "project_id",
                "workspace_id",
                "name",
                "description",
                "type",
                "last_used",
            ]
        );
        assert_eq!(
            estimate_point::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "project_id",
                "workspace_id",
                "estimate_id",
                "key",
                "description",
                "value",
            ]
        );
        assert_eq!(state::ORDERING, "sequence");
        assert_eq!(estimate::ORDERING, "name");
        assert_eq!(estimate_point::ORDERING, "value");
    }

    #[test]
    fn invite_favorite_columns_match_fixtures() {
        assert_eq!(
            workspace_member_invite::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "workspace_id",
                "email",
                "accepted",
                "token",
                "message",
                "responded_at",
                "role",
            ]
        );
        assert_eq!(
            user_favorite::COLUMNS,
            &[
                "id",
                "created_at",
                "updated_at",
                "created_by_id",
                "updated_by_id",
                "deleted_at",
                "workspace_id",
                "project_id",
                "user_id",
                "entity_type",
                "entity_identifier",
                "name",
                "is_folder",
                "sequence",
                "parent_id",
            ]
        );
        assert_eq!(workspace_member_invite::TABLE, "workspace_member_invites");
        assert_eq!(user_favorite::TABLE, "user_favorites");
    }

    #[test]
    fn enums_defaults_constraints_match_python() {
        assert_eq!(ROLE_CHOICES, &[(20, "Admin"), (15, "Member"), (5, "Guest")]);
        assert_eq!(DEFAULT_ROLE, 5);
        assert_eq!(Role::Admin.as_int(), 20);
        assert_eq!(Role::Member.as_int(), 15);
        assert_eq!(Role::Guest.as_int(), 5);
        assert_eq!(project::DEFAULT_NETWORK, 2);
        assert_eq!(project::NETWORK_CHOICES, &[(0, "Secret"), (2, "Public")]);
        let page_view: bool = project::DEFAULT_PAGE_VIEW;
        assert!(page_view);
        let members_can_edit_states: bool = project::DEFAULT_MEMBERS_CAN_EDIT_STATES;
        assert!(members_can_edit_states);
        let agent_ticking_enabled: bool = project::DEFAULT_AGENT_TICKING_ENABLED;
        assert!(agent_ticking_enabled);
        assert_eq!(project::DEFAULT_AGENT_INTERVAL_SECONDS, 10800);
        assert_eq!(project::DEFAULT_AGENT_MAX_TICKS, 10);
        assert_eq!(project::DEFAULT_TIMEZONE, "UTC");
        assert_eq!(project::DEFAULT_REPO_URL, "");
        assert_eq!(project::DEFAULT_BASE_BRANCH, "main");
        assert_eq!(project::default_logo_props(), serde_json::json!({}));
        assert_eq!(
            project::CONSTRAINTS,
            &[
                "project_unique_identifier_workspace_when_deleted_at_null",
                "project_unique_name_workspace_when_deleted_at_null",
                "project_unique_default_per_workspace",
            ]
        );
        assert_eq!(
            project_member::CONSTRAINT,
            "project_member_unique_project_member_when_deleted_at_null"
        );
        assert_eq!(
            project_identifier::CONSTRAINT,
            "unique_name_workspace_when_deleted_at_null"
        );
        assert_eq!(
            state::CONSTRAINT,
            "state_unique_name_project_when_deleted_at_null"
        );
        assert_eq!(
            estimate::CONSTRAINT,
            "estimate_unique_name_project_when_deleted_at_null"
        );
        assert_eq!(
            workspace_member_invite::CONSTRAINT,
            "workspace_member_invite_unique_email_workspace_when_deleted_at_null"
        );
        assert_eq!(
            user_favorite::CONSTRAINT,
            "user_favorite_unique_entity_type_entity_identifier_user_when_deleted_at_null"
        );
        assert_eq!(
            user_favorite::INDEXES,
            &[
                "fav_entity_type_idx",
                "fav_entity_identifier_idx",
                "fav_entity_idx",
            ]
        );
        assert_eq!(user_favorite::ENTITY_TYPE_PROJECT, "project");
        assert_eq!(estimate::EstimateType::Categories.as_str(), "categories");
        assert_eq!(estimate::EstimateType::Points.as_str(), "points");
        assert_eq!(estimate::DEFAULT_TYPE, estimate::EstimateType::Categories);
        let last_used: bool = estimate::DEFAULT_LAST_USED;
        assert!(!last_used);
        assert_eq!(estimate_point::DEFAULT_KEY, 0);
        let invite_accepted: bool = project_member_invite::DEFAULT_ACCEPTED;
        assert!(!invite_accepted);
        let ws_invite_accepted: bool = workspace_member_invite::DEFAULT_ACCEPTED;
        assert!(!ws_invite_accepted);
        let member_active: bool = project_member::DEFAULT_IS_ACTIVE;
        assert!(member_active);
        let member_sort: f64 = project_member::DEFAULT_SORT_ORDER;
        assert_eq!(member_sort, 65535.0);
        let is_folder: bool = user_favorite::DEFAULT_IS_FOLDER;
        assert!(!is_folder);
        let fav_sequence: f64 = user_favorite::DEFAULT_SEQUENCE;
        assert_eq!(fav_sequence, 65535.0);
    }

    #[test]
    fn default_states_seed_matches_python() {
        assert_eq!(state::DEFAULT_STATES.len(), 8);
        assert_eq!(
            state::DEFAULT_STATES[0],
            ("Backlog", "#60646C", 15000.0, "backlog", true)
        );
        assert_eq!(
            state::DEFAULT_STATES[4],
            ("In Test", "#14B8A6", 42500.0, "test", false)
        );
        assert_eq!(
            state::DEFAULT_STATES[7],
            ("Triage", "#4E5355", 65000.0, "triage", false)
        );
        assert_eq!(state::StateGroup::ALL.len(), 8);
        assert_eq!(state::StateGroup::Review.as_str(), "review");
        assert_eq!(state::StateGroup::Triage.as_str(), "triage");
        assert_eq!(state::DEFAULT_GROUP, "backlog");
        assert_eq!(state::TRIAGE_GROUP, "triage");
    }

    #[test]
    fn lookup_classifies_uuid_vs_identifier() {
        let hyphenated = "123e4567-e89b-12d3-a456-426614174000";
        match classify_lookup(hyphenated) {
            ProjectLookup::Pk(id) => assert_eq!(id.to_string(), hyphenated),
            ProjectLookup::Identifier(_) => panic!("uuid input must be a pk lookup"),
        }
        match classify_lookup("123E4567-E89B-12D3-A456-426614174000") {
            ProjectLookup::Pk(_) => {}
            ProjectLookup::Identifier(_) => panic!("upper-case uuid must be a pk lookup"),
        }
        match classify_lookup("123e4567e89b12d3a456426614174000") {
            ProjectLookup::Pk(_) => {}
            ProjectLookup::Identifier(_) => {
                panic!("32-hex simple form must be a pk lookup, like Python")
            }
        }
        assert_eq!(
            classify_lookup("ENG"),
            ProjectLookup::Identifier("ENG".to_string())
        );
        assert_eq!(
            classify_lookup(" eng "),
            ProjectLookup::Identifier("ENG".to_string())
        );
        assert_eq!(
            classify_lookup("my-project"),
            ProjectLookup::Identifier("MY-PROJECT".to_string())
        );
        assert_eq!(
            classify_lookup(""),
            ProjectLookup::Identifier(String::new())
        );
    }

    #[test]
    fn resolve_conditions_mirror_python_branches() {
        let pk = classify_lookup("123e4567-e89b-12d3-a456-426614174000");
        let pk_sql = select_where(project::TABLE, resolve_condition(&pk));
        assert!(pk_sql.contains("\"id\""), "pk branch filters id: {pk_sql}");
        assert!(
            pk_sql.contains("\"deleted_at\" IS NULL"),
            "pk branch keeps the soft-delete scope: {pk_sql}"
        );
        assert!(
            !pk_sql.contains("\"identifier\""),
            "uuid input is never tried as an identifier: {pk_sql}"
        );
        let slug = classify_lookup("eng");
        let slug_sql = select_where(project::TABLE, resolve_condition(&slug));
        assert!(
            slug_sql.contains("\"identifier\" = 'ENG'"),
            "identifier branch matches the upper-cased input exactly: {slug_sql}"
        );
        assert!(
            slug_sql.contains("\"deleted_at\" IS NULL"),
            "identifier branch keeps the soft-delete scope: {slug_sql}"
        );
        assert!(
            !slug_sql.contains("\"id\" ="),
            "non-uuid input is never tried as a pk: {slug_sql}"
        );
        assert_eq!(NOT_FOUND_DETAIL, "Project not found");
    }

    #[test]
    fn rewrite_targets_and_manager_scopes() {
        assert!(should_rewrite_pk(DETAIL_URL_NAME));
        assert!(should_rewrite_pk("project"));
        assert!(!should_rewrite_pk("project-member"));
        assert!(!should_rewrite_pk("state"));
        assert!(!should_rewrite_pk(""));
        let objects_sql = select_where(state::TABLE, objects_condition());
        assert!(
            objects_sql.contains("\"deleted_at\" IS NULL"),
            "{objects_sql}"
        );
        assert!(
            objects_sql.contains("\"group\" <> 'triage'"),
            "{objects_sql}"
        );
        let triage_sql = select_where(state::TABLE, triage_condition());
        assert!(
            triage_sql.contains("\"deleted_at\" IS NULL"),
            "{triage_sql}"
        );
        assert!(triage_sql.contains("\"group\" = 'triage'"), "{triage_sql}");
        for (table, view) in [
            (project::TABLE, project::VIEW),
            (project_member::TABLE, project_member::VIEW),
            (project_identifier::TABLE, project_identifier::VIEW),
            (state::TABLE, state::VIEW),
            (estimate::TABLE, estimate::VIEW),
            (estimate_point::TABLE, estimate_point::VIEW),
            (
                workspace_member_invite::TABLE,
                workspace_member_invite::VIEW,
            ),
            (user_favorite::TABLE, user_favorite::VIEW),
        ] {
            assert_eq!(view, format!("{table}_active"));
            let ddl = crate::soft_delete::active_view_ddl(table);
            assert!(ddl.contains(view), "{ddl}");
        }
    }

    #[test]
    fn save_rule_helpers_match_python() {
        assert_eq!(project::normalize_identifier(" eng "), "ENG");
        assert_eq!(project::normalize_identifier("my-project"), "MY-PROJECT");
        assert_eq!(sequence_on_add(None), None);
        assert_eq!(sequence_on_add(Some(35000.0)), Some(50000.0));
        assert_eq!(user_favorite::sequence_on_add(None), None);
        assert_eq!(user_favorite::sequence_on_add(Some(65535.0)), Some(75535.0));
        assert_eq!(project_member::sort_order_on_create(None), 65535.0);
        assert_eq!(project_member::sort_order_on_create(Some(45000.0)), 35000.0);
        assert_eq!(slugify_name("In Progress"), "in-progress");
        assert_eq!(slugify_name("Backlog"), "backlog");
        assert_eq!(slugify_name("  Todo  "), "todo");
        // Underscores survive like Django `[^\w\s-]` (`\w` keeps `_`).
        assert_eq!(slugify_name("my_project"), "my_project");
        assert_eq!(slugify_name("a__b"), "a__b");
        assert_eq!(slugify_name("_lead"), "lead");
        assert_eq!(slugify_name("trail_"), "trail");
        assert_eq!(slugify_name("a - _ b"), "a-_-b");
    }

    fn timestamp(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn rows_construct_field_for_field() {
        let created_at = timestamp(1_700_000_000);
        let project_row = project::Project {
            id: uuid::Uuid::nil(),
            created_at,
            updated_at: created_at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            name: "Eng".to_string(),
            description: String::new(),
            description_text: None,
            description_html: None,
            network: project::DEFAULT_NETWORK,
            workspace_id: uuid::Uuid::nil(),
            identifier: "ENG".to_string(),
            default_assignee_id: None,
            project_lead_id: None,
            emoji: None,
            icon_prop: None,
            module_view: false,
            cycle_view: false,
            issue_views_view: false,
            page_view: true,
            intake_view: false,
            is_time_tracking_enabled: false,
            is_issue_type_enabled: false,
            is_default: false,
            guest_view_all_features: false,
            members_can_edit_states: true,
            cover_image: None,
            cover_image_asset_id: None,
            estimate_id: None,
            archive_in: 0,
            close_in: 0,
            logo_props: project::default_logo_props(),
            default_state_id: None,
            archived_at: None,
            timezone: project::DEFAULT_TIMEZONE.to_string(),
            external_source: None,
            external_id: None,
            repo_url: project::DEFAULT_REPO_URL.to_string(),
            base_branch: project::DEFAULT_BASE_BRANCH.to_string(),
            agent_default_interval_seconds: project::DEFAULT_AGENT_INTERVAL_SECONDS,
            agent_default_max_ticks: project::DEFAULT_AGENT_MAX_TICKS,
            agent_review_default_interval_seconds: project::DEFAULT_AGENT_INTERVAL_SECONDS,
            agent_test_default_interval_seconds: project::DEFAULT_AGENT_INTERVAL_SECONDS,
            agent_ticking_enabled: true,
            default_agent_executor: "local_runner".to_string(),
        };
        let value = serde_json::to_value(&project_row).unwrap();
        assert_eq!(value["identifier"], serde_json::json!("ENG"));
        assert_eq!(value["network"], serde_json::json!(2));
        assert_eq!(value["archived_at"], serde_json::Value::Null);
        let back: project::Project = serde_json::from_value(value).unwrap();
        assert_eq!(back, project_row);

        let state_row = state::State {
            id: uuid::Uuid::nil(),
            created_at,
            updated_at: created_at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            project_id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            name: "Backlog".to_string(),
            description: String::new(),
            color: "#60646C".to_string(),
            slug: slugify_name("Backlog"),
            sequence: 15000.0,
            group: state::DEFAULT_GROUP.to_string(),
            is_triage: false,
            default: true,
            external_source: None,
            external_id: None,
        };
        assert_eq!(state_row.slug, "backlog");
        let identifier_row = project_identifier::ProjectIdentifier {
            id: 1,
            created_at,
            updated_at: created_at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: None,
            project_id: uuid::Uuid::nil(),
            name: "ENG".to_string(),
        };
        assert_eq!(identifier_row.id, 1);
    }
}
