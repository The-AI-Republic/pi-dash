//! Space social read queries: comments, issue reactions, comment reactions, votes.
//!
//! Port of the four `get_queryset` methods in
//! `apps/api/pi_dash/space/views/issue.py`
//! (`IssueCommentPublicViewSet`, `IssueReactionPublicViewSet`,
//! `CommentReactionPublicViewSet`, `IssueVotePublicViewSet`).
//! Fixture record
//! `rust-api/fixtures/space/queries/comments_reactions_votes.{sql,rows.json}`
//! (filed by PIDASHCONV-135; trace line in
//! `rust-api/fixtures/space/TRACE.md`).
//!
//! Conventions (same as [`super::project_meta`] and
//! [`super::intake_assets`]):
//!
//! * Builders return the SQL text with Postgres `$N` placeholders in first-
//!   appearance order (Django renders `%s` / `%(name)s`; same binding order).
//!   Execution belongs to the handlers layer (PIDASHCONV-176), which binds
//!   the documented `$N` params in order and maps row counts onto the
//!   `get()` contract (`0 -> DoesNotExist`, `>1 -> MultipleObjectsReturned`).
//! * `.get()` single-row board reads omit the `ORDER BY ... LIMIT 21`
//!   Django's `get()` adds: the scoped lookups below are unique by
//!   construction, so ordering/limit cannot change the row (or the 0/1
//!   outcome); the caller still treats `>1` rows as
//!   `MultipleObjectsReturned`.
//! * `select_related("project")` / `("workspace")` / `("issue")`
//!   (`views/issue.py:236-238`) adds no top-level predicate: the joined
//!   columns feed the serializer detail dicts (pinned by the `C1_comment`
//!   row in the `.rows.json` fixture), same rule as the intake list query.
//!   `.distinct()` likewise emits no `DISTINCT` — there are no multi-valued
//!   joins at the top level, only the `Exists` subquery.
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings; rows keep them as `String`. UUID and FK keys render as strings.
//! * Error bodies and status codes belong to the guards layer, not here; the
//!   Python line for each is cited so handlers wire the same mapping.
//! * Comment/reaction/vote *writes* (create/partial_update/destroy,
//!   `views/issue.py:257-339,366-427,451-519,543-591`) are handler work
//!   (PIDASHCONV-176); only their read/query halves are pinned here.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-reaction-wrong-kwarg (`views/issue.py:348-351`): the
//!   issue-reaction board lookup filters on `workspace__slug` and
//!   `project_id` kwargs the space routes never supply
//!   (`space/urls/issue.py` gives `anchor`/`issue_id` only), so both params
//!   are `NULL` and the lookup always misses. [`issue_reaction_dead_board_get_sql`]
//!   ports that as-is (text-identical to the intake dead-board get); the
//!   would-be list filter is [`issue_reaction_dead_list_sql`]. The list can
//!   therefore only serve `.none()`.
//! * BUG-vote-anchor-as-slug (`views/issue.py:528-530`): the vote board
//!   lookup passes the `anchor` URL value to the `workspace__slug` field.
//!   Unless a workspace slug literally equals an anchor the lookup misses.
//!   [`vote_dead_board_get_sql`] ports that as-is; the would-be list filter
//!   is [`vote_list_sql`]. The list can therefore only serve `.none()` in
//!   practice.
//! * QUIRK-anonymous-member (`views/issue.py:241-250`): `request.user.id` is
//!   evaluated even for the `AllowAny` anonymous list, so `$3` binds `NULL`
//!   and the `is_member` annotation is silently false. Kept.
//! * QUIRK-comment-order (`views/issue.py:252`): the comment list orders by
//!   ascending `created_at`, overriding the model's `-created_at` default
//!   (`db/models/issue.py:649-662`). Reaction lists order descending
//!   (`:359,:444`); the vote list has no ordering (`:526-541`). All kept.

use super::project_meta::BOARD_COLUMNS;

// ---------------------------------------------------------------------------
// Board closures
// ---------------------------------------------------------------------------

/// Social board get: `DeployBoard.objects.get(anchor=anchor,`
/// `entity_name="project")` (`views/issue.py:230,436,367-intended,544-intended`;
/// same call as the intake board get). `$1` = anchor.
///
/// Text-identical to [`intake_board_get_sql`]; pinned by equality below.
pub fn social_board_get_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1 AND \"deploy_boards\".\"entity_name\" = 'project')",
        BOARD_COLUMNS
            .iter()
            .map(|col| format!("\"deploy_boards\".\"{col}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Disabled-board / missing-board empty result: `IssueComment.objects.none()`
/// (`views/issue.py:253`) and `DeployBoard.DoesNotExist -> .none()`
/// (`:254-255`) — note: no 404 here, unlike the token-gated endpoints.
pub fn comment_none_sql() -> String {
    "SELECT \"issue_comments\".* FROM \"issue_comments\" WHERE (0 = 1)".to_string()
}

/// Disabled-board / missing-board empty result for issue reactions
/// (`views/issue.py:362-364`).
pub fn issue_reaction_none_sql() -> String {
    "SELECT \"issue_reactions\".* FROM \"issue_reactions\" WHERE (0 = 1)".to_string()
}

/// Disabled-board / missing-board empty result for comment reactions
/// (`views/issue.py:447-449`).
pub fn comment_reaction_none_sql() -> String {
    "SELECT \"comment_reactions\".* FROM \"comment_reactions\" WHERE (0 = 1)".to_string()
}

/// Disabled-board / missing-board empty result for votes
/// (`views/issue.py:539-541`).
pub fn vote_none_sql() -> String {
    "SELECT \"issue_votes\".* FROM \"issue_votes\" WHERE (0 = 1)".to_string()
}

// ---------------------------------------------------------------------------
// C1 comment list
// ---------------------------------------------------------------------------

/// C1 comment list (`views/issue.py:231-252`): workspace + issue + `EXTERNAL`
/// filter with the `is_member` Exists annotation, ordered ascending.
///
/// `$1` = workspace id (board row), `$2` = project id (board row), `$3` =
/// requesting user id — `NULL` for anonymous `AllowAny` reads, which makes
/// the annotation silently false (QUIRK-anonymous-member) — `$4` = issue id.
/// `extra_predicate` is the rendered `filter_queryset` conjunct
/// (`filterset_fields = ["issue__id", "workspace__id"]`, `:218`); `None`
/// renders the unfiltered shape.
pub fn comment_list_sql(extra_predicate: Option<&str>) -> String {
    let where_core = "\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"workspace_id\" = $1 AND \"issue_comments\".\"issue_id\" = $4 AND \"issue_comments\".\"access\" = 'EXTERNAL'";
    let where_clause = match extra_predicate {
        Some(predicate) => format!("({where_core} AND ({predicate}))"),
        None => format!("({where_core})"),
    };
    format!(
        "SELECT \"issue_comments\".*, EXISTS(SELECT 1 FROM \"project_members\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"workspace_id\" = $1 AND U0.\"project_id\" = $2 AND U0.\"member_id\" = $3 AND U0.\"is_active\" = true)) AS \"is_member\" FROM \"issue_comments\" WHERE {where_clause} ORDER BY \"issue_comments\".\"created_at\" ASC"
    )
}

// ---------------------------------------------------------------------------
// C4 issue-reaction list (dead path)
// ---------------------------------------------------------------------------

/// C4 dead board get: `DeployBoard.objects.get(workspace__slug=None,`
/// `project_id=None)` (`views/issue.py:348-351`). Both kwargs are absent in
/// the space URLs, so Django renders both lookups as `IS NULL` and the row
/// count always maps to `DoesNotExist`. No params.
///
/// Text-identical to [`intake_board_get_sql`]'s dead twin; pinned by equality
/// below.
pub fn issue_reaction_dead_board_get_sql() -> String {
    "SELECT \"deploy_boards\".* FROM \"deploy_boards\" INNER JOIN \"workspaces\" ON (\"deploy_boards\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" IS NULL AND \"deploy_boards\".\"project_id\" IS NULL)".to_string()
}

/// C4 would-be list filter (`views/issue.py:353-361`): `workspace__slug=None`
/// (join + `IS NULL`), `project_id=None` (`IS NULL`), `issue_id = $1`,
/// ordered descending. Unreachable — the dead board get above always misses
/// first — so in practice this view only serves
/// [`issue_reaction_none_sql`]. `$1` = issue id.
pub fn issue_reaction_dead_list_sql() -> String {
    "SELECT \"issue_reactions\".* FROM \"issue_reactions\" INNER JOIN \"workspaces\" ON (\"issue_reactions\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"issue_reactions\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" IS NULL AND \"issue_reactions\".\"project_id\" IS NULL AND \"issue_reactions\".\"issue_id\" = $1) ORDER BY \"issue_reactions\".\"created_at\" DESC".to_string()
}

// ---------------------------------------------------------------------------
// C5 comment-reaction list (live path)
// ---------------------------------------------------------------------------

/// C5 comment-reaction list (`views/issue.py:437-446`): board workspace +
/// project + comment scoping, ordered descending. `$1` = workspace id (board
/// row), `$2` = project id (board row), `$3` = comment id.
pub fn comment_reaction_list_sql() -> String {
    "SELECT \"comment_reactions\".* FROM \"comment_reactions\" WHERE (\"comment_reactions\".\"deleted_at\" IS NULL AND \"comment_reactions\".\"workspace_id\" = $1 AND \"comment_reactions\".\"project_id\" = $2 AND \"comment_reactions\".\"comment_id\" = $3) ORDER BY \"comment_reactions\".\"created_at\" DESC".to_string()
}

// ---------------------------------------------------------------------------
// C6 vote list (dead path)
// ---------------------------------------------------------------------------

/// C6 dead board get: `DeployBoard.objects.get(workspace__slug=<anchor>,`
/// `entity_name="project")` (`views/issue.py:528-530`) — the anchor string is
/// passed to the `workspace__slug` field. `$1` = anchor URL value.
pub fn vote_dead_board_get_sql() -> String {
    "SELECT \"deploy_boards\".* FROM \"deploy_boards\" INNER JOIN \"workspaces\" ON (\"deploy_boards\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $1 AND \"deploy_boards\".\"entity_name\" = 'project')".to_string()
}

/// C6 would-be list filter (`views/issue.py:532-537`): issue + board
/// workspace + board project, no ordering, no distinct. Unreachable in
/// practice via the dead board get above; served shape when the board row is
/// resolved. `$1` = issue id, `$2` = workspace id (board row), `$3` =
/// project id (board row).
pub fn vote_list_sql() -> String {
    "SELECT \"issue_votes\".* FROM \"issue_votes\" WHERE (\"issue_votes\".\"deleted_at\" IS NULL AND \"issue_votes\".\"issue_id\" = $1 AND \"issue_votes\".\"workspace_id\" = $2 AND \"issue_votes\".\"project_id\" = $3)".to_string()
}

#[cfg(test)]
mod tests {
    use super::super::intake_assets::{intake_board_get_sql, intake_dead_board_get_sql};
    use super::*;

    const FIXTURE_SQL: &str =
        include_str!("../../../../../fixtures/space/queries/comments_reactions_votes.sql");
    const FIXTURE_ROWS: &str =
        include_str!("../../../../../fixtures/space/queries/comments_reactions_votes.rows.json");

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

    /// The fixture's concrete fragment (Django `%()s` style, abbreviated
    /// `U0.` aliases) must appear verbatim in the builder's output. Used
    /// throughout here because this fixture abbreviates column qualification
    /// while the builders emit fully-qualified executable SQL.
    fn assert_builder_contains(builder_sql: &str, params: &[&str], fragment: &str) {
        let hay = squashed(&with_named_params(builder_sql, params));
        let needle = squashed(fragment);
        assert!(
            hay.contains(&needle),
            "fixture fragment not found in builder SQL:\n{needle}\n----\n{hay}"
        );
    }

    #[test]
    fn fixture_documents_all_four_viewsets() {
        // Every builder below must have its fixture section: C1 (comment
        // list), C4 (issue-reaction dead path), C5 (comment-reaction list),
        // C6 (vote dead path). The `.rows.json` side is covered by
        // `rows_fixture_covers_comment_shape_and_dead_paths`.
        for marker in ["-- C1 ", "-- C4 ", "-- C5 ", "-- C6 "] {
            assert!(
                FIXTURE_SQL.contains(marker),
                "fixture section missing for builder group: {marker}"
            );
        }
    }

    #[test]
    fn social_board_get_matches_intake_board_get_shape() {
        // Same ORM call (`DeployBoard.objects.get(anchor, entity_name)`);
        // equality with the fixture-pinned intake builder is the contract.
        assert_eq!(social_board_get_sql(), intake_board_get_sql());
    }

    #[test]
    fn reaction_dead_board_get_matches_intake_dead_shape() {
        // Same wrong-kwarg call shape (`workspace__slug=None`,
        // `project_id=None`); equality with the intake dead-board builder is
        // the contract (`views/issue.py:348-351`, BUG-reaction-wrong-kwarg).
        assert_eq!(
            issue_reaction_dead_board_get_sql(),
            intake_dead_board_get_sql()
        );
    }

    #[test]
    fn comment_list_matches_fixture_c1() {
        let sql = comment_list_sql(None);
        // EXISTS annotation with the member scope ...
        assert_builder_contains(
            &sql,
            &["workspace_id", "project_id", "user_id_or_None", "issue_id"],
            "EXISTS(SELECT 1 FROM \"project_members\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"workspace_id\" = %(workspace_id)s AND U0.\"project_id\" = %(project_id)s AND U0.\"member_id\" = %(user_id_or_None)s AND U0.\"is_active\" = true)) AS \"is_member\"",
        );
        // ... top-level EXTERNAL scoping ...
        assert_builder_contains(
            &sql,
            &["workspace_id", "project_id", "user_id_or_None", "issue_id"],
            "AND \"issue_comments\".\"workspace_id\" = %(workspace_id)s AND \"issue_comments\".\"issue_id\" = %(issue_id)s AND \"issue_comments\".\"access\" = 'EXTERNAL') ORDER BY \"issue_comments\".\"created_at\" ASC",
        );
        // No DISTINCT: only the Exists subquery joins, nothing multi-valued.
        assert!(!squashed(&sql).contains("DISTINCT"));
    }

    #[test]
    fn comment_list_applies_extra_predicate() {
        let sql = comment_list_sql(Some("\"issue_comments\".\"issue_id\" = $4"));
        assert!(squashed(&sql).contains("AND (\"issue_comments\".\"issue_id\" = $4))"));
    }

    #[test]
    fn none_builders_serve_empty_results() {
        for sql in [
            comment_none_sql(),
            issue_reaction_none_sql(),
            comment_reaction_none_sql(),
            vote_none_sql(),
        ] {
            assert!(
                squashed(&sql).contains("WHERE (0 = 1)"),
                "none() must render the empty result: {sql}"
            );
        }
    }

    #[test]
    fn reaction_dead_list_renders_nulled_scope() {
        // The wrong-kwarg scope can only ever match nothing: both board
        // columns render IS NULL and only the issue id stays a param.
        let sql = issue_reaction_dead_list_sql();
        let hay = squashed(&with_named_params(&sql, &["issue_id"]));
        assert!(hay.contains("\"workspaces\".\"slug\" IS NULL"));
        assert!(hay.contains("\"issue_reactions\".\"project_id\" IS NULL"));
        assert!(hay.contains("\"issue_reactions\".\"issue_id\" = %(issue_id)s"));
        assert!(hay.contains("ORDER BY \"issue_reactions\".\"created_at\" DESC"));
    }

    #[test]
    fn comment_reaction_list_matches_fixture_c5() {
        let sql = comment_reaction_list_sql();
        assert_builder_contains(
            &sql,
            &["workspace_id", "project_id", "comment_id"],
            "WHERE (\"comment_reactions\".\"deleted_at\" IS NULL",
        );
        let hay = squashed(&with_named_params(
            &sql,
            &["workspace_id", "project_id", "comment_id"],
        ));
        assert!(hay.contains("\"comment_reactions\".\"workspace_id\" = %(workspace_id)s"));
        assert!(hay.contains("\"comment_reactions\".\"project_id\" = %(project_id)s"));
        assert!(hay.contains("\"comment_reactions\".\"comment_id\" = %(comment_id)s"));
        assert!(hay.contains("ORDER BY \"comment_reactions\".\"created_at\" DESC"));
    }

    #[test]
    fn vote_builders_match_fixture_c6() {
        // Dead board get passes the anchor value to the slug field ...
        let board = vote_dead_board_get_sql();
        let hay = squashed(&with_named_params(&board, &["anchor"]));
        assert!(hay.contains("\"workspaces\".\"slug\" = %(anchor)s"));
        assert!(hay.contains("\"deploy_boards\".\"entity_name\" = 'project'"));
        // ... and the would-be list has no ordering and no distinct.
        let list = vote_list_sql();
        let hay = squashed(&with_named_params(
            &list,
            &["issue_id", "workspace_id", "project_id"],
        ));
        assert!(hay.contains("\"issue_votes\".\"issue_id\" = %(issue_id)s"));
        assert!(hay.contains("\"issue_votes\".\"workspace_id\" = %(workspace_id)s"));
        assert!(hay.contains("\"issue_votes\".\"project_id\" = %(project_id)s"));
        assert!(!hay.contains("ORDER BY"));
        assert!(!hay.contains("DISTINCT"));
    }

    #[test]
    fn rows_fixture_covers_comment_shape_and_dead_paths() {
        let rows: serde_json::Value =
            serde_json::from_str(FIXTURE_ROWS).expect("rows fixture parses");
        // C1 representative comment: EXTERNAL access with member flag.
        let comment = &rows["rows"]["C1_comment"];
        assert_eq!(comment["access"].as_str().unwrap(), "EXTERNAL");
        assert!(comment["is_member"].is_boolean());
        // C4/C6 dead paths are recorded as empty-result notes, not rows.
        let empty = &rows["rows_when_empty"];
        assert!(empty["C4_reaction_list"].is_string());
        assert!(empty["C6_vote_list"].is_string());
    }
}
