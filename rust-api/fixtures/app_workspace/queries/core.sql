-- queries/core.sql
-- Workspace core queries: list shapes, dashboard bundle, theme, export-CSV.
-- Sources: app/views/workspace/base.py:65-81 (WorkSpaceViewSet.get_queryset),
--   :204-240 (UserWorkSpacesEndpoint), :257-348 (dashboard), :351-357 (theme),
--   :368-390 (export-CSV). Roles app/permissions/base.py:13-16.
--   CLOSED_STATE_GROUPS=("completed","cancelled") utils/constants.py:88.
--
-- R1 WorkSpaceViewSet.get_queryset (:65-81):
--   member_count scalar subquery (:66-71): WorkspaceMember WHERE
--     workspace=OuterRef(id) AND member__is_bot=false AND is_active
--     (.order_by() clears default ordering; Func Count; NULL when zero rows)
--   pipeline (:73-81): super().get_queryset() [all] -> select_related(owner)
--     -> filter_queryset FIRST (:74: search ?search= on name :60,
--     filterset owner :61) -> .order_by("name") (:75)
--     -> membership scope (:76-79: member=:user AND is_active)
--     -> annotate(total_members=member_count) (:80).
--   PORT: filter_queryset runs BEFORE membership scoping here, but AFTER
--     in R2 (:235) — order differs between the two list paths.
-- R2 UserWorkSpacesEndpoint.get (:209-240, use_read_replica :207):
--   fields = CSV of ?fields=, empty -> [] -> None=all (:210, :236).
--   member_count (:211-216, same shape as R1); role subquery (:218-220):
--     WorkspaceMember WHERE workspace=OuterRef(id) AND member=:user
--     AND is_active -> values(role) (NULL if none; membership filter :230
--     guarantees a row in practice).
--   Prefetch workspace_member filtered to requester's active row (:223-228);
--   annotate(role, total_members) (:229) -> membership filter (:230)
--   -> distinct (:231) -> filter_queryset AFTER (:235) -> serialize :234-238.
-- R3 dashboard 9-query bundle, UserWorkspaceDashboardEndpoint.get (:262-348):
--   Q1 issue_activities (:264-274): actor=:user, slug, created_at__date >=
--     today-3mo; Cast(created_at->Date) AS created_date; GROUP BY date,
--     COUNT(*) AS activity_count; ORDER BY date.
--   Q2 completed_issues (:278-290): assignees=:user, slug,
--     completed_at__month=:month (default 1, :276 — BUG-PORT: January default
--     regardless of current month), completed_at NOT NULL;
--     day_of_month=ExtractDay, week_in_month=WeekInMonth (:257-259:
--     FLOOR(((day-1)/7)+1)::INT, buckets 1-5); GROUP BY week, COUNT AS
--     completed_count; ORDER BY week.
--   Q3 assigned_issues count (:292). Q4 pending count (:294-298):
--     ~Q(state__group IN CLOSED) i.e. open 5 groups.
--   Q5 completed count (:300-302): state__group="completed" LITERAL —
--     excludes cancelled, NOT the complement of Q4 — PORT.
--   Q6 issues_due_week count (:304-309): ExtractWeek(target_date)==current
--     ISO week (:307-308; NULL target_dates drop out) — PORT.
--   Q7 state_distribution (:311-317): GROUP BY state__group, COUNT, ORDER BY.
--   Q8 overdue (:319-325): ~Q(CLOSED), target_date < now,
--     completed_at NULL -> values(id,name,workspace__slug,project_id,
--     target_date). Q9 upcoming (:327-333): ~Q(CLOSED), start_date >= now,
--     completed_at NULL -> values(id,name,workspace__slug,project_id,
--     start_date). Response keys :336-346 (note Q3-6 collapse to _count).
-- R4 WorkspaceThemeViewSet.get_queryset (:356-357): super() queryset filtered
--   workspace__slug=:slug only (no select_related, no ordering override).
-- R5 export-CSV query, ExportWorkspaceUserActivityEndpoint.post (:379-390):
--   ?date required (:380-381); filters ~Q(field IN
--   ('comment','vote','reaction','draft')) (:384), slug (:385),
--   created_at__date=:date (:386), project membership of REQUESTER (:387-388:
--   member=request.user AND is_active), actor_id=:user_id (:389);
--   select_related(actor,workspace,issue,project); LIMIT 10000, no offset and
--   no explicit order_by (model default ordering) — PORT. NOTE: no
--   project__archived_at filter here, unlike user-activity R4 in profile.sql.
--
-- Representative SQL (placeholders :user, :slug, :month, :date, :user_id):
-- R1/R2 member_count + role subqueries:
SELECT w.*, (SELECT COUNT(wm.id) FROM workspace_members wm
  JOIN users m ON m.id = wm.member_id
  WHERE wm.workspace_id = w.id AND NOT m.is_bot AND wm.is_active
) AS total_members,
(SELECT wm2.role FROM workspace_members wm2
  WHERE wm2.workspace_id = w.id AND wm2.member_id = :user AND wm2.is_active
) AS role
FROM workspaces w
  JOIN workspace_members wmf ON wmf.workspace_id = w.id
WHERE wmf.member_id = :user AND wmf.is_active AND w.name ILIKE :search
ORDER BY w.name;
-- R3-Q1 activities (Q7 same GROUP shape on state_group):
SELECT CAST(created_at AS DATE) AS created_date, COUNT(*) AS activity_count
FROM issue_activities WHERE actor_id = :user
  AND workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND created_at::DATE >= CURRENT_DATE - INTERVAL '3 months'
GROUP BY 1 ORDER BY 1;
-- R3-Q2 completed by WeekInMonth (buckets 1-5):
SELECT (((EXTRACT(DAY FROM completed_at) - 1) / 7) + 1)::INTEGER AS week_in_month,
  COUNT(*) AS completed_count FROM issues
WHERE workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND :user IN (SELECT user_id FROM issue_assignees WHERE issue_id = issues.id)
  AND EXTRACT(MONTH FROM completed_at) = :month AND completed_at IS NOT NULL
GROUP BY 1 ORDER BY 1;
-- R3-Q4/Q5 counts; Q6 due-week; Q8 overdue:
SELECT COUNT(*) FROM issues WHERE workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND state_group NOT IN ('completed','cancelled') AND :user = ANY(assignee_ids);
SELECT COUNT(*) FROM issues WHERE workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND state_group = 'completed' AND :user = ANY(assignee_ids);
SELECT COUNT(*) FROM issues WHERE workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND EXTRACT(WEEK FROM target_date) = EXTRACT(WEEK FROM NOW()) AND :user = ANY(assignee_ids);
SELECT id, name, project_id, target_date FROM issues
WHERE state_group NOT IN ('completed','cancelled') AND target_date < NOW()
  AND completed_at IS NULL AND :user = ANY(assignee_ids);
-- R4 theme: SELECT * FROM workspace_themes WHERE workspace_id =
--   (SELECT id FROM workspaces WHERE slug = :slug);
-- R5 export-CSV:
SELECT * FROM issue_activities ia WHERE ia.field NOT IN ('comment','vote','reaction','draft')
  AND ia.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND ia.created_at::DATE = :date AND ia.actor_id = :user_id
  AND EXISTS(SELECT 1 FROM project_members pm WHERE pm.project_id = ia.project_id
    AND pm.member_id = :user AND pm.is_active) LIMIT 10000;