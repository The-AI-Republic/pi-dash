-- queries_issue.sql
-- CycleIssueViewSet.get_queryset + apply_annotations record: SQL shape + filter parity.
-- Source: app/views/cycle/issue.py:40-106. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
--
-- R1 get_queryset (:51-75):
--   sub_issues_count = (SELECT COUNT(*) FROM issues child
--     WHERE child.parent_id = cycle_issues.issue_id)            -- OuterRef('issue_id') (:56)
--     -- via .annotate(count=Count(id)).values('count'); NULL when zero children.
--   workspace__slug = :slug                                     (:61)
--   project_id = :project_id                                    (:62)
--   project__project_projectmember__member = :user AND is_active (:63-66)
--   project__archived_at IS NULL                                (:67)
--   cycle_id = :cycle_id                                        (:68)
--   select_related project, workspace, cycle, issue, issue__state, issue__project (:69-72)
--   prefetch issue__assignees, issue__labels                    (:73)
--   .distinct() (:74), then filter_queryset() -> ComplexFilterBackend (:43) +
--   IssueFilterSet (:44); declared filterset_fields (:49):
--     issue__labels__id, issue__assignees__id.
-- R2 apply_annotations (:77-106) — applied to the ISSUE queryset in list(), NOT to
--   the CycleIssue queryset above:
--   cycle_id = (SELECT cycle_id FROM cycle_issues WHERE issue_id = issues.id
--     AND deleted_at IS NULL LIMIT 1)                           (:80-82)
--   link_count = (SELECT COUNT(*) FROM issue_links WHERE issue_id = issues.id) (:84-89)
--   attachment_count = (SELECT COUNT(*) FROM file_assets WHERE issue_id = issues.id
--     AND entity_type = 'ISSUE_ATTACHMENT')                     (:90-98)
--   sub_issues_count = (SELECT COUNT(*) FROM issues WHERE parent_id = issues.id) (:99-104)
--   prefetch assignees, labels, issue_module__module, issue_cycle__cycle (:105).
-- R3 list() pipeline (:108-221): issue_filters(query_params, 'GET') legacy dict (:111)
--   + filter_queryset (ComplexFilterBackend/IssueFilterSet :119) on
--   Issue.issue_objects(issue_cycle__cycle_id=:cycle_id, bridge not deleted,
--   project, workspace slug) (:112-116); deepcopy for total count (:125);
--   apply_annotations (:128); order_issue_queryset default '-created_at' (:130-134);
--   grouper: group_by=false -> plain paginate (:213-221); group_by set ->
--   GroupedOffsetPaginator (:186-212); both group_by+sub_group_by ->
--   SubGroupedOffsetPaginator (:151-184); group_by == sub_group_by -> 400
--   {"error": "Group by and sub group by cannot have same parameters"} (:146-150).
--   count_filter for grouped pagination: intake status IN (1,-1,2) OR intake NULL,
--   archived NULL, not draft (:176-183, :204-211).
-- R4 create() (:223-297): issues list REQUIRED else 400 {"error": "Issues are required"}
--   (:227-228); completed destination cycle (end_date < now) -> 400
--   {"error": "The Cycle has already been completed so no new issues can be added"}
--   (:232-236) — NULL end_date passes (None < now is False... in Python3 None < datetime
--   raises TypeError — BUG-PORT: null-end cycle create raises TypeError -> 500).
--   Bridges in OTHER cycles (filter ~Q(cycle_id=:cycle_id), issue_id IN issues :239)
--   are MOVED via bulk_update(cycle_id) (:279); the rest bulk_create batch_size=10
--   with project/workspace(created from cycle.workspace_id)/created_by/updated_by (:244-257);
--   activity type cycle.activity.created with updated+created lists (:281-296);
--   201 {"message": "success"} (:297).
-- R5 destroy() (:299-324): filter(issue/cycle/project/workspace-slug) + activity
--   type cycle.activity.deleted (:307-322), .delete() (SOFT delete via manager),
--   204 (:324). No existence check — deleting a missing bridge still 204.
--
-- Representative SQL (placeholders :slug, :project_id, :user, :cycle_id):
SELECT DISTINCT cycle_issues.*,
  (SELECT COUNT(*) FROM issues child WHERE child.parent_id = cycle_issues.issue_id
  ) AS sub_issues_count
FROM cycle_issues
JOIN projects ON projects.id = cycle_issues.project_id
JOIN project_projectmember ON project_projectmember.project_id = projects.id
WHERE cycle_issues.workspace_slug = :slug AND cycle_issues.project_id = :project_id
  AND project_projectmember.member_id = :user AND project_projectmember.is_active = TRUE
  AND projects.archived_at IS NULL
  AND cycle_issues.cycle_id = :cycle_id
  AND cycle_issues.deleted_at IS NULL;
-- apply_annotations additions on the issue queryset:
--   (SELECT cycle_id FROM cycle_issues WHERE issue_id = issues.id AND deleted_at IS NULL LIMIT 1) AS cycle_id,
--   (SELECT COUNT(*) FROM issue_links WHERE issue_id = issues.id) AS link_count,
--   (SELECT COUNT(*) FROM file_assets WHERE issue_id = issues.id AND entity_type = 'ISSUE_ATTACHMENT') AS attachment_count,
--   (SELECT COUNT(*) FROM issues sub WHERE sub.parent_id = issues.id) AS sub_issues_count
-- ORDER default: issues.created_at DESC (order_issue_queryset, issue.py:130).
