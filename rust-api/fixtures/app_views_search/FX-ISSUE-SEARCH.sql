-- FX-ISSUE-SEARCH.sql
-- Trace: app/views/search/issue.py:1-166; route app/urls/search.py:18-20.
-- Method: compiler-form SQL rendered offline (no DB on this runner).
-- Output shape pinned by PROJECT_SEARCH_KEYS (test_search.py):
-- ["name","id","start_date","sequence_id","project__name",
--  "project__identifier","project_id","workspace__slug",
--  "state__name","state__group","state__color"], [:100], HTTP 200.
-- Class note (issue.py:18-22): stays on the primary (read-your-writes).

-- ------------------------------------------------------------------
-- 0. get() params (issue.py:104-113): search default False;
-- workspace_search default 'false'; parent/issue_relation/sub_issue/cycle
-- default 'false'; module default False; target_date default True (bool!);
-- issue_id default False. Only the string 'true' activates a flag;
-- target_date activates only on the string 'none'.
--
-- 1. base queryset (issue.py:115-120):
-- SELECT ... FROM issues
--   INNER JOIN projects ... INNER JOIN project_members ...
-- WHERE workspaces.slug = :slug
--   AND projectmember.member_id = :user_id
--   AND projectmember.is_active = true
--   AND projects.archived_at IS NULL
--   AND <IssueManager exclusions: triage/draft/archived/deleted>
SELECT id, name FROM issues
WHERE workspace_slug = :slug
  AND member_id = :user_id AND member_active = true
  AND project_archived_at IS NULL;

-- 2. workspace_search == 'false' -> filter(project_id=:url_project_id)
-- (issue.py:24-31,122-123). Any other value (incl. 'true') skips it.

-- 3. query truthy -> search_issues(query, issues) =
-- issue_search_queryset(include_comments=False).distinct()
-- (issue.py:33-40; search/issue.py:201-207): FTS match OR nameicontains
-- OR sequence_id-int (len<=20) OR project__identifier icontains.
-- See FX-FTS-CORE.sql for the expression.

-- 4. parent == 'true' AND issue_id -> exclude self, parent, children
-- (issue.py:42-50):
-- WHERE NOT (id = :issue_id OR id = :parent_id OR parent_id = :issue_id)
-- (no-op when the issue_id is unknown: .first() is None -> unchanged).
SELECT id FROM issues WHERE NOT (id = :x OR id = :p OR parent_id = :x);

-- 5. issue_relation == 'true' AND issue_id ->
-- filter_issues_excluding_related_issues (issue.py:52-70): collect both
-- columns of IssueRelation rows touching the issue, append issue_id,
-- exclude pk__in(all). (Unknown issue_id + .first() None -> unchanged,
-- but issue_id is still appended to the list before the guard.)
SELECT id FROM issues WHERE id NOT IN (:self, :rel1, :rel2);

-- 6. sub_issue == 'true' AND issue_id -> filter_root_issues_only
-- (issue.py:72-81): exclude self + keep parent__isnull, then exclude the
-- issue's own parent. BUG B5 (port as-is): `if issue.parent:` (:79) sits
-- OUTSIDE the `if issue:` guard (:77), so an unknown issue_id raises
-- AttributeError -> 500 instead of returning all root issues.
SELECT id FROM issues
WHERE id <> :x AND parent_id IS NULL AND id <> :parent_id;

-- 7. cycle == 'true' -> exclude_issues_in_cycles (issue.py:83-88):
-- WHERE NOT (issue_cycle IS NOT NULL AND issue_cycle.deleted_at IS NULL)
SELECT id FROM issues i
WHERE NOT EXISTS (SELECT 1 FROM cycle_issues c
  WHERE c.issue_id = i.id AND c.deleted_at IS NULL);

-- 8. module truthy (a module UUID string) -> exclude_issues_in_module
-- (issue.py:90-95):
-- WHERE NOT (issue_module.module = :module AND deleted_at IS NULL)
SELECT id FROM issues i
WHERE NOT EXISTS (SELECT 1 FROM module_issues m
  WHERE m.issue_id = i.id AND m.module_id = :module
    AND m.deleted_at IS NULL);

-- 9. target_date == 'none' -> filter(target_date__isnull=True)
-- (issue.py:97-102; default True never equals 'none').
SELECT id FROM issues WHERE target_date IS NULL;

-- 10. guest scoping (issue.py:146-149): ProjectMember role=5 active on
-- the URL project -> issues.filter(created_by = :user_id).
SELECT id FROM issues WHERE created_by_id = :user_id;

-- 11. final projection (issue.py:151-164): values(name, id, start_date,
-- sequence_id, project__name, project__identifier, project_id,
-- workspace__slug, state__name, state__group, state__color)[:100].
-- Example row:
-- {"name": "Login fails on SSO", "id": "11111111-1111-1111-1111-111111111111",
--  "start_date": null, "sequence_id": 42, "project__name": "Web",
--  "project__identifier": "WEB",
--  "project_id": "33333333-3333-3333-3333-333333333333",
--  "workspace__slug": "acme", "state__name": "Backlog",
--  "state__group": "unstarted", "state__color": "#ff0000"}
-- NOTE: state__* joins are LEFT (state nullable); missing state ->
-- state__name/group/color all null.
