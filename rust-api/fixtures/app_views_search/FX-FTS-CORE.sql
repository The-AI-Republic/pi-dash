-- FX-FTS-CORE.sql
-- Trace: search/issue.py:1-207; index exprs db/models/issue.py:255-266
--   (issues_fts_idx) and db/models/issue.py:654-662 (issue_comments_fts_idx).
-- Method: compiler-form SQL rendered offline (no DB on this runner).
-- Parity rule (search/issue.py:5-18): the runtime SearchVector expressions
-- must stay byte-for-byte identical to the index expressions or the planner
-- silently drops the index. ISSUE_FTS_CONFIG = 'english' (:36).

-- ------------------------------------------------------------------
-- 1. Index expressions (must match runtime vectors exactly).
-- issues_fts_idx: GinIndex(SearchVector('name', 'description_stripped',
--   config='english')) ->
--   CREATE INDEX issues_fts_idx ON issues USING gin
--     (to_tsvector('english'::regconfig,
--       COALESCE(name, '') || ' ' || COALESCE(description_stripped, '')));
-- issue_comments_fts_idx: GinIndex(SearchVector('comment_stripped',
--   config='english')) ->
--   CREATE INDEX issue_comments_fts_idx ON issue_comments USING gin
--     (to_tsvector('english'::regconfig, COALESCE(comment_stripped, '')));
-- Runtime vectors: ISSUE_SEARCH_VECTOR (search/issue.py:55-57),
-- ISSUE_COMMENT_SEARCH_VECTOR (:61-63).

-- ------------------------------------------------------------------
-- 2. Representative query A -- plain words: 'auth login'
-- search_query = SearchQuery('auth login', search_type='websearch',
--   config='english') -> websearch_to_tsquery('english', 'auth login').
-- _build_search_filter (:93-125) ORs: FTS match, name__icontains,
-- sequence_id ints (len<=20 gate), project__identifier icontains.
-- include_comments=False here (legacy contract).
SELECT i.* FROM issues i
WHERE i.deleted_at IS NULL
  AND (to_tsvector('english'::regconfig,
         COALESCE(i.name, '') || ' ' || COALESCE(i.description_stripped, ''))
       @@ websearch_to_tsquery('english'::regconfig, 'auth login')
       OR UPPER(i.name) LIKE UPPER('%auth login%')
       OR UPPER(projects.identifier) LIKE UPPER('%auth login%'));

-- 3. Representative query B -- websearch operators: 'auth OR billing -sso'
-- websearch_to_tsquery parses OR / minus / quotes itself; the surrounding
-- OR-chain is unchanged. A quoted phrase '"exact phrase"' likewise flows
-- through websearch_to_tsquery untouched.
SELECT i.* FROM issues i
WHERE i.deleted_at IS NULL
  AND (to_tsvector('english'::regconfig,
         COALESCE(i.name, '') || ' ' || COALESCE(i.description_stripped, ''))
       @@ websearch_to_tsquery('english'::regconfig, 'auth OR billing -sso')
       OR UPPER(i.name) LIKE UPPER('%auth OR billing -sso%')
       OR UPPER(projects.identifier) LIKE UPPER('%auth OR billing -sso%'));

-- 4. Representative query C -- comment match, include_comments=True
-- (GlobalSearchEndpoint.filter_issues, search/base.py:97):
-- third OR arm = id IN (comment subquery). The subquery uses
-- IssueComment.objects (SoftDeletionManager: soft-deleted excluded),
-- strips default ordering via .order_by() (:72-76), and selects issue_id
-- (:85-90). KNOWN LIMITATION (search/issue.py:78-83, port as-is): the
-- subquery does NOT filter IssueComment.access, so INTERNAL comment text
-- can surface an issue to a member who cannot read that comment.
SELECT i.* FROM issues i
WHERE i.deleted_at IS NULL
  AND (to_tsvector('english'::regconfig,
         COALESCE(i.name, '') || ' ' || COALESCE(i.description_stripped, ''))
       @@ websearch_to_tsquery('english'::regconfig, 'refund VN2')
       OR UPPER(i.name) LIKE UPPER('%refund VN2%')
       OR i.id IN (SELECT c.issue_id FROM issue_comments c
                   WHERE c.deleted_at IS NULL
                     AND to_tsvector('english'::regconfig,
                             COALESCE(c.comment_stripped, ''))
                         @@ websearch_to_tsquery('english'::regconfig, 'refund VN2'))
       OR UPPER(projects.identifier) LIKE UPPER('%refund VN2%'));

-- 5. Representative query D -- numeric token + rank order:
-- 'error 9999999999' with_rank=True. Token '9999999999' parses but exceeds
-- _SEQUENCE_ID_MAX 2147483647 (search/issue.py:45) -> skipped, no
-- sequence_id arm, no 500 (:113-123). with_rank annotates
-- _rank = ts_rank(vector, search_query) (:169-170); caller sorts
-- _rank DESC (comment-only matches rank 0 -> secondary recency sort,
-- :143-145). Tokens longer than 10 digits are skipped before int()
-- (:119-120); queries longer than 20 chars skip the int branch entirely
-- (:113).
SELECT i.*,
  ts_rank(to_tsvector('english'::regconfig,
            COALESCE(i.name, '') || ' ' || COALESCE(i.description_stripped, '')),
          websearch_to_tsquery('english'::regconfig, 'error 9999999999')) AS _rank
FROM issues i
WHERE i.deleted_at IS NULL
  AND (to_tsvector('english'::regconfig,
         COALESCE(i.name, '') || ' ' || COALESCE(i.description_stripped, ''))
       @@ websearch_to_tsquery('english'::regconfig, 'error 9999999999')
       OR UPPER(i.name) LIKE UPPER('%error 9999999999%')
       OR UPPER(projects.identifier) LIKE UPPER('%error 9999999999%'))
ORDER BY _rank DESC;

-- 6. with_headline (search/issue.py:171-182): annotates _headline =
-- ts_headline('english', description_stripped, search_query,
--   StartSel='<<', StopSel='>>', MaxWords=20, MinWords=10,
--   ShortWord=3, HighlightAll=false). extract_snippet (:189-198):
-- NULL or marker-less headline -> ''; else markers stripped.
-- Empty/falsy query -> queryset returned UNCHANGED, no annotation
-- (search/issue.py:161-162). search_issues wrapper (:201-207):
-- issue_search_queryset(q, include_comments=False).distinct().

-- ------------------------------------------------------------------
-- 7. EXPLAIN parity (domain disposition: identical EXPLAIN on
-- issues_fts_idx). Expected plan shape for queries A/B/D:
--   Bitmap Heap Scan on issues
--     Recheck Cond: (to_tsvector(...) @@ websearch_to_tsquery(...))
--     -> Bitmap Index Scan using issues_fts_idx
-- For query C the comment arm adds:
--   Nested Loop Semi Join
--     -> Bitmap Index Scan using issue_comments_fts_idx on issue_comments
-- If the Rust port's EXPLAIN loses either index scan, the vector
-- expression has diverged (search/issue.py:7-11) and the port is wrong.
