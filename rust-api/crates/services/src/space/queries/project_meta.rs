//! Space project/meta read queries: settings, boards, anchor, members, meta,
//! cycles, modules, states, labels.
//!
//! Port of `apps/api/pi_dash/space/views/{project,meta,cycle,module,state,label}.py`
//! (fixture record `rust-api/fixtures/space/queries/project_meta.{sql,rows.json}`,
//! filed by PIDASHCONV-135; trace: `space/views/project.py:19-86`,
//! `space/views/meta.py:16-32`, `space/views/cycle.py:15-28`,
//! `space/views/module.py:15-28`, `space/views/state.py:18-32`,
//! `space/views/label.py:15-28`). Seven small read queries plus the one
//! shared closure every taxonomy/member read funnels through: the unscoped
//! `DeployBoard.objects.filter(anchor=...).first()` board lookup.
//!
//! Conventions (match the fixture, which is the contract):
//!
//! * Builders return the SQL text with Postgres `$N` placeholders in first-
//!   appearance order (Django renders `%s` / `%(name)s`; same binding order).
//!   The `$N` params of each builder are documented on the builder.
//! * `.get()` single-row reads omit the `ORDER BY ... LIMIT 21` Django's
//!   `get()` adds: `anchor` is globally unique (`deploy_board.py:32`) and
//!   `Project.id` is the PK, so ordering/limit cannot change the row (or the
//!   0/1 outcome); the caller maps row counts onto `DoesNotExist` /
//!   `MultipleObjectsReturned`. For the anchor-get (M3) the combo is not
//!   unique, so the caller must still treat `>1` rows as
//!   `MultipleObjectsReturned`.
//! * `workspace__slug` filters take the board row's resolved `workspace_id`
//!   (and `project_id`) as params instead of re-joining `workspaces`: the
//!   board lookup already read that exact row, so the value is identical to
//!   what the slug join would match — this is the fixture's shape (M4/M6/M8/M9).
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings (same rule as `super::serializers::lite`); rows keep them as
//!   `String`. UUID and FK keys render as strings.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-boards (`project.py:32`): `deploy_board =
//!   DeployBoard.objects.filter(...).values_list` WITHOUT the call —
//!   `deploy_board` is the bound method, so `deploy_board.workspace` (`:34`)
//!   raises `AttributeError` and the endpoint ALWAYS 500s (via the dispatch
//!   `return exc` bug, `views/base.py:199-200`). [`workspace_boards_sql`]
//!   always returns [`BoardsError::ValuesListNotCalled`];
//!   [`workspace_boards_intended_sql`] records the SQL Django would have
//!   emitted, for the follow-up fix only.
//! * QUIRK-unscoped-board (`cycle.py:19`, `module.py:19`, and the same shape
//!   at `project.py:69`, `state.py:23`, `label.py:21`): the taxonomy/member
//!   board lookup is `.filter(anchor=...)` with NO `entity_name="project"`
//!   scoping — a board of any entity type with that anchor satisfies it.
//!   [`board_by_anchor_first_sql`] ports that as-is.
//! * BUG-triage-name (`state.py:27`, fixture B13): triage is excluded by
//!   NAME (`~Q(name="Triage")`), not by the `group` flag / `is_triage`
//!   column. Both predicates are kept: the name exclusion from the view plus
//!   the `group != 'triage'` half that `StateManager.get_queryset`
//!   (`db/models/state.py:75-79`) adds to every `State.objects` query.
//!   (Django canonically renders the manager half as `NOT (group =
//!   'triage')`; the fixture records the equivalent `!=` form.)

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DeployBoard reads (M1 settings get, M3 anchor get, board-first closure)
// ---------------------------------------------------------------------------

/// Full `deploy_boards` column list in Django field order
/// (`db/models/deploy_board.py:19-57` + audit/base cols; fixture
/// `models/deployboard.columns.json`). Shared by every board-row read.
pub const BOARD_COLUMNS: &[&str] = &[
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

/// Quote a column list for `"deploy_boards"`, in order.
fn board_select_list() -> String {
    BOARD_COLUMNS
        .iter()
        .map(|col| format!("\"deploy_boards\".\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// M1 settings get: `DeployBoard.objects.get(anchor=anchor,
/// entity_name="project")` (`views/project.py:22-25`; same call shape at
/// `views/meta.py:21` for the meta get's first half). Serialized with
/// `DeployBoardSerializer` (handlers layer). `$1` = anchor.
pub fn settings_get_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1 AND \"deploy_boards\".\"entity_name\" = 'project')",
        board_select_list()
    )
}

/// M3 anchor get: `DeployBoard.objects.get(workspace__slug=slug,
/// project_id=project_id, entity_name="project")` (`views/project.py:57-62`).
/// The `workspace__slug` lookup is Django's `INNER JOIN "workspaces" ... WHERE
/// "workspaces"."slug" = ...` (no `deleted_at` guard on the joined table —
/// forward-FK joins never apply the related manager). `$1` = workspace slug,
/// `$2` = project id.
pub fn anchor_get_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" INNER JOIN \"workspaces\" ON (\"deploy_boards\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $1 AND \"deploy_boards\".\"project_id\" = $2 AND \"deploy_boards\".\"entity_name\" = 'project')",
        board_select_list()
    )
}

/// Shared closure: `DeployBoard.objects.filter(anchor=anchor).first()`
/// (`views/project.py:69`, `views/cycle.py:19`, `views/module.py:19`,
/// `views/state.py:23`, `views/label.py:21`). `.first()` keeps the default
/// `Meta.ordering = ("-created_at",)` (`deploy_board.py:57`) with `LIMIT 1`.
/// Deliberately NO `entity_name` scoping (QUIRK-unscoped-board above).
/// `$1` = anchor.
pub fn board_by_anchor_first_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1) ORDER BY \"deploy_boards\".\"created_at\" DESC LIMIT 1",
        board_select_list()
    )
}

/// One `deploy_boards` row. Nullable per `models/deployboard.columns.json`
/// (`project_id`, `entity_identifier`, `entity_name`, `intake_id`,
/// audit cols, `deleted_at`); datetimes are pre-rendered strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardRow {
    /// `id` UUID PK (`db/models/base.py:17-18`).
    pub id: String,
    /// `created_at` (`db/mixins.py:16-20`, `auto_now_add`).
    pub created_at: String,
    /// `updated_at` (`auto_now`).
    pub updated_at: String,
    /// `created_by_id` FK `users` `SET_NULL` (`db/mixins.py:26-42`, via crum).
    pub created_by_id: Option<String>,
    /// `updated_by_id` FK `users` `SET_NULL`.
    pub updated_by_id: Option<String>,
    /// `deleted_at` (`db/mixins.py:61-64`).
    pub deleted_at: Option<String>,
    /// `workspace_id` FK `workspaces` (`workspace.py:185-195`).
    pub workspace_id: String,
    /// `project_id` FK `projects`, nullable (`WorkspaceBaseModel`).
    pub project_id: Option<String>,
    /// `entity_identifier` UUID, nullable (`deploy_board.py:29`).
    pub entity_identifier: Option<String>,
    /// `entity_name`, nullable (`deploy_board.py:30`).
    pub entity_name: Option<String>,
    /// `anchor` globally unique (`deploy_board.py:32`).
    pub anchor: String,
    /// `is_comments_enabled` (`deploy_board.py:33`).
    pub is_comments_enabled: bool,
    /// `is_reactions_enabled` (`deploy_board.py:34`).
    pub is_reactions_enabled: bool,
    /// `intake_id` FK, nullable (`deploy_board.py:35`).
    pub intake_id: Option<String>,
    /// `is_votes_enabled` (`deploy_board.py:36`).
    pub is_votes_enabled: bool,
    /// `view_props` jsonb (`deploy_board.py:37`).
    pub view_props: serde_json::Value,
    /// `is_activity_enabled` (`deploy_board.py:38`).
    pub is_activity_enabled: bool,
    /// `is_disabled` (`deploy_board.py:39`).
    pub is_disabled: bool,
}

// ---------------------------------------------------------------------------
// M2 workspace boards list (dead path: BUG-boards)
// ---------------------------------------------------------------------------

/// Failure modes of the workspace-boards read. There is exactly one: the
/// ported `values_list`-without-call bug (`views/project.py:32`), which
/// raises before any SQL runs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoardsError {
    /// BUG-boards (`project.py:32-34`): `...values_list` without the call
    /// binds the method, so `.workspace` raises `AttributeError`.
    #[error("BUG-boards (views/project.py:32): values_list bound without call; .workspace always raises AttributeError")]
    ValuesListNotCalled,
}

/// M2: `WorkspaceProjectDeployBoardEndpoint.get` (`views/project.py:28-51`).
/// NEVER returns SQL — the bug above raises first. Kept as a function (not a
/// constant) so the handlers layer calls the same shape as every other query
/// and maps the `Err` onto the dispatch-500 path. `_anchor` is accepted and
/// ignored to preserve the call shape.
pub fn workspace_boards_sql(_anchor: &str) -> Result<String, BoardsError> {
    Err(BoardsError::ValuesListNotCalled)
}

/// The SQL Django WOULD have emitted for M2 without the bug (fixture M2,
/// `views/project.py:33-49`): the 7-key project `.values(...)` with the
/// `Exists(... anchor + project_id + entity_name ...)` `is_public`
/// annotation, filtered to `is_public=True` within the board's workspace.
/// Documentation for the follow-up fix ONLY — never executed. `$1` = anchor
/// (bound twice: annotation subquery + outer `EXISTS`), `$2` = workspace id.
pub fn workspace_boards_intended_sql() -> String {
    "SELECT \"projects\".\"id\", \"projects\".\"identifier\", \"projects\".\"name\", \"projects\".\"description\", \"projects\".\"emoji\", \"projects\".\"icon_prop\", \"projects\".\"cover_image\", EXISTS(SELECT 1 AS \"a\" FROM \"deploy_boards\" U1 WHERE (U1.\"deleted_at\" IS NULL AND U1.\"anchor\" = $1 AND U1.\"project_id\" = (\"projects\".\"id\") AND U1.\"entity_name\" = 'project')) AS \"is_public\" FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"workspace_id\" = $2 AND EXISTS(SELECT 1 AS \"a\" FROM \"deploy_boards\" U2 WHERE (U2.\"deleted_at\" IS NULL AND U2.\"anchor\" = $1 AND U2.\"project_id\" = (\"projects\".\"id\") AND U2.\"entity_name\" = 'project')))".to_string()
}

/// One row of the intended M2 `.values("id", "identifier", "name",
/// "description", "emoji", "icon_prop", "cover_image")` list plus the
/// `is_public` annotation (`views/project.py:35-49`). Unreachable until the
/// bug is fixed; kept so the fix's row shape is already pinned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceBoardRow {
    /// `id` (`project.py:42`).
    pub id: String,
    /// `identifier` (`:43`).
    pub identifier: String,
    /// `name` (`:44`).
    pub name: String,
    /// `description` (`:45`, non-null text).
    pub description: String,
    /// `emoji` (`:46`, nullable).
    pub emoji: Option<String>,
    /// `icon_prop` (`:47`, nullable jsonb).
    pub icon_prop: Option<serde_json::Value>,
    /// `cover_image` (`:48`, nullable).
    pub cover_image: Option<String>,
    /// `is_public` `Exists` annotation (`:35-39`).
    pub is_public: bool,
}

// ---------------------------------------------------------------------------
// M4 members
// ---------------------------------------------------------------------------

/// M4: `ProjectMember.objects.filter(project, workspace,
/// is_active=True).values("id", "member", "member__display_name",
/// "member__avatar")` verbatim (`views/project.py:76-85`). `member` FK is
/// nullable (`project.py:333-339`), hence `LEFT OUTER JOIN`. No guard on
/// `users.deleted_at` — forward joins never apply the related manager, and
/// `User` (`user.py:128`) is not soft-deletable anyway. Default ordering
/// `Meta.ordering = ("-created_at",)` (`project.py:378`). `$1` = project id,
/// `$2` = workspace id (both from the already-read board row).
pub fn members_sql() -> String {
    "SELECT \"project_members\".\"id\", \"project_members\".\"member_id\" AS \"member\", \"users\".\"display_name\" AS \"member__display_name\", \"users\".\"avatar\" AS \"member__avatar\" FROM \"project_members\" LEFT OUTER JOIN \"users\" ON (\"project_members\".\"member_id\" = \"users\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"project_id\" = $1 AND \"project_members\".\"workspace_id\" = $2 AND \"project_members\".\"is_active\" = true) ORDER BY \"project_members\".\"created_at\" DESC".to_string()
}

/// One M4 `.values(...)` row, keys in view order (`project.py:80-85`).
/// The `member__*` join cols are `Option` (null when `member` is null).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemberRow {
    /// `id`.
    pub id: String,
    /// `member` = `member_id` (`project.py:82`).
    pub member: Option<String>,
    /// `member__display_name` (`users.display_name`, `user.py:64`).
    #[serde(rename = "member__display_name")]
    pub member_display_name: Option<String>,
    /// `member__avatar` (`users.avatar`, `user.py:68`).
    #[serde(rename = "member__avatar")]
    pub member_avatar: Option<String>,
}

// ---------------------------------------------------------------------------
// M5 meta project get
// ---------------------------------------------------------------------------

/// Full `projects` column list in Django field order
/// (`db/models/project.py:72-253`; fixture `models/project_lite.columns.json`).
pub const PROJECT_COLUMNS: &[&str] = &[
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

/// M5 second half: `Project.objects.get(id=project_id)` where `project_id =
/// deploy_board.entity_identifier` (`views/meta.py:26-27`). `Project` has no
/// custom manager, so `objects` is the inherited `SoftDeletionManager`
/// (`db/mixins.py:66-67`): `deleted_at IS NULL`. Serialized with
/// `ProjectLiteSerializer` (handlers layer, leaves in `super::serializers`).
/// `$1` = `entity_identifier` from the M1-shaped board get. Either
/// `DoesNotExist` (board or project) maps to `{"error": "Project is not
/// published"}` 404 (`meta.py:23,29`; bodies belong to the guards layer,
/// PIDASHCONV-172).
pub fn project_get_sql() -> String {
    let cols = PROJECT_COLUMNS
        .iter()
        .map(|col| format!("\"projects\".\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {cols} FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1)"
    )
}

// ---------------------------------------------------------------------------
// M6 cycles / M7 modules
// ---------------------------------------------------------------------------

/// M6: `Cycle.objects.filter(workspace__slug,
/// project_id).values("id", "name")` verbatim (`views/cycle.py:23-26`).
/// Default ordering `Meta.ordering = ("-created_at",)` (`cycle.py:86`).
/// `$1` = workspace id, `$2` = project id (board row).
pub fn cycles_sql() -> String {
    "SELECT \"cycles\".\"id\", \"cycles\".\"name\" FROM \"cycles\" WHERE (\"cycles\".\"deleted_at\" IS NULL AND \"cycles\".\"workspace_id\" = $1 AND \"cycles\".\"project_id\" = $2) ORDER BY \"cycles\".\"created_at\" DESC".to_string()
}

/// M7: same shape on `"modules"` (`views/module.py:23-26`; ordering
/// `module.py:113`). `$1` = workspace id, `$2` = project id.
pub fn modules_sql() -> String {
    "SELECT \"modules\".\"id\", \"modules\".\"name\" FROM \"modules\" WHERE (\"modules\".\"deleted_at\" IS NULL AND \"modules\".\"workspace_id\" = $1 AND \"modules\".\"project_id\" = $2) ORDER BY \"modules\".\"created_at\" DESC".to_string()
}

/// One M6/M7 `.values("id", "name")` row, keys in view order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaxonomyRow {
    /// `id`.
    pub id: String,
    /// `name`.
    pub name: String,
}

// ---------------------------------------------------------------------------
// M8 states
// ---------------------------------------------------------------------------

/// M8: `State.objects.filter(~Q(name="Triage"), workspace__slug,
/// project_id).values("name", "group", "color", "id", "sequence")` — key
/// order verbatim (`views/state.py:26-30`). `State.objects` =
/// `StateManager`, which pre-excludes `group="triage"`
/// (`db/models/state.py:75-79`); `Meta.ordering = ("sequence",)`
/// (`state.py:129`) is overridden here by nothing, so `sequence ASC` stands.
/// `$1` = workspace id, `$2` = project id.
pub fn states_sql() -> String {
    "SELECT \"states\".\"name\", \"states\".\"group\", \"states\".\"color\", \"states\".\"id\", \"states\".\"sequence\" FROM \"states\" WHERE (NOT (\"states\".\"name\" = 'Triage') AND \"states\".\"deleted_at\" IS NULL AND \"states\".\"workspace_id\" = $1 AND \"states\".\"project_id\" = $2 AND \"states\".\"group\" != 'triage') ORDER BY \"states\".\"sequence\" ASC".to_string()
}

/// One M8 `.values(...)` row, keys in view order (`state.py:30`).
/// `sequence` is a float (`FloatField`, `state.py:97`); DRF renders it as a
/// JSON number (`1000.0` in the fixture).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateRow {
    /// `name`.
    pub name: String,
    /// `group` (`StateGroup` value, `state.py:16-27`).
    pub group: String,
    /// `color`.
    pub color: String,
    /// `id`.
    pub id: String,
    /// `sequence`.
    pub sequence: f64,
}

// ---------------------------------------------------------------------------
// M9 labels
// ---------------------------------------------------------------------------

/// M9: `Label.objects.filter(workspace__slug,
/// project_id).values("id", "name", "color", "parent")` verbatim
/// (`views/label.py:23-26`). `parent` is the self-FK column rendered as
/// `parent_id AS "parent"`. Default ordering `Meta.ordering =
/// ("-created_at",)` (`label.py:44`). `$1` = workspace id, `$2` = project id.
pub fn labels_sql() -> String {
    "SELECT \"labels\".\"id\", \"labels\".\"name\", \"labels\".\"color\", \"labels\".\"parent_id\" AS \"parent\" FROM \"labels\" WHERE (\"labels\".\"deleted_at\" IS NULL AND \"labels\".\"workspace_id\" = $1 AND \"labels\".\"project_id\" = $2) ORDER BY \"labels\".\"created_at\" DESC".to_string()
}

/// One M9 `.values(...)` row, keys in view order (`label.py:26`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelRow {
    /// `id`.
    pub id: String,
    /// `name`.
    pub name: String,
    /// `color`.
    pub color: String,
    /// `parent` = `parent_id` (self-FK, nullable, `label.py:11-16`).
    pub parent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded Django shapes, committed by the fixture issue (PIDASHCONV-135).
    const FIXTURE_SQL: &str =
        include_str!("../../../../../fixtures/space/queries/project_meta.sql");
    /// Representative rows for the `.values(...)` reads.
    const FIXTURE_ROWS: &str =
        include_str!("../../../../../fixtures/space/queries/project_meta.rows.json");

    /// Collapse every whitespace run to one space (Django's compiler wraps
    /// lines; the text is what matters).
    fn squashed(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Render a builder's `$N` placeholders Django-style (`%(name)s`, as the
    /// fixture records them) for a containment check against the fixture.
    fn with_named_params(sql: &str, names: &[&str]) -> String {
        let mut out = sql.to_string();
        for (i, name) in names.iter().enumerate().rev() {
            out = out.replace(&format!("${}", i + 1), &format!("%({name})s"));
        }
        out
    }

    /// The builder's Django-shaped text must appear verbatim in the fixture
    /// (whitespace-insensitive): emitted SQL identical to Django's.
    fn assert_in_fixture(sql: &str, params: &[&str]) {
        let haystack = squashed(FIXTURE_SQL);
        let needle = squashed(&with_named_params(sql, params));
        assert!(
            haystack.contains(&needle),
            "builder SQL not found in fixture:\n{needle}"
        );
    }

    #[test]
    fn settings_get_matches_fixture_m1() {
        assert_in_fixture(&settings_get_sql(), &["anchor"]);
        assert_eq!(BOARD_COLUMNS.len(), 18);
    }

    #[test]
    fn anchor_get_selects_full_board_row_with_workspace_join() {
        let sql = anchor_get_sql();
        assert!(sql.contains(
            "FROM \"deploy_boards\" INNER JOIN \"workspaces\" ON (\"deploy_boards\".\"workspace_id\" = \"workspaces\".\"id\")"
        ));
        assert!(sql.contains(
            "WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $1 AND \"deploy_boards\".\"project_id\" = $2 AND \"deploy_boards\".\"entity_name\" = 'project')"
        ));
        for col in BOARD_COLUMNS {
            assert!(
                sql.contains(&format!("\"deploy_boards\".\"{col}\"")),
                "{col}"
            );
        }
    }

    #[test]
    fn board_first_closure_is_unscoped_with_default_ordering() {
        let sql = board_by_anchor_first_sql();
        assert!(
            !sql.contains("\"entity_name\" = "),
            "closure takes any entity type"
        );
        assert!(sql.contains(
            "WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1) ORDER BY \"deploy_boards\".\"created_at\" DESC LIMIT 1"
        ));
    }

    #[test]
    fn workspace_boards_is_a_dead_path() {
        assert_eq!(
            workspace_boards_sql("pub-abc123"),
            Err(BoardsError::ValuesListNotCalled)
        );
        // The intended shape stays pinned for the follow-up fix: 7 project
        // keys + is_public EXISTS over anchor + project_id + entity_name.
        let intended = workspace_boards_intended_sql();
        for frag in [
            "\"projects\".\"cover_image\",",
            "AS \"is_public\"",
            "U1.\"anchor\" = $1",
            "U1.\"project_id\" = (\"projects\".\"id\")",
            "U1.\"entity_name\" = 'project'",
            "\"projects\".\"workspace_id\" = $2",
        ] {
            assert!(intended.contains(frag), "{frag}");
        }
        let fixture = squashed(FIXTURE_SQL);
        for frag in [
            "AS \"is_public\"",
            "U1.\"anchor\" = %(anchor)s",
            "\"projects\".\"workspace_id\" = %(workspace_id)s",
        ] {
            assert!(fixture.contains(frag), "fixture: {frag}");
        }
    }

    #[test]
    fn members_matches_fixture_m4() {
        assert_in_fixture(&members_sql(), &["project_id", "workspace_id"]);
    }

    #[test]
    fn project_get_selects_full_project_row() {
        let sql = project_get_sql();
        assert_eq!(PROJECT_COLUMNS.len(), 46);
        assert!(sql.contains("\"projects\".\"members_can_edit_states\""));
        assert!(sql.ends_with(
            "FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1)"
        ));
    }

    #[test]
    fn cycles_matches_fixture_m6() {
        assert_in_fixture(&cycles_sql(), &["workspace_id", "project_id"]);
    }

    #[test]
    fn modules_matches_fixture_m7_shape() {
        // Fixture M7 is prose ("same shape on modules"); pin the exact text
        // against the M6 shape with the table swapped.
        assert_eq!(
            modules_sql(),
            cycles_sql().replace("\"cycles\"", "\"modules\"")
        );
    }

    #[test]
    fn states_matches_fixture_m8() {
        assert_in_fixture(&states_sql(), &["workspace_id", "project_id"]);
    }

    #[test]
    fn labels_matches_fixture_m9() {
        assert_in_fixture(&labels_sql(), &["workspace_id", "project_id"]);
    }

    fn fixture_rows() -> serde_json::Value {
        serde_json::from_str(FIXTURE_ROWS).expect("rows fixture parses")
    }

    #[test]
    fn member_row_byte_matches_fixture() {
        let row = MemberRow {
            id: "m1".to_string(),
            member: Some("11111111-1111-1111-1111-111111111111".to_string()),
            member_display_name: Some("Ada L".to_string()),
            member_avatar: Some(String::new()),
        };
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"{"id":"m1","member":"11111111-1111-1111-1111-111111111111","member__display_name":"Ada L","member__avatar":""}"#
        );
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            fixture_rows()["rows"]["M4_members"][0]
        );
        // Nullable member FK: no user row, the whole join side is null
        // (`project.py:333-339`; `LEFT OUTER JOIN`).
        let orphan = MemberRow {
            id: "m2".to_string(),
            member: None,
            member_display_name: None,
            member_avatar: None,
        };
        assert_eq!(
            serde_json::to_string(&orphan).unwrap(),
            r#"{"id":"m2","member":null,"member__display_name":null,"member__avatar":null}"#
        );
    }

    #[test]
    fn cycle_row_byte_matches_fixture() {
        let row = TaxonomyRow {
            id: "55555555-5555-5555-5555-555555555555".to_string(),
            name: "Sprint 3".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"{"id":"55555555-5555-5555-5555-555555555555","name":"Sprint 3"}"#
        );
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            fixture_rows()["rows"]["M6_cycles"][0]
        );
    }

    #[test]
    fn state_row_byte_matches_fixture() {
        let row = StateRow {
            name: "In Progress".to_string(),
            group: "started".to_string(),
            color: "#ff0000".to_string(),
            id: "44444444-4444-4444-4444-444444444444".to_string(),
            sequence: 1000.0,
        };
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r##"{"name":"In Progress","group":"started","color":"#ff0000","id":"44444444-4444-4444-4444-444444444444","sequence":1000.0}"##
        );
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            fixture_rows()["rows"]["M8_states"][0]
        );
    }

    #[test]
    fn label_row_byte_matches_fixture() {
        let row = LabelRow {
            id: "77777777-7777-7777-7777-777777777777".to_string(),
            name: "bug".to_string(),
            color: "#00ff00".to_string(),
            parent: None,
        };
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r##"{"id":"77777777-7777-7777-7777-777777777777","name":"bug","color":"#00ff00","parent":null}"##
        );
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            fixture_rows()["rows"]["M9_labels"][0]
        );
    }
}
