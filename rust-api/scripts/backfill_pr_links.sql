-- PDASHOSS01-355: Idempotent backfill of github_pull_request_links rows
-- created since migration 0152 into git_code_review_links (provider='github').
-- Matched on metadata.legacy_github_pull_request_link_id, safe to run more
-- than once. Second run inserts 0 rows.
--
-- To run: psql "$DATABASE_URL" -f rust-api/scripts/backfill_pr_links.sql
-- Or via Rust helper: backfill_github_pr_links(pool) in
-- services/src/integrations/code_reviews.rs

-- Step 1: Show missing count
SELECT count(*) AS missing
FROM github_pull_request_links l
WHERE l.deleted_at IS NULL
  AND NOT EXISTS (
    SELECT 1 FROM git_code_review_links r
    WHERE r.metadata->>'legacy_github_pull_request_link_id' = l.id::text
  );

-- Step 2: Insert missing rows. repo_external_id is resolved from
-- git_repositories when a matching repo exists, else ''.
-- State: merged -> 'merged', closed -> 'closed', else 'open'.
INSERT INTO git_code_review_links
  (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   project_id, workspace_id, issue_id, provider, host_url, namespace, repo_name,
   repo_external_id, external_id, external_iid, url, title, state, merged, draft,
   remote_updated_at, metadata)
SELECT
  gen_random_uuid(),
  l.created_at,
  l.updated_at,
  l.created_by_id,
  l.updated_by_id,
  NULL,
  l.project_id,
  l.workspace_id,
  l.issue_id,
  'github',
  'https://github.com',
  LOWER(l.repo_owner),
  LOWER(l.repo_name),
  COALESCE((SELECT external_id FROM git_repositories g
            WHERE g.provider='github' AND g.host_url='https://github.com'
              AND LOWER(g.namespace)=LOWER(l.repo_owner)
              AND LOWER(g.name)=LOWER(l.repo_name)
              AND g.deleted_at IS NULL LIMIT 1), ''),
  '',
  l.pr_number::text,
  l.url,
  l.title,
  CASE WHEN l.merged THEN 'merged' WHEN l.state='closed' THEN 'closed' ELSE 'open' END,
  l.merged,
  l.draft,
  l.pr_updated_at,
  jsonb_build_object('legacy_github_pull_request_link_id', l.id::text)
FROM github_pull_request_links l
WHERE l.deleted_at IS NULL
  AND NOT EXISTS (
    SELECT 1 FROM git_code_review_links r
    WHERE r.metadata->>'legacy_github_pull_request_link_id' = l.id::text
  )
ON CONFLICT DO NOTHING;

-- Step 3: Verify missing is now 0
SELECT count(*) AS missing_after
FROM github_pull_request_links l
WHERE l.deleted_at IS NULL
  AND NOT EXISTS (
    SELECT 1 FROM git_code_review_links r
    WHERE r.metadata->>'legacy_github_pull_request_link_id' = l.id::text
  );
