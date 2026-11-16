-- queries/profile.sql
-- Workspace user-profile reads: profile, stats bundle, issues, activity, graphs.
-- Sources: app/views/workspace/user.py:99-251 (profile issues), :281-369
--   (profile), :371-394 (activity), :397-521 (stats), :524-559 (graphs);
--   dashboard graph cross-ref app/views/workspace/base.py:257-290.
--   NOTE: no permission_classes on :281,:397,:524,:541 classes (any
--   authenticated caller; siblings use WorkspaceViewer/EntityPermission) — PORT.
--   Last-visited endpoint (:69-96) has NO fixture query here: user.last_workspace_id
--   (:73) raises AttributeError — the field lives on Profile (db/models/user.py:236),
--   not User — so the 500 fires before any Workspace lookup (the :81 DoesNotExist
--   path and :75-79 None branch are unreachable); full record in F-W24-15.
--
-- R1 profile-issues annotations, apply_annotations (:105-134):
--   cycle_id = Subquery(CycleIssue WHERE issue=OuterRef(id) AND
--     deleted_at NULL -> cycle_id [:1]) (:108-110);
--   link_count (:113-117), attachment_count (:119-126: FileAsset WHERE
--   issue_id=OuterRef(id) AND entity_type=ISSUE_ATTACHMENT), sub_issues_count
--   (:128-132: Issue WHERE parent=OuterRef(id)) — each .order_by() cleared +
--   Func Count scalar subquery — 0 when empty, never NULL (Func is not an
--   Aggregate, so no GROUP BY: scalar COUNT always returns one row — PORT);
--   prefetch assignees, labels, issue_module__module (:133).
-- R2 profile-issues pipeline (:136-250): legacy filters (:137); order_by
--   default "-created_at" (:139); base (:140-148): id IN (SELECT id WHERE
--   (assignee=:uid OR created_by=:uid OR subscriber=:uid) AND slug) AND slug
--   AND project__projectmember__(request.user, active) (:146-147: viewer sees
--   only issues in THEIR OWN projects — PORT); filterset (:151) then
--   **filters (:154); deepcopy total-count qs (:157); annotate (:160); order
--   helper (:163); group/sub-group (:168-172); group==sub ->400 (:176-182);
--   grouped (:218-242) / sub-grouped (:184-215) / plain (:244-250) paginate,
--   grouped counts filtered intake (status 1|-1|2 OR NULL) + unarchived +
--   not draft (:207-214 / :234-241).
--   group_by_fields=issue_group_values (utils/grouper.py:146-219): state_id
--   branch (:153-157) filters is_triage=False BUT State.objects=StateManager
--   (db/models/state.py:79-83) adds NOT ("group" = 'triage') on top — PORT.
-- R3 profile read, WorkspaceUserProfileEndpoint.get (:281-368):
--   user = User.objects.get(pk=:uid) (:283: missing -> 404 {"error": "The
--   required object does not exist."} via BaseAPIView.handle_exception — PORT);
--   requester .get(slug, active) (:285-287); role>=15 LITERAL (:289: MEMBER
--   value but literal — PORT; guests keep project_data=[] :288).
--   projects (:291-296): slug + requester's active memberships + unarchived,
--   annotated with 4 counts (:297-342, each with archived NULL + is_draft
--   false): created_issues (:298-306), assigned_issues (:308-316),
--   completed_issues (:318-327: completed_at NOT NULL + assignee),
--   pending_issues (:329-341: state__group IN ('backlog','unstarted',
--   'started') LITERAL trio (:332-336) — excludes review/test, NOT the
--   CLOSED complement — PORT); values(id, logo_props, 4 counts) (:343-351).
--   user_data 8 keys (:356-365): email, first_name, last_name, avatar_url,
--   cover_image_url, date_joined, user_timezone, display_name.
-- R4 user-activity (:371-394, EntityPermission): ?project= multi (:375);
--   filters ~Q(field IN 4) (:378: same exclusion as export-CSV), slug (:379),
--   requester project scope (:380-381) + project__archived_at NULL (:382 —
--   ADDS archived filter that export-CSV R5 LACKS — PORT), actor=:uid (:383:
--   spelled `actor=` not `actor_id=`, same effect — PORT);
--   select_related(actor,workspace,issue,project) (:384);
--   optional project__in (:386-387: raw query strings — PORT);
--   paginate order default "-created_at" (:390).
-- R5 user-stats 9-query bundle (:397-521): filters (:399) applied to 7 of 9
--   (cycle queries Q8/Q9 take NO filters — PORT).
--   Q1 state_distribution (:401-413): (assignee=:uid AND
--   issue_assignee.deleted_at NULL) + slug + requester project scope +
--   filters; GROUP BY state__group, COUNT, ORDER BY group.
--   Q2 priority_distribution (:417-436): same scope; GROUP BY priority,
--   COUNT; .filter(priority_count__gte=1) (:427) ALWAYS-TRUE (grouped counts
--   are >=1 by construction) — PORT; Case/When order urgent=0..none=4,
--   unknown->5 (:428-434); ORDER BY priority_order (:435).
--   Q3 created count (:438-447); Q4 assigned count (:449-458);
--   Q5 pending count (:460-470: ~Q(CLOSED)); Q6 completed count (:472-482:
--   literal "completed"); Q7 subscribed count (:484-494: IssueSubscriber +
--   project unarchived (:490) — ONLY archived-filtered query in bundle —
--   PORT); Q8 upcoming_cycles (:496-500: cycle.start > now, NO requester
--   project scope — PORT); Q9 present_cycle (:502-507: start<now<end;
--   SINGULAR var name, plural response key :518 — PORT).
--   Response keys :510-520 (created_issues vs assigned_issues scalar-name
--   asymmetry — PORT).
-- R6 activity graph, UserActivityGraphEndpoint.get (:524-538): actor=:user,
--   slug, created_at__date >= today-6mo (:530: SIX months vs dashboard Q1
--   THREE months — PORT); Cast+GROUP+COUNT+ORDER same shape as dashboard Q1.
--   TZ asymmetry (USE_TZ, TIME_ZONE="UTC", settings/common.py:361-362 —
--   PORT): created_at__date__gte compiles to (created_at AT TIME ZONE
--   'UTC')::date, but the Cast annotation stays a plain cast
--   (created_at)::date.
-- R7 completed graph (:541-559): month default 1 (:543: same January-default
--   quirk as dashboard Q2 — PORT); ExtractWeek(completed_at) AS
--   completed_week (:552) then week = completed_week % 4 (:553: mod-4 buckets
--   0-3 — vs dashboard WeekInMonth buckets 1-5 — PORT); GROUP BY week,
--   COUNT(completed_week) (:555: counts non-null weeks ~= rows — PORT);
--   ORDER BY week.
--   TZ (USE_TZ, TIME_ZONE="UTC" — PORT): all three EXTRACTs convert —
--   EXTRACT(WEEK|MONTH FROM completed_at AT TIME ZONE 'UTC') in the select
--   week, the COUNT(week) and the month filter.
-- R8 dashboard-graph cross-ref (base.py:257-290): WeekInMonth custom Func
--   (:257-259: FLOOR(((day-1)/7)+1)::INT); dashboard completed (:278-290)
--   buckets 1-5 vs R7 mod-4 buckets 0-3 — same family, different buckets;
--   PORT each exactly.
--
-- Representative SQL (placeholders :user viewer, :uid target, :slug, :month):
-- R1 annotations on issues i:
SELECT i.*,
  (SELECT ci.cycle_id FROM cycle_issues ci WHERE ci.issue_id = i.id
    AND ci.deleted_at IS NULL LIMIT 1) AS cycle_id,
  (SELECT COUNT(*) FROM issue_links l WHERE l.issue_id = i.id) AS link_count,
  (SELECT COUNT(*) FROM file_assets f WHERE f.issue_id = i.id
    AND f.entity_type = 'ISSUE_ATTACHMENT') AS attachment_count,
  (SELECT COUNT(*) FROM issues s WHERE s.parent_id = i.id) AS sub_issues_count
FROM issues i WHERE i.id IN
  (SELECT i2.id FROM issues i2 LEFT JOIN issue_assignees ia ON ia.issue_id = i2.id
   LEFT JOIN issue_subscribers s2 ON s2.issue_id = i2.id
   WHERE i2.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
   AND (ia.user_id = :uid OR i2.created_by_id = :uid OR s2.subscriber_id = :uid))
  AND EXISTS(SELECT 1 FROM project_members pm WHERE pm.project_id = i.project_id
    AND pm.member_id = :user AND pm.is_active);
-- R3 project_data with 4 counts (pending trio literal):
SELECT p.id, p.logo_props,
  COUNT(CASE WHEN pi.created_by_id = :uid AND pi.archived_at IS NULL AND NOT pi.is_draft THEN 1 END) AS created_issues,
  COUNT(CASE WHEN :uid = ANY(pi.assignee_ids) AND pi.archived_at IS NULL AND NOT pi.is_draft THEN 1 END) AS assigned_issues,
  COUNT(CASE WHEN pi.completed_at IS NOT NULL AND :uid = ANY(pi.assignee_ids) AND pi.archived_at IS NULL AND NOT pi.is_draft THEN 1 END) AS completed_issues,
  COUNT(CASE WHEN pi.state_group IN ('backlog','unstarted','started') AND :uid = ANY(pi.assignee_ids) AND pi.archived_at IS NULL AND NOT pi.is_draft THEN 1 END) AS pending_issues
FROM projects p WHERE p.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND p.archived_at IS NULL
  AND EXISTS(SELECT 1 FROM project_members pm WHERE pm.project_id = p.id AND pm.member_id = :user AND pm.is_active)
GROUP BY p.id; -- user_data: SELECT email,first_name,last_name,avatar_url,
--   cover_image_url,date_joined,user_timezone,display_name FROM users WHERE id = :uid;
-- R5-Q2 priority (always-true HAVING + Case order):
SELECT priority, COUNT(*) AS priority_count,
  CASE priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2
  WHEN 'low' THEN 3 WHEN 'none' THEN 4 ELSE 5 END AS priority_order
FROM issues WHERE ... GROUP BY priority HAVING COUNT(*) >= 1 ORDER BY priority_order;
-- R7 completed graph (mod-4 buckets 0-3):
SELECT (EXTRACT(WEEK FROM completed_at AT TIME ZONE 'UTC')::INT % 4) AS week, COUNT(*) AS completed_count
FROM issues WHERE workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND :user IN (SELECT user_id FROM issue_assignees WHERE issue_id = issues.id)
  AND EXTRACT(MONTH FROM completed_at AT TIME ZONE 'UTC') = :month AND completed_at IS NOT NULL
GROUP BY 1 ORDER BY 1;