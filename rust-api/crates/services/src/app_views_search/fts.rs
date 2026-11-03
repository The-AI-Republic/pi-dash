//! D-29 issue full-text search closure: SQL kernels and snippet rule.
//!
//! Port of `apps/api/pi_dash/search/issue.py` (`:36-207`): the one FTS
//! topological closure every search path composes —
//!
//! * `ISSUE_FTS_CONFIG` (`:36`) — the `'english'` config shared by the
//!   runtime vectors and both GIN indexes;
//! * `ISSUE_SEARCH_VECTOR` (`:55-57`) / `ISSUE_COMMENT_SEARCH_VECTOR`
//!   (`:61-63`) — [`issue_vector_sql`] / [`comment_vector_sql`]. Both must
//!   parse to the same expression tree as the index expressions or the
//!   planner silently drops the index (`:7-11`; parity pinned by
//!   `FX-FTS-CORE.sql` §7 and `test_fts_uses_issues_fts_idx`);
//! * `_matching_comment_issue_ids` (`:66-90`) — [`comment_arm_sql`];
//! * `_build_search_filter` (`:93-125`) — [`search_filter_sql`];
//! * `issue_search_queryset` (`:128-186`) — [`search_applies`],
//!   [`rank_sql`], [`headline_sql`];
//! * `extract_snippet` (`:189-198`) — [`extract_snippet`];
//! * `search_issues` (`:201-207`) — the wrapper contract is documented on
//!   [`search_filter_sql`] (`include_comments=False` + `DISTINCT`, owned by
//!   the handler).
//!
//! Tokenisation (`\b\d+\b`, 20-char query gate, 10-digit token gate, int4
//! cap) and `icontains` escaping live in
//! [`crate::assistant::tools_issues`] (`search_sequence_tokens`,
//! `escape_icontains`, `SEQUENCE_ID_MAX`) and are reused here, not forked:
//! the `#[test] shared_arms_match_canonical_predicate` below locks the
//! shared arms of [`search_filter_sql`] byte-for-byte to that module's
//! [`crate::assistant::tools_issues::ISSUE_FTS_SQL`].
//!
//! LIKE spelling: this crate renders `icontains` as
//! `… ILIKE '%' || <p> || '%' ESCAPE '\'` with the pattern bound through
//! `escape_icontains`. Django emits `UPPER(<col>::text) LIKE UPPER(…)` with
//! no `ESCAPE` clause (backslash is Postgres's default `LIKE` escape, and
//! the pattern carries the backslashes). The two spellings match the same
//! rows; the crate uses one spelling everywhere.
//!
//! SQL fragments take the bind placeholder (`$1`, …) as a parameter so the
//! handler's `SqlBuilder` can thread numbering; see `queries_search`.

/// Full-text config (`search/issue.py:36`).
pub const FTS_CONFIG: &str = "english";

/// Headline delimiters (`search/issue.py:51`).
pub const HEADLINE_START_SEL: &str = "<<";
/// Headline delimiters (`search/issue.py:52`).
pub const HEADLINE_STOP_SEL: &str = ">>";

/// Runtime issue vector (`ISSUE_SEARCH_VECTOR`, `search/issue.py:55-57`).
///
/// `table` is the outer query's issue alias (Django: `"issues"`). The
/// expression parses to the `issues_fts_idx` index expression
/// (`db/models/issue.py:255-266`, `FX-FTS-CORE.sql` §1); only quoting,
/// qualification and whitespace may differ, never the tree.
pub fn issue_vector_sql(table: &str) -> String {
    format!(
        "to_tsvector('english'::regconfig, \
         COALESCE({table}.name, '') || ' ' || COALESCE({table}.description_stripped, ''))"
    )
}

/// Runtime comment vector (`ISSUE_COMMENT_SEARCH_VECTOR`,
/// `search/issue.py:61-63`). Parses to the `issue_comments_fts_idx`
/// expression (`db/models/issue.py:654-662`).
pub fn comment_vector_sql(table: &str) -> String {
    format!("to_tsvector('english'::regconfig, COALESCE({table}.comment_stripped, ''))")
}

/// `SearchQuery(query, search_type="websearch", config="english")`
/// (`search/issue.py:164-166`). `query_param` is the bind placeholder for
/// the raw query text; websearch operators (`OR`, `-`, quotes) flow through
/// untouched (`FX-FTS-CORE.sql` §3).
pub fn websearch_sql(query_param: &str) -> String {
    format!("websearch_to_tsquery('english'::regconfig, {query_param})")
}

/// FTS arm over the issue's own vector (`Q(_fts=search_query)`).
pub fn fts_match_sql(table: &str, query_param: &str) -> String {
    format!(
        "{} @@ {}",
        issue_vector_sql(table),
        websearch_sql(query_param)
    )
}

/// `name__icontains` substring fallback (`search/issue.py:99-101`).
/// `like_param` binds `escape_icontains(query)`.
pub fn name_fallback_sql(table: &str, like_param: &str) -> String {
    format!("{table}.name ILIKE '%' || {like_param} || '%' ESCAPE '\\'")
}

/// Legacy `sequence_id` exact-int arm (`search/issue.py:104-107,
/// :113-123`). Tokens come from
/// [`crate::assistant::tools_issues::search_sequence_tokens`], which owns
/// the 20-char query gate, the 10-digit token gate and the int4 cap;
/// `seq_param` binds that token list as an int array.
pub fn sequence_arm_sql(seq_param: &str) -> String {
    format!("issues.sequence_id IN (SELECT * FROM UNNEST({seq_param}::int[]))")
}

/// Legacy `project__identifier` icontains arm (`search/issue.py:108,124`).
pub fn identifier_arm_sql(like_param: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM projects \
         WHERE projects.id = issues.project_id \
         AND projects.identifier ILIKE '%' || {like_param} || '%' ESCAPE '\\')"
    )
}

/// Comment-text arm (`Q(id__in=_matching_comment_issue_ids(...))`,
/// `search/issue.py:102-103,111-112`).
///
/// The subquery uses the soft-deletion-excluded comment set
/// (`IssueComment.objects`, `:69-70`), strips default ordering (no
/// `ORDER BY` inside the `IN`, `:72-76`) and selects `issue_id` (`:85-90`).
/// Ported limitation (`:78-83`, as-is): no `IssueComment.access` filter, so
/// INTERNAL comment text can surface an issue to a member who cannot read
/// that comment.
pub fn comment_arm_sql(query_param: &str) -> String {
    format!(
        "issues.id IN (SELECT issue_id FROM issue_comments \
         WHERE deleted_at IS NULL AND {} @@ {})",
        comment_vector_sql("issue_comments"),
        websearch_sql(query_param)
    )
}

/// The `_build_search_filter` OR-chain (`search/issue.py:93-125`), in Django
/// arm order: FTS, name fallback, optional comment arm, sequence ints,
/// project identifier.
///
/// `fts_param` binds the raw query, `seq_param` the
/// `search_sequence_tokens` list, `like_param` the
/// `escape_icontains(query)` output (`$1`/`$2`/`$3` stay separate: Django
/// escapes `LIKE` metacharacters only in the `icontains` branches, never in
/// the FTS query).
pub fn search_filter_sql(
    include_comments: bool,
    fts_param: &str,
    seq_param: &str,
    like_param: &str,
) -> String {
    let mut arms = vec![
        fts_match_sql("issues", fts_param),
        name_fallback_sql("issues", like_param),
    ];
    if include_comments {
        arms.push(comment_arm_sql(fts_param));
    }
    arms.push(sequence_arm_sql(seq_param));
    arms.push(identifier_arm_sql(like_param));
    format!("({})", arms.join(" OR "))
}

/// `_rank` annotation (`SearchRank(vector, query)`,
/// `search/issue.py:169-170`): 2-arg `ts_rank`, no weights. Comment-only
/// matches rank 0 — the caller secondary-sorts by recency (`:143-145`).
pub fn rank_sql(table: &str, query_param: &str) -> String {
    format!(
        "ts_rank({}, {})",
        issue_vector_sql(table),
        websearch_sql(query_param)
    )
}

/// `_headline` annotation (`SearchHeadline`, `search/issue.py:171-182`).
/// Django renders `ts_headline(config, field, query, options)` with the
/// options as one bound literal; the literal text below is that exact
/// string (`StartSel`/`StopSel` values carry Django's inner quotes).
pub fn headline_sql(query_param: &str) -> String {
    format!(
        "ts_headline('english'::regconfig, description_stripped, {}, \
         'StartSel=''<<'', StopSel=''>>'', MaxWords=20, MinWords=10, \
         ShortWord=3, HighlightAll=false')",
        websearch_sql(query_param)
    )
}

/// Whether the FTS machinery applies at all (`issue_search_queryset
/// :161-162`): empty/falsy query returns the queryset unchanged, with no
/// annotation. Python falsiness is emptiness here — a whitespace-only query
/// is truthy and DOES apply (it matches the `icontains` arms for strings
/// containing a space). This differs deliberately from the assistant tool
/// path (`search_uses_fts` strips first); that strip belongs to
/// `assistant/tools/issues.py:69`, not to this closure.
pub fn search_applies(query: &str) -> bool {
    !query.is_empty()
}

/// Public snippet from a `_headline` value (`extract_snippet`,
/// `search/issue.py:189-198`): `NULL`/missing markers mean `ts_headline`
/// returned filler (`:47-50`) → `""`; otherwise the marker delimiters are
/// stripped.
pub fn extract_snippet(headline: Option<&str>) -> String {
    match headline {
        Some(text) if text.contains(HEADLINE_START_SEL) => text
            .replace(HEADLINE_START_SEL, "")
            .replace(HEADLINE_STOP_SEL, ""),
        _ => String::new(),
    }
}

/// Re-exported `icontains` escaper for the `$3` binding.
pub use crate::assistant::tools_issues::escape_icontains as escape_like;
/// Re-exported for callers that bind the three filter parameters: call
/// `search_sequence_tokens(query)` for `$2` and `escape_icontains(query)`
/// for `$3` (`$1` is the raw query).
pub use crate::assistant::tools_issues::search_sequence_tokens as sequence_tokens;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::tools_issues::{
        escape_icontains, search_sequence_tokens, ISSUE_FTS_SQL, SEQUENCE_ID_MAX,
    };

    // search/issue.py:55-57 — runtime vector text (table-qualified here;
    // the index expression is the same tree unqualified).
    #[test]
    fn issue_vector_matches_index_shape() {
        assert_eq!(
            issue_vector_sql("issues"),
            "to_tsvector('english'::regconfig, \
             COALESCE(issues.name, '') || ' ' || COALESCE(issues.description_stripped, ''))"
        );
    }

    // search/issue.py:61-63.
    #[test]
    fn comment_vector_matches_index_shape() {
        assert_eq!(
            comment_vector_sql("issue_comments"),
            "to_tsvector('english'::regconfig, COALESCE(issue_comments.comment_stripped, ''))"
        );
    }

    // The shared arms stay byte-identical to the canonical assistant-path
    // predicate: one spelling across the crate, Django arm order kept.
    #[test]
    fn shared_arms_match_canonical_predicate() {
        assert_eq!(search_filter_sql(false, "$1", "$2", "$3"), ISSUE_FTS_SQL);
    }

    // search/issue.py:110-112 — the comment arm sits third, between the
    // name fallback and the sequence arm.
    #[test]
    fn comment_arm_position_and_shape() {
        let sql = search_filter_sql(true, "$1", "$2", "$3");
        let fts = sql.find("@@ websearch_to_tsquery").unwrap();
        let name = sql.find("issues.name ILIKE").unwrap();
        let comment = sql.find("issues.id IN (SELECT issue_id").unwrap();
        let seq = sql.find("issues.sequence_id IN").unwrap();
        let ident = sql.find("projects.identifier ILIKE").unwrap();
        assert!(fts < name && name < comment && comment < seq && seq < ident);
        assert!(sql.contains(
            "SELECT issue_id FROM issue_comments \
             WHERE deleted_at IS NULL AND \
             to_tsvector('english'::regconfig, COALESCE(issue_comments.comment_stripped, '')) \
             @@ websearch_to_tsquery('english'::regconfig, $1)"
        ));
    }

    // search/issue.py:169-170 — 2-arg ts_rank over the issue vector.
    #[test]
    fn rank_shape() {
        assert_eq!(
            rank_sql("issues", "$1"),
            format!(
                "ts_rank({}, {})",
                issue_vector_sql("issues"),
                websearch_sql("$1")
            )
        );
    }

    // search/issue.py:171-182 — config-first ts_headline, exact options.
    #[test]
    fn headline_shape() {
        assert_eq!(
            headline_sql("$1"),
            "ts_headline('english'::regconfig, description_stripped, \
             websearch_to_tsquery('english'::regconfig, $1), \
             'StartSel=''<<'', StopSel=''>>'', MaxWords=20, MinWords=10, \
             ShortWord=3, HighlightAll=false')"
        );
    }

    // search/issue.py:161-162 — only the empty query skips; whitespace
    // applies (Python truthiness, not stripped).
    #[test]
    fn empty_query_skips_whitespace_applies() {
        assert!(!search_applies(""));
        assert!(search_applies(" "));
        assert!(search_applies("auth"));
    }

    // search/issue.py:189-198 — snippet vectors.
    #[test]
    fn snippet_vectors() {
        assert_eq!(extract_snippet(None), "");
        assert_eq!(extract_snippet(Some("")), "");
        assert_eq!(extract_snippet(Some("plain filler text")), "");
        assert_eq!(
            extract_snippet(Some("the <<match>> here")),
            "the match here"
        );
        // Stop-only marker with no start marker is still filler.
        assert_eq!(extract_snippet(Some("odd >> tail")), "");
    }

    // search/issue.py:45,113-123 — int4 gate vectors (owned by
    // search_sequence_tokens; pinned here as the closure's contract).
    #[test]
    fn sequence_gate_vectors() {
        assert_eq!(SEQUENCE_ID_MAX, 2_147_483_647);
        assert_eq!(search_sequence_tokens("424242"), vec![424242]);
        assert_eq!(
            search_sequence_tokens("error 9999999999"),
            Vec::<i64>::new()
        );
        assert_eq!(search_sequence_tokens("2147483648"), Vec::<i64>::new());
        assert_eq!(search_sequence_tokens("2147483647"), vec![2_147_483_647]);
        // >20-char queries skip the int branch entirely.
        assert_eq!(
            search_sequence_tokens("123456789012345678901"),
            Vec::<i64>::new()
        );
        // >10-digit runs are skipped before parsing.
        assert_eq!(search_sequence_tokens("12345678901"), Vec::<i64>::new());
    }

    // icontains escaping contract ($3 binding).
    #[test]
    fn like_escape_vectors() {
        assert_eq!(escape_icontains("100%_\\"), "100\\%\\_\\\\");
        assert_eq!(escape_icontains("auth login"), "auth login");
    }

    // websearch operators pass through to the tsquery parser untouched.
    #[test]
    fn websearch_passthrough_shape() {
        assert_eq!(
            websearch_sql("$1"),
            "websearch_to_tsquery('english'::regconfig, $1)"
        );
    }
}
