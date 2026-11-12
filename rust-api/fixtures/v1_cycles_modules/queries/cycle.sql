-- queries/cycle.sql — FX-CYCMOD-04. Transcribed from the get_queryset chains cited per block.
-- Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52 (zero diff to HEAD, verified 2026-09-29).

-- Q1 CycleList queryset (views/cycle.py:89-167) + GET archived filter (views/cycle.py:197).
-- Cycle.objects.filter(workspace__slug, project_id, project__project_projectmember__member=<user> AND is_active)
--   .select_related(project, workspace, owned_by)
--   .annotate(total_issues=Count(issue_cycle) FILTER non-draft non-archived live issues)  (:100-109)
--   .annotate(completed/cancelled/started/unstarted/backlog_issues=Count(issue_cycle__issue__state__group='X') same FILTER)  (:110-164)
--   .order_by(<order_by or -created_at>).distinct()  (:165-167)
SELECT "cycles".*, COUNT("ci"."id") FILTER (
    WHERE "i"."archived_at" IS NULL AND "i"."is_draft" = FALSE AND "ci"."deleted_at" IS NULL
  ) AS "total_issues",
  COUNT("ci"."issue__state__group") FILTER (
    WHERE "ci"."issue__state__group" = '<group>' AND "i"."archived_at" IS NULL
      AND "i"."is_draft" = FALSE AND "ci"."deleted_at" IS NULL
  ) AS "<group>_issues"  -- repeated per group: completed/cancelled/started/unstarted/backlog
  FROM "cycles"
  INNER JOIN "projects" ON ("cycles"."project_id" = "projects"."id")
  INNER JOIN "project_members" ON ("projects"."id" = "project_members"."project_id"
    AND "project_members"."member_id" = %(user)s AND "project_members"."is_active" = TRUE)
  LEFT OUTER JOIN "cycle_issues" "ci" ON ("cycles"."id" = "ci"."cycle_id")
  LEFT OUTER JOIN "issues" "i" ON ("ci"."issue_id" = "i"."id")
  WHERE ("cycles"."workspace_id" = %(workspace)s AND "cycles"."project_id" = %(project)s
    AND "cycles"."deleted_at" IS NULL AND "cycles"."archived_at" IS NULL)  -- GET adds archived_at IS NULL (:197)
  GROUP BY "cycles"."id" ORDER BY "cycles"."created_at" DESC;

-- Q2 CycleDetail queryset (views/cycle.py:370-448): identical annotations to Q1, no archived filter
-- in the queryset itself; GET applies .filter(archived_at__isnull=True).get(pk) (views/cycle.py:469).
-- Same SELECT as Q1 with WHERE "cycles"."id" = %(pk)s AND "cycles"."archived_at" IS NULL.

-- Q3 Archived-cycles list (views/cycle.py:622-724): same base + archived_at__isnull=False (:630),
-- PLUS estimate annotations (:699-721):
--   .annotate(total_estimates=Sum(issue_cycle__issue__estimate_point__key))  (:699)
--   .annotate(completed_estimates=Sum(... FILTER state__group='completed' + live))  (:700-710)
--   .annotate(started_estimates=Sum(... FILTER state__group='started' + live))  (:711-721)
-- Same SELECT as Q1 plus SUM("estimate_points"."key") columns, WHERE archived_at IS NOT NULL.

-- Q4 CycleIssue list queryset (views/cycle.py:815-837) vs GET inline queryset (views/cycle.py:862-895).
-- get_queryset: CycleIssue.objects.annotate(sub_issues_count=Issue.issue_objects.filter(parent=OuterRef(issue_id))...Count) (:817-822)
--   .filter(workspace__slug, project_id, member-active, cycle_id) (:823-829)
--   .select_related(project, workspace, cycle, issue+state+project) (:830-833)
--   .prefetch_related(issue__assignees, issue__labels) (:834) .order_by(-created_at).distinct()
-- GET (:862-895) instead queries Issue.issue_objects.filter(issue_cycle__cycle_id, issue_cycle__deleted_at IS NULL)
--   .annotate(sub_issues_count=same, bridge_id=F(issue_cycle__id)) (:864-870)
--   .filter(project_id, workspace__slug) (:871-872) + select/prefetch assignees/labels (:873-878)
--   .order_by(<order_by or created_at>) (:879)
--   .annotate(link_count=IssueLink Count subquery) (:880-885)
--   .annotate(attachment_count=FileAsset ISSUE_ATTACHMENT Count subquery) (:886-894)
SELECT "issues".*,
  (SELECT COUNT(U0."id") FROM "issues" U0 WHERE (U0."deleted_at" IS NULL AND U0."parent_id" = "issues"."id")) AS "sub_issues_count",
  "cycle_issues"."id" AS "bridge_id",
  (SELECT COUNT(U0."id") FROM "issue_links" U0 WHERE U0."issue_id" = "issues"."id") AS "link_count",
  (SELECT COUNT(U0."id") FROM "file_assets" U0 WHERE (U0."issue_id" = "issues"."id" AND U0."entity_type" = 'ISSUE_ATTACHMENT')) AS "attachment_count"
  FROM "issues" INNER JOIN "cycle_issues" ON ("issues"."id" = "cycle_issues"."issue_id")
  WHERE ("cycle_issues"."cycle_id" = %(cycle)s AND "cycle_issues"."deleted_at" IS NULL
    AND "issues"."project_id" = %(project)s AND "issues"."workspace_id" = %(workspace)s
    AND "issues"."deleted_at" IS NULL) ORDER BY "issues"."created_at" ASC;

-- Q5 CycleIssue detail (views/cycle.py:1063-1085 get; queryset 1027-1052): CycleIssue.objects.get(
--   workspace__slug, project_id, cycle_id, issue_id) — single-row fetch, no annotations in handler.

-- Q6 Transfer (utils/cycle_transfer_issues.py:36-479): guard new_cycle completed (:59-67); old_cycle counts
-- re-annotated WITH extra issue__deleted_at + cycle deleted_at filters (:69-143, .first() :143);
-- estimate_type check estimate__type='points' (:152-157); assignee/label estimate + issue distributions
-- (:165-399: assignee-estimate :165-230, label-estimate :231-265, estimate chart :266-285,
-- assignee-issue :287-350, label-issue :351-399); completion_chart burndown_plot(issues) (:400-407);
-- progress_snapshot saved on source cycle (:409-434); incomplete issues (state__group IN OPEN_STATE_GROUPS,
-- issue live) moved to new_cycle via bulk_update(cycle_id) (:436-459); issue_activity.delay created (:462-478).
