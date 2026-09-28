//! Space public single-issue retrieve query (R1).
//!
//! Port of `IssueRetrievePublicEndpoint.get`
//! (`apps/api/pi_dash/space/views/issue.py:597-773`, `AllowAny` at `:595`).
//! Fixture record `rust-api/fixtures/space/queries/issue_retrieve.{sql,rows.json}`
//! (filed by PIDASHCONV-135; trace: `space/views/issue.py:597-773`).
//!
//! The endpoint is one closure in two steps:
//!
//! 1. Board lookup: `DeployBoard.objects.get(anchor=anchor)` (`:598`) — a
//!    `.get()` with NO `entity_name` scoping, so a board of any entity type
//!    with that anchor satisfies it. This is neither the project-scoped
//!    settings get nor the `.filter().first()` closure in
//!    [`super::project_meta`]; [`retrieve_board_get_sql`] pins the exact
//!    shape. Bad anchor raises `DoesNotExist`, which the dispatch path turns
//!    into the 500 documented in `views/base.py:199-200` (same `return exc`
//!    bug the guards layer records) — there is no `{"error": ...}` 404 here,
//!    unlike the list endpoint (`issue.py:80-82`).
//! 2. Single-issue queryset (`:600-771`): `Issue.issue_objects.filter(pk,
//!    workspace__slug, project_id)` with `select_related(workspace, project,
//!    state, parent)` (`:606`), `prefetch_related(assignees, labels,
//!    issue_module__module)` (`:607`), the `cycle_id` subquery (`:608-612`),
//!    the three `Coalesce`+`ArrayAgg` id-list annotations (`:613-644`), the
//!    two `Prefetch`es with `select_related` (`:645-651`), the
//!    `vote_items`/`reaction_items` `ArrayAgg(Case/When/JSONObject)`
//!    annotations (`:652-745`), the 23-key `.values(...)` list (`:746-770`),
//!    and `.first()` (`:771`) whose `None` renders as `Response(None)` 200
//!    (`:773`) — see [`empty_body`].
//!
//! Conventions (same as [`super::project_meta`], which owns the shared
//! board-column list this module reuses):
//!
//! * Builders return the SQL text with Postgres `$N` placeholders in first-
//!   appearance order (Django renders `%(name)s`; same binding order).
//!   `$1` = issue id, `$2` = workspace slug (resolved from the already-read
//!   board row's `workspace.slug`), `$3` = project id (the board row's
//!   `project_id`). Execution belongs to the handlers layer
//!   (PIDASHCONV-175), which binds the documented `$N` params in order and
//!   maps the `.first()` outcome (`Some` row / `None` → [`empty_body`]).
//! * `.get()` board reads omit the `ORDER BY ... LIMIT 21` Django's `get()`
//!   adds: `anchor` is globally unique (`db/models/deploy_board.py:32`), so
//!   ordering/limit cannot change the row (or the 0/1 outcome); the caller
//!   maps row counts onto `DoesNotExist` / `MultipleObjectsReturned`.
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings; rows keep them as `String`. UUID and FK keys render as strings.
//! * The fixture abbreviates the `FROM`/join block and the manager-scope
//!   conjuncts as prose, so the `#[cfg(test)]` suite pins the recorded
//!   fragments inside the builder output (the `assert_builder_contains`
//!   direction from `intake_assets.rs`), never a full-statement containment.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-reaction-avatar (`issue.py:713,716,722`): the `reaction_items`
//!   `avatar_url` inner `When`s read `votes__actor__avatar_asset` /
//!   `votes__actor__avatar` (copy-pasted from `vote_items` at `:665-677`)
//!   instead of `issue_reactions__actor__avatar_asset` /
//!   `issue_reactions__actor__avatar`. [`issue_retrieve_sql`] keeps the
//!   wrong `votes` refs verbatim; the fixture records them at
//!   `issue_retrieve.sql:80-85`.
//! * QUIRK-unscoped-board-get (`issue.py:598`): the retrieve board lookup is
//!   `.get(anchor=anchor)` with NO `entity_name="project"` scoping. Kept.
//! * QUIRK-active-assignee (`issue.py:622-633`): `assignee_ids` requires an
//!   ACTIVE `ProjectMember` row (`assignees__member_project__is_active`);
//!   soft-deleted through rows excluded. Kept.
//! * QUIRK-archived-module (`issue.py:634-643`): `module_ids` excludes
//!   archived modules (`module__archived_at__isnull`) — stricter than the
//!   grouper default annotation, which only guards null. Kept.
//! * QUIRK-first-null (`issue.py:771-773`): a miss is `.first()` → `None`,
//!   returned as `Response(None)` 200, not a 404. [`empty_body`] pins the
//!   `"null"` body; handlers own the status.

use serde::{Deserialize, Serialize};

use super::project_meta::BOARD_COLUMNS;

// ---------------------------------------------------------------------------
// R1 board get (unscoped .get)
// ---------------------------------------------------------------------------

/// Board lookup: `DeployBoard.objects.get(anchor=anchor)`
/// (`views/issue.py:598`). Full `deploy_boards` row (same
/// [`BOARD_COLUMNS`](super::project_meta::BOARD_COLUMNS) order as every
/// other board read); deliberately NO `entity_name` conjunct
/// (QUIRK-unscoped-board-get above). `$1` = anchor.
pub fn retrieve_board_get_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1)",
        BOARD_COLUMNS
            .iter()
            .map(|col| format!("\"deploy_boards\".\"{col}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

// ---------------------------------------------------------------------------
// R1 single-issue queryset
// ---------------------------------------------------------------------------

/// The 23 `.values(...)` keys in verbatim view order
/// (`views/issue.py:746-770`). [`IssueRetrieveRow`] serializes in this
/// order, so `serde_json::to_string` is byte-identical to the fixture row.
pub const RETRIEVE_VALUES_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "created_by",
    "state__group",
    "vote_items",
    "reaction_items",
];

/// `label_ids` annotation (`issue.py:613-621`): distinct label ids whose
/// through (`issue_labels`) row is live; `Coalesce(..., [])` so the key is
/// `[]`, never null.
fn label_ids_sql() -> String {
    "COALESCE(ARRAY_AGG(DISTINCT \"labels\".\"id\") FILTER (WHERE NOT (\"labels\".\"id\" IS NULL) AND \"label_through\".\"deleted_at\" IS NULL), '{}') AS \"label_ids\"".to_string()
}

/// `assignee_ids` annotation (`issue.py:622-633`): distinct assignee ids
/// requiring an ACTIVE `project_members` row plus a live
/// (`issue_assignees`) through row; `Coalesce(..., [])`. Django joins
/// `project_members` on the member FK alone (no project scoping), same as
/// the merged `app_issues` precedent; kept.
fn assignee_ids_sql() -> String {
    "COALESCE(ARRAY_AGG(DISTINCT \"assignees\".\"id\") FILTER (WHERE NOT (\"assignees\".\"id\" IS NULL) AND \"project_members\".\"is_active\" = true AND \"assignee_through\".\"deleted_at\" IS NULL), '{}') AS \"assignee_ids\"".to_string()
}

/// `module_ids` annotation (`issue.py:634-643`): distinct module ids
/// excluding archived modules and soft-deleted (`issue_modules`) through
/// rows; `Coalesce(..., [])`.
fn module_ids_sql() -> String {
    "COALESCE(ARRAY_AGG(DISTINCT \"modules\".\"id\") FILTER (WHERE NOT (\"modules\".\"id\" IS NULL) AND \"modules\".\"archived_at\" IS NULL AND \"module_through\".\"deleted_at\" IS NULL), '{}') AS \"module_ids\"".to_string()
}

/// `cycle_id` subquery annotation (`issue.py:608-612`):
/// `CycleIssue.objects.filter(issue=OuterRef("id"),
/// deleted_at__isnull=True).values("cycle_id")[:1]`.
fn cycle_id_sql() -> String {
    "(SELECT U0.\"cycle_id\" FROM \"cycle_issues\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"issue_id\" = (\"issues\".\"id\")) LIMIT 1) AS \"cycle_id\"".to_string()
}

/// `vote_items` annotation (`issue.py:652-698`): one
/// `{"vote", "actor_details"}` object per live vote; empty set aggregates
/// to NULL (no `Coalesce` — the null shape is the contract).
fn vote_items_sql() -> String {
    "ARRAY_AGG(DISTINCT (CASE WHEN (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL) THEN JSON_BUILD_OBJECT('vote', \"votes\".\"vote\", 'actor_details', JSON_BUILD_OBJECT('id', \"vote_actor\".\"id\", 'first_name', \"vote_actor\".\"first_name\", 'last_name', \"vote_actor\".\"last_name\", 'avatar', \"vote_actor\".\"avatar\", 'avatar_url', (CASE WHEN (\"vote_actor\".\"avatar_asset_id\" IS NOT NULL) THEN CONCAT('/api/assets/v2/static/', \"vote_actor\".\"avatar_asset_id\", '/') WHEN (\"vote_actor\".\"avatar_asset_id\" IS NULL) THEN \"vote_actor\".\"avatar\" ELSE NULL END), 'display_name', \"vote_actor\".\"display_name\") ) ELSE NULL END)) FILTER (WHERE CASE WHEN (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL) THEN true ELSE false END) AS \"vote_items\"".to_string()
}

/// `reaction_items` annotation (`issue.py:699-744`): one
/// `{"reaction", "actor_details"}` object per live reaction; empty set
/// aggregates to NULL (no `Coalesce`).
///
/// BUG-reaction-avatar (`issue.py:713,716,722`): the `avatar_url` inner
/// `When`s read the VOTE actor columns (`votes__actor__*`) instead of
/// `issue_reactions__actor__*`. The wrong `votes` refs below are the port,
/// not a typo.
fn reaction_items_sql() -> String {
    "ARRAY_AGG(DISTINCT (CASE WHEN (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL) THEN JSON_BUILD_OBJECT('reaction', \"issue_reactions\".\"reaction\", 'actor_details', JSON_BUILD_OBJECT('id', \"reaction_actor\".\"id\", 'first_name', \"reaction_actor\".\"first_name\", 'last_name', \"reaction_actor\".\"last_name\", 'avatar', \"reaction_actor\".\"avatar\", 'avatar_url', (CASE WHEN (\"votes\".\"actor_avatar_asset\" IS NOT NULL) THEN CONCAT('/api/assets/v2/static/', \"votes\".\"actor_avatar_asset\", '/') WHEN (\"votes\".\"actor_avatar_asset\" IS NULL) THEN \"votes\".\"actor_avatar\" ELSE NULL END), 'display_name', \"reaction_actor\".\"display_name\") ) ELSE NULL END)) FILTER (WHERE CASE WHEN (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL) THEN true ELSE false END) AS \"reaction_items\"".to_string()
}

/// R1 single-issue read (`views/issue.py:600-771`).
///
/// `Issue.issue_objects` (manager scope: live rows, non-triage state,
/// non-archived, non-draft — `db/models/issue.py:95-104`; the `states` and
/// `projects` halves join those tables, per `db/src/space/columns.rs`)
/// `.filter(pk, workspace__slug, project_id)` with the `select_related`
/// joins (`:606`), the through-table joins behind the three id-list
/// annotations, and the actor joins behind the vote/reaction aggregates
/// (mirroring the `Prefetch(...select_related("issue","actor"))` at
/// `:645-651`). `GROUP BY "issues"."id", "states"."group"` with
/// `LIMIT 1` is the `.first()` at `:771`.
///
/// `$1` = issue id, `$2` = workspace slug, `$3` = project id.
pub fn issue_retrieve_sql() -> String {
    format!(
        "SELECT \"issues\".\"id\", \"issues\".\"name\", \"issues\".\"state_id\", \"issues\".\"sort_order\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"description_stripped\", \"issues\".\"description_binary\", {module_ids}, {label_ids}, {assignee_ids}, \"issues\".\"estimate_point_id\" AS \"estimate_point\", \"issues\".\"priority\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"project_id\", \"issues\".\"parent_id\", {cycle_id}, \"issues\".\"created_by_id\" AS \"created_by\", \"states\".\"group\" AS \"state__group\", {vote_items}, {reaction_items} FROM \"issues\" INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") LEFT OUTER JOIN \"issues\" T_parent ON (\"issues\".\"parent_id\" = T_parent.\"id\") LEFT OUTER JOIN \"issue_labels\" \"label_through\" ON (\"issues\".\"id\" = \"label_through\".\"issue_id\") LEFT OUTER JOIN \"labels\" ON (\"label_through\".\"label_id\" = \"labels\".\"id\") LEFT OUTER JOIN \"issue_assignees\" \"assignee_through\" ON (\"issues\".\"id\" = \"assignee_through\".\"issue_id\") LEFT OUTER JOIN \"users\" \"assignees\" ON (\"assignee_through\".\"assignee_id\" = \"assignees\".\"id\") LEFT OUTER JOIN \"project_members\" ON (\"assignees\".\"id\" = \"project_members\".\"member_id\") LEFT OUTER JOIN \"module_issues\" \"module_through\" ON (\"issues\".\"id\" = \"module_through\".\"issue_id\") LEFT OUTER JOIN \"modules\" ON (\"module_through\".\"module_id\" = \"modules\".\"id\") LEFT OUTER JOIN \"issue_votes\" \"votes\" ON (\"issues\".\"id\" = \"votes\".\"issue_id\") LEFT OUTER JOIN \"users\" \"vote_actor\" ON (\"votes\".\"actor_id\" = \"vote_actor\".\"id\") LEFT OUTER JOIN \"issue_reactions\" ON (\"issues\".\"id\" = \"issue_reactions\".\"issue_id\") LEFT OUTER JOIN \"users\" \"reaction_actor\" ON (\"issue_reactions\".\"actor_id\" = \"reaction_actor\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND \"states\".\"group\" != 'triage' AND \"issues\".\"archived_at\" IS NULL AND \"projects\".\"archived_at\" IS NULL AND \"issues\".\"is_draft\" = false AND \"issues\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 AND \"issues\".\"project_id\" = $3) GROUP BY \"issues\".\"id\", \"states\".\"group\" LIMIT 1",
        module_ids = module_ids_sql(),
        label_ids = label_ids_sql(),
        assignee_ids = assignee_ids_sql(),
        cycle_id = cycle_id_sql(),
        vote_items = vote_items_sql(),
        reaction_items = reaction_items_sql(),
    )
}

/// Body for the `.first()` miss (`views/issue.py:771-773`): the queryset
/// is `None`, and `Response(None)` renders JSON `null` with status 200 —
/// not a 404. Handlers own the status; this pins the body bytes.
pub fn empty_body() -> String {
    "null".to_string()
}

// ---------------------------------------------------------------------------
// R1 row shapes (23 .values() keys, verbatim order)
// ---------------------------------------------------------------------------

/// `actor_details` object inside vote/reaction items
/// (`views/issue.py:659-677,705-723`): 6 keys in `JSONObject` order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrieveActorDetails {
    /// `actor.id`.
    pub id: String,
    /// `actor.first_name`.
    pub first_name: String,
    /// `actor.last_name`.
    pub last_name: String,
    /// `actor.avatar`.
    pub avatar: String,
    /// `avatar_url`: `/api/assets/v2/static/<asset>/` when the actor has
    /// an avatar asset, else the raw `avatar` value, else null.
    pub avatar_url: Option<String>,
    /// `actor.display_name`.
    pub display_name: String,
}

/// One `vote_items` element (`issue.py:657-680`): `{"vote", "actor_details"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrieveVoteItem {
    /// `votes.vote`.
    pub vote: i32,
    /// Actor snapshot (6 keys).
    pub actor_details: RetrieveActorDetails,
}

/// One `reaction_items` element (`issue.py:702-725`):
/// `{"reaction", "actor_details"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrieveReactionItem {
    /// `issue_reactions.reaction`.
    pub reaction: String,
    /// Actor snapshot (6 keys).
    pub actor_details: RetrieveActorDetails,
}

/// One R1 `.values(...)` row: all 23 keys in verbatim view order
/// (`issue.py:746-770`; [`RETRIEVE_VALUES_FIELDS`]). `vote_items` /
/// `reaction_items` are `None` when the issue has no live votes/reactions
/// (the `ArrayAgg` has no `Coalesce`); the id lists are `[]`, never null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueRetrieveRow {
    /// `id` (`:747`).
    pub id: String,
    /// `name` (`:748`).
    pub name: String,
    /// `state_id` (`:749`).
    pub state_id: Option<String>,
    /// `sort_order` (`:750`, float).
    pub sort_order: f64,
    /// `description_json` (`:751`).
    pub description_json: serde_json::Value,
    /// `description_html` (`:752`).
    pub description_html: Option<String>,
    /// `description_stripped` (`:753`).
    pub description_stripped: Option<String>,
    /// `description_binary` (`:754`).
    pub description_binary: Option<String>,
    /// `module_ids` (`:755`).
    pub module_ids: Vec<String>,
    /// `label_ids` (`:756`).
    pub label_ids: Vec<String>,
    /// `assignee_ids` (`:757`).
    pub assignee_ids: Vec<String>,
    /// `estimate_point` = `estimate_point_id` (`:758`).
    pub estimate_point: Option<String>,
    /// `priority` (`:759`).
    pub priority: Option<String>,
    /// `start_date` (`:760`).
    pub start_date: Option<String>,
    /// `target_date` (`:761`).
    pub target_date: Option<String>,
    /// `sequence_id` (`:762`).
    pub sequence_id: i64,
    /// `project_id` (`:763`).
    pub project_id: String,
    /// `parent_id` (`:764`).
    pub parent_id: Option<String>,
    /// `cycle_id` subquery annotation (`:765`).
    pub cycle_id: Option<String>,
    /// `created_by` = `created_by_id` (`:766`).
    pub created_by: Option<String>,
    /// `state__group` (`:767`).
    #[serde(rename = "state__group")]
    pub state_group: Option<String>,
    /// `vote_items` (`:768`; null when empty).
    pub vote_items: Option<Vec<RetrieveVoteItem>>,
    /// `reaction_items` (`:769`; null when empty).
    pub reaction_items: Option<Vec<RetrieveReactionItem>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SQL: &str =
        include_str!("../../../../../fixtures/space/queries/issue_retrieve.sql");
    const FIXTURE_ROWS: &str =
        include_str!("../../../../../fixtures/space/queries/issue_retrieve.rows.json");

    /// Collapse every whitespace run to one space (Django's compiler wraps
    /// lines; the text is what matters).
    fn squashed(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Render a builder's `$N` placeholders Django-style (`%(name)s`, as the
    /// fixture records them) for a check against the fixture.
    fn with_named_params(sql: &str, names: &[&str]) -> String {
        let mut out = sql.to_string();
        for (i, name) in names.iter().enumerate().rev() {
            out = out.replace(&format!("${}", i + 1), &format!("%({name})s"));
        }
        out
    }

    /// The fixture's concrete fragment (Django `%()s` style) must appear
    /// verbatim in the builder's output. Use because the fixture abbreviates
    /// the statement (prose `FROM`/`WHERE`) while the builder emits
    /// fully-qualified executable SQL.
    fn assert_builder_contains(builder_sql: &str, params: &[&str], fragment: &str) {
        let hay = squashed(&with_named_params(builder_sql, params));
        let needle = squashed(fragment);
        assert!(
            hay.contains(&needle),
            "fixture fragment not found in builder SQL:\n{needle}\n----\n{hay}"
        );
    }

    /// A fixture fragment identified by line for a readable failure.
    fn fixture_fragment(needle: &str) -> String {
        let hay = squashed(FIXTURE_SQL);
        let needle = squashed(needle);
        assert!(
            hay.contains(&needle),
            "fragment missing from fixture:\n{needle}"
        );
        needle
    }

    #[test]
    fn board_get_is_unscoped_without_first() {
        let sql = retrieve_board_get_sql();
        // Same full board row as every other board read ...
        for col in BOARD_COLUMNS {
            assert!(
                sql.contains(&format!("\"deploy_boards\".\"{col}\"")),
                "{col}"
            );
        }
        // ... but a .get() on anchor alone: no entity_name scoping ...
        assert!(
            !sql.contains("\"entity_name\" = "),
            "retrieve board get takes any entity type (issue.py:598)"
        );
        // ... and no .first() ordering/limit.
        assert!(!sql.contains("ORDER BY"), "get() adds no ordering");
        assert!(!sql.contains("LIMIT"), "get() adds no limit");
        assert!(
            sql.ends_with("WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1)"),
            "{sql}"
        );
    }

    #[test]
    fn values_list_has_23_keys_in_view_order() {
        assert_eq!(RETRIEVE_VALUES_FIELDS.len(), 23);
        assert_eq!(
            RETRIEVE_VALUES_FIELDS,
            &[
                "id",
                "name",
                "state_id",
                "sort_order",
                "description_json",
                "description_html",
                "description_stripped",
                "description_binary",
                "module_ids",
                "label_ids",
                "assignee_ids",
                "estimate_point",
                "priority",
                "start_date",
                "target_date",
                "sequence_id",
                "project_id",
                "parent_id",
                "cycle_id",
                "created_by",
                "state__group",
                "vote_items",
                "reaction_items",
            ]
        );
        // Row struct serializes in the same order (serde emits declaration
        // order): covered by retrieve_row_byte_matches_fixture below.
    }

    #[test]
    fn id_list_annotations_match_fixture_guards() {
        let sql = issue_retrieve_sql();
        let params = &["issue_id", "slug", "project_id"];
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("COALESCE(ARRAY_AGG(DISTINCT \"labels\".\"id\") FILTER (WHERE NOT (\"labels\".\"id\" IS NULL) AND \"label_through\".\"deleted_at\" IS NULL), '{}') AS \"label_ids\""),
        );
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("COALESCE(ARRAY_AGG(DISTINCT \"assignees\".\"id\") FILTER (WHERE NOT (\"assignees\".\"id\" IS NULL) AND \"project_members\".\"is_active\" = true AND \"assignee_through\".\"deleted_at\" IS NULL), '{}') AS \"assignee_ids\""),
        );
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("COALESCE(ARRAY_AGG(DISTINCT \"modules\".\"id\") FILTER (WHERE NOT (\"modules\".\"id\" IS NULL) AND \"modules\".\"archived_at\" IS NULL AND \"module_through\".\"deleted_at\" IS NULL), '{}') AS \"module_ids\""),
        );
    }

    #[test]
    fn cycle_subquery_matches_fixture() {
        assert_builder_contains(
            &issue_retrieve_sql(),
            &["issue_id", "slug", "project_id"],
            &fixture_fragment("(SELECT U0.\"cycle_id\" FROM \"cycle_issues\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"issue_id\" = (\"issues\".\"id\")) LIMIT 1) AS \"cycle_id\""),
        );
    }

    #[test]
    fn vote_items_match_fixture() {
        let sql = issue_retrieve_sql();
        let params = &["issue_id", "slug", "project_id"];
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("JSON_BUILD_OBJECT('vote', \"votes\".\"vote\""),
        );
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment(
                "'avatar_url', (CASE WHEN (\"vote_actor\".\"avatar_asset_id\" IS NOT NULL) THEN CONCAT('/api/assets/v2/static/', \"vote_actor\".\"avatar_asset_id\", '/') WHEN (\"vote_actor\".\"avatar_asset_id\" IS NULL) THEN \"vote_actor\".\"avatar\" ELSE NULL END)",
            ),
        );
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("FILTER (WHERE CASE WHEN (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL) THEN true ELSE false END) AS \"vote_items\""),
        );
    }

    #[test]
    fn reaction_items_port_the_avatar_copy_paste_bug() {
        let sql = issue_retrieve_sql();
        let params = &["issue_id", "slug", "project_id"];
        // Reaction identity still reads the reaction table (fixture
        // `issue_retrieve.sql:74-75` wraps the line after the paren; the
        // squashed text keeps that space, so pin the space-free builder
        // text plus the fixture's key fragment separately).
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("'reaction', \"issue_reactions\".\"reaction\""),
        );
        assert!(
            squashed(&sql)
                .contains("JSON_BUILD_OBJECT('reaction', \"issue_reactions\".\"reaction\""),
            "reaction identity"
        );
        // ... but the avatar_url branches read the VOTE actor columns
        // (issue.py:713,716,722) instead of issue_reactions__actor__*.
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment(
                "'avatar_url', (CASE WHEN (\"votes\".\"actor_avatar_asset\" IS NOT NULL) THEN CONCAT('/api/assets/v2/static/', \"votes\".\"actor_avatar_asset\", '/') WHEN (\"votes\".\"actor_avatar_asset\" IS NULL) THEN \"votes\".\"actor_avatar\" ELSE NULL END)",
            ),
        );
        // The builder must NOT contain a corrected reaction-actor avatar ref.
        assert!(
            !sql.contains("reaction_actor\".\"avatar_asset"),
            "bug port: no corrected avatar_asset ref in reaction branch"
        );
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("FILTER (WHERE CASE WHEN (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL) THEN true ELSE false END) AS \"reaction_items\""),
        );
    }

    #[test]
    fn select_aliases_and_where_match_fixture() {
        let sql = issue_retrieve_sql();
        let params = &["issue_id", "slug", "project_id"];
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment("\"issues\".\"estimate_point_id\" AS \"estimate_point\""),
        );
        assert_builder_contains(
            &sql,
            params,
            &fixture_fragment(
                "\"issues\".\"created_by_id\" AS \"created_by\", \"states\".\"group\" AS \"state__group\"",
            ),
        );
        // Fixture WHERE literals (issue_retrieve.sql:90-92; the manager
        // exclusions are prose there, pinned as builder text below).
        for frag in [
            "\"issues\".\"deleted_at\" IS NULL",
            "\"issues\".\"id\" = %(issue_id)s",
            "\"workspaces\".\"slug\" = %(slug)s",
            "\"issues\".\"project_id\" = %(project_id)s",
        ] {
            assert_builder_contains(&sql, params, &fixture_fragment(frag));
        }
        // Manager scope (IssueManager, db/models/issue.py:95-104; same
        // halves as db::space::issue_objects_scope plus the projects join
        // half the columns layer documents as queries-owned).
        let flat = squashed(&sql);
        for frag in [
            "\"states\".\"group\" != 'triage'",
            "\"issues\".\"archived_at\" IS NULL",
            "\"projects\".\"archived_at\" IS NULL",
            "\"issues\".\"is_draft\" = false",
        ] {
            assert!(flat.contains(&squashed(frag)), "manager scope: {frag}");
        }
        // `.first()` (:771): single-row close.
        assert!(
            squashed(&sql).ends_with("GROUP BY \"issues\".\"id\", \"states\".\"group\" LIMIT 1"),
            "first() close"
        );
    }

    #[test]
    fn through_joins_use_real_table_and_member_join_shape() {
        // The fixture abbreviates the FROM block as prose, so these
        // builder-owned joins need their own pins. `ModuleIssue` lives in
        // `module_issues` (`db/models/module.py`); `issue_modules` exists
        // nowhere and would fail at execution with 42P01.
        let flat = squashed(&issue_retrieve_sql());
        assert!(
            flat.contains("LEFT OUTER JOIN \"module_issues\" \"module_through\""),
            "module through table"
        );
        assert!(
            !flat.contains("issue_modules"),
            "no invented issue_modules relation"
        );
        // Django joins project_members on the member FK alone
        // (`assignees__member_project`, no project scoping), same as the
        // merged app_issues precedent.
        assert!(
            flat.contains(
                "LEFT OUTER JOIN \"project_members\" ON (\"assignees\".\"id\" = \"project_members\".\"member_id\")"
            ),
            "unscoped member join"
        );
    }

    fn fixture_rows() -> serde_json::Value {
        serde_json::from_str(FIXTURE_ROWS).expect("rows fixture parses")
    }

    #[test]
    fn retrieve_row_byte_matches_fixture() {
        let row = IssueRetrieveRow {
            id: "99999999-9999-9999-9999-999999999999".to_string(),
            name: "Login broken".to_string(),
            state_id: Some("44444444-4444-4444-4444-444444444444".to_string()),
            sort_order: 65535.0,
            description_json: serde_json::json!({}),
            description_html: Some("<p>details</p>".to_string()),
            description_stripped: Some("details".to_string()),
            description_binary: None,
            module_ids: vec![],
            label_ids: vec!["77777777-7777-7777-7777-777777777777".to_string()],
            assignee_ids: vec!["11111111-1111-1111-1111-111111111111".to_string()],
            estimate_point: None,
            priority: Some("high".to_string()),
            start_date: None,
            target_date: Some("2026-10-01".to_string()),
            sequence_id: 42,
            project_id: "33333333-3333-3333-3333-333333333333".to_string(),
            parent_id: None,
            cycle_id: Some("55555555-5555-5555-5555-555555555555".to_string()),
            created_by: Some("11111111-1111-1111-1111-111111111111".to_string()),
            state_group: Some("started".to_string()),
            vote_items: Some(vec![RetrieveVoteItem {
                vote: 1,
                actor_details: RetrieveActorDetails {
                    id: "11111111-1111-1111-1111-111111111111".to_string(),
                    first_name: "Ada".to_string(),
                    last_name: "L".to_string(),
                    avatar: String::new(),
                    avatar_url: Some(String::new()),
                    display_name: "Ada L".to_string(),
                },
            }]),
            reaction_items: Some(vec![RetrieveReactionItem {
                reaction: "+1".to_string(),
                actor_details: RetrieveActorDetails {
                    id: "11111111-1111-1111-1111-111111111111".to_string(),
                    first_name: "Ada".to_string(),
                    last_name: "L".to_string(),
                    avatar: String::new(),
                    avatar_url: Some(String::new()),
                    display_name: "Ada L".to_string(),
                },
            }]),
        };
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            fixture_rows()["rows"][0]
        );
        // Key order is the contract too: serde emits declaration order, so
        // the first and last keys pin the 23-key verbatim sequence.
        let text = serde_json::to_string(&row).unwrap();
        assert!(text
            .starts_with(r#"{"id":"99999999-9999-9999-9999-999999999999","name":"Login broken""#));
        assert!(text.contains(r#""state__group":"started","vote_items":[{"vote":1"#));
        assert!(text.contains(r#""reaction_items":[{"reaction":"+1","actor_details":{"id":"11111111-1111-1111-1111-111111111111"#));
        assert!(text.ends_with(r#""display_name":"Ada L"}}]}"#));
    }

    #[test]
    fn empty_vote_reaction_aggregates_are_null_and_miss_is_null_body() {
        // No Coalesce on vote_items/reaction_items (issue.py:652-744): an
        // issue with no live votes/reactions aggregates to NULL, while the
        // id lists stay [].
        let row = IssueRetrieveRow {
            id: "99999999-9999-9999-9999-999999999999".to_string(),
            name: "Login broken".to_string(),
            state_id: Some("44444444-4444-4444-4444-444444444444".to_string()),
            sort_order: 65535.0,
            description_json: serde_json::json!({}),
            description_html: Some("<p>details</p>".to_string()),
            description_stripped: Some("details".to_string()),
            description_binary: None,
            module_ids: vec![],
            label_ids: vec![],
            assignee_ids: vec![],
            estimate_point: None,
            priority: Some("high".to_string()),
            start_date: None,
            target_date: None,
            sequence_id: 42,
            project_id: "33333333-3333-3333-3333-333333333333".to_string(),
            parent_id: None,
            cycle_id: None,
            created_by: Some("11111111-1111-1111-1111-111111111111".to_string()),
            state_group: Some("started".to_string()),
            vote_items: None,
            reaction_items: None,
        };
        let text = serde_json::to_string(&row).unwrap();
        assert!(text.contains(r#""module_ids":[],"label_ids":[]"#));
        assert!(text.contains(r#""vote_items":null,"reaction_items":null"#));
        // `.first()` miss (:771) renders Response(None) 200 (:773).
        assert_eq!(empty_body(), "null");
        assert_eq!(
            serde_json::to_value::<Option<IssueRetrieveRow>>(None)
                .unwrap_or(serde_json::Value::Null)
                .to_string(),
            empty_body()
        );
    }
}
