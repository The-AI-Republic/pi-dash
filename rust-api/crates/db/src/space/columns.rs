//! DeployBoard-anchored read columns + manager scopes for D-02.
//!
//! Translation of the read-only `db` models the space views use (there is
//! no `models.py` in `space/`): column lists in Django field-definition
//! order with defaults, `db_table`, ordering and `unique_together`, plus
//! the manager semantics as `WHERE` predicates. Fixture source of truth:
//! `rust-api/fixtures/space/models/*.columns.json` (recorded by
//! PIDASHCONV-135, traced to exact Python lines in
//! `rust-api/fixtures/space/TRACE.md`); the `#[cfg(test)]` suite asserts
//! these consts equal the fixture column lists table by table.
//!
//! Manager map (`apps/api/pi_dash/db/mixins.py:56-67`):
//! - `objects` = `SoftDeletionManager`: `deleted_at IS NULL`
//!   ([`live_rows_scope`]).
//! - `all_objects` = plain `Manager`: no scope, sees soft-deleted rows
//!   ([`unscoped_scope`]). The asset restore endpoint uses it
//!   (`space/views/asset.py:183`).
//! - `Issue.issue_objects` = `IssueManager` (`db/models/issue.py:95-104`):
//!   soft-delete scope plus `state__group != triage`, `archived_at IS NULL`,
//!   `project__archived_at IS NULL`, `is_draft = false`
//!   ([`issue_objects_scope`]). `Issue` declares no `objects =`, so
//!   `Issue.objects` is the plain inherited soft-delete scope (includes
//!   triage/archived/draft rows); intake list/create use it
//!   (`space/views/intake.py:66,145`).
//! - `State.objects` = `StateManager`: scope minus triage group
//!   (`db/models/state.py:79-83`) ([`state_default_scope`]);
//!   `State.triage_objects` = `TriageStateManager`: triage rows only
//!   (`:86-90`) ([`state_triage_scope`]); `State.all_state_objects` is
//!   the plain manager.
//!
//! Two scope parts cannot be single-table predicates and are owned by the
//! queries layer, which holds the joins: `IssueManager`'s
//! `project__archived_at IS NULL` (join to `projects`) and `state__group`
//! (join to `states`; the qualified column below names the join alias the
//! queries layer must use).

use sea_query::{Alias, Condition, Expr};

/// Value of `State.group` that marks a triage state
/// (`db/models/state.py:22`, `StateGroup.TRIAGE`).
pub const STATE_GROUP_TRIAGE: &str = "triage";

// ---------------------------------------------------------------------------
// Manager read-scopes as WHERE predicates
// ---------------------------------------------------------------------------

/// Default read scope: live rows only (`deleted_at IS NULL`).
///
/// Mirrors `SoftDeletionManager.get_queryset`
/// (`db/mixins.py:56-58`); delegates to the kernel helper so the predicate
/// cannot drift from [`crate::soft_delete::active_condition`].
pub fn live_rows_scope() -> Condition {
    crate::soft_delete::active_condition()
}

/// Unscoped reads: no `WHERE` at all, sees soft-deleted rows.
///
/// Mirrors `all_objects` (plain `Manager`, `db/mixins.py:67`) and
/// `State.all_state_objects` (`db/models/state.py:110`). The only
/// space read that uses it is the asset restore path
/// (`space/views/asset.py:183`).
pub fn unscoped_scope() -> Condition {
    Condition::all()
}

/// `Issue.issue_objects` (`db/models/issue.py:95-104,229`).
///
/// Single-table conjuncts: not soft-deleted, `archived_at IS NULL`,
/// `is_draft = false`, plus the joined-states conjunct
/// `states.group != 'triage'` (the queries layer joins `states` under the
/// `states` alias). The remaining `project__archived_at IS NULL` conjunct
/// needs the `projects` join and is applied by the queries layer.
pub fn issue_objects_scope() -> Condition {
    Condition::all()
        .add(Expr::col(Alias::new("deleted_at")).is_null())
        .add(Expr::col(Alias::new("archived_at")).is_null())
        .add(Expr::col(Alias::new("is_draft")).eq(false))
        .add(Expr::col((Alias::new("states"), Alias::new("group"))).ne(STATE_GROUP_TRIAGE))
}

/// `State.objects` (`db/models/state.py:79-83,109`): live rows whose group
/// is not triage.
pub fn state_default_scope() -> Condition {
    Condition::all()
        .add(Expr::col(Alias::new("deleted_at")).is_null())
        .add(Expr::col(Alias::new("group")).ne(STATE_GROUP_TRIAGE))
}

/// `State.triage_objects` (`db/models/state.py:86-90,111`): live rows whose
/// group is triage. Space intake create reads through this manager with an
/// auto-create fallback (`space/views/intake.py:129-142`).
pub fn state_triage_scope() -> Condition {
    Condition::all()
        .add(Expr::col(Alias::new("deleted_at")).is_null())
        .add(Expr::col(Alias::new("group")).eq(STATE_GROUP_TRIAGE))
}

// ---------------------------------------------------------------------------
// DeployBoard
// ---------------------------------------------------------------------------

/// `db/models/deploy_board.py:19-57`, table `deploy_boards`.
pub mod deploy_board {
    /// Physical table (`Meta.db_table`, `deploy_board.py:55`).
    pub const TABLE: &str = "deploy_boards";
    /// Default ordering (`Meta.ordering`, `:57`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together` (`:46`).
    pub const UNIQUE_TOGETHER: &[&str] = &["entity_name", "entity_identifier", "deleted_at"];
    /// Partial unique constraint backing the anchor scope (`:47-53`).
    pub const ENTITY_SCOPE_CONSTRAINT: &str =
        "deploy_board_unique_entity_name_entity_identifier_when_deleted_at_null";
    /// Columns of the partial unique index (`:49`).
    pub const ENTITY_SCOPE_COLUMNS: &[&str] = &["entity_name", "entity_identifier"];
    /// `WHERE` of the partial unique index (`:50`): the entity scope is
    /// unique among live rows only.
    pub const ENTITY_SCOPE_WHERE: &str = "deleted_at IS NULL";
    /// `anchor` is globally unique (`unique=True, db_index=True`, `:32`).
    pub const ANCHOR_UNIQUE: bool = true;
    /// Columns in Django field-definition order: `BaseModel.id`
    /// (`db/models/base.py:17-18`), audit cols
    /// (`db/mixins.py:16-42,61-64`), workspace/project FKs
    /// (`WorkspaceBaseModel`, `db/models/workspace.py:185-195`), then
    /// `deploy_board.py:30-43`.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
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
    /// Anchor default expression for documentation (`get_anchor`, `:14-15`).
    pub const ANCHOR_DEFAULT: &str = "get_anchor(uuid4hex)";
    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    /// `anchor` defaults to `get_anchor` (`uuid4().hex`, `:14-15,32`).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,                         // id (uuid4 pk)
        None,                         // created_at (auto_now_add)
        None,                         // updated_at (auto_now)
        None,                         // created_by_id (SET_NULL)
        None,                         // updated_by_id (SET_NULL)
        None,                         // deleted_at
        None,                         // workspace_id (required FK)
        None,                         // project_id (nullable FK)
        None,                         // entity_identifier (UUIDField null)
        None,                         // entity_name (varchar(30) null blank)
        Some("get_anchor(uuid4hex)"), // anchor
        Some("false"),                // is_comments_enabled
        Some("false"),                // is_reactions_enabled
        None,                         // intake_id (SET_NULL)
        Some("false"),                // is_votes_enabled
        Some("dict"),                 // view_props (JSONField)
        Some("true"),                 // is_activity_enabled
        Some("false"),                // is_disabled
    ];
}

// ---------------------------------------------------------------------------
// Project family: Project, ProjectMember, ProjectPublicMember,
// WorkspaceMember, User identity subset
// ---------------------------------------------------------------------------

/// `db/models/project.py:72-253`, table `projects`.
pub mod project {
    /// Physical table (`Meta.db_table`, `project.py:248`).
    pub const TABLE: &str = "projects";
    /// Default ordering (`Meta.ordering`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together` (`project.py:249-252`).
    pub const UNIQUE_TOGETHER: &[&str] = &[
        "identifier+workspace+deleted_at",
        "name+workspace+deleted_at",
    ];
    /// Columns in Django field-definition order (`project.py:72-226`).
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
    ];
}

/// `db/models/project.py:332-378`, table `project_members`.
pub mod project_member {
    /// Physical table.
    pub const TABLE: &str = "project_members";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together` (`project.py:366-378`).
    pub const UNIQUE_TOGETHER: &[&str] = &["project", "member", "deleted_at"];
    /// Columns in Django field-definition order (`project.py:332-364`).
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
}

/// `db/models/project.py:442-461`, table `project_public_members`.
pub mod project_public_member {
    /// Physical table.
    pub const TABLE: &str = "project_public_members";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together` (`project.py:449-461`).
    pub const UNIQUE_TOGETHER: &[&str] = &["project", "member", "deleted_at"];
    /// Columns in Django field-definition order.
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
    ];
}

/// `db/models/workspace.py:198-227`, table `workspace_members`.
pub mod workspace_member {
    /// Physical table.
    pub const TABLE: &str = "workspace_members";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together` (`workspace.py:215-227`).
    pub const UNIQUE_TOGETHER: &[&str] = &["workspace", "member", "deleted_at"];
    /// Columns in Django field-definition order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "workspace_id",
        "member_id",
        "role",
        "company_role",
        "is_active",
    ];
}

/// Identity columns of `db/models/user.py:56-137` read by the space
/// serializers (subset: `User` is not a `BaseModel` — no
/// `deleted_at`/`created_by` — `objects = UserManager()`, `user.py:128`,
/// ordering `-created_at`, `user.py:133-137`).
pub mod user_identity {
    /// Physical table.
    pub const TABLE: &str = "users";
    /// Default ordering (`Meta.ordering`, `user.py:133-137`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// Space-read identity subset in field-definition order, including the
    /// `avatar_asset` FK (`user.py:69-75`) the public serializers expand.
    pub const COLUMNS: &[&str] = &[
        "id",
        "username",
        "email",
        "display_name",
        "first_name",
        "last_name",
        "avatar",
        "avatar_asset_id",
        "is_bot",
        "user_timezone",
    ];
}

// ---------------------------------------------------------------------------
// Issue graph: Issue, IssueLink, IssueRelation
// ---------------------------------------------------------------------------

/// `db/models/issue.py:107-265`, table `issues`.
///
/// `workpad` (`issue.py:198`) must never leak to space output (serializer
/// exclude, `space/serializer/issue.py:185`). `assignees` and `labels`
/// are M2M relations through `IssueAssignee`/`IssueLabel` (`:161-169`):
/// they hold fixture positions below but are NOT physical columns —
/// SELECT builders must skip them.
pub mod issue {
    /// Physical table (`Meta.db_table`, `issue.py:250-265`).
    pub const TABLE: &str = "issues";
    /// Default ordering (`Meta.ordering`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// Columns in Django field-definition order (`issue.py:115-227`).
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
        // M2M through IssueAssignee: no physical column (see module docs).
        "assignees",
        "sequence_id",
        // M2M through IssueLabel: no physical column (see module docs).
        "labels",
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
    /// M2M entries in [`COLUMNS`] that are not physical columns.
    pub const M2M_ENTRIES: &[&str] = &["assignees", "labels"];
    /// `priority` default (`issue.py:142-147`).
    pub const PRIORITY_DEFAULT: &str = "none";
    /// `description_html` blank default (`issue.py:139`).
    pub const DESCRIPTION_HTML_DEFAULT: &str = "<p></p>";
}

/// `db/models/issue.py:471-481`, table `issue_links` (no
/// `unique_together`/constraints).
pub mod issue_link {
    /// Physical table.
    pub const TABLE: &str = "issue_links";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together`: none.
    pub const UNIQUE_TOGETHER: &[&str] = &[];
    /// Columns in Django field-definition order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "title",
        "url",
        "issue_id",
        "metadata",
    ];
}

/// `db/models/issue.py:396-417`, table `issue_relations`. `relation_type`
/// declares no `choices=` (`issue.py:399-403`); the valid values are the
/// `IssueRelationChoices` values (`issue.py:372-379`): `duplicate`,
/// `relates_to`, `blocked_by`, `start_before`, `finish_before`,
/// `implemented_by`.
pub mod issue_relation {
    /// Physical table.
    pub const TABLE: &str = "issue_relations";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together` (`issue.py:396-417`).
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "related_issue", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str =
        "issue_relation_unique_issue_related_issue_when_deleted_at_null";
    /// Columns in Django field-definition order.
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
        "related_issue_id",
        "relation_type",
    ];
    /// `relation_type` default (`issue.py:399-403`).
    pub const RELATION_TYPE_DEFAULT: &str = "blocked_by";
}

// ---------------------------------------------------------------------------
// Comments, reactions, votes
// ---------------------------------------------------------------------------

/// `db/models/issue.py:550-662`, table `issue_comments`.
pub mod issue_comment {
    /// Physical table.
    pub const TABLE: &str = "issue_comments";
    /// Default ordering (`Meta.ordering`, `issue.py:649-662`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// Columns in Django field-definition order (`issue.py:557-594`).
    /// `access` choices are `INTERNAL`/`EXTERNAL`, default `INTERNAL`
    /// (`:575-579`); space list filters `EXTERNAL`, create forces
    /// `EXTERNAL` (`space/views/issue.py:232-273`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "comment_stripped",
        "comment_json",
        "comment_html",
        "description_id",
        "attachments",
        "labels",
        "issue_id",
        "actor_id",
        "access",
        "external_source",
        "external_id",
        "speaker_type",
        "speaker_label",
        "speaker_agent_run_id",
        "edited_at",
        "parent_id",
    ];
    /// `access` default (`issue.py:575-579`).
    pub const ACCESS_DEFAULT: &str = "INTERNAL";
}

/// `db/models/issue.py:726-747`, table `issue_reactions`.
pub mod issue_reaction {
    /// Physical table.
    pub const TABLE: &str = "issue_reactions";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together`.
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "actor", "reaction", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str =
        "issue_reaction_unique_issue_actor_reaction_when_deleted_at_null";
    /// Columns in Django field-definition order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "actor_id",
        "issue_id",
        "reaction",
    ];
}

/// `db/models/issue.py:753-774`, table `comment_reactions`.
pub mod comment_reaction {
    /// Physical table.
    pub const TABLE: &str = "comment_reactions";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together`.
    pub const UNIQUE_TOGETHER: &[&str] = &["comment", "actor", "reaction", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str =
        "comment_reaction_unique_comment_actor_reaction_when_deleted_at_null";
    /// Columns in Django field-definition order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "actor_id",
        "comment_id",
        "reaction",
    ];
}

/// `db/models/issue.py:780-797`, table `issue_votes`. One row per
/// (`issue`, `actor`) while not deleted.
pub mod issue_vote {
    /// Physical table.
    pub const TABLE: &str = "issue_votes";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together`.
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "actor", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str = "issue_vote_unique_issue_actor_when_deleted_at_null";
    /// Columns in Django field-definition order.
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
        "actor_id",
        "vote",
    ];
    /// `vote` default (`issue.py:783`).
    pub const VOTE_DEFAULT: i32 = 1;
}

// ---------------------------------------------------------------------------
// Intake
// ---------------------------------------------------------------------------

/// `db/models/intake.py:50-80`, table `intake_issues`.
pub mod intake_issue {
    /// Physical table (`Meta.db_table`, `intake.py:78`).
    pub const TABLE: &str = "intake_issues";
    /// Default ordering (`Meta.ordering`, `:80`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// Columns in Django field-definition order (`intake.py:51-74`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "intake_id",
        "issue_id",
        "status",
        "snoozed_till",
        "duplicate_to_id",
        "source",
        "source_email",
        "external_source",
        "external_id",
        "extra",
    ];
    /// `status` default: `PENDING` (`intake.py:53-62`).
    pub const STATUS_DEFAULT: i32 = status::PENDING;
    /// `source` default (`intake.py:70`; `SourceType.IN_APP` at
    /// `space/views/intake.py:169`).
    pub const SOURCE_DEFAULT: &str = "IN_APP";

    /// `IntakeIssueStatus` values (`db/models/intake.py:42-47`).
    pub mod status {
        /// Pending.
        pub const PENDING: i32 = -2;
        /// Rejected.
        pub const REJECTED: i32 = -1;
        /// Snoozed.
        pub const SNOOZED: i32 = 0;
        /// Accepted.
        pub const ACCEPTED: i32 = 1;
        /// Duplicate.
        pub const DUPLICATE: i32 = 2;
    }
}

// ---------------------------------------------------------------------------
// File assets
// ---------------------------------------------------------------------------

/// `db/models/asset.py:28-74`, table `file_assets`.
///
/// Extends `BaseModel` directly: no project/workspace base, every entity
/// link nullable. No `objects =` assignment in the file, so `objects` is
/// the inherited `SoftDeletionManager` and `all_objects` the plain manager
/// (`db/mixins.py:66-67`); restore reads through `all_objects`.
pub mod file_asset {
    /// Physical table (`Meta.db_table`, `asset.py:67`).
    pub const TABLE: &str = "file_assets";
    /// Default ordering (`Meta.ordering`, `:68`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// Index names (`Meta.indexes`, `asset.py:69-74`).
    pub const INDEXES: &[&str] = &[
        "asset_entity_type_idx",
        "asset_entity_identifier_idx",
        "asset_entity_idx",
        "asset_asset_idx",
    ];
    /// Columns in Django field-definition order (`asset.py:45-62`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "attributes",
        "asset",
        "user_id",
        "workspace_id",
        "draft_issue_id",
        "project_id",
        "issue_id",
        "comment_id",
        "page_id",
        "entity_type",
        "entity_identifier",
        "is_deleted",
        "is_archived",
        "external_id",
        "external_source",
        "size",
        "is_uploaded",
        "storage_metadata",
    ];
    /// All `EntityTypeContext` values (`asset.py:33-43`).
    pub const ENTITY_TYPES: &[&str] = &[
        "ISSUE_ATTACHMENT",
        "ISSUE_DESCRIPTION",
        "COMMENT_DESCRIPTION",
        "PAGE_DESCRIPTION",
        "USER_COVER",
        "USER_AVATAR",
        "WORKSPACE_LOGO",
        "PROJECT_COVER",
        "DRAFT_ISSUE_ATTACHMENT",
        "DRAFT_ISSUE_DESCRIPTION",
    ];
    /// Entity types the space asset get accepts (`entity_type__in`,
    /// `space/views/asset.py:45-55`).
    pub const SPACE_READ_ENTITY_TYPES: &[&str] = &["ISSUE_DESCRIPTION", "COMMENT_DESCRIPTION"];
}

// ---------------------------------------------------------------------------
// Cycle, Module, State, Label (+ link tables)
// ---------------------------------------------------------------------------

/// `db/models/cycle.py:60-86`, table `cycles`.
pub mod cycle {
    /// Physical table.
    pub const TABLE: &str = "cycles";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// Columns in Django field-definition order (`cycle.py:61-80`).
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
        "start_date",
        "end_date",
        "owned_by_id",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "progress_snapshot",
        "archived_at",
        "logo_props",
        "timezone",
        "version",
    ];
}

/// `db/models/cycle.py:104-124`, table `cycle_issues`.
pub mod cycle_issue {
    /// Physical table.
    pub const TABLE: &str = "cycle_issues";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together` (`cycle.py:104-124`).
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "cycle", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str = "cycle_issue_when_deleted_at_null";
    /// Columns in Django field-definition order.
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
        "cycle_id",
    ];
}

/// `db/models/module.py:67-113`, table `modules`. `members` is an M2M
/// relation through `ModuleMember` (`module.py:87-93`): it holds a fixture
/// position below but is NOT a physical column — SELECT builders must
/// skip it.
pub mod module {
    /// Physical table.
    pub const TABLE: &str = "modules";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together`.
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str = "module_unique_name_project_when_deleted_at_null";
    /// Columns in Django field-definition order (`module.py:68-99`).
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
        "description_text",
        "description_html",
        "start_date",
        "target_date",
        "status",
        "lead_id",
        // M2M through ModuleMember: no physical column (see module docs).
        "members",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "archived_at",
        "logo_props",
    ];
    /// M2M entries in [`COLUMNS`] that are not physical columns.
    pub const M2M_ENTRIES: &[&str] = &["members"];
    /// `status` default (`module.py:74-85`).
    pub const STATUS_DEFAULT: &str = "planned";
}

/// `db/models/module.py:152-168`, table `module_issues`.
pub mod module_issue {
    /// Physical table.
    pub const TABLE: &str = "module_issues";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together`.
    pub const UNIQUE_TOGETHER: &[&str] = &["issue", "module", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str = "module_issue_unique_issue_module_when_deleted_at_null";
    /// Columns in Django field-definition order.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "project_id",
        "workspace_id",
        "module_id",
        "issue_id",
    ];
}

/// `db/models/state.py:93-129`, table `states`.
pub mod state {
    /// Physical table.
    pub const TABLE: &str = "states";
    /// Default ordering (`Meta.ordering`): by sequence.
    pub const ORDERING: &[&str] = &["sequence"];
    /// `Meta.unique_together`.
    pub const UNIQUE_TOGETHER: &[&str] = &["name", "project", "deleted_at"];
    /// Partial unique constraint name.
    pub const UNIQUE_CONSTRAINT: &str = "state_unique_name_project_when_deleted_at_null";
    /// Columns in Django field-definition order (`state.py:94-107`).
    /// `default` is a Rust keyword but a plain column name here.
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
}

/// `db/models/label.py:11-44`, table `labels` (extends
/// `WorkspaceBaseModel`; no `unique_together` — uniqueness comes from the
/// two partial constraints below).
pub mod label {
    /// Physical table.
    pub const TABLE: &str = "labels";
    /// Default ordering.
    pub const ORDERING: &[&str] = &["-created_at"];
    /// `Meta.unique_together`: none (`label.py:26-44`).
    pub const UNIQUE_TOGETHER: &[&str] = &[];
    /// Columns in Django field-definition order (`label.py:12-24`).
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    use serde_json::Value;

    /// Column names from a fixture `columns` array, whose entries are
    /// either bare names (`"id"`), names with annotations
    /// (`"project_id (FK CASCADE)"`), or objects (`{"name": ..., ...}`).
    fn column_names(columns: &Value) -> Vec<String> {
        columns
            .as_array()
            .expect("fixture columns must be an array")
            .iter()
            .map(|entry| match entry {
                Value::String(s) => s
                    .split_whitespace()
                    .next()
                    .expect("column entry must be non-empty")
                    .to_owned(),
                Value::Object(map) => map
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("column object must carry a name")
                    .to_owned(),
                _ => panic!("unexpected column entry shape: {entry}"),
            })
            .collect()
    }

    /// Like [`strings`], but a missing (`null`) array means "none" — the
    /// Label fixture records no `unique_together` key value at all.
    fn strings_or_empty(value: &Value) -> Vec<String> {
        if value.is_null() {
            Vec::new()
        } else {
            strings(value)
        }
    }

    fn strings(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("must be an array")
            .iter()
            .map(|v| v.as_str().expect("must be a string array").to_owned())
            .collect()
    }

    /// Render `SELECT "id" FROM <table> WHERE <scope>` in Postgres dialect.
    fn select_where(table: &str, scope: Condition) -> String {
        let mut select = Query::select();
        select
            .column(Alias::new("id"))
            .from(Alias::new(table))
            .cond_where(scope);
        select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn deployboard_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/deployboard.columns.json"
        ))
        .unwrap();
        assert_eq!(fixture["db_table"].as_str().unwrap(), deploy_board::TABLE);
        assert_eq!(strings(&fixture["ordering"]), deploy_board::ORDERING);
        assert_eq!(
            strings(&fixture["unique_together"]),
            deploy_board::UNIQUE_TOGETHER
        );
        let expected: Vec<String> = deploy_board::COLUMNS
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(column_names(&fixture["columns"]), expected);
        assert_eq!(
            deploy_board::DEFAULTS.len(),
            deploy_board::COLUMNS.len(),
            "one default slot per column"
        );
        assert_eq!(
            (deploy_board::ANCHOR_UNIQUE, deploy_board::ANCHOR_DEFAULT),
            (true, "get_anchor(uuid4hex)"),
        );
        assert_eq!(
            deploy_board::ENTITY_SCOPE_WHERE,
            "deleted_at IS NULL",
            "partial unique index covers live rows only"
        );
    }

    #[test]
    fn project_family_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/project_lite.columns.json"
        ))
        .unwrap();
        let models = &fixture["models"];
        for (key, table, columns, ordering) in [
            (
                "Project",
                project::TABLE,
                project::COLUMNS,
                project::ORDERING,
            ),
            (
                "ProjectMember",
                project_member::TABLE,
                project_member::COLUMNS,
                project_member::ORDERING,
            ),
            (
                "ProjectPublicMember",
                project_public_member::TABLE,
                project_public_member::COLUMNS,
                project_public_member::ORDERING,
            ),
            (
                "WorkspaceMember",
                workspace_member::TABLE,
                workspace_member::COLUMNS,
                workspace_member::ORDERING,
            ),
            (
                "User_identity",
                user_identity::TABLE,
                user_identity::COLUMNS,
                user_identity::ORDERING,
            ),
        ] {
            let model = &models[key];
            assert_eq!(model["db_table"].as_str().unwrap(), table, "{key}");
            assert_eq!(strings(&model["ordering"]), ordering, "{key}");
            let expected: Vec<String> = columns.iter().map(ToString::to_string).collect();
            assert_eq!(column_names(&model["columns"]), expected, "{key}");
        }
        assert_eq!(
            strings(&models["Project"]["unique_together"]),
            project::UNIQUE_TOGETHER,
            "Project",
        );
    }

    #[test]
    fn issue_graph_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/issue_public.columns.json"
        ))
        .unwrap();
        let models = &fixture["models"];
        for (key, table, columns, ordering) in [
            ("Issue", issue::TABLE, issue::COLUMNS, issue::ORDERING),
            (
                "IssueLink",
                issue_link::TABLE,
                issue_link::COLUMNS,
                issue_link::ORDERING,
            ),
            (
                "IssueRelation",
                issue_relation::TABLE,
                issue_relation::COLUMNS,
                issue_relation::ORDERING,
            ),
        ] {
            let model = &models[key];
            assert_eq!(model["db_table"].as_str().unwrap(), table, "{key}");
            assert_eq!(strings(&model["ordering"]), ordering, "{key}");
            let expected: Vec<String> = columns.iter().map(ToString::to_string).collect();
            assert_eq!(column_names(&model["columns"]), expected, "{key}");
        }
        assert_eq!(
            strings(&models["IssueLink"]["unique_together"]),
            issue_link::UNIQUE_TOGETHER,
            "IssueLink",
        );
        assert_eq!(
            strings(&models["IssueRelation"]["unique_together"]),
            issue_relation::UNIQUE_TOGETHER,
            "IssueRelation",
        );
        assert_eq!(issue::PRIORITY_DEFAULT, "none");
        assert_eq!(issue_relation::RELATION_TYPE_DEFAULT, "blocked_by");
    }

    #[test]
    fn comment_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/comment.columns.json"
        ))
        .unwrap();
        assert_eq!(fixture["db_table"].as_str().unwrap(), issue_comment::TABLE);
        assert_eq!(strings(&fixture["ordering"]), issue_comment::ORDERING);
        let expected: Vec<String> = issue_comment::COLUMNS
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(column_names(&fixture["columns"]), expected);
        assert_eq!(issue_comment::ACCESS_DEFAULT, "INTERNAL");
    }

    #[test]
    fn reaction_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/reaction.columns.json"
        ))
        .unwrap();
        let models = &fixture["models"];
        for (key, table, columns, ordering, unique_together) in [
            (
                "IssueReaction",
                issue_reaction::TABLE,
                issue_reaction::COLUMNS,
                issue_reaction::ORDERING,
                issue_reaction::UNIQUE_TOGETHER,
            ),
            (
                "CommentReaction",
                comment_reaction::TABLE,
                comment_reaction::COLUMNS,
                comment_reaction::ORDERING,
                comment_reaction::UNIQUE_TOGETHER,
            ),
        ] {
            let model = &models[key];
            assert_eq!(model["db_table"].as_str().unwrap(), table, "{key}");
            assert_eq!(strings(&model["ordering"]), ordering, "{key}");
            assert_eq!(strings(&model["unique_together"]), unique_together, "{key}");
            let expected: Vec<String> = columns.iter().map(ToString::to_string).collect();
            assert_eq!(column_names(&model["columns"]), expected, "{key}");
        }
    }

    #[test]
    fn vote_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/vote.columns.json"
        ))
        .unwrap();
        assert_eq!(fixture["db_table"].as_str().unwrap(), issue_vote::TABLE);
        assert_eq!(strings(&fixture["ordering"]), issue_vote::ORDERING);
        assert_eq!(
            strings(&fixture["unique_together"]),
            issue_vote::UNIQUE_TOGETHER
        );
        let expected: Vec<String> = issue_vote::COLUMNS
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(column_names(&fixture["columns"]), expected);
        assert_eq!(issue_vote::VOTE_DEFAULT, 1);
    }

    #[test]
    fn intake_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/intake.columns.json"
        ))
        .unwrap();
        assert_eq!(fixture["db_table"].as_str().unwrap(), intake_issue::TABLE);
        assert_eq!(strings(&fixture["ordering"]), intake_issue::ORDERING);
        let expected: Vec<String> = intake_issue::COLUMNS
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(column_names(&fixture["columns"]), expected);
        assert_eq!(intake_issue::STATUS_DEFAULT, -2);
        assert_eq!(intake_issue::SOURCE_DEFAULT, "IN_APP");
        assert_eq!(
            [
                intake_issue::status::PENDING,
                intake_issue::status::REJECTED,
                intake_issue::status::SNOOZED,
                intake_issue::status::ACCEPTED,
                intake_issue::status::DUPLICATE,
            ],
            [-2, -1, 0, 1, 2],
        );
    }

    #[test]
    fn asset_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/asset.columns.json"
        ))
        .unwrap();
        assert_eq!(fixture["db_table"].as_str().unwrap(), file_asset::TABLE);
        assert_eq!(strings(&fixture["ordering"]), file_asset::ORDERING);
        let expected: Vec<String> = file_asset::COLUMNS
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(column_names(&fixture["columns"]), expected);
        assert_eq!(file_asset::ENTITY_TYPES.len(), 10);
        for entity_type in file_asset::SPACE_READ_ENTITY_TYPES {
            assert!(
                file_asset::ENTITY_TYPES.contains(entity_type),
                "space-read entity type {entity_type} must be a known EntityTypeContext value"
            );
        }
    }

    #[test]
    fn cycle_module_state_label_columns_match_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/space/models/cycle_module_state_label.columns.json"
        ))
        .unwrap();
        let models = &fixture["models"];
        for (key, table, columns, ordering) in [
            ("Cycle", cycle::TABLE, cycle::COLUMNS, cycle::ORDERING),
            (
                "CycleIssue",
                cycle_issue::TABLE,
                cycle_issue::COLUMNS,
                cycle_issue::ORDERING,
            ),
            ("Module", module::TABLE, module::COLUMNS, module::ORDERING),
            (
                "ModuleIssue",
                module_issue::TABLE,
                module_issue::COLUMNS,
                module_issue::ORDERING,
            ),
            ("State", state::TABLE, state::COLUMNS, state::ORDERING),
            ("Label", label::TABLE, label::COLUMNS, label::ORDERING),
        ] {
            let model = &models[key];
            assert_eq!(model["db_table"].as_str().unwrap(), table, "{key}");
            assert_eq!(strings(&model["ordering"]), ordering, "{key}");
            let expected: Vec<String> = columns.iter().map(ToString::to_string).collect();
            assert_eq!(column_names(&model["columns"]), expected, "{key}");
        }
        assert_eq!(
            strings(&models["CycleIssue"]["unique_together"]),
            cycle_issue::UNIQUE_TOGETHER,
            "CycleIssue",
        );
        assert_eq!(
            strings(&models["Module"]["unique_together"]),
            module::UNIQUE_TOGETHER,
            "Module",
        );
        assert_eq!(
            strings(&models["ModuleIssue"]["unique_together"]),
            module_issue::UNIQUE_TOGETHER,
            "ModuleIssue",
        );
        assert_eq!(
            strings(&models["State"]["unique_together"]),
            state::UNIQUE_TOGETHER,
            "State",
        );
        assert_eq!(
            strings_or_empty(&models["Label"]["unique_together"]),
            label::UNIQUE_TOGETHER,
            "Label",
        );
        // States order by sequence, every other space table by -created_at.
        assert_eq!(state::ORDERING, &["sequence"]);
    }

    #[test]
    fn live_rows_scope_filters_soft_deleted() {
        assert_eq!(
            select_where("issues", live_rows_scope()),
            r#"SELECT "id" FROM "issues" WHERE "deleted_at" IS NULL"#
        );
    }

    #[test]
    fn unscoped_scope_has_no_where_clause() {
        // An empty `Condition::all()` renders as `WHERE TRUE`: no row is
        // filtered, so soft-deleted rows stay visible — the `all_objects`
        // semantics (`db/mixins.py:67`).
        assert_eq!(
            select_where("file_assets", unscoped_scope()),
            r#"SELECT "id" FROM "file_assets" WHERE TRUE"#
        );
    }

    #[test]
    fn issue_objects_scope_excludes_triage_archived_draft() {
        assert_eq!(
            select_where("issues", issue_objects_scope()),
            r#"SELECT "id" FROM "issues" WHERE "deleted_at" IS NULL AND "archived_at" IS NULL AND "is_draft" = FALSE AND "states"."group" <> 'triage'"#
        );
    }

    #[test]
    fn state_scopes_split_on_triage_group() {
        assert_eq!(
            select_where("states", state_default_scope()),
            r#"SELECT "id" FROM "states" WHERE "deleted_at" IS NULL AND "group" <> 'triage'"#
        );
        assert_eq!(
            select_where("states", state_triage_scope()),
            r#"SELECT "id" FROM "states" WHERE "deleted_at" IS NULL AND "group" = 'triage'"#
        );
        assert_eq!(STATE_GROUP_TRIAGE, "triage");
    }
}
