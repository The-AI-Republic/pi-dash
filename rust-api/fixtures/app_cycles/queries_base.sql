-- queries_base.sql
-- CycleViewSet.get_queryset record: SQL shape + result rows (rows in queries_base.rows.json).
-- Source: app/views/cycle/base.py:69-182. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
--
-- R1 base filters, in order (:90-99):
--   workspace__slug = :slug                                     (:93)
--   project_id = :project_id                                    (:94)
--   project__project_projectmember__member = :user
--     AND project__project_projectmember__is_active = true      (:96-97)
--   project__archived_at IS NULL                                (:99)
-- R2 joins (:100-112): select_related project, workspace, owned_by;
--   prefetch issue_cycle__issue__assignees (users: avatar_asset, first_name, id, distinct) (:102-106);
--   prefetch issue_cycle__issue__labels (labels: name, color, id, distinct) (:108-112).
-- R3 annotations:
--   is_favorite = EXISTS(SELECT 1 FROM user_favorites
--     WHERE user_id = :user AND entity_identifier = cycles.id   -- OuterRef('pk') (:73)
--       AND entity_type = 'cycle' AND project_id = :project_id
--       AND workspace__slug = :slug)                            (:70-76, :113)
--   total_issues = COUNT(DISTINCT issue_cycle.issue_id)
--     FILTER (issue.archived_at IS NULL AND issue.is_draft = false
--       AND cycle_issues.deleted_at IS NULL
--       AND issue.deleted_at IS NULL)                           (:114-125)
--   completed_issues = same + issue.state__group = 'completed'  (:126-138)
--   cancelled_issues = same + issue.state__group IN ('cancelled')
--     -- BUG-PORT (:144): single-element __in list; semantically = 'cancelled', keep the IN shape.
--                                                             (:139-151)
--   status = CASE
--     WHEN start_date <= :now AND end_date >= :now THEN 'CURRENT'  (:154-157)
--     WHEN start_date > :now THEN 'UPCOMING'                      (:158)
--     WHEN end_date < :now THEN 'COMPLETED'                       (:159)
--     WHEN start_date IS NULL AND end_date IS NULL THEN 'DRAFT'   (:160-163)
--     ELSE 'DRAFT' END                                          (:164)
--   -- :now is timezone.now() round-tripped through the project tz back to UTC
--   -- (:78-88: semantically == timezone.now(); PORT the value, not the detour).
--   -- Open-ended cycles fall to DRAFT (NULL comparisons are never true).
--   assignee_ids = COALESCE(ARRAY_AGG(DISTINCT issue_assignees.user_id)
--     FILTER (WHERE issue_assignees.user_id IS NOT NULL
--       AND issue_assignee.deleted_at IS NULL), '{}')           (:168-178)
-- R4 tail: .order_by('-is_favorite', 'name') (:179) then .distinct() (:180),
--   then filter_queryset() (search/order backends).
-- R5 list() OVERRIDE (base.py:183-268): re-orders to ('-is_favorite', '-created_at')
--   (:189) — the get_queryset name ordering is DEAD on list; PORT the effective order.
--   list filters archived_at IS NULL (:185); cycle_view='current' adds
--   start_date <= :now AND end_date >= :now (:205) and returns early with 200 (:236-237)
--   even when the filtered set is EMPTY (data=[] is falsy -> falls THROUGH to the
--   unfiltered-values branch — BUG-PORT: `if data:` on an empty ValuesQuerySet is
--   False, so cycle_view=current with no current cycle returns ALL cycles, not []).
--   Both branches project via .values() (:207-232 current, :239-265 all) then
--   user_timezone_converter(start_date, end_date -> project tz) (:233-234, :266-267).
--   NOTE the values() lists OMIT started/unstarted/backlog counts and sub_issues:
--   list rows carry total/cancelled/completed + assignee_ids + status + version + created_by.
-- R6 retrieve() (base.py:410-475) re-queries with an extra sub_issues annotation
--   (count of child issues in this cycle: parent IS NOT NULL, same project,
--   issue_cycle.cycle_id = pk, not soft-deleted — :418-427) and 404s
--   {"error": "Cycle not found"} when the values row is None (:458-459).
--
-- Representative SQL (placeholders :slug, :project_id, :user, :now):
SELECT DISTINCT cycles.*,
  EXISTS(SELECT 1 FROM user_favorites
    WHERE user_favorites.user_id = :user
      AND user_favorites.entity_identifier = cycles.id
      AND user_favorites.entity_type = 'cycle'
      AND user_favorites.project_id = :project_id
      AND user_favorites.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  ) AS is_favorite,
  COUNT(DISTINCT issue.id) FILTER (WHERE issue.archived_at IS NULL AND issue.is_draft = FALSE
    AND cycle_issues.deleted_at IS NULL AND issue.deleted_at IS NULL) AS total_issues,
  COUNT(DISTINCT issue.id) FILTER (WHERE issue.state_group = 'completed'
    AND issue.archived_at IS NULL AND issue.is_draft = FALSE
    AND cycle_issues.deleted_at IS NULL AND issue.deleted_at IS NULL) AS completed_issues,
  COUNT(DISTINCT issue.id) FILTER (WHERE issue.state_group IN ('cancelled')
    AND issue.archived_at IS NULL AND issue.is_draft = FALSE
    AND cycle_issues.deleted_at IS NULL AND issue.deleted_at IS NULL) AS cancelled_issues,
  CASE WHEN cycles.start_date <= :now AND cycles.end_date >= :now THEN 'CURRENT'
       WHEN cycles.start_date > :now THEN 'UPCOMING'
       WHEN cycles.end_date < :now THEN 'COMPLETED'
       WHEN cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT'
       ELSE 'DRAFT' END AS status,
  COALESCE(ARRAY_AGG(DISTINCT issue_assignees.user_id)
    FILTER (WHERE issue_assignees.user_id IS NOT NULL
      AND issue_assignee.deleted_at IS NULL), '{}') AS assignee_ids
FROM cycles
JOIN projects ON projects.id = cycles.project_id
JOIN project_projectmember ON project_projectmember.project_id = projects.id
LEFT JOIN cycle_issues ON cycle_issues.cycle_id = cycles.id
LEFT JOIN issues issue ON issue.id = cycle_issues.issue_id
LEFT JOIN issue_assignees ON issue_assignees.issue_id = issue.id
WHERE projects.slug_ws = :slug AND cycles.project_id = :project_id
  AND project_projectmember.member_id = :user AND project_projectmember.is_active = TRUE
  AND projects.archived_at IS NULL
GROUP BY cycles.id
ORDER BY is_favorite DESC, cycles.name ASC;
-- EFFECTIVE list order: ORDER BY is_favorite DESC, cycles.created_at DESC (base.py:189).
