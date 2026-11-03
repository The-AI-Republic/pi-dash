-- FX-VIEW-ISSUES.sql
-- Trace: app/views/view/base.py:138-253 (WorkspaceViewIssuesViewSet);
--   manager exclusions db/models/issue.py:95-104; envelope keys
--   utils/paginator.py:642-694 (pinned by PAGINATED_KEYS); row keys
--   app/serializers/view.py:14-53 (pinned by VIEW_ISSUE_KEYS).
-- Method: compiler-form SQL rendered offline (no DB on this runner).
-- Route: GET /api/workspaces/<slug>/issues/ (app/urls/views.py:47-51).
-- Gate: allow_roles=[ADMIN, MEMBER, GUEST] WORKSPACE-level (base.py:216).

-- ------------------------------------------------------------------
-- 1. get_queryset (base.py:212-213) + IssueManager (issue.py:95-104)
-- SELECT <issue columns> FROM issues
--   INNER JOIN projects ON (issues.project_id = projects.id)
--   INNER JOIN workspaces ON (issues.workspace_id = workspaces.id)
-- WHERE workspaces.slug = :slug
--   AND issues.deleted_at IS NULL            -- SoftDeletionManager
--   AND NOT (issues.state_id IN (SELECT ... WHERE "group" = 'triage'))
--   AND issues.archived_at IS NULL
--   AND projects.archived_at IS NULL
--   AND issues.is_draft = false
SELECT id FROM issues
WHERE workspace_slug = :slug
  AND deleted_at IS NULL
  AND state_group <> 'triage'
  AND archived_at IS NULL
  AND project_archived_at IS NULL
  AND is_draft = false;

-- ------------------------------------------------------------------
-- 2. legacy filters: issue_filters(query_params, 'GET') ANDed as
-- field lookups (base.py:226-227), then the guest permission Q
-- (base.py:142-162, applied :230-232):
-- WHERE (
--   (projectmember.role = 5 AND projects.guest_view_all_features = true)
--   OR (projectmember.role = 5 AND projects.guest_view_all_features = false
--       AND issues.created_by_id = :user_id)
--   OR (projectmember.role > 5)
-- ) AND projectmember.member_id = :user_id
--   AND projectmember.is_active = true;

-- ------------------------------------------------------------------
-- 3. count queryset: deepcopy + .only('id') (base.py:235-236) ->
-- SELECT COUNT(*) FROM (<query above>);

-- ------------------------------------------------------------------
-- 4. apply_annotations (base.py:164-210). Four subquery annotations +
-- three prefetches. cycle_id: first live cycle for the issue:
--   (SELECT U0.cycle_id FROM cycle_issues U0
--    WHERE (U0.issue_id = issues.id AND U0.deleted_at IS NULL)
--    LIMIT 1) AS cycle_id
-- link_count / attachment_count / sub_issues_count share one shape:
-- COUNT over a correlated filter (Func(F('id'), function='Count')):
--   (SELECT COUNT(U0.id) FROM issue_links U0
--    WHERE U0.issue_id = issues.id) AS link_count
--   (SELECT COUNT(U0.id) FROM file_assets U0
--    WHERE (U0.issue_id = issues.id
--           AND U0.entity_type = 'issue_attachment')) AS attachment_count
--   (SELECT COUNT(U0.id) FROM issues U0
--    WHERE (U0.parent_id = issues.id
--           AND <IssueManager exclusions again>)) AS sub_issues_count
-- Prefetches (no JOIN; separate queries): issue_assignee -> IssueAssignee,
-- label_issue -> IssueLabel, issue_module -> ModuleIssue (base.py:192-209).
SELECT
  (SELECT c.cycle_id FROM cycle_issues c
    WHERE c.issue_id = i.id AND c.deleted_at IS NULL LIMIT 1) AS cycle_id,
  (SELECT COUNT(l.id) FROM issue_links l WHERE l.issue_id = i.id) AS link_count,
  (SELECT COUNT(f.id) FROM file_assets f
    WHERE f.issue_id = i.id AND f.entity_type = 'issue_attachment') AS attachment_count,
  (SELECT COUNT(s.id) FROM issues s
    WHERE s.parent_id = i.id AND s.deleted_at IS NULL
      AND s.is_draft = false AND s.archived_at IS NULL) AS sub_issues_count,
  i.id, i.name
FROM issues i
WHERE i.workspace_slug = :slug;

-- ------------------------------------------------------------------
-- 5. ordering: order_issue_queryset(issue_queryset, order_by_param),
-- default order_by_param '-created_at' (base.py:223,242-244); then
-- paginate(order_by, request, queryset, on_results=ViewIssueListSerializer,
-- total_count_queryset) (base.py:247-253). Cursor protocol default
-- '<per_page>:0:0'; per_page default 1000 max 1000
-- (utils/paginator.py:642-652).

-- 6. paginator envelope keys (exact, utils/paginator.py:654-694):
-- ["grouped_by", "sub_grouped_by", "total_count", "next_cursor",
--  "prev_cursor", "next_page_results", "prev_page_results", "count",
--  "total_pages", "total_results", "extra_stats", "results"]

-- 7. on_results row shape: ViewIssueListSerializer.to_representation
-- (serializers/view.py:24-53). Example row (keys pinned by VIEW_ISSUE_KEYS):
-- {"id": "11111111-1111-1111-1111-111111111111", "name": "Login fails on SSO",
--  "state_id": "22222222-2222-2222-2222-222222222222", "sort_order": 65535.0,
--  "completed_at": null, "estimate_point": null, "priority": "high",
--  "start_date": null, "target_date": "2026-10-01", "sequence_id": 42,
--  "project_id": "33333333-3333-3333-3333-333333333333",
--  "parent_id": null, "cycle_id": null, "sub_issues_count": 0,
--  "created_at": "2026-09-01T10:00:00.000000Z",
--  "updated_at": "2026-09-02T10:00:00.000000Z",
--  "created_by": "44444444-4444-4444-4444-444444444444",
--  "updated_by": "44444444-4444-4444-4444-444444444444",
--  "attachment_count": 1, "link_count": 0, "is_draft": false,
--  "archived_at": null, "state__group": "unstarted",
--  "assignee_ids": ["55555555-5555-5555-5555-555555555555"],
--  "label_ids": [], "module_ids": []}
-- Field notes: estimate_point renders estimate_point_id (FK id, :30);
-- created_by/updated_by render created_by_id/updated_by_id (:42-43);
-- cycle_id/sub_issues_count/attachment_count/link_count are the :164-191
-- annotations (null when the subquery finds nothing); state__group is
-- None when instance.state is None (:48); assignee/label/module id lists
-- come from the :192-209 prefetches via get_assignee_ids/get_label_ids/
-- get_module_ids (:15-22).
