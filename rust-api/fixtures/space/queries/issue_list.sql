-- queries/issue_list.sql
-- ProjectIssuesPublicEndpoint.get views/issue.py:76-211. AllowAny (:74).
-- Bad anchor -> {"error": "Project is not published"} 404 (:80-82).
-- Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.

-- L1 anchor lookup: DeployBoard.objects.filter(anchor, entity_name="project")
-- .first() views/issue.py:80; project_id=entity_identifier, slug=workspace.slug
-- (:84-85).

-- L2 base queryset views/issue.py:87-126:
-- Issue.issue_objects (triage/archived/draft EXCLUDED — manager, issue.py:95-104)
--   .filter(workspace__slug, project_id)
--   .select_related(workspace, project, state, parent) — single JOIN row, no SQL fanout
--   .prefetch_related(assignees, labels, issue_module__module) — separate queries
--   .prefetch_related(Prefetch(issue_reactions, IssueReaction.objects.select_related("actor"))) (:91-96)
--   .prefetch_related(Prefetch(votes, IssueVote.objects.select_related("actor"))) (:97)
--   .annotate(cycle_id=Subquery(CycleIssue...values("cycle_id")[:1])) (:98-102)
--   .annotate(link_count=IssueLink...Count...values("count")) (:103-108)
--   .annotate(attachment_count=FileAsset...ISSUE_ATTACHMENT...Count) (:109-117)
--   .annotate(sub_issues_count=Issue.issue_objects.filter(parent=OuterRef("id"))...Count) (:118-123)
--   NOTE: sub_issues_count re-applies the IssueManager exclusions (triage
--   children NOT counted) — PORT.
--   .distinct() (:124)
--   .filter(**issue_filters(query_params, "GET")) (:77,126)
--   order_issue_queryset(order_by=request.GET.get("order_by","-created_at")) (:78,129-131)
--   issue_queryset_grouper(queryset, group_by, sub_group_by) (:134-138;
--   grouper.py:28-70): GROUP_FILTER_MAPPER pre-filters for label/assignee/module
--   group keys (:43-45); default_annotations add Coalesce+ArrayAgg for the
--   non-grouped id-lists — BUG-PORT grouper.py:67 `or` is always True so ALL
--   three annotations are always applied.
SELECT "issues"."id", ...,
  (SELECT U0."cycle_id" FROM "cycle_issues" U0 WHERE (U0."deleted_at" IS NULL
    AND U0."issue_id" = ("issues"."id")) LIMIT 1) AS "cycle_id",
  (SELECT COUNT(U0."id") AS "count" FROM "issue_links" U0
    WHERE U0."issue_id" = ("issues"."id")) AS "link_count",
  (SELECT COUNT(U0."id") AS "count" FROM "file_assets" U0
    WHERE (U0."issue_id" = ("issues"."id")
      AND U0."entity_type" = 'ISSUE_ATTACHMENT')) AS "attachment_count",
  (SELECT COUNT(U0."id") AS "count" FROM "issues" U0
    WHERE (U0."deleted_at" IS NULL AND U0."parent_id" = ("issues"."id")
      AND NOT ...triage/archived/draft exclusions...)) AS "sub_issues_count"
  FROM "issues"
  INNER JOIN "workspaces" ... INNER JOIN "projects" ...
  LEFT OUTER JOIN "states" ... LEFT OUTER JOIN "issues" T_parent ...
  WHERE (deleted_at IS NULL AND triage/archived/draft exclusions
    AND "issues"."workspace_id" = %(workspace_id)s
    AND "issues"."project_id" = %(project_id)s AND <issue_filters Q>)
  ORDER BY <order_issue_queryset> LIMIT ... OFFSET ...;

-- L3 grouped paginate (group_by only): GroupedOffsetPaginator with
-- group_by_fields=issue_group_values(field=group_by, slug, project_id, filters)
-- (:189-194), count_filter=Q((issue_intake__status=1|-1|2|isnull),
-- archived_at__isnull, is_draft=False) (:196-203). group_by==sub_group_by ->
-- {"error": "Group by and sub group by cannot have same parameters"} 400 (:142-146).

-- L4 sub-grouped paginate: SubGroupedOffsetPaginator + sub_group_by_fields
-- (:156-167), same count_filter (:170-177).

-- L5 issue_on_results (runs on EVERY path incl. ungrouped, grouper.py:73-182):
-- annotates vote_items/reaction_items ArrayAgg(Case/When/JSONObject) with
-- avatar_url=Case(When(avatar_asset NOT NULL, Concat('/api/assets/v2/static/',
-- avatar_asset, '/')), default=avatar) (grouper.py:124-135,158-169 — CORRECT
-- issue_reactions__ refs here; the retrieve endpoint has the copy-paste bug),
-- .values(*required_fields, "vote_items", "reaction_items") where
-- required_fields=[id,name,state_id,sort_order,estimate_point,priority,
-- start_date,target_date,sequence_id,project_id,parent_id,cycle_id,created_by,
-- state__group] + original_list subset: ["assignee_ids","label_ids",
-- "module_ids"] with the grouped field SWAPPED for its FIELD_MAPPER lookup
-- (grouper.py:76-109): e.g. group_by="labels__id" -> [..., "labels__id"].
-- vote_items element shape: {"vote": int, "actor_details": {id, first_name,
-- last_name, avatar, avatar_url, display_name}}; reaction_items element shape:
-- {"reaction": str, "actor_details": {...same...}}.
